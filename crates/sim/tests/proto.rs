//! Protocol: commands and events must survive the wire,
//! from the client's `ClientMsg` to the server, and from the server's
//! snapshots back to a `WorldView`.

use sim::ai::AiLevel;
use sim::game::*;
use sim::map::Map;
use sim::mapgen::{generate, Layout, MapSpec};
use sim::math::v2;
use sim::proto::{ClientMsg, GameInfo, ServerMsg, SlotKind, SnapshotEncoder, WorldView};
use sim::rng::Rng;

fn game() -> Game {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    Game::new_with(map, setups, 7, CoreRules::default().map_start())
}

fn home(g: &Game, owner: u8) -> u8 {
    g.igloos.iter().position(|i| i.owner == owner).unwrap() as u8
}

#[test]
fn capture_command_round_trips() {
    // A capture order's unit list and penguin id come back exactly as sent.
    let msg = ClientMsg::Cmd(Command::Capture { units: vec![3, 14, 42], penguin: 7 });
    let decoded = ClientMsg::decode(&msg.encode()).unwrap();
    let ClientMsg::Cmd(Command::Capture { units, penguin }) = decoded else {
        panic!("not a Capture command");
    };
    assert_eq!(units, vec![3, 14, 42]);
    assert_eq!(penguin, 7);
}

#[test]
fn game_info_carries_the_tech_switch() {
    // The client must learn whether igloo tiers and research are on.
    for tech in [false, true] {
        let mut g = game();
        g.core.tech = tech;
        let bytes = ServerMsg::GameStart(Box::new(GameInfo::from_game(&g, 0))).encode();
        let info = match ServerMsg::decode(&bytes).unwrap() {
            ServerMsg::GameStart(info) => *info,
            _ => panic!("expected GameStart"),
        };
        assert_eq!(info.tech, tech);
    }
}

#[test]
fn big_battle_snapshot_fits_the_budget() {
    // The T312 setup: four tribes of 100 snowballers on the biggest map.
    // Ten seconds in, a fresh client's first snapshot holds everything,
    // so this is the worst case the wire has to carry.
    let spec = MapSpec {
        name: "Big Battle".into(),
        size: 160,
        igloos: 4,
        layout: Layout::Sides,
        hills: 0.2,
        ice: 0.1,
        trees: 0.1,
        spires: 0.05,
        workers: 0,
        snowballers: 100,
        ..MapSpec::default()
    };
    let setups = (0..4)
        .map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None })
        .collect();
    let mut g = Game::new_with(generate(&spec), setups, 7, CoreRules::default().map_start());
    assert!(g.penguins.len() >= 400);

    // March everyone on the middle, so positions and orders are all live.
    let centre = v2(g.map.w as f32 / 2.0, g.map.h as f32 / 2.0);
    for player in 0..4u8 {
        let units: Vec<u16> = g.penguins.iter().filter(|p| p.owner == player).map(|p| p.id).collect();
        g.command(player, Command::Move { units, target: centre, run: false, queue: false });
    }
    for _ in 0..10 * sim::TICK_RATE {
        g.tick();
    }

    let mut enc = SnapshotEncoder::new();
    let bytes = enc.encode(&g, &[]);
    println!("400-penguin snapshot after 10s: {} bytes ({} KiB)", bytes.len(), bytes.len() / 1024);
    assert!(bytes.len() < 64 * 1024, "snapshot is {} bytes, over 64 KiB", bytes.len());

    // And it must still decode whole.
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    view.apply(&bytes).expect("snapshot applies");
}

// -------------------------------------------------------------------------
// Fuzz: the decoders must never panic on garbage, and a truncated real
// message or snapshot must be an error, not a partial decode.

fn random_bytes(rng: &mut Rng, len: usize) -> Vec<u8> {
    (0..len).map(|_| rng.below(256) as u8).collect()
}

