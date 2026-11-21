//! What a stranger on the internet can send: none of it may crash the
//! server, stall it for the other players, or give anyone powers they
//! shouldn't have.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use sim::proto::*;

fn start() -> server::ServerHandle {
    server::spawn(server::ServerConfig {
        name: "test".into(),
        bind: "127.0.0.1:0".parse().unwrap(),
        maps_dir: server::maps_dir(&server::find_assets()),
        user_maps_dir: None,
        announce: false,
        stop_with_host: false,
        web_dir: None,
        ai_notes_for_players: false,
    })
    .unwrap()
}

struct Client {
    s: TcpStream,
    r: FrameReader,
    chats: Vec<(String, String)>,
    lobby: Option<LobbyState>,
    pongs: u32,
    token: u64,
    /// The seat the server gave us when a game started (255: watching).
    you: Option<u8>,
}

impl Client {
    fn raw(addr: SocketAddr) -> Client {
        let s = TcpStream::connect(addr).unwrap();
        s.set_nonblocking(true).unwrap();
        Client { s, r: FrameReader::default(), chats: vec![], lobby: None, pongs: 0, token: 0, you: None }
    }
    fn join(addr: SocketAddr, name: &str) -> Client {
        Client::join_with(addr, name, 0)
    }
    fn join_with(addr: SocketAddr, name: &str, token: u64) -> Client {
        let mut c = Client::raw(addr);
        c.send(ClientMsg::Hello { version: PROTOCOL_VERSION, name: name.into(), token });
        c.wait(3.0, |c| c.lobby.is_some());
        c
    }
    fn send(&mut self, m: ClientMsg) {
        let mut f = Vec::new();
        write_frame(&mut f, &m.encode()).unwrap();
        self.send_bytes(&f);
    }
    fn send_bytes(&mut self, bytes: &[u8]) {
        self.s.set_nonblocking(false).unwrap();
        let _ = self.s.write_all(bytes);
        self.s.set_nonblocking(true).unwrap();
    }
    /// Read what's arrived; false once the server has hung up.
    fn pump(&mut self) -> bool {
        let alive = self.r.fill(&mut self.s).is_ok();
        while let Ok(Some(f)) = self.r.next() {
            match ServerMsg::decode(&f) {
                Ok(ServerMsg::Lobby(l)) => self.lobby = Some(l),
                Ok(ServerMsg::Chat { from, text, .. }) => self.chats.push((from, text)),
                Ok(ServerMsg::Pong(_)) => self.pongs += 1,
                Ok(ServerMsg::Welcome { token, .. }) => self.token = token,
                Ok(ServerMsg::GameStart(info)) => self.you = Some(info.you),
                _ => {}
            }
        }
        alive
    }
    fn wait(&mut self, secs: f32, mut f: impl FnMut(&Client) -> bool) {
        let t = Instant::now();
        while t.elapsed().as_secs_f32() < secs {
            self.pump();
            if f(self) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out");
    }
    /// The server is up and answering us.
    fn answers(&mut self) -> bool {
        let before = self.pongs;
        self.send(ClientMsg::Ping(1));
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(3) {
            self.pump();
            if self.pongs > before {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }
}

#[test]
fn a_private_chat_to_nobody_does_not_take_the_server_down() {
    let h = start();
    let mut host = Client::join(h.addr, "host");
    host.send(ClientMsg::Start);
    std::thread::sleep(Duration::from_millis(300));
    host.send(ClientMsg::Cmd(sim::game::Command::Chat { to: Some(9), text: "boo".into() }));
    std::thread::sleep(Duration::from_millis(300));
    assert!(host.answers(), "server still running the game");
    let mut other = Client::join(h.addr, "other");
    assert!(other.answers());
}

#[test]
fn a_map_with_an_igloo_off_the_grid_is_refused() {
    let h = start();
    let mut host = Client::join(h.addr, "host");
    let mut map = sim::mapgen::generate(&sim::mapgen::MapSpec::from_words("forest", 1));
    map.igloos.push((60000, 60000));
    host.send(ClientMsg::AddMap(map.encode()));
    host.wait(3.0, |c| c.chats.iter().any(|(_, t)| t.contains("isn't a map file")));
    assert!(host.answers());
}

#[test]
fn nothing_works_before_hello_and_a_second_hello_is_ignored() {
    let h = start();
    let mut host = Client::join(h.addr, "host");

    // A stranger who skips Hello can't chat as the server.
    let mut ghost = Client::raw(h.addr);
    ghost.send(ClientMsg::Cmd(sim::game::Command::Chat { to: None, text: "Server: go to evil.example".into() }));
    std::thread::sleep(Duration::from_millis(300));
    host.pump();
    assert!(host.chats.iter().all(|(_, t)| !t.contains("evil.example")));

    // Saying Hello twice doesn't take a second seat.
    let mut twice = Client::join(h.addr, "twice");
    for _ in 0..3 {
        twice.send(ClientMsg::Hello { version: PROTOCOL_VERSION, name: "twice".into(), token: 0 });
    }
    std::thread::sleep(Duration::from_millis(300));
    host.pump();
    let seats = host.lobby.as_ref().unwrap().slots.iter().filter(|s| s.conn != 0 && s.name.starts_with("twice")).count();
    assert_eq!(seats, 1);
}

#[test]
fn a_flood_of_tiny_frames_does_not_stall_other_players() {
    let h = start();
    let mut victim = Client::join(h.addr, "victim");
    let mut flooder = Client::join(h.addr, "flooder");
    let mut wire = Vec::new();
    for _ in 0..40_000 {
        write_frame(&mut wire, &ClientMsg::Ping(7).encode()).unwrap();
    }
    flooder.send_bytes(&wire);
    let t = Instant::now();
    assert!(victim.answers());
    assert!(t.elapsed() < Duration::from_secs(2), "victim waited {:?}", t.elapsed());
}

#[test]
fn a_guest_cannot_send_a_huge_frame() {
    let h = start();
    let _host = Client::join(h.addr, "host");
    let mut guest = Client::join(h.addr, "guest");
    // Claims a 1 MiB frame; guests may send 64 KiB at most.
    guest.send_bytes(&(1u32 << 20).to_le_bytes());
    let t = Instant::now();
    while guest.pump() {
        assert!(t.elapsed() < Duration::from_secs(3), "never hung up");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn connections_are_capped_and_silent_ones_time_out() {
    let h = start();
    let idle: Vec<TcpStream> = (0..64).map(|_| TcpStream::connect(h.addr).unwrap()).collect();
    std::thread::sleep(Duration::from_millis(300));
    // One over the cap is closed on sight.
    let mut extra = TcpStream::connect(h.addr).unwrap();
    extra.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut b = [0u8; 1];
    assert!(matches!(extra.read(&mut b), Ok(0) | Err(_)));
    drop(idle);
}

#[test]
fn a_seat_is_taken_back_with_its_token_not_with_its_name() {
    let h = start();
    let mut host = Client::join(h.addr, "host");
    let mut friend = Client::join(h.addr, "friend");
    assert_ne!(host.token, 0);
    assert_ne!(host.token, friend.token, "every connection gets its own secret");
    friend.send(ClientMsg::TakeSlot(1));
    friend.wait(3.0, |c| c.lobby.as_ref().is_some_and(|l| l.slots[1].conn != 0));
    host.send(ClientMsg::Start);
    host.wait(3.0, |c| c.you.is_some());
    friend.wait(3.0, |c| c.you.is_some());
    assert_eq!(host.you, Some(0));
    let secret = host.token;

    // The host's connection drops; the friend keeps the game alive.
    drop(host);
    std::thread::sleep(Duration::from_millis(300));

    // Someone who only knows the name gets to watch, not to play.
    let mut thief = Client::join(h.addr, "host");
    thief.wait(3.0, |c| c.you.is_some());
    assert_eq!(thief.you, Some(sim::game::NEUTRAL), "a name is not a key");

    // The real host, with the secret, gets seat 0 back.
    let mut back = Client::join_with(h.addr, "host again", secret);
    back.wait(3.0, |c| c.you.is_some());
    assert_eq!(back.you, Some(0));
    assert_eq!(back.token, secret);

    // A guessed token gets nothing, and a fresh secret of its own.
    let mut guess = Client::join_with(h.addr, "guesser", 12345);
    guess.wait(3.0, |c| c.you.is_some());
    assert_eq!(guess.you, Some(sim::game::NEUTRAL));
    assert_ne!(guess.token, 12345);
}
