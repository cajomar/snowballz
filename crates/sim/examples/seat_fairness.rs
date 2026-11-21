//! Does every starting seat have the same chance? Plays each shipped map
//! with Hard computers in every seat over several seeds and tallies, per
//! seat, how long its tribe lasted and how many penguins it had left.
//! `cargo run -p sim --release --example seat_fairness [map-name ...]`
//! (`SB_SEEDS=n` seeds per map, `SB_MINUTES=m` game length.)

use sim::ai::AiLevel;
use sim::game::*;
use sim::map::Map;

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps");
    let want: Vec<String> = std::env::args().skip(1).collect();
    let seeds: u64 = std::env::var("SB_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(6);
    let minutes: u32 = std::env::var("SB_MINUTES").ok().and_then(|s| s.parse().ok()).unwrap_or(14);
    let mut files: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "sbmap")).collect();
    files.sort();
    for path in files {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        if !want.is_empty() && !want.contains(&name) {
            continue;
        }
        let map = Map::load_file(&path).unwrap();
        let n = map.igloos.len();
        // Per seat: total share of penguins at the end, and times it was beaten.
        let mut share = vec![0.0f64; n];
        let mut fell = vec![0u32; n];
        let mut wins = vec![0u32; n];
        for seed in 1..=seeds {
            let setups = (0..n).map(|i| PlayerSetup { name: format!("AI {i}"), color: i as u8, team: i as u8, ai: Some(AiLevel::Hard) }).collect();
            let mut g = Game::new_with(map.clone(), setups, seed, CoreRules::default());
            g.roll_ai_tempers(seed);
            for _ in 0..minutes * 60 * sim::TICK_RATE {
                g.tick();
                g.events.clear();
                if g.winner.is_some() {
                    break;
                }
            }
            let mut count = vec![0usize; n];
            for p in g.penguins.iter().filter(|p| p.owner != NEUTRAL && p.act != Act::Frozen) {
                count[p.owner as usize] += 1;
            }
            let total: usize = count.iter().sum::<usize>().max(1);
            for i in 0..n {
                share[i] += count[i] as f64 / total as f64;
                if g.players[i].out {
                    fell[i] += 1;
                }
            }
            if let Some(w) = g.winner {
                wins[w as usize] += 1;
            }
        }
        let even = 1.0 / n as f64;
        let line: Vec<String> = (0..n).map(|i| format!("{:>3.0}%{}", share[i] / seeds as f64 * 100.0, if fell[i] > 0 { format!("/{}f", fell[i]) } else { String::new() })).collect();
        let (lo, hi) = share.iter().fold((f64::MAX, 0.0f64), |(l, h), &s| (l.min(s), h.max(s)));
        println!("{name:<22} {n} seats, even {:>3.0}%: {}   (spread {:.2}x, wins {:?})", even * 100.0, line.join(" "), if lo > 0.0 { hi / lo } else { f64::INFINITY }, wins);
    }
}
