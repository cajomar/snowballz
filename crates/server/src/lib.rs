//! Snowballz game server: lobby, authoritative simulation and networking.
//!
//! The same server runs as a dedicated process (`snowballz-server`) or on a
//! background thread inside the client when you host a game.

mod admin;
#[cfg(feature = "llm")]
mod engine;
mod strategist;
pub use strategist::{prefetch as prefetch_llm, setup as setup_llm, Advice, Advisor};
pub mod upnp;
mod ws;

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sim::ai::AiLevel;
use sim::game::{Command, Event, Game, PlayerSetup, MAX_PLAYERS, PLAYER_COLORS};
use sim::map::{list_maps, Map};
use sim::mapgen;
use sim::proto::{
    discovery_beacon, write_frame, ClientMsg, FrameReader, GameInfo, LobbyState, MapInfo, ServerMsg,
    Slot, SlotKind, SnapshotEncoder, DISCOVERY_PORT, PROTOCOL_VERSION,
};
use sim::TICK_RATE;

/// Biggest frame the host may send (a map to add to the lobby).
pub(crate) const HOST_FRAME: usize = 6 << 20;
/// Biggest frame anyone else may send: chat, commands and slot changes.
const GUEST_FRAME: usize = 64 << 10;
/// Connections held at once. Behind a proxy every client shares one address,
/// so there is no per-address limit to fall back on.
const MAX_CONNS: usize = 64;
/// How long a connection may sit before saying anything useful.
const HTTP_WITHIN: Duration = Duration::from_secs(10);
const HELLO_WITHIN: Duration = Duration::from_secs(15);
/// A download that hasn't finished by now isn't going to.
const FILE_WITHIN: Duration = Duration::from_secs(60);
/// Clients ping every 2 s, so this much silence means they are gone.
const IDLE_AFTER: Duration = Duration::from_secs(90);
/// Frames handled per connection per server loop, so one chatty client
/// can't starve the others or the simulation.
const FRAMES_PER_LOOP: usize = 200;
/// Maps added in the lobby are kept in memory; this bounds the total.
const MAX_MAP_MEMORY: usize = 64 << 20;
/// How often (in ticks) spectators are told what the computer players think.
const AI_NOTES_EVERY: u32 = 10;
/// Lobby chat from one connection comes no faster than this.
const CHAT_GAP: Duration = Duration::from_millis(300);

/// Locate the `assets` directory (next to the binary, the working dir, or
/// the source tree when running through cargo).
pub fn find_assets() -> PathBuf {
    if let Ok(p) = std::env::var("SNOWBALLZ_ASSETS") {
        return PathBuf::from(p);
    }
    let mut candidates = vec![PathBuf::from("assets")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("assets"));
            candidates.push(dir.join("../../assets"));
            candidates.push(dir.join("../share/snowballz/assets"));
        }
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
    candidates
        .into_iter()
        .find(|p| p.join("maps").is_dir())
        .unwrap_or_else(|| PathBuf::from("assets"))
}

/// Whether players are shown what the computers think: on for now, while
/// the computers are being tuned. `SNOWBALLZ_AI_NOTES=0` turns it off.
pub fn ai_notes_for_players() -> bool {
    !std::env::var("SNOWBALLZ_AI_NOTES").is_ok_and(|v| v == "0")
}

#[derive(Clone)]
pub struct ServerConfig {
    pub name: String,
    pub bind: SocketAddr,
    pub maps_dir: PathBuf,
    /// Where maps added in the lobby are saved and listed from next time.
    /// `None` keeps them in memory only.
    pub user_maps_dir: Option<PathBuf>,
    /// Broadcast LAN discovery beacons.
    pub announce: bool,
    /// Shut down when the host disconnects (embedded servers).
    pub stop_with_host: bool,
    /// Serve the browser build from this directory on the same port.
    pub web_dir: Option<PathBuf>,
    /// Players see the panel of what the computer players are thinking, as
    /// spectators do (for playtesting). Off, only spectators see it.
    pub ai_notes_for_players: bool,
}

