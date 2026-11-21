//! End-to-end: real server, real sockets, two clients and an AI.

use std::io::Write;
use std::net::TcpStream;
use std::time::{Duration, Instant};

use sim::ai::AiLevel;
use sim::game::{Command, Kind};
use sim::proto::*;

struct TestClient {
    s: TcpStream,
    r: FrameReader,
    world: Option<WorldView>,
    lobby: Option<LobbyState>,
    chats: Vec<String>,
    /// The computer players' thoughts received so far (spectators only).
    notes: Vec<Vec<(u8, Vec<String>)>>,
}

impl TestClient {
    fn connect(addr: std::net::SocketAddr, name: &str) -> TestClient {
        let s = TcpStream::connect(addr).unwrap();
        s.set_nonblocking(true).unwrap();
        let mut c = TestClient { s, r: FrameReader::default(), world: None, lobby: None, chats: vec![], notes: vec![] };
        c.send(ClientMsg::Hello { version: PROTOCOL_VERSION, name: name.into(), token: 0 });
        c
    }
    fn send(&mut self, m: ClientMsg) {
        let mut f = Vec::new();
        write_frame(&mut f, &m.encode()).unwrap();
        self.s.set_nonblocking(false).unwrap();
        self.s.write_all(&f).unwrap();
        self.s.set_nonblocking(true).unwrap();
    }
    fn pump(&mut self) {
        self.r.fill(&mut self.s).unwrap();
        while let Some(f) = self.r.next().unwrap() {
            match ServerMsg::decode(&f).unwrap() {
                ServerMsg::Lobby(l) => self.lobby = Some(l),
                ServerMsg::GameStart(info) => self.world = Some(WorldView::new(*info)),
                ServerMsg::Snapshot(b) => {
                    self.world.as_mut().unwrap().apply(&b).unwrap();
                }
                ServerMsg::Chat { from, text, .. } => self.chats.push(format!("{from}: {text}")),
                ServerMsg::AiNotes(n) => self.notes.push(n),
                _ => {}
            }
        }
    }
    fn wait(&mut self, secs: f32, mut f: impl FnMut(&TestClient) -> bool) {
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
}

#[test]
fn lobby_and_game() {
    let cfg = server::ServerConfig {
        name: "test".into(),
        bind: "127.0.0.1:0".parse().unwrap(),
        maps_dir: server::maps_dir(&server::find_assets()),
        user_maps_dir: None,
        announce: false,
        stop_with_host: true,
        web_dir: None,
        ai_notes_for_players: false,
    };
    let h = server::spawn(cfg).unwrap();
    let mut a = TestClient::connect(h.addr, "alice");
    a.wait(2.0, |c| c.lobby.is_some());
    let mut b = TestClient::connect(h.addr, "bob");
    b.wait(2.0, |c| c.lobby.as_ref().is_some_and(|l| l.slots.iter().filter(|s| s.kind == SlotKind::Human).count() == 2));
    a.send(ClientMsg::SetSlot { slot: 2, kind: SlotKind::Ai(AiLevel::Easy) });
    a.send(ClientMsg::Start);
    a.wait(2.0, |c| c.world.as_ref().is_some_and(|w| !w.penguins.is_empty()));
    b.wait(2.0, |c| c.world.as_ref().is_some_and(|w| !w.penguins.is_empty()));

    let me = a.world.as_ref().unwrap().info.you;
    let mine: Vec<u16> = a.world.as_ref().unwrap().penguins.iter()
        .filter(|p| p.owner == me && p.kind == Kind::Chieftain).map(|p| p.id).collect();
    let start = a.world.as_ref().unwrap().penguins[mine[0] as usize].pos;
    let target = a.world.as_ref().unwrap().info.map.w as f32 / 2.0;
    a.send(ClientMsg::Cmd(Command::Move { units: mine.clone(), target: sim::math::v2(target, target), run: true, queue: false }));
    a.send(ClientMsg::Cmd(Command::Chat { to: None, text: "hello bots".into() }));
    a.wait(3.0, |c| c.world.as_ref().unwrap().penguins[mine[0] as usize].pos.dist(start) > 2.0);
    b.wait(3.0, |c| c.world.as_ref().unwrap().penguins[mine[0] as usize].pos.dist(start) > 2.0);
    b.wait(3.0, |c| c.chats.iter().any(|m| m.contains("hello bots")));
    let (wa, wb) = (a.world.as_ref().unwrap(), b.world.as_ref().unwrap());
    println!("tick a {} b {}, chats {:?}", wa.tick, wb.tick, b.chats);
    assert_eq!(wa.penguins.len(), wb.penguins.len());
}

#[test]
fn only_spectators_hear_what_the_computers_think() {
    let cfg = server::ServerConfig {
        name: "test".into(),
        bind: "127.0.0.1:0".parse().unwrap(),
        maps_dir: server::maps_dir(&server::find_assets()),
        user_maps_dir: None,
        announce: false,
        stop_with_host: true,
        web_dir: None,
        ai_notes_for_players: false,
    };
    let h = server::spawn(cfg).unwrap();
    let mut a = TestClient::connect(h.addr, "alice");
    a.wait(2.0, |c| c.lobby.is_some());
    let slots = a.lobby.as_ref().unwrap().slots.len() as u8;
    let mut b = TestClient::connect(h.addr, "bob");
    b.wait(2.0, |c| c.lobby.as_ref().is_some_and(|l| l.slots.iter().filter(|s| s.kind == SlotKind::Human).count() == 2));
    for slot in 2..slots {
        a.send(ClientMsg::SetSlot { slot, kind: SlotKind::Ai(AiLevel::Easy) });
    }
    // Every seat is taken, so the third person only watches.
    let mut c = TestClient::connect(h.addr, "carol");
    c.wait(2.0, |c| c.lobby.as_ref().is_some_and(|l| l.spectators.iter().any(|n| n == "carol")));
    a.send(ClientMsg::Start);
    c.wait(10.0, |c| c.world.is_some() && !c.notes.is_empty());
    a.wait(3.0, |c| c.world.is_some());
    b.wait(3.0, |c| c.world.is_some());
    // A few more seconds of play: the watcher keeps hearing, the players never do.
    let heard = c.notes.len();
    c.wait(5.0, |c| c.notes.len() > heard + 2);
    a.pump();
    b.pump();
    assert!(a.notes.is_empty() && b.notes.is_empty(), "players were told what the computers think");
    let first = &c.notes[0];
    assert!(!first.is_empty(), "an entry for each computer player");
    assert!(first[0].1[0].contains("level"), "{:?}", first[0].1);
}

#[test]
fn players_hear_what_the_computers_think_when_the_server_shares_it() {
    let cfg = server::ServerConfig {
        name: "test".into(),
        bind: "127.0.0.1:0".parse().unwrap(),
        maps_dir: server::maps_dir(&server::find_assets()),
        user_maps_dir: None,
        announce: false,
        stop_with_host: true,
        web_dir: None,
        ai_notes_for_players: true,
    };
    let h = server::spawn(cfg).unwrap();
    let mut a = TestClient::connect(h.addr, "alice");
    a.wait(2.0, |c| c.lobby.is_some());
    let slots = a.lobby.as_ref().unwrap().slots.len() as u8;
    for slot in 1..slots {
        a.send(ClientMsg::SetSlot { slot, kind: SlotKind::Ai(AiLevel::Easy) });
    }
    a.send(ClientMsg::Start);
    a.wait(10.0, |c| c.world.is_some() && !c.notes.is_empty());
    assert!(!a.notes.is_empty(), "the player was shown what the computers think");
    assert!(a.notes[0][0].1[0].contains("level"), "{:?}", a.notes[0]);
}
