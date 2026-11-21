//! Open water and shore fishing: workers fish any bank, slowly and without end.

use sim::game::*;
use sim::map::{Ground, Map, ResourceKind};
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};

fn map(name: &str) -> Map {
    let path = format!("{}/../../assets/maps/{name}.sbmap", env!("CARGO_MANIFEST_DIR"));
    Map::load_file(std::path::Path::new(&path)).unwrap()
}

fn game() -> Game {
    let setups = (0..6).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    Game::new(map("pinewood-hollow"), setups, 3)
}

/// The igloo with a bank closest to it, and that bank.
fn igloo_by_water(g: &Game) -> (usize, sim::game::Shore) {
    g.igloos
        .iter()
        .enumerate()
        .filter(|(_, ig)| ig.owner != NEUTRAL)
        .flat_map(|(i, ig)| g.shores.iter().map(move |s| (i, s.clone(), s.pos.dist(ig.center))))
        .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap())
        .map(|(i, s, _)| (i, s))
        .unwrap()
}

fn water_tiles(m: &Map) -> usize {
    m.ground.iter().filter(|&&g| g == Ground::Water).count()
}

#[test]
fn water_is_not_walkable_and_has_banks() {
    let g = game();
    assert!(water_tiles(&g.map) > 100, "pinewood hollow has open water");
    for y in 0..g.map.h {
        for x in 0..g.map.w {
            if g.map.ground_at(x, y) == Ground::Water {
                assert!(!g.grid.walkable(x, y), "water at {x},{y} is walkable");
            }
        }
    }
    assert!(!g.shores.is_empty());
    for s in &g.shores {
        assert!(g.grid.walkable(s.tile.0, s.tile.1), "a bank must be somewhere you can stand");
    }
}

#[test]
fn every_igloo_can_still_reach_the_others() {
    for name in ["aurora-basin", "glacier-crossroads", "pinewood-hollow", "shattered-shelf", "the-long-white"] {
        let m = map(name);
        let n = m.igloos.len();
        let setups = (0..n).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
        let g = Game::new(m, setups, 1);
        let door = |i: usize| g.igloos[i].door.tile();
        for i in 1..n {
            let mut pf = sim::path::PathFinder::new((g.map.w * g.map.h) as usize);
            let path = pf.find(&g.grid, door(0), door(i), 200_000);
            assert!(path.last() == Some(&door(i)), "{name}: igloo {i} is cut off by water");
        }
    }
}

/// A worker in a game with no fish holes left fishes a bank and the fish
/// arrive at the igloo, one per `SHORE_SLOWDOWN` times a hole's time at best.
#[test]
fn workers_fish_the_bank_when_the_holes_are_gone() {
    let mut g = game();
    for r in g.resources.iter_mut().filter(|r| r.kind == ResourceKind::Fish) {
        r.amount = 0;
        r.max = 0;
    }
    // An igloo with a bank close by.
    let (ig, _) = igloo_by_water(&g);
    let player = g.igloos[ig].owner;
    for ig in g.igloos.iter_mut().filter(|i| i.owner == player) {
        ig.fish = 0;
    }
    let mut fishing_on_bank = false;
    let mut most_fish = 0;
    for _ in 0..240 * sim::TICK_RATE {
        g.tick();
        g.events.clear();
        most_fish = most_fish.max(g.igloos.iter().filter(|i| i.owner == player).map(|i| i.fish as u32).sum::<u32>());
        fishing_on_bank |= g.penguins.iter().any(|p| {
            p.owner == player && p.act == Act::Fish && g.shores.iter().any(|s| s.pos.dist(p.pos) < 0.6)
        });
    }
    assert!(fishing_on_bank, "a worker stood on a bank and fished");
    assert!(most_fish > 0, "bank fish reached the igloo (fish is spent on warmth, so look at the peak)");
    // Banks never run dry: no resource point was involved.
    assert!(g.resources.iter().all(|r| r.amount == 0 || r.kind != ResourceKind::Fish));
}

#[test]
fn a_bank_is_slower_than_a_hole() {
    assert!(SHORE_SLOWDOWN >= 2);
}

