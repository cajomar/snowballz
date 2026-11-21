//! Times one tick of a full core-rules game on the biggest map.
//!
//! Not run by default: `cargo test --release -p sim --test bench -- --ignored --nocapture`.

use sim::ai::AiLevel;
use sim::game::{Game, PlayerSetup};
use sim::map::Map;

/// The biggest map in assets/maps/.
const BIG_MAP: &str = "the-long-white";

#[test]
#[ignore]
fn bench_core_game() {
    let path = format!("{}/../../assets/maps/{BIG_MAP}.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..4)
        .map(|i| PlayerSetup {
            name: format!("AI {i}"),
            color: i as u8,
            team: i as u8,
            ai: Some(if i % 2 == 0 { AiLevel::Normal } else { AiLevel::Hard }),
        })
        .collect();
    // SB_SEED=n tries other games.
    let seed = std::env::var("SB_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(42);
    let mut g = Game::new_with(map, setups, seed, sim::game::CoreRules::default());

    let ticks = 5 * 60 * sim::TICK_RATE;
    let mut total = 0.0f64;
    let mut worst = 0.0f64;
    for _ in 0..ticks {
        let t = std::time::Instant::now();
        g.tick();
        let us = t.elapsed().as_secs_f64() * 1e6;
        total += us;
        if us > worst {
            worst = us;
        }
        std::mem::take(&mut g.events);
    }
    let mean = total / ticks as f64;
    println!(
        "{BIG_MAP}: 5 game minutes ({} ticks), {} penguins: mean {mean:.1} us/tick, worst {worst:.1} us/tick",
        ticks, g.penguins.len()
    );
}

/// Same game as `bench_core_game`, but prints one line per game minute.
/// Slowdowns as the battle grows show up in the mean us/tick.
#[test]
#[ignore]
fn bench_per_minute() {
    let path = format!("{}/../../assets/maps/{BIG_MAP}.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..4)
        .map(|i| PlayerSetup {
            name: format!("AI {i}"),
            color: i as u8,
            team: i as u8,
            ai: Some(if i % 2 == 0 { AiLevel::Normal } else { AiLevel::Hard }),
        })
        .collect();
    // SB_SEED=n tries other games.
    let seed = std::env::var("SB_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(42);
    let mut g = Game::new_with(map, setups, seed, sim::game::CoreRules::default());

    for minute in 1..=5 {
        let mut total = 0.0f64;
        for _ in 0..sim::TICK_RATE {
            let t = std::time::Instant::now();
            g.tick();
            total += t.elapsed().as_secs_f64() * 1e6;
            std::mem::take(&mut g.events);
        }
        println!(
            "{BIG_MAP} minute {minute}: {} penguins, {} snowballs: mean {:.1} us/tick",
            g.penguins.len(),
            g.snowballs.len(),
            total / sim::TICK_RATE as f64
        );
    }
}

/// A long game from the small start, as played: four computers on the
/// biggest map for SB_MINUTES (20) minutes. Prints per minute the tribe
/// sizes, the mean and worst tick, and the snapshot one player gets (with
/// fog) so the late game's cost shows.
#[test]
#[ignore]
fn bench_long_game() {
    let path = format!("{}/../../assets/maps/{BIG_MAP}.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..4)
        .map(|i| PlayerSetup { name: format!("AI {i}"), color: i as u8, team: i as u8, ai: Some(AiLevel::Hard) })
        .collect();
    let seed = std::env::var("SB_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(42);
    let minutes: u32 = std::env::var("SB_MINUTES").ok().and_then(|s| s.parse().ok()).unwrap_or(20);
    let mut g = Game::new_with(map, setups, seed, sim::game::CoreRules::default());
    let mut enc = sim::proto::SnapshotEncoder::new();
    for minute in 1..=minutes {
        let (mut total, mut worst, mut fog_us, mut enc_us) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        let (mut bytes, mut biggest) = (0usize, 0usize);
        let ticks = 60 * sim::TICK_RATE;
        for _ in 0..ticks {
            let t = std::time::Instant::now();
            g.tick();
            let us = t.elapsed().as_secs_f64() * 1e6;
            total += us;
            worst = worst.max(us);
            let events = std::mem::take(&mut g.events);
            let t = std::time::Instant::now();
            let fov = sim::fog::Fov::compute(&g, g.team(0));
            fog_us += t.elapsed().as_secs_f64() * 1e6;
            let t = std::time::Instant::now();
            let n = enc.encode_for(&g, &events, Some(&fov)).len();
            enc_us += t.elapsed().as_secs_f64() * 1e6;
            bytes += n;
            biggest = biggest.max(n);
        }
        let alive: Vec<usize> = (0..4u8).map(|p| g.penguins.iter().filter(|q| q.owner == p).count()).collect();
        let t = ticks as f64;
        println!(
            "minute {minute:2}: {alive:?} = {} penguins ({} records), {} snowballs; tick mean {:.0} us worst {:.0} us; fog {:.0} us, encode {:.0} us; snapshot mean {} B max {} B ({} KB/s)",
            alive.iter().sum::<usize>(),
            g.penguins.len(),
            g.snowballs.len(),
            total / t,
            worst,
            fog_us / t,
            enc_us / t,
            bytes / ticks as usize,
            biggest,
            bytes / 60 / 1024,
        );
    }
}