pub struct ServerHandle {
    pub addr: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// The server works out each client's field of view this often (in ticks).
const FOV_EVERY: u32 = 3;

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Start a server on a background thread.
pub fn spawn(cfg: ServerConfig) -> io::Result<ServerHandle> {
    let listener = TcpListener::bind(cfg.bind)?;
    let addr = listener.local_addr()?;
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    let thread = std::thread::Builder::new().name("snowballz-server".into()).spawn(move || {
        if let Err(e) = Server::new(cfg, listener).and_then(|mut s| s.run(&s2)) {
            eprintln!("server error: {e}");
        }
    })?;
    Ok(ServerHandle { addr, stop, thread: Some(thread) })
}

/// Run a server on the current thread until stopped.
pub fn run(cfg: ServerConfig) -> io::Result<()> {
    let listener = TcpListener::bind(cfg.bind)?;
    println!("snowballz server '{}' listening on {}", cfg.name, listener.local_addr()?);
    Server::new(cfg, listener)?.run(&AtomicBool::new(false))
}

#[derive(PartialEq)]
enum ConnState {
    Handshake,
    Lobby,
}

/// How bytes on a connection are to be read. Native clients speak the framed
/// protocol directly; browsers arrive as HTTP and either fetch the web build
/// or upgrade to a WebSocket that carries the same frames.
enum Wire {
    /// Nothing decided yet: the first bytes say which it is.
    Sniff,
    /// An HTTP request head is arriving.
    Http,
    Raw,
    Ws(ws::Decoder),
}

struct Conn {
    id: u32,
    stream: TcpStream,
    wire: Wire,
    /// Bytes of a request head while it's still arriving.
    head: Vec<u8>,
    reader: FrameReader,
    outbox: Vec<u8>,
    /// Hang up once everything queued has been written (HTTP responses).
    close_when_flushed: bool,
    name: String,
    state: ConnState,
    /// Player index in the running game, if playing.
    player: Option<u8>,
    encoder: Option<SnapshotEncoder>,
    /// What this player's team can see, refreshed a few times a second.
    fov: Option<sim::fog::Fov>,
    dead: bool,
    /// Secret handed out in Welcome; whoever shows it may take this
    /// connection's seat back in a running game.
    token: u64,
    born: Instant,
    /// When bytes last arrived.
    last_rx: Instant,
    last_chat: Instant,
    /// An admin page request waiting for the server to answer it.
    admin: Option<String>,
}

impl Conn {
    /// Has this connection overstayed what its stage of life allows?
    fn expired(&self, now: Instant) -> bool {
        let age = now.duration_since(self.born);
        if self.close_when_flushed {
            return age > FILE_WITHIN;
        }
        match (&self.wire, &self.state) {
            (Wire::Sniff | Wire::Http, _) => age > HTTP_WITHIN,
            (_, ConnState::Handshake) => age > HELLO_WITHIN,
            (_, ConnState::Lobby) => now.duration_since(self.last_rx) > IDLE_AFTER,
        }
    }

    /// Too much unread input already: leave the rest in the socket.
    fn backed_up(&self) -> bool {
        let cap = 2 * HOST_FRAME;
        self.reader.buffered() > cap || matches!(&self.wire, Wire::Ws(d) if d.buffered() > cap)
    }

    fn send(&mut self, msg: &ServerMsg) {
        let payload = msg.encode();
        let mut frame = Vec::with_capacity(payload.len() + 4);
        let _ = write_frame(&mut frame, &payload);
        match self.wire {
            Wire::Ws(_) => self.outbox.extend_from_slice(&ws::binary(&frame)),
            _ => self.outbox.extend_from_slice(&frame),
        }
        self.flush();
    }

    /// Read what's arrived and sort it into frames for `reader`.
    fn pump(&mut self, web_dir: Option<&Path>) {
        let mut tmp = [0u8; 16384];
        loop {
            if self.backed_up() {
                return;
            }
            match self.stream.read(&mut tmp) {
                Ok(0) => {
                    // A browser that has its file may hang up before we finish writing.
                    self.dead |= !self.close_when_flushed;
                    return;
                }
                Ok(n) => {
                    self.last_rx = Instant::now();
                    self.feed(&tmp[..n], web_dir)
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    self.dead = true;
                    return;
                }
            }
            if self.dead || self.close_when_flushed {
                return;
            }
        }
    }

    fn feed(&mut self, mut bytes: &[u8], web_dir: Option<&Path>) {
        if matches!(self.wire, Wire::Sniff) {
            self.head.extend_from_slice(bytes);
            bytes = &[];
            // The first frame of the raw protocol starts with a small
            // little-endian length, which never looks like "GET ".
            let n = self.head.len().min(4);
            if self.head[..n] != b"GET "[..n] {
                self.wire = Wire::Raw;
                self.reader.push(&std::mem::take(&mut self.head));
            } else if n == 4 {
                self.wire = Wire::Http;
            }
        }
        match &mut self.wire {
            Wire::Sniff => {}
            Wire::Raw => self.reader.push(bytes),
            Wire::Http => {
                self.head.extend_from_slice(bytes);
                if self.head.len() > 16 << 10 {
                    self.dead = true;
                    return;
                }
                match ws::parse_request(&self.head) {
                    Ok(None) => {}
                    Ok(Some((req, used))) => {
                        let rest = self.head.split_off(used);
                        self.head.clear();
                        if let Some(key) = req.websocket_key() {
                            self.outbox.extend_from_slice(&ws::upgrade_response(key));
                            self.wire = Wire::Ws(ws::Decoder::default());
                            self.feed(&rest, web_dir);
                        } else if admin::wants(&req.path) {
                            // Answered by the server, which has the game.
                            self.admin = Some(req.path.clone());
                        } else {
                            self.outbox.extend_from_slice(&ws::serve(web_dir, &req));
                            self.close_when_flushed = true;
                        }
                        self.flush();
                    }
                    Err(_) => self.dead = true,
                }
            }
            Wire::Ws(dec) => {
                dec.push(bytes);
                loop {
                    match dec.next() {
                        Ok(Some(ws::Event::Data(d))) => self.reader.push(&d),
                        Ok(Some(ws::Event::Ping(p))) => self.outbox.extend_from_slice(&ws::pong(&p)),
                        Ok(Some(ws::Event::Close)) => {
                            self.outbox.extend_from_slice(&ws::close());
                            self.close_when_flushed = true;
                            break;
                        }
                        Ok(None) => break,
                        Err(_) => {
                            self.dead = true;
                            break;
                        }
                    }
                }
                self.flush();
            }
        }
    }

