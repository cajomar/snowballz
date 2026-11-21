//! A big battle: eight tribes, 255 snowballers each, on a big open map.
//!
//! Run with `cargo test -p sim --release --test scale -- --ignored --nocapture`.

use sim::game::{Command, Event, Game, Kind, PlayerSetup};
use sim::mapgen::{generate, Layout, MapSpec};
use sim::math::v2;

const PLAYERS: usize = 8;
const PER_PLAYER: u8 = 255;
const SECONDS: u32 = 40;

#[test]
#[ignore = "slow benchmark: run with --ignored --nocapture to see the numbers"]
fn four_hundred_penguin_battle() {
    // The biggest map, two lines of igloos facing each other over open snow.
    let spec = MapSpec {
        name: "Big Battle".into(),
        size: 160,
        igloos: PLAYERS,
        layout: Layout::Sides,
        hills: 0.2,
        ice: 0.1,
        trees: 0.1,
        spires: 0.05,
        workers: 0,
        snowballers: 0,
        ..MapSpec::default()
    };
    let setups = (0..PLAYERS)
        .map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None })
        .collect();
    let mut g = Game::new_with(generate(&spec), setups, 7, sim::game::CoreRules::default().map_start());
    // A start has few soldiers under armies, so give each tribe its army.
    for i in 0..PLAYERS {
        for _ in 0..PER_PLAYER {
            g.spawn_for_test(i as u8, Kind::Snowballer);
        }
    }
    // Every igloo also has its Chieftain.
    assert_eq!(g.penguins.iter().filter(|p| p.kind == Kind::Snowballer).count(), PLAYERS * PER_PLAYER as usize);

    // March every tribe on the middle, so they all meet and fight.
    let centre = v2(g.map.w as f32 / 2.0, g.map.h as f32 / 2.0);
    for player in 0..PLAYERS as u8 {
        let units: Vec<u16> = g.penguins.iter().filter(|p| p.owner == player && p.kind == Kind::Snowballer).map(|p| p.id).collect();
        assert_eq!(units.len(), PER_PLAYER as usize, "player {player}");
        g.command(player, Command::Move { units, target: centre, run: false, queue: false });
    }

    let ticks = SECONDS * sim::TICK_RATE;
    let (mut total, mut worst, mut thrown, mut frozen) = (0u128, 0u128, 0u32, 0u32);
    let mut enc = sim::proto::SnapshotEncoder::new();
    let (mut bytes, mut biggest) = (0usize, 0usize);
    let start = std::time::Instant::now();
    for _ in 0..ticks {
        let t = std::time::Instant::now();
        g.tick();
        let us = t.elapsed().as_micros();
        total += us;
        worst = worst.max(us);
        let events = std::mem::take(&mut g.events);
        let snap = enc.encode(&g, &events).len();
        bytes += snap;
        biggest = biggest.max(snap);
        for e in events {
            match e {
                Event::Thrown(_) => thrown += 1,
                Event::Frozen { .. } => frozen += 1,
                _ => {}
            }
        }
    }
    let wall = start.elapsed();
    let average = total as f64 / ticks as f64;
    println!(
        "2040-penguin battle: {SECONDS}s game in {wall:.2?}, average tick {:.3} ms ({average:.1} us), worst {worst} us",
        average / 1000.0
    );
    println!(
        "  {} penguins alive, {} snowballs thrown, {} frozen; {} ticks at {} Hz",
        g.penguins.len(),
        thrown,
        frozen,
        ticks,
        sim::TICK_RATE
    );
    println!("  snapshots: {} bytes on average, {} at most ({} KB/s at 20 Hz)", bytes / ticks as usize, biggest, bytes / SECONDS as usize / 1024);
    assert!(thrown > 0, "the tribes never met");
}
