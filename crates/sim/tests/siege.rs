//! Knocking a tribe out (DESIGN.md section 0): when the last igloo falls
//! and there are no buildings, the tribe is out, even with penguins still
//! on the ice.

use sim::game::*;
use sim::map::Map;
use sim::math::{v2, Vec2};

fn map(name: &str) -> Map {
    let path = format!("{}/../../assets/maps/{name}.sbmap", env!("CARGO_MANIFEST_DIR"));
    Map::load_file(std::path::Path::new(&path)).unwrap()
}

fn setups() -> Vec<PlayerSetup> {
    (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect()
}

fn game() -> Game {
    Game::new_with(map("fixtures/trio"), setups(), 7, CoreRules::default().map_start())
}

fn home(g: &Game, owner: u8) -> u8 {
    g.igloos.iter().position(|i| i.owner == owner).unwrap() as u8
}

/// A spot outside an igloo's door, `d` tiles out.
fn outside(g: &Game, ig: u8, d: f32) -> Vec2 {
    let door = g.igloos[ig as usize].door;
    let c = g.igloos[ig as usize].center;
    let at = door + (door - c).norm() * d;
    let (x, y) = g.grid.nearest_walkable(at.tile(), 4).unwrap();
    v2(x as f32 + 0.5, y as f32 + 0.5)
}

/// The middle of an open, walkable patch `w` x `h` tiles, away from igloos.
fn open_ground(g: &Game, w: i32, h: i32) -> Vec2 {
    for y in 3..g.map.h - h - 3 {
        for x in 3..g.map.w - w - 3 {
            let clear = (0..h).all(|dy| (0..w).all(|dx| g.grid.walkable(x + dx, y + dy)));
            let c = v2(x as f32 + w as f32 / 2.0, y as f32 + h as f32 / 2.0);
            if clear && g.igloos.iter().all(|ig| ig.door.dist(c) > 12.0) {
                return c;
            }
        }
    }
    panic!("no open ground");
}

#[test]
fn losing_the_last_igloo_puts_a_tribe_out() {
    let mut g = game();
    let ig = home(&g, 1);
    // One igloo and no other buildings.
    assert_eq!(g.igloos.iter().filter(|i| i.owner == 1).count(), 1);
    assert!(!g.buildings.iter().any(|b| b.alive && b.owner == 1));
    // A few penguins outside, the igloo empty.
    let away = open_ground(&g, 8, 4);
    for p in &mut g.penguins {
        if p.owner == 1 {
            p.pos = away;
            p.snowballs = 0;
            p.inside = None;
            p.order = Order::None;
        }
    }
    g.igloos[ig as usize].occupants.clear();
    let out = g.penguins.iter().filter(|p| p.owner == 1 && p.inside.is_none()).count();
    assert!(out >= 3, "a few penguins outside: {out}");
    // One snowball from now; raiders finish it off.
    g.igloos[ig as usize].hp = 1;
    let spot = outside(&g, ig, 4.0);
    let raiders: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Snowballer).take(6).map(|p| p.id).collect();
    assert!(!raiders.is_empty(), "no raiders");
    let mut ev = Vec::new();
    for _ in 0..180 * sim::TICK_RATE {
        for &r in &raiders {
            let p = &mut g.penguins[r as usize];
            p.pos = spot;
            p.snowballs = p.snowballs.max(p.kind.stats().ammo);
            p.warmth = p.max_warmth;
            p.cold = false;
            p.inside = None;
            p.order = Order::Hold { pos: spot };
        }
        for p in &mut g.penguins {
            if p.owner == 1 {
                p.snowballs = 0;
                p.pos = away;
            }
        }
        g.tick();
        ev.extend(std::mem::take(&mut g.events));
        if g.igloos[ig as usize].owner == NEUTRAL {
            break;
        }
    }
    assert!(ev.iter().any(|e| matches!(e, Event::Collapsed { igloo, owner: 1 } if *igloo == ig)), "hp left {}", g.igloos[ig as usize].hp);
    assert_eq!(g.igloos[ig as usize].owner, NEUTRAL, "a ruin now");
    // No igloo, no buildings: the next victory check puts the tribe out,
    // even though its penguins are still on the ice.
    for _ in 0..20 {
        for p in &mut g.penguins {
            p.snowballs = 0;
        }
        g.tick();
        ev.extend(std::mem::take(&mut g.events));
    }
    assert!(g.players[1].out, "the tribe has no igloo and no buildings");
    assert!(ev.iter().any(|e| matches!(e, Event::PlayerOut { player: 1 })));
    assert_eq!(g.winner, Some(0));
}