    fn flush(&mut self) {
        while !self.outbox.is_empty() {
            match self.stream.write(&self.outbox) {
                Ok(0) => {
                    self.dead = true;
                    return;
                }
                Ok(n) => {
                    self.outbox.drain(..n);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    self.dead = true;
                    return;
                }
            }
        }
        if self.close_when_flushed && self.outbox.is_empty() {
            self.dead = true;
        }
        // A client that can't keep up for this long is gone.
        if self.outbox.len() > 8 << 20 {
            self.dead = true;
        }
    }
}

struct Running {
    game: Game,
    /// Rejoin token of the human in each player slot (0 for computers).
    tokens: Vec<u64>,
    ended_at: Option<Instant>,
    /// A language model advising the computer players, if one is set up.
    strategist: Option<strategist::Strategist>,
}

/// Map indices travel as a `u8`, so the lobby holds at most this many.
const MAX_MAPS: usize = 256;

struct MapEntry {
    info: MapInfo,
    path: PathBuf,
    /// Maps added this session keep their bytes; others are read from `path`.
    bytes: Option<Vec<u8>>,
}

fn map_info(m: &Map) -> MapInfo {
    MapInfo { name: m.name.clone(), description: m.description.clone(), players: m.igloos.len().min(MAX_PLAYERS) as u8 }
}

struct Server {
    cfg: ServerConfig,
    listener: TcpListener,
    udp: Option<UdpSocket>,
    conns: Vec<Conn>,
    next_conn: u32,
    host: u32,
    maps: Vec<MapEntry>,
    map: u8,
    slots: Vec<Slot>,
    game: Option<Running>,
    last_beacon: Instant,
    had_host: bool,
    /// The host connection has left (embedded servers stop then).
    host_gone: bool,
    /// Key to the admin page; `None` turns it off.
    admin_key: Option<String>,
}

impl Server {
    fn new(cfg: ServerConfig, listener: TcpListener) -> io::Result<Server> {
        listener.set_nonblocking(true)?;
        let dirs = std::iter::once(&cfg.maps_dir).chain(cfg.user_maps_dir.as_ref());
        let maps: Vec<MapEntry> = dirs
            .flat_map(|dir| list_maps(dir))
            .filter_map(|(_, path)| {
                let m = Map::load_file(&path).ok()?;
                Some(MapEntry { info: map_info(&m), path, bytes: None })
            })
            .collect();
        if maps.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no maps found in {}", cfg.maps_dir.display()),
            ));
        }
        let udp = if cfg.announce {
            UdpSocket::bind("0.0.0.0:0").ok().and_then(|s| s.set_broadcast(true).ok().map(|_| s))
        } else {
            None
        };
        // Only a server that serves the web build has an admin page.
        let admin_key = cfg.web_dir.as_ref().and_then(|_| admin::load_key());
        let mut s = Server {
            admin_key,
            cfg,
            listener,
            udp,
            conns: Vec::new(),
            next_conn: 1,
            host: 0,
            maps,
            map: 0,
            slots: Vec::new(),
            game: None,
            last_beacon: Instant::now() - Duration::from_secs(5),
            had_host: false,
            host_gone: false,
        };
        // Default to Aurora Basin if present: it's small and quick.
        if let Some(i) = s.maps.iter().position(|m| m.info.name == "Aurora Basin") {
            s.map = i as u8;
        }
        s.reset_slots();
        Ok(s)
    }

    fn reset_slots(&mut self) {
        let n = self.maps[self.map as usize].info.players as usize;
        let mut slots: Vec<Slot> = (0..n)
            .map(|i| Slot { kind: SlotKind::Open, name: String::new(), color: i as u8, team: i as u8, conn: 0 })
            .collect();
        // Keep the humans (and AIs) that were already seated.
        let mut i = 0;
        for old in self.slots.iter().filter(|s| matches!(s.kind, SlotKind::Human | SlotKind::Ai(_))) {
            if i < n {
                slots[i] = Slot { color: old.color, team: old.team.min(n as u8 - 1), ..old.clone() };
                i += 1;
            }
        }
        // Humans who lost their seat become spectators.
        self.slots = slots;
    }

    fn run(&mut self, stop: &AtomicBool) -> io::Result<()> {
        let tick_len = Duration::from_secs_f64(1.0 / TICK_RATE as f64);
        let mut next_tick = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            self.accept();
            self.read_all();
            self.answer_admin();
            self.reap();
            if self.cfg.stop_with_host && self.host_gone {
                break;
            }

            if self.game.is_some() {
                let now = Instant::now();
                let mut steps = 0;
                while now >= next_tick && steps < 5 {
                    self.tick_guarded();
                    next_tick += tick_len;
                    steps += 1;
                }
                if now > next_tick + tick_len * 10 {
                    next_tick = now; // fell far behind (debugger, suspend): don't fast-forward
                }
            } else {
                next_tick = Instant::now();
            }

            if self.last_beacon.elapsed() > Duration::from_secs(1) {
                self.beacon();
            }
            for c in &mut self.conns {
                c.flush();
            }
            let wait = next_tick.saturating_duration_since(Instant::now()).min(Duration::from_millis(10));
            std::thread::sleep(wait.max(Duration::from_millis(1)));
        }
        Ok(())
    }

    /// Answer admin page requests from the running game.
    fn answer_admin(&mut self) {
        let game = self.game.as_ref().map(|r| &r.game);
        for c in self.conns.iter_mut() {
            if let Some(path) = c.admin.take() {
                c.outbox.extend_from_slice(&admin::answer(&path, self.admin_key.as_deref(), game));
                c.close_when_flushed = true;
                c.flush();
            }
        }
    }

    fn accept(&mut self) {
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if self.conns.len() >= MAX_CONNS || stream.set_nonblocking(true).is_err() {
                        continue; // dropped: the client sees the connection close
                    }
                    let _ = stream.set_nodelay(true);
                    let id = self.next_conn;
                    self.next_conn += 1;
                    self.conns.push(Conn {
                        id,
                        stream,
                        wire: Wire::Sniff,
                        head: Vec::new(),
                        reader: FrameReader::default(),
                        outbox: Vec::new(),
                        close_when_flushed: false,
                        name: String::new(),
                        state: ConnState::Handshake,
                        player: None,
                        encoder: None,
                        fov: None,
                        dead: false,
                        token: 0,
                        born: Instant::now(),
                        last_rx: Instant::now(),
                        last_chat: Instant::now() - CHAT_GAP,
                        admin: None,
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
    }

    fn read_all(&mut self) {
        let mut msgs = Vec::new();
        let web_dir = self.cfg.web_dir.as_deref();
        let host = self.host;
        for c in &mut self.conns {
            c.reader.set_limit(if c.id == host { HOST_FRAME } else { GUEST_FRAME });
            c.pump(web_dir);
            for _ in 0..FRAMES_PER_LOOP {
                match c.reader.next() {
                    Ok(Some(f)) => match ClientMsg::decode(&f) {
                        Ok(m) => msgs.push((c.id, m)),
                        Err(_) => c.dead = true,
                    },
                    Ok(None) => break,
                    Err(_) => {
                        c.dead = true;
                        break;
                    }
                }
            }
        }
        for (id, m) in msgs {
            // A bug triggered by one client's message costs that client its
            // connection, not everybody their game.
            let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.handle(id, m)));
            if done.is_err() {
                eprintln!("panic while handling a message from connection {id}; dropping it");
                if let Some(c) = self.conn(id) {
                    c.dead = true;
                }
            }
        }
    }

    /// One simulation tick. If it panics the game is over, but the server
    /// and its lobby carry on.
    fn tick_guarded(&mut self) {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.tick())).is_err() {
            eprintln!("panic in the simulation; ending the game");
            self.end_game();
            self.system_chat("The game hit an internal error and was ended.");
            self.broadcast_lobby();
        }
    }

    fn reap(&mut self) {
        let now = Instant::now();
        for c in &mut self.conns {
            c.dead |= c.expired(now);
        }
        let dead: Vec<u32> = self.conns.iter().filter(|c| c.dead).map(|c| c.id).collect();
        if dead.is_empty() {
            return;
        }
        self.conns.retain(|c| !c.dead);
        for id in &dead {
            for s in &mut self.slots {
                if s.conn == *id {
                    *s = Slot { kind: SlotKind::Open, name: String::new(), conn: 0, ..s.clone() };
                }
            }
        }
        if self.host != 0 && dead.contains(&self.host) {
            self.host_gone = true;
        }
        if dead.contains(&self.host) {
            self.host = self.conns.iter().find(|c| c.state == ConnState::Lobby).map_or(0, |c| c.id);
        }
        // A dedicated server whose players all left goes back to the lobby.
        if self.game.is_some() && !self.conns.iter().any(|c| c.player.is_some()) {
            self.end_game();
        }
        self.broadcast_lobby();
    }

    fn conn(&mut self, id: u32) -> Option<&mut Conn> {
        self.conns.iter_mut().find(|c| c.id == id)
    }

    fn handle(&mut self, id: u32, msg: ClientMsg) {
        // Hello comes first and only once: everything else needs a name and
        // a seat in the lobby, and a second Hello would claim another slot.
        let said_hello = self.conn(id).is_some_and(|c| c.state == ConnState::Lobby);
        if said_hello == matches!(msg, ClientMsg::Hello { .. }) && !matches!(msg, ClientMsg::Ping(_)) {
            return;
        }
        let is_host = id == self.host;
        match msg {
            ClientMsg::Hello { version, name, token } => {
                let name: String = name.chars().filter(|c| !c.is_control()).take(24).collect();
                let name = if name.trim().is_empty() { format!("Penguin {id}") } else { name };
                let name = self.unique_name(name, id);
                let Some(c) = self.conn(id) else { return };
                if version != PROTOCOL_VERSION {
                    c.send(&ServerMsg::Reject(format!(
                        "version mismatch (server v{PROTOCOL_VERSION}, you v{version})"
                    )));
                    c.dead = true;
                    return;
                }
                // Whoever holds a seat's token gets that seat back; anyone else
                // gets a new token of their own. A name proves nothing.
                let held = token != 0 && self.game.as_ref().is_some_and(|r| r.tokens.contains(&token));
                let token = if held { token } else { fresh_token() };
                let Some(c) = self.conn(id) else { return };
                c.name = name.clone();
                c.state = ConnState::Lobby;
                c.token = token;
                c.send(&ServerMsg::Welcome { conn: id, token });
                if self.host == 0 {
                    self.host = id;
                    self.had_host = true;
                }
                if let Some(slot) = self.slots.iter_mut().find(|s| s.kind == SlotKind::Open) {
                    *slot = Slot { kind: SlotKind::Human, name: name.clone(), conn: id, ..slot.clone() };
                }
                // Rejoin a running game?
                if let Some(run) = &self.game {
                    let seat = held.then(|| run.tokens.iter().position(|&t| t == token)).flatten();
                    let taken = seat.is_some_and(|s| self.conns.iter().any(|c| c.player == Some(s as u8)));
                    let player = if taken { None } else { seat.map(|s| s as u8) };
                    self.join_game(id, player);
                }
                self.broadcast_lobby();
                self.system_chat(&format!("{name} joined"));
            }
            ClientMsg::SetMap(m) if is_host && self.game.is_none() => {
                if (m as usize) < self.maps.len() {
                    self.map = m;
                    self.reset_slots();
                    self.broadcast_lobby();
                }
            }
            ClientMsg::AddMap(bytes) if is_host && self.game.is_none() => self.add_map(id, bytes),
            ClientMsg::SetSlot { slot, kind } if is_host && self.game.is_none() => {
                if let Some(s) = self.slots.get_mut(slot as usize) {
                    if s.kind == SlotKind::Human && s.conn != 0 {
                        return; // can't evict a human by changing their slot
                    }
                    let name = match kind {
                        SlotKind::Ai(l) => format!("{} Bot", ai_name(slot as usize, l)),
                        _ => String::new(),
                    };
                    if !matches!(kind, SlotKind::Human) {
                        *s = Slot { kind, name, conn: 0, ..s.clone() };
                    }
                    self.broadcast_lobby();
                }
            }
            ClientMsg::SetTeam { slot, team } if self.game.is_none() => {
                let n = self.slots.len() as u8;
                if let Some(s) = self.slots.get_mut(slot as usize) {
                    if (is_host || s.conn == id) && team < n {
                        s.team = team;
                        self.broadcast_lobby();
                    }
                }
            }
            ClientMsg::SetColor { slot, color } if self.game.is_none() => {
                if (color as usize) < PLAYER_COLORS.len() && !self.slots.iter().any(|s| s.color == color) {
                    if let Some(s) = self.slots.get_mut(slot as usize) {
                        if is_host || s.conn == id {
                            s.color = color;
                            self.broadcast_lobby();
                        }
                    }
                }
            }
            ClientMsg::TakeSlot(slot) if self.game.is_none() => {
                let name = self.conn(id).map(|c| c.name.clone()).unwrap_or_default();
                if self.slots.get(slot as usize).is_some_and(|s| s.kind == SlotKind::Open) {
                    for s in &mut self.slots {
                        if s.conn == id {
                            *s = Slot { kind: SlotKind::Open, name: String::new(), conn: 0, ..s.clone() };
                        }
                    }
                    let s = &mut self.slots[slot as usize];
                    *s = Slot { kind: SlotKind::Human, name, conn: id, ..s.clone() };
                    self.broadcast_lobby();
                }
            }
            ClientMsg::Start if is_host && self.game.is_none() => self.start_game(),
            ClientMsg::BackToLobby if is_host => {
                if self.game.is_some() {
                    self.end_game();
                    self.broadcast_lobby();
                }
            }
            ClientMsg::Cmd(cmd) => {
                let player = self.conn(id).and_then(|c| c.player);
                match (&mut self.game, player, cmd) {
                    (Some(run), Some(p), cmd) => run.game.command(p, cmd),
                    (_, _, Command::Chat { text, .. }) => {
                        // Lobby / spectator chat goes to everyone.
                        let from = self.conn(id).map(|c| c.name.clone()).unwrap_or_default();
                        let color = self.slots.iter().find(|s| s.conn == id).map_or(255, |s| s.color);
                        let text: String = text.chars().filter(|c| !c.is_control()).take(200).collect();
                        let gap_ok = self.conn(id).is_some_and(|c| {
                            let ok = c.last_chat.elapsed() >= CHAT_GAP;
                            if ok {
                                c.last_chat = Instant::now();
                            }
                            ok
                        });
                        if gap_ok && !text.trim().is_empty() {
                            self.broadcast(&ServerMsg::Chat { from, color, text, private: false });
                        }
                    }
                    _ => {}
                }
            }
            ClientMsg::Ping(t) => {
                if let Some(c) = self.conn(id) {
                    c.send(&ServerMsg::Pong(t));
                }
            }
            _ => {}
        }
    }

    /// Puts a map from the host in the lobby, saves it if there's a user maps
    /// directory, and selects it.
    fn add_map(&mut self, id: u32, bytes: Vec<u8>) {
        if self.maps.len() >= MAX_MAPS {
            return self.refuse(id, "the lobby already has as many maps as it can hold".into());
        }
        let held: usize = self.maps.iter().filter_map(|m| m.bytes.as_ref()).map(Vec::len).sum();
        if held + bytes.len() > MAX_MAP_MEMORY {
            return self.refuse(id, "the lobby has no room for more added maps".into());
        }
        let mut map = match Map::parse(&bytes) {
            Ok(m) => m,
            Err(e) => return self.refuse(id, format!("that isn't a map file ({e})")),
        };
        // Names and descriptions go out to everyone in every lobby update.
        let (long_name, long_desc) = (map.name.chars().count() > 40, map.description.chars().count() > 300);
        map.name = map.name.chars().filter(|c| !c.is_control()).take(40).collect();
        map.description = map.description.chars().filter(|c| !c.is_control()).take(300).collect();
        if let Err(e) = mapgen::check(&map) {
            return self.refuse(id, format!("the map can't be played: {e}"));
        }
        // Lobby names must be unique: "Name 2", "Name 3"... like player names.
        let renamed = self.maps.iter().any(|m| m.info.name == map.name);
        let bytes = if renamed || long_name || long_desc {
            let base = map.name.clone();
            if renamed {
                map.name = (2..)
                    .map(|i| format!("{base} {i}"))
                    .find(|n| !self.maps.iter().any(|m| m.info.name == *n))
                    .unwrap();
            }
            map.encode()
        } else {
            bytes
        };
        let path = match &self.cfg.user_maps_dir {
            Some(dir) => match save_map(dir, &map.name, &bytes) {
                Ok(p) => p,
                Err(e) => return self.refuse(id, format!("couldn't save the map: {e}")),
            },
            None => PathBuf::new(),
        };
        self.maps.push(MapEntry { info: map_info(&map), path, bytes: Some(bytes) });
        self.map = (self.maps.len() - 1) as u8;
        self.reset_slots();
        self.broadcast_lobby();
    }

    /// Tells one player, in lobby chat, why something they asked for didn't happen.
    fn refuse(&mut self, id: u32, text: String) {
        let text = format!("Couldn't add the map: {text}");
        if let Some(c) = self.conn(id) {
            c.send(&ServerMsg::Chat { from: String::new(), color: 255, text, private: false });
        }
    }

    fn load_map(&self, i: usize) -> Option<Map> {
        let e = &self.maps[i];
        match &e.bytes {
            Some(b) => Map::parse(b).ok(),
            None => Map::load_file(&e.path).ok(),
        }
    }

    fn unique_name(&self, name: String, id: u32) -> String {
        let taken = |n: &str| self.conns.iter().any(|c| c.id != id && c.name == n);
        if !taken(&name) {
            return name;
        }
        (2..).map(|i| format!("{name} {i}")).find(|n| !taken(n)).unwrap()
    }

    fn lobby_state(&self) -> LobbyState {
        LobbyState {
            server_name: self.cfg.name.clone(),
            host: self.host,
            maps: self.maps.iter().map(|m| m.info.clone()).collect(),
            map: self.map,
            slots: self.slots.clone(),
            spectators: self
                .conns
                .iter()
                .filter(|c| c.state == ConnState::Lobby && !self.slots.iter().any(|s| s.conn == c.id))
                .map(|c| c.name.clone())
                .collect(),
            in_game: self.game.is_some(),
        }
    }

    fn broadcast(&mut self, msg: &ServerMsg) {
        for c in &mut self.conns {
            if c.state == ConnState::Lobby {
                c.send(msg);
            }
        }
    }

    fn broadcast_lobby(&mut self) {
        let msg = ServerMsg::Lobby(self.lobby_state());
        self.broadcast(&msg);
    }

    fn system_chat(&mut self, text: &str) {
        self.broadcast(&ServerMsg::Chat { from: String::new(), color: 255, text: text.into(), private: false });
    }

    fn start_game(&mut self) {
        let seated: Vec<&Slot> =
            self.slots.iter().filter(|s| matches!(s.kind, SlotKind::Human | SlotKind::Ai(_))).collect();
        if seated.is_empty() {
            return;
        }
        let Some(map) = self.load_map(self.map as usize) else { return };
        let setups: Vec<PlayerSetup> = seated
            .iter()
            .map(|s| PlayerSetup {
                name: s.name.clone(),
                color: s.color,
                team: s.team,
                ai: match s.kind {
                    SlotKind::Ai(l) => Some(l),
                    // For screenshots and soak tests: a computer plays for the humans.
                    _ if std::env::var_os("SNOWBALLZ_AUTOPILOT").is_some() => Some(AiLevel::Hard),
                    _ => None,
                },
            })
            .collect();
        let tokens: Vec<u64> = seated
            .iter()
            .map(|s| if s.kind == SlotKind::Human { self.conns.iter().find(|c| c.id == s.conn).map_or(0, |c| c.token) } else { 0 })
            .collect();
        let seats: Vec<(u32, u8)> = seated
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind == SlotKind::Human)
            .map(|(i, s)| (s.conn, i as u8))
            .collect();
        // SNOWBALLZ_SEED fixes the game for screenshots and soak tests.
        let seed = std::env::var("SNOWBALLZ_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| {
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64)
            });
        // The new core rules (DESIGN.md section 0).
        let core = sim::game::CoreRules::default();
        // SNOWBALLZ_START=map: every tribe starts with the map's people, not
        // the small start (the client's self-test sets it).
        let core = if std::env::var("SNOWBALLZ_START").is_ok_and(|v| v == "map") { core.map_start() } else { core };
        let mut game = Game::new_with(map, setups, seed, core);
        // Every game the computers are somebody new.
        game.roll_ai_tempers(seed);
        // For screenshots: start in a given season or storm.
        if let Ok(w) = std::env::var("SNOWBALLZ_WEATHER") {
            use sim::game::Season;
            match w.as_str() {
                "thaw" => game.weather.season = Season::Thaw,
                "summer" => game.weather.season = Season::Midsummer,
                "night" => game.weather.season = Season::PolarNight,
                "blizzard" => game.weather.blizzard = u16::MAX,
                "calm" => game.weather.calm = true,
                _ => {}
            }
        }
        // For looking at the art: every building by your igloo.
        // SNOWBALLZ_SHOWCASE=0.5 has them half built.
        if let Ok(v) = std::env::var("SNOWBALLZ_SHOWCASE") {
            game.showcase(0, v.parse().unwrap_or(1.0));
        }
        // For measuring big games: SNOWBALLZ_CROWD=N gives every tribe N
        // more snowballers at its igloo (SNOWBALLZ_CROWD_KIND=Packer, say,
        // for another kind).
        if let Some(n) = std::env::var("SNOWBALLZ_CROWD").ok().and_then(|v| v.parse::<u32>().ok()) {
            let kind = std::env::var("SNOWBALLZ_CROWD_KIND")
                .ok()
                .and_then(|k| sim::game::Kind::ALL.into_iter().find(|c| c.stats().name.eq_ignore_ascii_case(&k)))
                .unwrap_or(sim::game::Kind::Snowballer);
            for i in 0..game.igloos.len() {
                if game.igloos[i].owner != sim::game::NEUTRAL {
                    for _ in 0..n {
                        game.spawn_for_test(i as u8, kind);
                    }
                }
            }
        }
        self.game = Some(Running { game, tokens, ended_at: None, strategist: strategist::Strategist::from_env() });
        let ids: Vec<u32> = self.conns.iter().filter(|c| c.state == ConnState::Lobby).map(|c| c.id).collect();
        for id in ids {
            let player = seats.iter().find(|(c, _)| *c == id).map(|(_, p)| *p);
            self.join_game(id, player);
        }
        self.broadcast_lobby();
    }

    fn join_game(&mut self, id: u32, player: Option<u8>) {
        let Some(run) = &self.game else { return };
        let info = GameInfo::from_game(&run.game, player.unwrap_or(sim::game::NEUTRAL));
        let msg = ServerMsg::GameStart(Box::new(info));
        if let Some(c) = self.conns.iter_mut().find(|c| c.id == id) {
            c.player = player;
            c.encoder = Some(SnapshotEncoder::new());
            c.send(&msg);
        }
    }

    fn end_game(&mut self) {
        self.game = None;
        for c in &mut self.conns {
            c.player = None;
            c.encoder = None;
        }
    }

    fn tick(&mut self) {
        let Some(run) = &mut self.game else { return };
        run.game.tick();
        if let Some(s) = &mut run.strategist {
            s.step(&mut run.game);
        }
        let events = std::mem::take(&mut run.game.events);
        if run.game.winner.is_some() && run.ended_at.is_none() {
            run.ended_at = Some(Instant::now());
        }

        let mut chats = Vec::new();
        for e in &events {
            if let Event::Chat { from, to, text } = e {
                chats.push((*from, *to, text.clone()));
            }
        }
        for (from, to, text) in &chats {
            // Computers deal with each other directly (`ai/diplomacy.rs`);
            // answering each other's chat would only start them chattering.
            let who = run.game.players.get(*from as usize).map(|p| p.name.clone()).unwrap_or_default();
            if let Some(s) = &mut run.strategist {
                s.heard(&who, text);
            }
            if run.game.players.get(*from as usize).is_none_or(|p| p.ai.is_some()) {
                continue;
            }
            match &mut run.strategist {
                // With a model, deals and facts are the game's, said in the
                // computer's voice; small talk is the model's own answer.
                Some(s) => {
                    for (ai, r) in run.game.ai_replies_full(*from, *to, text) {
                        s.answer(&mut run.game, ai, r.to, text, r.text, r.binding);
                    }
                }
                None => run.game.deliver_chat_to_ai(*from, *to, text),
            }
        }
        // AI replies land in the event list immediately; send them next tick.
        let game = &run.game;
        // For screenshots: everyone sees everything, as a spectator does.
        let watch_all = std::env::var_os("SNOWBALLZ_SPECTATE").is_some();
        // What the computer players are thinking goes to those who see everything.
        let notes = (game.tick % AI_NOTES_EVERY == 0).then(|| game.ai_notes()).filter(|n| !n.is_empty());
        for c in &mut self.conns {
            let Some(enc) = &mut c.encoder else { continue };
            // Players see only what their team sees. Spectators, beaten
            // players and everyone once the match is won see it all.
            let blind = c.player.is_none_or(|p| game.players.get(p as usize).is_none_or(|pl| pl.out)) || game.winner.is_some() || watch_all;
            if blind {
                c.fov = None;
            } else if c.fov.as_ref().is_none_or(|f| game.tick.wrapping_sub(f.at_tick) >= FOV_EVERY) {
                c.fov = c.player.map(|p| sim::fog::Fov::compute(game, game.team(p)));
            }
            let snap = enc.encode_for(game, &events, c.fov.as_ref());
            c.send(&ServerMsg::Snapshot(snap));
            if let Some(n) = notes.as_ref().filter(|_| blind || self.cfg.ai_notes_for_players) {
                c.send(&ServerMsg::AiNotes(n.clone()));
            }
            for (from, to, text) in &chats {
                let visible = match to {
                    None => true,
                    Some(t) => c.player == Some(*t) || c.player == Some(*from),
                };
                if visible {
                    let Some(p) = game.players.get(*from as usize) else { continue };
                    let mut name = p.name.clone();
                    if let Some(t) = to {
                        let Some(target) = game.players.get(*t as usize) else { continue };
                        name = format!("{} → {}", p.name, target.name);
                    }
                    c.send(&ServerMsg::Chat { from: name, color: p.color, text: text.clone(), private: to.is_some() });
                }
            }
        }
    }

    fn beacon(&mut self) {
        self.last_beacon = Instant::now();
        let Some(udp) = &self.udp else { return };
        let (map, max) = {
            let m = &self.maps[self.map as usize].info;
            (m.name.clone(), m.players)
        };
        let players = self.slots.iter().filter(|s| s.kind == SlotKind::Human).count() as u8;
        let port = self.listener.local_addr().map_or(0, |a| a.port());
        let b = discovery_beacon(port, &self.cfg.name, &map, players, max, self.game.is_some());
        let _ = udp.send_to(&b, ("255.255.255.255", DISCOVERY_PORT));
    }
}

