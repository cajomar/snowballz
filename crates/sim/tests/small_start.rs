//! A small start (the `armies` rule, DESIGN.md 0.2 and PLAN.md C7 stage 4):
//! a tribe begins with its Chieftain and two workers, and the frozen always
//! come back as plain workers. (Newcomers swim in from the sea to fill
//! slots: see tests/slots.rs.)

use sim::ai::AiLevel;
use sim::game::*;
use sim::map::Map;

fn map(name: &str) -> Map {
    let path = format!("{}/../../assets/maps/{name}.sbmap", env!("CARGO_MANIFEST_DIR"));
    Map::load_file(std::path::Path::new(&path)).unwrap()
}

fn game(players: usize) -> Game {
    let setups = (0..players).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let mut g = Game::new(map("pinewood-hollow"), setups, 7);
    g.weather.calm = true;
    g
}

fn home(g: &Game, owner: u8) -> u8 {
    g.igloos.iter().position(|i| i.owner == owner).unwrap() as u8
}

fn count(g: &Game, owner: u8, kind: Kind) -> usize {
    g.penguins.iter().filter(|p| p.owner == owner && p.kind == kind).count()
}

#[test]
fn a_tribe_starts_with_its_chieftain_and_two_workers() {
    let g = game(2);
    for p in 0..2 {
        assert_eq!(count(&g, p, Kind::Chieftain), 1);
        assert_eq!(count(&g, p, Kind::Worker), START_WORKERS as usize);
        assert_eq!(g.penguins.iter().filter(|q| q.owner == p).count(), 1 + START_WORKERS as usize, "nobody else");
    }
    // Without the small start, the map's people.
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let big = Game::new_with(map("pinewood-hollow"), setups, 7, CoreRules::default().map_start());
    assert!(big.penguins.iter().filter(|q| q.owner == 0).count() > 3 + START_WORKERS as usize);
}

#[test]
fn the_frozen_always_come_back_as_plain_workers() {
    let mut g = game(2);
    let ig = home(&g, 0);
    let enemy = g.penguins.iter().find(|p| p.owner == 1).unwrap().id;
    for kind in [Kind::Snowballer, Kind::Slinger, Kind::Warmer] {
        let v = g.spawn_for_test(ig, kind);
        g.hurt_for_test(v, 255, enemy);
        assert_eq!(g.penguins[v as usize].act, Act::Frozen);
        g.thaw_for_test(v, 0);
        assert_eq!(g.penguins[v as usize].kind, Kind::Worker, "a thawed {kind:?} is a worker");
        // Thawing out wild too.
        let w = g.spawn_for_test(ig, kind);
        g.hurt_for_test(w, 255, enemy);
        g.thaw_for_test(w, NEUTRAL);
        assert_eq!(g.penguins[w as usize].kind, Kind::Worker);
    }
}

#[test]
fn computers_grow_from_the_small_start_and_fight() {
    let setups = (0..3).map(|i| PlayerSetup { name: format!("AI {i}"), color: i as u8, team: i as u8, ai: Some(AiLevel::Hard) }).collect();
    let mut g = Game::new(map("pinewood-hollow"), setups, 5);
    let mut thrown = 0;
    for _ in 0..12 * 60 * sim::TICK_RATE {
        g.tick();
        thrown += g.events.iter().filter(|e| matches!(e, Event::Thrown { .. })).count();
        g.events.clear();
    }
    for p in 0..3 {
        let n = g.penguins.iter().filter(|q| q.owner == p && q.act != Act::Frozen).count();
        let fighters = g.penguins.iter().filter(|q| q.owner == p && q.kind.fights() && q.act != Act::Frozen).count();
        assert!(n >= 10, "player {p} has only {n} penguins after twelve minutes");
        assert!(fighters >= 3, "player {p} has only {fighters} fighters");
    }
    assert!(thrown > 50, "they hardly fought ({thrown} throws)");
}