/// Every `ClientMsg` variant, with payloads that use each field type.
fn all_client_msgs() -> Vec<ClientMsg> {
    let u = vec![1u16, 2, 3];
    vec![
        ClientMsg::Hello { version: 1, name: "fuzz".into(), token: 7 },
        ClientMsg::SetMap(0),
        ClientMsg::SetSlot { slot: 0, kind: SlotKind::Open },
        ClientMsg::SetSlot { slot: 1, kind: SlotKind::Ai(AiLevel::Hard) },
        ClientMsg::SetTeam { slot: 0, team: 1 },
        ClientMsg::SetColor { slot: 0, color: 2 },
        ClientMsg::TakeSlot(0),
        ClientMsg::Start,
        ClientMsg::Ping(42),
        ClientMsg::BackToLobby,
        ClientMsg::Cmd(Command::Move { units: u.clone(), target: v2(1.5, -2.25), run: true, queue: false }),
        ClientMsg::Cmd(Command::Move { units: vec![], target: v2(0.0, 0.0), run: false, queue: false }),
        ClientMsg::Cmd(Command::Enter { units: u.clone(), igloo: 3 }),
        ClientMsg::Cmd(Command::Gather { units: u.clone(), resource: 7 }),
        ClientMsg::Cmd(Command::Chop { units: u.clone(), tile: (3, -5) }),
        ClientMsg::Cmd(Command::Build { units: u.clone(), kind: BuildKind::Tower, tiles: vec![(0, 1), (2, 3)] }),
        ClientMsg::Cmd(Command::Build { units: u.clone(), kind: BuildKind::Palisade, tiles: vec![(4, 5)] }),
        ClientMsg::Cmd(Command::Build { units: u.clone(), kind: BuildKind::Palisade, tiles: vec![(6, 7)] }),
        ClientMsg::Cmd(Command::Build { units: u.clone(), kind: BuildKind::Sauna, tiles: vec![] }),
        ClientMsg::Cmd(Command::Repair { units: u.clone(), building: 4 }),
        ClientMsg::Cmd(Command::Demolish { building: 5 }),
        ClientMsg::Cmd(Command::Capture { units: u.clone(), penguin: 6 }),
        ClientMsg::Cmd(Command::Release { igloo: 3 }),
        ClientMsg::Cmd(Command::Stop { units: u }),
        ClientMsg::Cmd(Command::WarCry { unit: 9 }),
        ClientMsg::Cmd(Command::SetRally { building: 8, at: sim::math::v2(12.5, 30.0) }),
        ClientMsg::Cmd(Command::Fish { units: vec![3, 14], tile: (60, 24) }),
        ClientMsg::Cmd(Command::Concede),
        ClientMsg::Cmd(Command::Tend { units: vec![5], building: 9 }),
        ClientMsg::Cmd(Command::Garrison { units: vec![2, 8, 11], building: 4 }),
        ClientMsg::Cmd(Command::Chat { to: Some(1), text: "fuzz".into() }),
        ClientMsg::Cmd(Command::Chat { to: None, text: "a rather long chat message, for good measure".into() }),
    ]
}

#[test]
fn random_client_msgs_never_panic() {
    // Random garbage is either a valid message or an error, never a panic.
    let mut rng = Rng::new(0xF177);
    for _ in 0..2000 {
        let len = rng.below(128) as usize;
        let buf = random_bytes(&mut rng, len);
        let _ = ClientMsg::decode(&buf);
    }
}

#[test]
fn truncated_client_msgs_are_errors() {
    // A real message has no trailing bytes, so every strict prefix is
    // corrupt and must fail to decode.
    for (i, msg) in all_client_msgs().into_iter().enumerate() {
        let full = msg.encode();
        assert!(ClientMsg::decode(&full).is_ok(), "message {i} decodes");
        for k in 0..full.len() {
            assert!(
                ClientMsg::decode(&full[..k]).is_err(),
                "message {i}: prefix of length {k} decoded"
            );
        }
    }
}

