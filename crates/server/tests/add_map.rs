//! A host adds a generated map to the lobby: real server, real sockets.

use std::io::Write;
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use sim::ai::AiLevel;
use sim::mapgen::{self, MapSpec};
use sim::proto::*;

struct TestClient {
    s: TcpStream,
    r: FrameReader,
    world: Option<WorldView>,
    lobby: Option<LobbyState>,
    chats: Vec<String>,
}

impl TestClient {
    fn connect(addr: std::net::SocketAddr, name: &str) -> TestClient {
        let s = TcpStream::connect(addr).unwrap();
        s.set_nonblocking(true).unwrap();
        let mut c = TestClient { s, r: FrameReader::default(), world: None, lobby: None, chats: vec![] };
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
                ServerMsg::Chat { text, .. } => self.chats.push(text),
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

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!("snowballz-{tag}-{}-{nanos}", std::process::id()))
}

fn forest() -> sim::map::Map {
    mapgen::generate(&MapSpec::from_words("forest", 1))
}

#[test]
fn add_map_round_trip() {
    let m = forest();
    let bytes = m.encode();
    let msg = ClientMsg::AddMap(bytes.clone());
    match ClientMsg::decode(&msg.encode()).unwrap() {
        ClientMsg::AddMap(b) => assert_eq!(b, bytes),
        _ => panic!("wrong message"),
    }
}

#[test]
fn oversized_add_map_is_rejected() {
    let mut payload = vec![10u8];
    payload.extend_from_slice(&((2 << 20) as u32 + 1).to_le_bytes());
    payload.extend(std::iter::repeat_n(0, (2 << 20) + 1));
    assert!(ClientMsg::decode(&payload).is_err());
}

#[test]
fn host_adds_map_and_it_plays() {
    let dir = temp_dir("maps");
    let cfg = server::ServerConfig {
        name: "test".into(),
        bind: "127.0.0.1:0".parse().unwrap(),
        maps_dir: server::maps_dir(&server::find_assets()),
        user_maps_dir: Some(dir.clone()),
        announce: false,
        stop_with_host: true,
        web_dir: None,
        ai_notes_for_players: false,
    };
    let h = server::spawn(cfg).unwrap();
    let mut host = TestClient::connect(h.addr, "alice");
    host.wait(2.0, |c| c.lobby.is_some());
    let before = host.lobby.as_ref().unwrap().maps.len();

    // A plain new map: listed, selected, saved under its slug.
    let m = forest();
    let name = m.name.clone();
    host.send(ClientMsg::AddMap(m.encode()));
    host.wait(2.0, |c| c.lobby.as_ref().is_some_and(|l| l.maps.len() == before + 1));
    let l = host.lobby.as_ref().unwrap();
    assert_eq!(l.map as usize, before);
    assert_eq!(l.maps[before].name, name);
    assert!(dir.join(format!("{}.sbmap", name.to_lowercase().replace(|c: char| !c.is_ascii_alphanumeric(), "-"))).is_file());

    // The same map again: the lobby name gets a suffix, and so does the file.
    host.send(ClientMsg::AddMap(m.encode()));
    host.wait(2.0, |c| c.lobby.as_ref().is_some_and(|l| l.maps.len() == before + 2));
    let l = host.lobby.as_ref().unwrap();
    assert_eq!(l.maps[before + 1].name, format!("{name} 2"));
    assert_eq!(l.map as usize, before + 1);
    let saved = std::fs::read_dir(&dir).unwrap().count();
    assert_eq!(saved, 2);

    // A map that can't be played is refused, with the reason.
    let mut bad = forest();
    bad.igloos.truncate(1);
    host.send(ClientMsg::AddMap(bad.encode()));
    host.wait(2.0, |c| c.chats.iter().any(|t| t.contains("needs at least two igloos")));
    assert_eq!(host.lobby.as_ref().unwrap().maps.len(), before + 2);

    // Bytes that aren't a map are refused too.
    host.send(ClientMsg::AddMap(b"not a map".to_vec()));
    host.wait(2.0, |c| c.chats.iter().any(|t| t.contains("isn't a map file")));

    // The added map really plays: fill the seats with computers and start.
    host.send(ClientMsg::AddMap(m.encode()));
    host.wait(2.0, |c| c.lobby.as_ref().is_some_and(|l| l.maps.len() == before + 3));
    let seats = host.lobby.as_ref().unwrap().slots.len();
    for slot in 1..seats {
        host.send(ClientMsg::SetSlot { slot: slot as u8, kind: SlotKind::Ai(AiLevel::Easy) });
    }
    host.send(ClientMsg::Start);
    host.wait(2.0, |c| c.world.as_ref().is_some_and(|w| !w.penguins.is_empty()));

    drop(h);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn only_the_host_can_add_maps() {
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
    let mut host = TestClient::connect(h.addr, "alice");
    host.wait(2.0, |c| c.lobby.is_some());
    let mut guest = TestClient::connect(h.addr, "bob");
    guest.wait(2.0, |c| c.lobby.is_some());
    let before = host.lobby.as_ref().unwrap().maps.len();

    guest.send(ClientMsg::AddMap(forest().encode()));
    std::thread::sleep(Duration::from_millis(200));
    host.pump();
    assert_eq!(host.lobby.as_ref().unwrap().maps.len(), before);
    assert!(!guest.chats.iter().any(|c| c.starts_with("Couldn't add")), "guests aren't even told: {:?}", guest.chats);
}

#[test]
fn map_is_kept_in_memory_without_a_user_dir() {
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
    let mut host = TestClient::connect(h.addr, "alice");
    host.wait(2.0, |c| c.lobby.is_some());
    let before = host.lobby.as_ref().unwrap().maps.len();
    host.send(ClientMsg::AddMap(forest().encode()));
    host.wait(2.0, |c| c.lobby.as_ref().is_some_and(|l| l.maps.len() == before + 1));
    let seats = host.lobby.as_ref().unwrap().slots.len();
    for slot in 1..seats {
        host.send(ClientMsg::SetSlot { slot: slot as u8, kind: SlotKind::Ai(AiLevel::Easy) });
    }
    host.send(ClientMsg::Start);
    host.wait(2.0, |c| c.world.as_ref().is_some_and(|w| !w.penguins.is_empty()));
}
