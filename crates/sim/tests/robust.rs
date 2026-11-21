//! Hostile input: nothing a client or a map file sends may panic the
//! server or make it slow for everyone else.

use sim::game::*;
use sim::map::Map;
use sim::proto::{write_frame, FrameReader};

fn map() -> Map {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    Map::load_file(std::path::Path::new(&path)).unwrap()
}

fn game() -> Game {
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    Game::new(map(), setups, 7)
}

#[test]
fn private_chat_to_a_missing_seat_is_dropped() {
    let mut g = game();
    g.command(0, Command::Chat { to: Some(9), text: "hello?".into() });
    g.command(0, Command::Chat { to: Some(1), text: "hi".into() });
    g.tick();
    let chats: Vec<_> = g.events.iter().filter(|e| matches!(e, Event::Chat { .. })).collect();
    assert_eq!(chats.len(), 1, "only the message to a real seat is sent");
}

#[test]
fn chat_loses_control_characters() {
    let mut g = game();
    g.command(0, Command::Chat { to: None, text: "a\u{7}b\nc".into() });
    g.tick();
    let text = g.events.iter().find_map(|e| if let Event::Chat { text, .. } = e { Some(text.clone()) } else { None });
    assert_eq!(text.as_deref(), Some("abc"));
}

#[test]
fn maps_with_things_off_the_grid_are_refused() {
    let mut m = map();
    assert!(Map::parse(&m.encode()).is_ok());
    m.igloos.push((60000, 60000));
    assert!(Map::parse(&m.encode()).is_err(), "igloo far off the map");

    let mut m = map();
    let (w, h) = (m.w as u16, m.h as u16);
    m.igloos.push((w - 2, h - 2)); // starts inside the map but its 4x4 body doesn't
    assert!(Map::parse(&m.encode()).is_err());

    let mut m = map();
    m.powerups.push((0, w, 0));
    assert!(Map::parse(&m.encode()).is_err(), "powerup past the right edge");

    let mut m = map();
    m.rules.snowballer_warmth = 255;
    assert!(Map::parse(&m.encode()).is_err(), "warmth that would overflow a byte");

    let mut m = map();
    m.rules.workers = 255;
    m.rules.snowballers = 255;
    m.igloos = vec![(5, 5); 30];
    assert!(Map::parse(&m.encode()).is_err(), "more penguins than ids");
}

#[test]
fn a_flood_of_tiny_frames_is_read_in_linear_time() {
    let mut wire = Vec::new();
    for i in 0..300_000u32 {
        write_frame(&mut wire, &i.to_le_bytes()).unwrap();
    }
    let started = std::time::Instant::now();
    let mut r = FrameReader::default();
    r.push(&wire);
    let mut n = 0u32;
    while let Some(f) = r.next().unwrap() {
        assert_eq!(f, n.to_le_bytes());
        n += 1;
    }
    assert_eq!(n, 300_000);
    assert!(started.elapsed().as_secs() < 2, "took {:?}", started.elapsed());
}

#[test]
fn frames_split_across_reads_still_join_up() {
    let mut wire = Vec::new();
    write_frame(&mut wire, b"hello").unwrap();
    write_frame(&mut wire, b"world!").unwrap();
    let mut r = FrameReader::default();
    let mut got = Vec::new();
    for b in &wire {
        r.push(&[*b]);
        while let Some(f) = r.next().unwrap() {
            got.push(f);
        }
    }
    assert_eq!(got, vec![b"hello".to_vec(), b"world!".to_vec()]);
}

#[test]
fn events_that_point_outside_the_view_are_dropped() {
    use sim::proto::{GameInfo, SnapshotEncoder, WorldView};
    let g = game();
    let (mut enc, mut view) = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 0)));
    let events = vec![
        Event::Captured { igloo: 200, from: 0, to: 1 },
        Event::Collapsed { igloo: 99, owner: 0 },
        Event::Frozen { penguin: 60000 },
        Event::Placed { building: 5000 },
        Event::Captured { igloo: 0, from: 0, to: 1 },
    ];
    let bytes = enc.encode(&g, &events);
    let got = view.apply(&bytes).unwrap();
    assert_eq!(got.len(), 1, "only the event naming a real igloo survives: {got:?}");
    assert!(matches!(got[0], Event::Captured { igloo: 0, .. }));
}

/// Commands built from random numbers: ids and tiles are as often wrong as
/// right. Whatever a client sends, the game must carry on.
fn random_command(rng: &mut sim::rng::Rng, g: &Game) -> Command {
    let n = g.penguins.len() as u32 + 5;
    let units = |rng: &mut sim::rng::Rng| (0..rng.below(5)).map(|_| rng.below(n) as u16).collect::<Vec<u16>>();
    let tile = |rng: &mut sim::rng::Rng| (rng.below(g.map.w as u32 + 40) as i32 - 20, rng.below(g.map.h as u32 + 40) as i32 - 20);
    let at = |rng: &mut sim::rng::Rng| sim::math::v2(rng.range(-50.0, g.map.w as f32 + 50.0), rng.range(-50.0, g.map.h as f32 + 50.0));
    use sim::game::BuildKind;
    match rng.below(27) {
        0 => Command::Move { units: units(rng), target: at(rng), run: rng.chance(0.5), queue: rng.chance(0.5) },
        1 => Command::Enter { units: units(rng), igloo: rng.below(12) as u8 },
        2 => Command::Gather { units: units(rng), resource: rng.below(200) as u16 },
        3 => Command::Chop { units: units(rng), tile: tile(rng) },
        4 => Command::Build { units: units(rng), kind: BuildKind::from_u8(rng.below(18) as u8), tiles: (0..rng.below(6)).map(|_| tile(rng)).collect() },
        5 => Command::Repair { units: units(rng), building: rng.below(80) as u16 },
        6 => Command::Demolish { building: rng.below(80) as u16 },
        7 => Command::Capture { units: units(rng), penguin: rng.below(n) as u16 },
        8 => Command::Release { igloo: rng.below(12) as u8 },
        9 => Command::Stop { units: units(rng) },
        10 => Command::Hold { units: units(rng) },
        14 => Command::Upgrade { igloo: rng.below(12) as u8 },
        15 => Command::StartResearch { igloo: rng.below(12) as u8, branch: rng.below(5) as u8 },
        16 => Command::WarCry { unit: rng.below(n) as u16 },
        17 => Command::Thaw { units: units(rng), penguin: rng.below(n) as u16 },
        19 => Command::SetRally { building: rng.below(80) as u16, at: at(rng) },
        20 => Command::Fish { units: units(rng), tile: tile(rng) },
        21 => Command::Tend { units: units(rng), building: rng.below(80) as u16 },
        25 => Command::Concede,
        22 => Command::Garrison { units: units(rng), building: rng.below(80) as u16 },
        _ => Command::Chat { to: rng.chance(0.5).then(|| rng.below(14) as u8), text: "x".repeat(rng.below(300) as usize) },
    }
}

#[test]
fn random_commands_never_panic_the_game() {
    for (seed, core) in [(1, CoreRules::default().map_start()), (2, CoreRules::default()), (3, CoreRules::default())] {
        let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
        let mut g = Game::new_with(map(), setups, seed, core);
        let mut rng = sim::rng::Rng::new(seed * 77);
        for _ in 0..600 {
            for _ in 0..8 {
                let c = random_command(&mut rng, &g);
                g.command(rng.below(3) as u8, c); // player 2 doesn't exist
            }
            g.tick();
            g.events.clear();
        }
    }
}