#[test]
fn two_workers_do_not_share_a_bank() {
    let mut g = game();
    for r in g.resources.iter_mut().filter(|r| r.kind == ResourceKind::Fish) {
        r.amount = 0;
        r.max = 0;
    }
    for _ in 0..120 * sim::TICK_RATE {
        g.tick();
        g.events.clear();
    }
    let mut used = std::collections::HashSet::new();
    for p in g.penguins.iter().filter(|p| p.act == Act::Fish) {
        let at = (p.pos.x as i32, p.pos.y as i32);
        assert!(used.insert(at), "two workers fish from {at:?}");
    }
}

#[test]
fn water_survives_snow_and_reaches_the_client() {
    let mut g = game();
    let before = water_tiles(&g.map);
    let info = GameInfo::from_game(&g, 0);
    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(info);
    for _ in 0..60 * sim::TICK_RATE {
        g.tick();
        let ev = std::mem::take(&mut g.events);
        view.apply(&enc.encode(&g, &ev)).unwrap();
    }
    assert_eq!(water_tiles(&g.map), before, "snow and trampling never touch water");
    assert_eq!(water_tiles(&view.info.map), before, "and the client's copy keeps it");
}

/// Right-click water with workers: each takes a bank near the click and
/// stays on it, even when a fish hole is free.
#[test]
fn workers_ordered_to_fish_stay_on_their_bank() {
    let mut g = game();
    let (ig, bank) = igloo_by_water(&g);
    let player = g.igloos[ig].owner;
    let (dx, dy) = [(0, 1), (1, 0), (-1, 0), (0, -1)].into_iter().find(|&(dx, dy)| g.map.ground_at(bank.tile.0 + dx, bank.tile.1 + dy) == Ground::Water).unwrap();
    let water = (bank.tile.0 + dx, bank.tile.1 + dy);
    let workers: Vec<u16> = g.penguins.iter().filter(|p| p.owner == player && p.kind == Kind::Worker).map(|p| p.id).take(3).collect();
    // Not water: nothing happens.
    g.command(player, Command::Fish { units: workers.clone(), tile: (bank.tile.0, bank.tile.1) });
    g.command(player, Command::Fish { units: workers.clone(), tile: water });
    // Each is seen fishing on a bank (they walk home with each catch, so
    // not all at once), and fish holes are in reach all the while.
    let mut seen = vec![false; workers.len()];
    for _ in 0..200 * sim::TICK_RATE {
        g.tick();
        g.events.clear();
        for (k, &w) in workers.iter().enumerate() {
            let p = &g.penguins[w as usize];
            seen[k] |= p.act == Act::Fish && g.shores.iter().any(|s| s.pos.dist(p.pos) < 0.6);
        }
    }
    assert!(seen.iter().all(|&s| s), "every ordered worker fished a bank: {seen:?}");
    // Another player's workers can't be sent: they stay where they are.
    let theirs = g.penguins.iter().find(|p| p.owner != player && p.kind == Kind::Worker).map(|p| p.id).unwrap();
    let before = g.penguins[theirs as usize].order;
    g.command(player, Command::Fish { units: vec![theirs], tile: water });
    assert_eq!(g.penguins[theirs as usize].order, before);
}

#[test]
fn fish_holes_become_ponds_with_banks() {
    let path = format!("{}/../../assets/maps/pinewood-hollow.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let holes: Vec<(i32, i32)> = map.resources.iter().filter(|r| r.kind == ResourceKind::Fish).map(|r| (r.x as i32, r.y as i32)).collect();
    assert!(holes.len() >= 10, "the map file still lists its holes");
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let g = Game::new(map, setups, 1);
    assert!(g.resources.iter().all(|r| r.kind != ResourceKind::Fish), "no fish holes in the game");
    for (x, y) in holes {
        assert_eq!(g.map.ground_at(x, y), sim::map::Ground::Water, "a pond where the hole was ({x},{y})");
        assert!(g.shores.iter().any(|s| (s.tile.0 - x).abs() <= 2 && (s.tile.1 - y).abs() <= 2), "with a bank to fish from");
    }
}
