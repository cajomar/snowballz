//! Each kind does its own job without being told: Shieldbearers step in
//! front, Slingers keep their distance, Skiers hunt the soft targets and
//! knock throwers (and shields, falling with them) flat, Packers go for buildings, Sleds bring the frozen
//! home. Three kinds were cut from the cast and can't be trained any more.

use sim::game::*;
use sim::map::Map;
use sim::math::{v2, Vec2};

fn game() -> Game {
    let path = format!("{}/../../assets/maps/glacier-crossroads.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let mut g = Game::new_with(map, setups, 7, CoreRules::default().map_start());
    g.weather.calm = true;
    g
}

/// A stretch of open level ground, 16 by 9 tiles, far from every igloo.
fn field(g: &Game) -> Vec2 {
    for y in 5..g.map.h - 10 {
        for x in 3..g.map.w - 18 {
            let ok = (0..16).all(|dx| (0..9).all(|dy| g.grid.walkable(x + dx, y + dy) && g.snow_at(v2((x + dx) as f32 + 0.5, (y + dy) as f32 + 0.5)) > 0));
            let at = v2(x as f32 + 0.5, y as f32 + 4.5);
            if ok && g.igloos.iter().all(|ig| ig.door.dist(at) > 18.0 && ig.door.dist(at + v2(15.0, 0.0)) > 18.0) && (g.map.height_at(at) - g.map.height_at(at + v2(15.0, 0.0))).abs() < 0.3 {
                return at;
            }
        }
    }
    panic!("no open field");
}

/// One of `owner`'s workers, made a `kind` and stood at `at`.
fn put(g: &mut Game, owner: u8, kind: Kind, at: Vec2) -> u16 {
    let id = g
        .penguins
        .iter()
        .find(|p| p.owner == owner && p.kind == Kind::Worker)
        .unwrap_or_else(|| panic!("P{owner} has no worker left"))
        .id;
    g.retrain_for_test(id, kind);
    for ig in g.igloos.iter_mut() {
        ig.occupants.retain(|&o| o != id);
    }
    let max = g.max_warmth_of(kind);
    let p = &mut g.penguins[id as usize];
    p.pos = at;
    p.inside = None;
    p.order = Order::None;
    p.carry = Carry::None;
    p.warmth = max;
    id
}

/// Run, keeping everyone warm so cold never sends them home; returns the events.
fn run(g: &mut Game, seconds: f32) -> Vec<Event> {
    let mut out = Vec::new();
    for _ in 0..(seconds * sim::TICK_RATE as f32) as u32 {
        for p in g.penguins.iter_mut().filter(|p| p.act != Act::Frozen && p.owner != NEUTRAL) {
            p.warmth = p.max_warmth;
            p.cold = false;
        }
        g.tick();
        out.extend(std::mem::take(&mut g.events));
    }
    out
}

#[test]
fn a_slinger_backs_away_from_anyone_close_and_wont_throw_at_them() {
    let mut g = game();
    let at = field(&g);
    let sling = put(&mut g, 0, Kind::Slinger, at);
    let foe = put(&mut g, 1, Kind::Snowballer, at + v2(1.8, 0.0));
    // The enemy just stands there; only the slinger acts.
    g.penguins[foe as usize].snowballs = 0;
    let events = run(&mut g, 3.0);
    let close_throw = events.iter().any(|e| matches!(e, Event::Thrown(s) if s.thrower == sling && s.from.dist(s.to) < 3.0));
    assert!(!close_throw, "a slinger threw at someone right next to it");
    let d = g.penguins[sling as usize].pos.dist(g.penguins[foe as usize].pos);
    assert!(d > 3.0, "the slinger stayed {d:.1} tiles from its enemy");
}

#[test]
fn a_shieldbearer_steps_between_its_friends_and_the_enemy() {
    let mut g = game();
    let at = field(&g);
    let shield = put(&mut g, 0, Kind::Shield, at + v2(-1.5, 0.0));
    let friends: Vec<u16> = (0..3).map(|k| put(&mut g, 0, Kind::Snowballer, at + v2(0.0, k as f32 - 1.0))).collect();
    // The line holds its ground; the enemy waits out of reach.
    g.command(0, Command::Hold { units: friends.clone() });
    let foe = put(&mut g, 1, Kind::Snowballer, at + v2(9.0, 0.0));
    g.command(1, Command::Hold { units: vec![foe] });
    run(&mut g, 5.0);
    let mid = friends.iter().map(|&f| g.penguins[f as usize].pos.x).sum::<f32>() / 3.0;
    let sx = g.penguins[shield as usize].pos.x;
    assert!(sx > mid + 0.5, "the shield stood at x {sx:.1}, its friends at {mid:.1}, the enemy to the east");
}

/// Warmth lost by a thrower that a shield either covers (between it and
/// the enemy) or doesn't (behind it).
fn warmth_lost(shield_in_front: bool) -> u32 {
    let mut g = game();
    let at = field(&g);
    let target = put(&mut g, 0, Kind::Snowballer, at);
    g.penguins[target as usize].snowballs = 0;
    let shield = put(&mut g, 0, Kind::Shield, at + v2(if shield_in_front { 0.9 } else { -0.9 }, 0.0));
    g.penguins[shield as usize].snowballs = 0;
    let foe = put(&mut g, 1, Kind::Snowballer, at + v2(5.0, 0.0));
    let mut lost = 0;
    for _ in 0..20 * sim::TICK_RATE {
        // Pinned in place, and only the target is in reach of the foe.
        g.penguins[target as usize].pos = at;
        g.penguins[shield as usize].pos = at + v2(if shield_in_front { 0.9 } else { -0.9 }, 0.0);
        g.penguins[foe as usize].pos = at + v2(5.0, 0.0);
        g.penguins[foe as usize].warmth = g.penguins[foe as usize].max_warmth;
        let before = g.penguins[target as usize].warmth;
        g.tick();
        g.events.clear();
        let p = &mut g.penguins[target as usize];
        lost += before.saturating_sub(p.warmth) as u32;
        p.warmth = p.max_warmth;
        p.cold = false;
        p.act = if p.act == Act::Frozen { Act::Idle } else { p.act };
    }
    lost
}

#[test]
fn a_shield_only_covers_those_behind_it() {
    let (covered, open) = (warmth_lost(true), warmth_lost(false));
    assert!(open > 0, "the foe never hit");
    assert!(covered < open, "behind a shield it lost {covered} warmth, with the shield at its back {open}");
}

#[test]
fn a_skier_knocks_a_slinger_flat_and_takes_a_shield_down_with_it() {
    let charge = |victim: Kind| {
        let mut g = game();
        let at = field(&g);
        let ski = put(&mut g, 0, Kind::Skier, at);
        let v = put(&mut g, 1, victim, at + v2(6.0, 0.0));
        g.penguins[v as usize].snowballs = 0;
        g.penguins[ski as usize].snowballs = 0;
        g.command(0, Command::Move { units: vec![ski], target: at + v2(12.0, 0.0), run: true, queue: false });
        let events = run(&mut g, 3.0);
        (events, ski, v)
    };
    let (events, ski, v) = charge(Kind::Slinger);
    assert!(events.iter().any(|e| matches!(e, Event::KnockedDown { penguin, by } if *penguin == v && *by == ski)), "the slinger stayed up");
    let (events, ski, v) = charge(Kind::Shield);
    assert!(events.iter().any(|e| matches!(e, Event::Bounced { skier, shield } if *skier == ski && *shield == v)), "the skier went through a shield");
    assert!(events.iter().any(|e| matches!(e, Event::KnockedDown { penguin, by } if *penguin == v && *by == ski)), "the shield stayed up: the wall never opened");
}

#[test]
fn an_idle_skier_goes_after_a_slinger_rather_than_a_nearer_snowballer() {
    let mut g = game();
    let at = field(&g);
    let ski = put(&mut g, 0, Kind::Skier, at);
    let sling = put(&mut g, 1, Kind::Slinger, at + v2(12.0, 3.0));
    let line = put(&mut g, 1, Kind::Snowballer, at + v2(7.0, -3.0));
    for id in [sling, line] {
        g.penguins[id as usize].snowballs = 0;
    }
    let start = g.penguins[sling as usize].pos.dist(at);
    run(&mut g, 1.5);
    let p = g.penguins[ski as usize].pos;
    assert!(p.dist(g.penguins[sling as usize].pos) < start - 2.0, "the skier didn't head for the slinger");
    assert!(p.y > at.y, "the skier went the snowballer's way");
}

#[test]
fn a_packer_aims_at_a_building_before_the_penguins_beside_it() {
    let mut g = game();
    let home = g.igloos.iter().find(|i| i.owner == 1).unwrap();
    let (door, center) = (home.door, home.center);
    // Beside the enemy door: a snowballer right there, and the igloo itself.
    let out = (door - center).norm();
    let packer = put(&mut g, 0, Kind::Packer, door + out * 3.0);
    let foe = put(&mut g, 1, Kind::Snowballer, door + out * 1.5);
    g.penguins[foe as usize].snowballs = 0;
    let events = run(&mut g, 3.0);
    let first = events.iter().find_map(|e| match e {
        Event::Thrown(s) if s.thrower == packer => Some(s.to),
        _ => None,
    });
    let to = first.expect("the packer never threw");
    assert!(to.dist(center) < to.dist(g.penguins[foe as usize].pos), "its first throw went at the penguin ({to:?}), not the igloo");
}

#[test]
fn a_sled_brings_a_frozen_enemy_home_where_it_joins_the_tribe() {
    let mut g = game();
    let door = g.igloos.iter().find(|i| i.owner == 0).unwrap().door;
    let sled = put(&mut g, 0, Kind::Sledder, door + v2(0.0, 2.0));
    let foe = put(&mut g, 1, Kind::Snowballer, door + v2(0.0, 12.0));
    // Frozen solid out there: nobody's now.
    let hitter = g.penguins[sled as usize].id;
    for _ in 0..20 {
        if g.penguins[foe as usize].act == Act::Frozen {
            break;
        }
        g.hurt_for_test(foe, 3, hitter);
    }
    assert_eq!(g.penguins[foe as usize].act, Act::Frozen);
    let events = run(&mut g, 60.0);
    assert!(events.iter().any(|e| matches!(e, Event::PickedUp { penguin, by } if *penguin == foe && *by == sled)), "the sled never picked it up");
    let p = &g.penguins[foe as usize];
    assert_eq!(p.owner, 0, "the frozen penguin didn't join the sled's tribe");
    assert_ne!(p.act, Act::Frozen, "still frozen");
}

#[test]
fn a_sled_doesnt_fight() {
    assert!(!Kind::Sledder.fights());
    assert!(Kind::Skier.fights() && Kind::Skier.stats().speed > Kind::Sledder.stats().speed);
}

#[test]
fn a_skier_goes_for_a_shield_to_open_the_wall() {
    let mut g = game();
    let at = field(&g);
    let ski = put(&mut g, 0, Kind::Skier, at);
    let v = put(&mut g, 1, Kind::Shield, at + v2(8.0, 0.0));
    g.penguins[v as usize].snowballs = 0;
    let events = run(&mut g, 5.0);
    assert!(events.iter().any(|e| matches!(e, Event::KnockedDown { penguin, by } if *penguin == v && *by == ski)), "the skier left the shield standing");
}
