//! The slow cold (`CoreRules::cold`, parked: off by default) and shelter.

use sim::game::*;
use sim::map::Map;
use sim::math::{v2, Vec2};
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};

fn game() -> Game {
    let mut g = cold_game();
    g.core.cold = true;
    g.weather.calm = true;
    g
}

fn cold_game() -> Game {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let mut g = Game::new_with(map, setups, 9, sim::game::CoreRules::default().map_start());
    // Fed, so hunger takes no warmth either.
    for ig in &mut g.igloos {
        ig.fish = 500;
    }
    g
}

/// Walkable open ground far from every door.
fn wilderness(g: &Game) -> Vec2 {
    for y in 2..g.map.h - 2 {
        for x in 2..g.map.w - 2 {
            let p = v2(x as f32 + 0.5, y as f32 + 0.5);
            if g.grid.walkable(x, y)
                && g.igloos.iter().all(|ig| ig.door.dist(p) > 12.0)
                && g.seals.iter().all(|s| s.pos.dist(p) > 20.0)
            {
                return p;
            }
        }
    }
    panic!("no wilderness on this map");
}

/// One of player 0's soldiers, alone at `at`, everyone else tucked away.
fn lone_soldier(g: &mut Game, at: Vec2) -> usize {
    let i = g.penguins.iter().position(|p| p.owner == 0 && p.kind.soldier()).unwrap();
    for p in &mut g.penguins {
        p.snowballs = 0;
    }
    g.penguins[i].pos = at;
    i
}

/// Hold penguin `i` at `at` with everyone's hands empty for `secs`.
fn hold(g: &mut Game, i: usize, at: Vec2, secs: u32) {
    for _ in 0..secs * sim::TICK_RATE {
        g.penguins[i].pos = at;
        for p in &mut g.penguins {
            p.snowballs = 0;
        }
        g.tick();
        assert_ne!(g.penguins[i].act, Act::Frozen, "the cold alone froze a penguin");
    }
}

#[test]
fn the_cold_wears_penguins_down_but_never_freezes_them() {
    let mut g = game();
    let at = wilderness(&g);
    let i = lone_soldier(&mut g, at);
    let start = g.penguins[i].warmth;
    assert!(!g.sheltered(i));
    // Deep winter lasts 240 s; the floor is reached well inside it.
    hold(&mut g, i, at, 150);
    assert_eq!(g.penguins[i].warmth, EXPOSURE_FLOOR, "a long stay in the open wears warmth down to the floor");
    assert!(start > EXPOSURE_FLOOR);
}

#[test]
fn home_ground_gives_back_what_the_cold_took() {
    let mut g = game();
    let at = wilderness(&g);
    let i = lone_soldier(&mut g, at);
    let start = g.penguins[i].warmth;
    hold(&mut g, i, at, 60);
    assert!(g.penguins[i].warmth < start);
    let ig = g.igloos.iter().position(|ig| ig.owner == 0).unwrap();
    let home = g.igloos[ig].door + v2(0.0, 1.0);
    hold(&mut g, i, home, 120);
    assert_eq!(g.penguins[i].warmth, start);
}

#[test]
fn a_siege_huddles_in_the_doorway() {
    let mut g = game();
    let enemy = g.igloos.iter().position(|ig| ig.owner == 1).unwrap();
    let at = g.igloos[enemy].door + v2(0.0, 2.0);
    let i = lone_soldier(&mut g, at);
    assert!(!g.sheltered(i), "enemy ground is cold");
    g.igloos[enemy].takeover = Some(Takeover { by: 0, ticks_left: 100, total: 100 });
    assert!(g.sheltered(i), "the ground of an igloo we are taking over is not");
}

#[test]
fn home_ground_keeps_the_cold_out() {
    let mut g = game();
    let ig = g.igloos.iter().position(|ig| ig.owner == 0).unwrap();
    let at = g.igloos[ig].door + v2(0.0, 1.0);
    let i = lone_soldier(&mut g, at);
    let start = g.penguins[i].warmth;
    assert!(g.sheltered(i));
    hold(&mut g, i, at, 60);
    assert!(g.penguins[i].warmth >= start);
}

#[test]
fn clients_see_who_is_out_in_the_cold() {
    let mut g = game();
    let at = wilderness(&g);
    let i = lone_soldier(&mut g, at);
    let home = g.penguins.iter().position(|p| p.owner == 0 && p.inside.is_none() && p.id as usize != i).unwrap();
    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    g.tick();
    let ev = std::mem::take(&mut g.events);
    view.apply(&enc.encode(&g, &ev)).unwrap();
    assert!(view.penguins[i].exposed);
    assert_eq!(view.penguins[home].exposed, !g.sheltered(home));
}

#[test]
fn the_cold_is_off_by_default() {
    let mut g = cold_game();
    assert!(!g.core.cold);
    // A blizzard chills penguins in the open whatever the cold rule says.
    g.weather.calm = true;
    g.weather.blizzard = 0;
    let at = wilderness(&g);
    let i = lone_soldier(&mut g, at);
    let start = g.penguins[i].warmth;
    hold(&mut g, i, at, 120);
    assert_eq!(g.penguins[i].warmth, start, "nothing wears warmth down in the open");
    // And clients are not told anyone is exposed.
    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    g.tick();
    let ev = std::mem::take(&mut g.events);
    view.apply(&enc.encode(&g, &ev)).unwrap();
    assert!(!view.penguins[i].exposed);
}