#[test]
fn a_hut_keeps_a_tribe_in() {
    let mut g = game();
    let ig = home(&g, 1);
    // One igloo, and a finished hut behind it, out of the raiders' reach.
    assert_eq!(g.igloos.iter().filter(|i| i.owner == 1).count(), 1);
    let door = g.igloos[ig as usize].door;
    let center = g.igloos[ig as usize].center;
    let spot = outside(&g, ig, 4.0);
    let at = door + (center - door).norm() * 8.0;
    assert!(spot.dist(at) > 10.0, "the hut must sit behind the base");
    let tile = (at.x as i32 - 1, at.y as i32 - 1);
    assert!(g.map.in_bounds(tile.0, tile.1));
    g.buildings.push(Building {
        id: g.buildings.len() as u16,
        kind: BuildKind::Sauna,
        owner: 1,
        tile,
        hp: BuildKind::Sauna.max_hp(),
        built: BuildKind::Sauna.snow(),
        wood_in: BuildKind::Sauna.wood() as u8,
        alive: true,
        fuel: 0,
        lit: false,
        tended: 0,
        starved: false,
        rally: None,
        slots: 1,
        want: 1,
        upgrading: None,
        reload: 0,
    });
    // A few penguins outside, the igloo empty.
    let away = open_ground(&g, 8, 4);
    for p in &mut g.penguins {
        if p.owner == 1 {
            p.pos = away;
            p.snowballs = 0;
            p.inside = None;
            p.order = Order::None;
        }
    }
    g.igloos[ig as usize].occupants.clear();
    let out = g.penguins.iter().filter(|p| p.owner == 1 && p.inside.is_none()).count();
    assert!(out >= 3, "a few penguins outside: {out}");
    // One snowball from now; raiders finish it off.
    g.igloos[ig as usize].hp = 1;
    let raiders: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Snowballer).take(6).map(|p| p.id).collect();
    assert!(!raiders.is_empty(), "no raiders");
    let mut ev = Vec::new();
    for _ in 0..180 * sim::TICK_RATE {
        for &r in &raiders {
            let p = &mut g.penguins[r as usize];
            p.pos = spot;
            p.snowballs = p.snowballs.max(p.kind.stats().ammo);
            p.warmth = p.max_warmth;
            p.cold = false;
            p.inside = None;
            p.order = Order::Hold { pos: spot };
        }
        for p in &mut g.penguins {
            if p.owner == 1 {
                p.snowballs = 0;
                p.pos = away;
            }
        }
        g.tick();
        ev.extend(std::mem::take(&mut g.events));
        if g.igloos[ig as usize].owner == NEUTRAL {
            break;
        }
    }
    assert!(ev.iter().any(|e| matches!(e, Event::Collapsed { igloo, owner: 1 } if *igloo == ig)), "hp left {}", g.igloos[ig as usize].hp);
    assert_eq!(g.igloos[ig as usize].owner, NEUTRAL, "a ruin now");
    // No igloo, but the hut stands: the tribe is still in the game.
    for _ in 0..20 {
        for p in &mut g.penguins {
            p.snowballs = 0;
        }
        g.tick();
        ev.extend(std::mem::take(&mut g.events));
    }
    assert!(!g.players[1].out, "a finished hut keeps the tribe in");
    assert!(!ev.iter().any(|e| matches!(e, Event::PlayerOut { player: 1 })));
    assert_eq!(g.winner, None, "both tribes still in the game");
}
