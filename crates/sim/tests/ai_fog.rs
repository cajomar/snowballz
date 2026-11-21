//! The computer plays under fog: it can't march on an igloo it hasn't found.

use sim::ai::AiLevel;
use sim::game::*;
use sim::map::Map;

fn known(g: &Game, player: usize) -> usize {
    let d = &g.ai_debug()[player];
    let after = d.split("known ").nth(1).unwrap();
    after.split(' ').next().unwrap().parse().unwrap()
}

#[test]
fn the_ai_scouts_before_it_attacks() {
    let path = format!("{}/../../assets/maps/pinewood-hollow.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("AI {i}"), color: i as u8, team: i as u8, ai: Some(AiLevel::Hard) }).collect();
    let mut g = Game::new_with(map, setups, 42, CoreRules::default().map_start());
    g.weather.calm = true;
    for _ in 0..50 * sim::TICK_RATE {
        g.tick();
        g.events.clear();
    }
    // Fifty seconds in, nobody has seen anyone else's igloo and nobody marches.
    for p in 0..2 {
        assert_eq!(known(&g, p), 1, "AI {p} already knows igloos it has never seen: {}", g.ai_debug()[p]);
        assert!(g.ai_debug()[p].contains("target None"), "AI {p} picked a target blind");
    }
    // Later, exploring has turned something up.
    for _ in 0..10 * 60 * sim::TICK_RATE {
        g.tick();
        g.events.clear();
    }
    assert!((0..2).any(|p| known(&g, p) >= 2), "nobody found anything in ten minutes: {:?}", g.ai_debug());
}