#[test]
fn random_snapshots_never_panic() {
    // One view sees the whole stream, so its vectors can only grow once.
    let g = game();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    let mut rng = Rng::new(0xF178);
    for _ in 0..2000 {
        let len = rng.below(128) as usize;
        let buf = random_bytes(&mut rng, len);
        let _ = view.apply(&buf);
    }
}

#[test]
fn truncated_snapshots_are_errors() {
    // A real snapshot applies whole, but every strict prefix is corrupt.
    let mut g = game();
    for _ in 0..10 {
        g.tick();
    }
    let mut enc = SnapshotEncoder::new();
    let full = enc.encode(&g, &[]);
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    assert!(view.apply(&full).is_ok(), "the full snapshot applies");
    for k in 0..full.len() {
        assert!(
            view.apply(&full[..k]).is_err(),
            "snapshot prefix of length {k} applied"
        );
    }
}

/// Snapshots only carry the fields that changed, but a client that applies
/// every one of them must still see exactly what the game holds.
#[test]
fn penguin_deltas_rebuild_every_penguin() {
    let mut g = game();
    let (a, b) = (home(&g, 0), home(&g, 1));
    let (ca, cb) = (g.igloos[a as usize].center, g.igloos[b as usize].center);
    let army = |g: &Game, owner: u8| -> Vec<u16> { g.penguins.iter().filter(|p| p.owner == owner).map(|p| p.id).collect() };
    let (mine, theirs) = (army(&g, 0), army(&g, 1));
    g.command(0, Command::Move { units: mine, target: cb, run: true, queue: false });
    g.command(1, Command::Move { units: theirs, target: ca, run: true, queue: false });

    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    let (mut first, mut later, mut ticks) = (0, 0, 0);
    for t in 0..20 * 40 {
        g.tick();
        let events = std::mem::take(&mut g.events);
        let snap = enc.encode(&g, &events);
        if t == 0 {
            first = snap.len();
        } else {
            later += snap.len();
            ticks += 1;
        }
        view.apply(&snap).unwrap();
        for p in &g.penguins {
            let v = &view.penguins[p.id as usize];
            let ctx = format!("penguin {} at tick {}", p.id, g.tick);
            assert!((v.pos.x - p.pos.x).abs() < 0.02 && (v.pos.y - p.pos.y).abs() < 0.02, "{ctx}: position {:?} vs {:?}", v.pos, p.pos);
            assert_eq!((v.act, v.kind, v.owner, v.home), (p.act, p.kind, p.owner, p.home), "{ctx}");
            assert_eq!((v.warmth, v.snowballs, v.carry, v.cold, v.courage), (p.warmth, p.snowballs, p.carry, p.cold, p.courage), "{ctx}");
        }
    }
    // Deltas have to be much smaller than a full snapshot for the whole army.
    assert!(later / ticks < first / 2, "a delta averaged {} bytes, the first snapshot {first}", later / ticks);
}

#[test]
fn what_the_computers_think_survives_the_wire() {
    let notes = vec![(1u8, vec!["AI 1 - Hard level".to_string(), "5:02 Hears Ann: \"truce?\"".to_string()]), (3u8, vec![])];
    let bytes = ServerMsg::AiNotes(notes.clone()).encode();
    match ServerMsg::decode(&bytes).unwrap() {
        ServerMsg::AiNotes(back) => assert_eq!(back, notes),
        _ => panic!("wrong message"),
    }
    // Too much is cut down rather than refused.
    let long = vec![(0u8, (0..100).map(|i| "x".repeat(500) + &i.to_string()).collect::<Vec<_>>())];
    match ServerMsg::decode(&ServerMsg::AiNotes(long).encode()).unwrap() {
        ServerMsg::AiNotes(back) => {
            assert_eq!(back[0].1.len(), sim::proto::AI_NOTE_LINES);
            assert!(back[0].1.iter().all(|l| l.chars().count() <= sim::proto::AI_NOTE_WIDTH));
        }
        _ => panic!("wrong message"),
    }
}
