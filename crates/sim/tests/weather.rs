//! Blizzards chill penguins caught out in the open, and the chill wears
//! off once the storm has passed.

use sim::game::*;
use sim::map::Map;
use sim::math::{v2, Vec2};

fn game() -> Game {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    Game::new(map, setups, 9)
}

/// Walkable open ground far from every door.
fn wilderness(g: &Game) -> Vec2 {
    for y in 2..g.map.h - 2 {
        for x in 2..g.map.w - 2 {
            let p = v2(x as f32 + 0.5, y as f32 + 0.5);
            if g.grid.walkable(x, y)
                && g.igloos.iter().all(|ig| ig.door.dist(p) > 12.0)
            {
                return p;
            }
        }
    }
    panic!("no wilderness on this map");
}

#[test]
fn a_blizzard_chills_penguins_in_the_open_and_it_wears_off() {
    let mut g = game();
    let at = wilderness(&g);
    let i = g.penguins.iter().position(|p| p.owner == 0 && p.kind.soldier()).unwrap();
    g.penguins[i].pos = at;
    assert!(!g.sheltered(i));
    let start = g.penguins[i].warmth;
    // The storm blows for 20 s on our penguin alone; keep it pinned so the
    // random chance can neither start nor end another one. Everyone's hands
    // stay empty so no fighting muddies the numbers.
    for _ in 0..20 * sim::TICK_RATE {
        g.weather.blizzard = BLIZZARD_TICKS;
        g.penguins[i].pos = at;
        for p in &mut g.penguins {
            p.snowballs = 0;
        }
        g.tick();
        assert_ne!(g.penguins[i].act, Act::Frozen, "the blizzard alone froze a penguin");
    }
    let p = &g.penguins[i];
    assert!(p.chill > 0, "the blizzard left no chill, warmth {}", p.warmth);
    assert!(p.warmth < start, "warmth {} -> {}", start, p.warmth);
    // The storm passes: the chill wears off, one bite at a time, and
    // returns the warmth it took. Twenty-four seconds covers it all no
    // matter where the tick boundary falls.
    for _ in 0..4 * BLIZZARD_CHILL_TICKS {
        g.weather.blizzard = 0;
        g.penguins[i].pos = at;
        g.tick();
    }
    let p = &g.penguins[i];
    assert_eq!(p.chill, 0, "the chill did not wear off, warmth {}", p.warmth);
}