/// A secret nobody could guess: the standard library seeds its hash maps
/// from the operating system, so hashing the clock with one of those is
/// unpredictable without a random-number crate.
fn fresh_token() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    h.write_u128(now);
    h.finish().max(1)
}

fn ai_name(slot: usize, level: AiLevel) -> &'static str {
    const NAMES: [&str; 10] =
        ["Pebble", "Flipper", "Waddles", "Tux", "Nippy", "Frosty", "Sleet", "Mumble", "Blizzard", "Pingu"];
    let _ = level;
    NAMES[slot % NAMES.len()]
}

/// Convenience for the client: the maps directory inside `assets`.
pub fn maps_dir(assets: &Path) -> PathBuf {
    assets.join("maps")
}

/// Where players keep their own maps: `$XDG_DATA_HOME/snowballz/maps`,
/// else `~/.local/share/snowballz/maps`, else `%APPDATA%\snowballz\maps`.
pub fn user_maps_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))?;
    Some(base.join("snowballz").join("maps"))
}

/// File-name form of a map name: lowercase, anything but letters and digits
/// becomes '-'.
fn slug(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    if s.is_empty() {
        "map".into()
    } else {
        s
    }
}

/// Writes a map as `<slug>.sbmap` in `dir`, using `-2`, `-3`... rather than
/// overwriting a map that's already there.
fn save_map(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let slug = slug(name);
    let mut i = 1;
    loop {
        let file = if i == 1 { format!("{slug}.sbmap") } else { format!("{slug}-{i}.sbmap") };
        let path = dir.join(file);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                f.write_all(bytes)?;
                return Ok(path);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => i += 1,
            Err(e) => return Err(e),
        }
    }
}
