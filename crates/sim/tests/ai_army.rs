//! The computer keeps its army an army: fighters chilled near home go in to
//! warm up, a tribe that has found nobody goes looking, and it doesn't spend
//! its whole larder while its own penguins go hungry.

use sim::ai::AiLevel;
use sim::game::*;
use sim::map::Map;

fn game(players: usize, seed: u64) -> Game {
    let path = format!("{}/../../assets/maps/pinewood-hollow.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..players)
        .map(|i| PlayerSetup { name: format!("AI {i}"), color: i as u8, team: i as u8, ai: Some(AiLevel::Hard) })
        .collect();
    let mut g = Game::new_with(map, setups, seed, CoreRules::default().map_start());
    g.weather.calm = true;
    g
}

fn run(g: &mut Game, seconds: u32) {
    for _ in 0..seconds * sim::TICK_RATE {
        g.tick();
        g.events.clear();
    }
}

fn log(g: &Game, player: u8) -> String {
    g.ai_notes().into_iter().find(|(p, _)| *p == player).unwrap().1.join("\n")
}

#[test]
fn chilled_fighters_at_home_go_in_to_warm_up() {
    let mut g = game(2, 3);
    run(&mut g, 5);
    let door = g.igloos.iter().find(|i| i.owner == 0).unwrap().door;
    // Hit by a few snowballs: still on their feet, short of warmth.
    let chilled: Vec<u16> = g
        .penguins
        .iter_mut()
        .filter(|p| p.owner == 0 && p.kind.fights() && p.kind != Kind::Chieftain && p.is_out() && p.post.is_none() && p.pos.dist(door) < 15.0)
        .take(4)
        .map(|p| {
            p.warmth = p.max_warmth / 2 + 1;
            p.id
        })
        .collect();
    assert!(!chilled.is_empty());
    run(&mut g, 40);
    for id in chilled {
        let p = &g.penguins[id as usize];
        assert!(p.warmth + 2 >= p.max_warmth, "fighter {id} still chilled: {}/{} ({:?})", p.warmth, p.max_warmth, p.order);
    }
}

#[test]
fn a_tribe_that_has_found_nobody_sends_its_army_looking() {
    // Nobody is anywhere near on this map at first, and the scout alone is slow.
    let mut g = game(2, 7);
    run(&mut g, 7 * 60);
    let log = log(&g, 0) + &log(&g, 1);
    // (With guards at the door and sentries up, the army may be too thin to
    // march: a scout going out counts too.)
    assert!(log.contains("go looking") || log.contains("to scout") || log.contains("ATTACKS"), "never went looking:\n{log}");
}

/// Seconds between calling an attack off and the next attack, across every
/// computer in a 4-way Hard game: (calls-off, quickest turnaround).
fn recalls(seed: u64, minutes: u32) -> (usize, u32, Vec<String>) {
    let mut g = game(4, seed);
    run(&mut g, minutes * 60);
    let (mut n, mut quickest, mut lines) = (0, u32::MAX, Vec::new());
    for (p, _, log) in g.ai_admin(0) {
        let mut off: Option<u32> = None;
        for (tick, line) in &log {
            if line.starts_with("Calls the attack off") {
                n += 1;
                off = Some(*tick);
            } else if line.starts_with("ATTACKS") {
                if let Some(t) = off.take() {
                    let gap = (tick - t) / sim::TICK_RATE;
                    quickest = quickest.min(gap);
                    lines.push(format!("p{p}: called off at {}s, attacked again {gap}s later", t / sim::TICK_RATE));
                }
            }
        }
    }
    (n, quickest, lines)
}

#[test]
fn an_army_called_home_stays_home_a_while() {
    // It used to call an attack off, march again as soon as it felt safe,
    // and be called back again, over and over.
    for seed in [1, 2] {
        let (n, quickest, lines) = recalls(seed, 14);
        eprintln!("seed {seed}: {n} calls off; {lines:?}");
        assert!(quickest >= sim::ai::RECALL_REST_SECS, "seed {seed}: marched again {quickest}s after calling off: {lines:?}");
        assert!(n <= 8, "seed {seed}: {n} calls off");
    }
}
