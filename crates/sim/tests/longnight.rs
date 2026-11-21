//! Ending a match: conceding, and the Long Night closing in.

use sim::game::*;
use sim::map::Map;
use sim::math::v2;
use sim::proto::GameInfo;

fn game() -> Game {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let mut g = Game::new_with(map, setups, 3, sim::game::CoreRules::default().map_start());
    g.weather.calm = true;
    g
}

fn run(g: &mut Game, secs: u32) {
    for _ in 0..secs * sim::TICK_RATE {
        g.tick();
        g.events.clear();
    }
}

#[test]
fn conceding_freezes_the_tribe_and_hands_the_game_over() {
    let mut g = game();
    g.command(0, Command::Concede);
    assert!(g.players[0].out);
    assert!(g.penguins.iter().filter(|p| p.owner == 0).count() == 0, "nobody still belongs to the tribe");
    assert!(g.penguins.iter().any(|p| p.was == 0 && p.act == Act::Frozen), "they froze, they didn't vanish");
    assert!(g.igloos.iter().all(|i| i.owner != 0), "its igloos fell in");
    run(&mut g, 2);
    assert_eq!(g.winner, Some(1));
}

#[test]
fn the_ring_closes_from_the_edge_to_the_middle() {
    let (w, h) = (128, 128);
    assert_eq!(night_radius(w, h, 1000, 999), None);
    let a = night_radius(w, h, 1000, 1000).unwrap();
    assert!(a > 80.0, "starts past the corners: {a}");
    let b = night_radius(w, h, 1000, 1000 + LONG_NIGHT_CLOSE / 2).unwrap();
    assert!(b < a && b > LONG_NIGHT_MIN_RADIUS);
    let end = night_radius(w, h, 1000, 1000 + LONG_NIGHT_CLOSE * 3).unwrap();
    assert_eq!(end, LONG_NIGHT_MIN_RADIUS, "and stops at the minimum");
}

#[test]
fn penguins_caught_outside_the_ring_freeze_but_those_inside_do_not() {
    let mut g = game();
    g.long_night_at = 0;
    g.tick = LONG_NIGHT_CLOSE + 1;
    let (mid, r) = g.night_ring().unwrap();
    let ids: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Snowballer).map(|p| p.id).take(2).collect();
    let (out, safe) = (ids[0], ids[1]);
    // One out at the edge of the map, one at the middle.
    let spot = |near: sim::math::Vec2| {
        let (x, y) = g.grid.nearest_walkable(near.tile(), 8).unwrap();
        v2(x as f32 + 0.5, y as f32 + 0.5)
    };
    let (edge, centre) = (spot(v2(2.0, 2.0)), spot(mid));
    assert!(edge.dist(mid) > r + 5.0 && centre.dist(mid) < r);
    for _ in 0..120 * sim::TICK_RATE {
        for (id, at) in [(out, edge), (safe, centre)] {
            let p = &mut g.penguins[id as usize];
            if p.act != Act::Frozen {
                p.pos = at;
                p.order = Order::Hold { pos: at };
            }
        }
        g.tick();
        g.events.clear();
        if g.penguins[out as usize].act == Act::Frozen {
            break;
        }
    }
    assert_eq!(g.penguins[out as usize].act, Act::Frozen, "left out in the Long Night");
    assert_ne!(g.penguins[safe as usize].act, Act::Frozen, "safe in the middle");
}

#[test]
fn igloos_out_in_the_cold_fall_in() {
    let mut g = game();
    g.long_night_at = 0;
    g.tick = LONG_NIGHT_CLOSE + 1;
    let (mid, r) = g.night_ring().unwrap();
    let far: Vec<usize> = (0..g.igloos.len()).filter(|&i| g.igloos[i].owner != NEUTRAL && g.igloos[i].center.dist(mid) > r).collect();
    assert!(!far.is_empty(), "an igloo lies outside the final ring");
    run(&mut g, 400);
    for i in far {
        assert_eq!(g.igloos[i].owner, NEUTRAL, "igloo {i} wasted away");
    }
}

#[test]
fn when_the_night_is_over_the_biggest_flock_wins() {
    let mut g = game();
    g.long_night_at = 100;
    g.tick = 100 + LONG_NIGHT_CLOSE + LONG_NIGHT_FINAL - 2;
    // Player 1 is left with fewer penguins and nobody is outside the ring.
    let mut n = 0;
    for p in g.penguins.iter_mut().filter(|p| p.owner == 1) {
        n += 1;
        if n > 3 {
            p.owner = NEUTRAL;
        }
    }
    g.long_night_at = g.tick - LONG_NIGHT_CLOSE - LONG_NIGHT_FINAL + 2;
    for i in 0..g.igloos.len() {
        g.igloos[i].center = g.night_ring().unwrap().0;
    }
    run_ticks(&mut g, 5);
    assert_eq!(g.winner, Some(0), "the tribe with more penguins standing wins");
}

fn run_ticks(g: &mut Game, n: u32) {
    for _ in 0..n {
        g.tick();
        g.events.clear();
    }
}

#[test]
fn the_start_tick_reaches_clients() {
    let mut g = game();
    g.long_night_at = 12345;
    let info = GameInfo::from_game(&g, 0);
    assert_eq!(info.long_night_at, 12345);
}

#[test]
fn the_ai_runs_for_the_middle_when_the_ring_closes() {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: (i == 1).then_some(sim::ai::AiLevel::Normal) }).collect();
    let mut g = Game::new_with(map, setups, 3, sim::game::CoreRules::default().map_start());
    g.weather.calm = true;
    g.long_night_at = 0;
    g.tick = LONG_NIGHT_CLOSE / 2;
    let (mid, r) = g.night_ring().unwrap();
    // Some of its penguins are out on the far side of the ring.
    let strays: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 1 && p.inside.is_none()).map(|p| p.id).take(3).collect();
    for (k, id) in strays.into_iter().enumerate() {
        g.penguins[id as usize].pos = mid + v2(1.0, 1.0).norm() * (r + 2.0 + k as f32);
    }
    let outside = |g: &Game| g.penguins.iter().filter(|p| p.owner == 1 && p.act != Act::Frozen && p.pos.dist(mid) > r).count();
    let before = outside(&g);
    assert!(before > 0, "some of its penguins start outside the ring");
    run(&mut g, 6);
    assert!(g.penguins.iter().any(|p| p.owner == 1 && matches!(p.order, Order::Move { run: true, .. })), "it ordered a run for the middle");
}
