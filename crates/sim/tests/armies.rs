//! Armies: every igloo has a Chieftain, its soldiers follow him and form up
//! around him, a frozen Chieftain is replaced from home, and watchtowers
//! man themselves.

use sim::game::*;
use sim::map::Map;
use sim::math::{v2, Vec2};
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};

fn game() -> Game {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let mut g = Game::new_with(map, setups, 7, CoreRules::default().map_start());
    g.weather.calm = true;
    g
}

fn run(g: &mut Game, secs: u32) {
    for _ in 0..secs * sim::TICK_RATE {
        g.tick();
        g.events.clear();
    }
}

fn home(g: &Game, p: u8) -> u8 {
    g.igloos.iter().position(|i| i.owner == p).unwrap() as u8
}

fn chief(g: &Game, p: u8) -> u16 {
    g.penguins.iter().find(|q| q.owner == p && q.kind == Kind::Chieftain).expect("a chieftain").id
}

fn soldiers(g: &Game, p: u8) -> Vec<u16> {
    g.penguins.iter().filter(|q| q.owner == p && q.kind.fights() && q.kind != Kind::Chieftain && q.is_out()).map(|q| q.id).collect()
}

/// Open walkable ground `d` tiles from the igloo door, away from everything.
fn field(g: &Game, p: u8, d: f32) -> Vec2 {
    let door = g.igloos[home(g, p) as usize].door;
    for k in 0..64 {
        let at = door + Vec2::from_angle(k as f32 / 64.0 * 6.283) * d;
        let (x, y) = at.tile();
        if (-3..=3).all(|dy| (-3..=3).all(|dx| g.grid.walkable(x + dx, y + dy))) {
            return at;
        }
    }
    // Crowded round the door: settle for room for a small huddle.
    for k in 0..64 {
        let at = door + Vec2::from_angle(k as f32 / 64.0 * 6.283) * d;
        let (x, y) = at.tile();
        if (-1..=1).all(|dy| (-1..=1).all(|dx| g.grid.walkable(x + dx, y + dy))) {
            return at;
        }
    }
    panic!("no open ground");
}

#[test]
fn a_tribe_starts_with_its_chieftain_and_two_soldiers() {
    let g = game();
    for p in 0..2 {
        let chiefs = g.penguins.iter().filter(|q| q.owner == p && q.kind == Kind::Chieftain).count();
        assert_eq!(chiefs, 1);
        assert_eq!(soldiers(&g, p).len(), START_SOLDIERS as usize);
        assert!(g.penguins.iter().filter(|q| q.owner == p && q.kind == Kind::Worker).count() > 5, "the rest are workers");
    }
}

#[test]
fn the_army_follows_its_chieftain_and_spreads_out() {
    let mut g = game();
    let ig = home(&g, 0);
    for _ in 0..6 {
        g.spawn_for_test(ig, Kind::Snowballer);
    }
    let c = chief(&g, 0);
    let to = field(&g, 0, 14.0);
    g.command(0, Command::Move { units: vec![c], target: to, run: false, queue: false });
    run(&mut g, 30);
    let at = g.penguins[c as usize].pos;
    assert!(at.dist(to) < 2.0, "the chieftain got there");
    let army = soldiers(&g, 0);
    for &s in &army {
        let d = g.penguins[s as usize].pos.dist(at);
        assert!(d < 7.0, "soldier {s} is {d:.1} tiles from its chieftain");
    }
    for (k, &a) in army.iter().enumerate() {
        for &b in &army[k + 1..] {
            let d = g.penguins[a as usize].pos.dist(g.penguins[b as usize].pos);
            assert!(d > 1.0, "soldiers {a} and {b} stand in a clump ({d:.2} apart)");
        }
    }
}

#[test]
fn a_soldier_sent_off_alone_stays_where_it_was_sent() {
    let mut g = game();
    let s = soldiers(&g, 0)[0];
    let spot = field(&g, 0, 9.0);
    g.command(0, Command::Move { units: vec![s], target: spot, run: false, queue: false });
    run(&mut g, 20);
    assert!(g.penguins[s as usize].pos.dist(spot) < 1.5, "it went where it was told");
    let c = chief(&g, 0);
    let far = field(&g, 0, 16.0);
    g.command(0, Command::Move { units: vec![c], target: far, run: false, queue: false });
    run(&mut g, 20);
    assert!(g.penguins[s as usize].pos.dist(spot) < 1.5, "it stayed put instead of following");
}

#[test]
fn soldiers_picked_with_their_chieftain_fall_in_behind_him() {
    let mut g = game();
    let s = soldiers(&g, 0)[0];
    let spot = field(&g, 0, 9.0);
    g.command(0, Command::Move { units: vec![s], target: spot, run: false, queue: false });
    run(&mut g, 15);
    // Now pick it together with its chieftain and send them both somewhere.
    let c = chief(&g, 0);
    let far = field(&g, 0, 16.0);
    g.command(0, Command::Move { units: vec![c, s], target: far, run: false, queue: false });
    assert_eq!(g.penguins[s as usize].order, Order::None, "it falls in rather than walking to the spot");
    run(&mut g, 25);
    let d = g.penguins[s as usize].pos.dist(g.penguins[c as usize].pos);
    assert!(d < 6.0, "back with its chieftain ({d:.1} tiles off)");
}

#[test]
fn a_frozen_chieftain_is_replaced_from_home_after_a_while() {
    let mut g = game();
    let mut view = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 0)));
    let c = chief(&g, 0);
    let people = g.penguins.len();
    let enemy = chief(&g, 1);
    g.hurt_for_test(c, 255, enemy);
    assert_eq!(g.penguins[c as usize].kind, Kind::Snowballer, "the crown goes home: the frozen one is a plain snowballer");
    assert!(g.penguins.iter().all(|q| !(q.owner == 0 && q.kind == Kind::Chieftain)), "no chieftain for now");
    for _ in 0..(CHIEF_RETURN_TICKS / sim::TICK_RATE - 5) * sim::TICK_RATE {
        g.tick();
        let ev = std::mem::take(&mut g.events);
        view.1.apply(&view.0.encode(&g, &ev)).unwrap();
    }
    assert!(g.penguins.iter().all(|q| !(q.owner == 0 && q.kind == Kind::Chieftain)), "back too soon");
    for _ in 0..10 * sim::TICK_RATE {
        g.tick();
        let ev = std::mem::take(&mut g.events);
        view.1.apply(&view.0.encode(&g, &ev)).unwrap();
    }
    let new = chief(&g, 0);
    assert_ne!(new, c);
    assert_eq!(g.penguins[new as usize].home, home(&g, 0));
    assert!((new as usize) < people, "one of its own penguins was raised, nobody new was made");
    assert_eq!(view.1.penguins[new as usize].kind, Kind::Chieftain, "the client sees the new chieftain");
}

#[test]
fn a_founded_igloo_raises_its_own_chieftain() {
    let mut g = game();
    let free = g.igloos.iter().position(|i| i.owner == NEUTRAL).unwrap();
    g.igloos[free].owner = 0;
    let w = g.penguins.iter().find(|q| q.owner == 0 && q.kind == Kind::Worker).unwrap().id;
    g.penguins[w as usize].home = free as u8;
    g.penguins[w as usize].pos = g.igloos[free].door + v2(1.0, 0.0);
    run(&mut g, 2);
    let chiefs: Vec<u8> = g.penguins.iter().filter(|q| q.owner == 0 && q.kind == Kind::Chieftain).map(|q| q.home).collect();
    assert_eq!(chiefs.len(), 2, "one chieftain per igloo");
    assert!(chiefs.contains(&(free as u8)));
}

#[test]
fn a_watchtower_mans_itself() {
    let mut g = game();
    let ig = home(&g, 0);
    for _ in 0..4 {
        g.spawn_for_test(ig, Kind::Snowballer);
    }
    let door = g.igloos[ig as usize].door;
    let t = (0..32)
        .map(|k| (door + Vec2::from_angle(k as f32 / 32.0 * 6.283) * 6.0).tile())
        .find(|&t| g.placement_problem(0, BuildKind::Tower, t).is_none())
        .expect("room for a tower");
    let b = g.place_finished_for_test(0, BuildKind::Tower, t);
    run(&mut g, 20);
    assert_eq!(g.garrison(b).count(), TOWER_SENTRIES, "sentries climbed up on their own");
    assert!(g.garrison(b).all(|p| p.kind != Kind::Chieftain), "not the chieftain");
}

/// Enemy snowballers (P1's workers, retrained) standing still at `spots`.
fn foes_at(g: &mut Game, spots: &[Vec2]) {
    for &at in spots {
        let id = g.penguins.iter().find(|q| q.owner == 1 && q.kind == Kind::Worker && q.is_out()).expect("a P1 worker").id;
        g.retrain_for_test(id, Kind::Snowballer);
        let p = &mut g.penguins[id as usize];
        p.pos = at;
        p.order = Order::Hold { pos: at };
        p.snowballs = 0;
    }
}

/// Run with everyone kept warm, so nobody freezes or runs home.
fn calm(g: &mut Game, secs: u32) {
    for _ in 0..secs * sim::TICK_RATE {
        for p in g.penguins.iter_mut().filter(|p| p.owner != NEUTRAL && p.act != Act::Frozen) {
            p.warmth = p.max_warmth;
        }
        g.tick();
        g.events.clear();
    }
}

fn kinds(g: &Game, kind: Kind) -> Vec<Vec2> {
    g.penguins.iter().filter(|q| q.owner == 0 && q.kind == kind && q.is_out() && q.post.is_none()).map(|q| q.pos).collect()
}

#[test]
fn shields_wall_off_the_enemy_with_gaps_and_slingers_stand_back() {
    let mut g = game();
    let ig = home(&g, 0);
    for k in [Kind::Shield, Kind::Shield, Kind::Shield, Kind::Shield, Kind::Snowballer, Kind::Snowballer, Kind::Snowballer, Kind::Slinger, Kind::Slinger] {
        g.spawn_for_test(ig, k);
    }
    calm(&mut g, 5);
    let c = chief(&g, 0);
    let e = field(&g, 0, 13.0);
    let at = g.penguins[c as usize].pos;
    let ahead = (e - at).norm();
    let side = v2(-ahead.y, ahead.x);
    foes_at(&mut g, &[e, e + side * 1.5, e - side * 1.5]);
    calm(&mut g, 20);
    // (The chieftain stops short of them, the army facing their middle.)
    let at = g.penguins[c as usize].pos;
    let foes: Vec<Vec2> = g.penguins.iter().filter(|q| q.owner == 1 && q.kind == Kind::Snowballer && q.pos.dist(at) < 16.0).map(|q| q.pos).collect();
    let mid = foes.iter().fold(Vec2::ZERO, |a, &b| a + b) * (1.0 / foes.len() as f32);
    let ahead = (mid - at).norm();
    let side = v2(-ahead.y, ahead.x);
    let fwd = |p: Vec2| (p - at).dot(ahead);
    let shields = kinds(&g, Kind::Shield);
    let throwers = kinds(&g, Kind::Snowballer);
    let slingers = kinds(&g, Kind::Slinger);
    let wall = shields.iter().map(|&p| fwd(p)).fold(f32::MAX, f32::min);
    assert!(wall > 1.2, "the wall stands ahead of the chieftain, toward the enemy ({wall:.1})");
    for &t in &throwers {
        assert!(fwd(t) < wall + 0.1, "a thrower stands in front of the wall ({:.1} vs {wall:.1})", fwd(t));
    }
    let line = throwers.iter().map(|&p| fwd(p)).fold(f32::MAX, f32::min);
    for &s in &slingers {
        assert!(fwd(s) < line, "a slinger stands among the throwers ({:.1} vs {line:.1})", fwd(s));
    }
    let mut across: Vec<f32> = shields.iter().map(|&p| (p - at).dot(side)).collect();
    across.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let gaps: Vec<f32> = across.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(gaps.iter().any(|&d| d > 1.6), "no gap in the wall: {gaps:?}");
    assert!(gaps.iter().any(|&d| d < 1.5), "the shields stand apart, not in pairs: {gaps:?}");
}

#[test]
fn surrounded_the_shields_form_a_ring() {
    let mut g = game();
    let ig = home(&g, 0);
    for _ in 0..4 {
        g.spawn_for_test(ig, Kind::Shield);
    }
    let c = chief(&g, 0);
    let to = field(&g, 0, 14.0);
    g.command(0, Command::Move { units: vec![c], target: to, run: false, queue: false });
    calm(&mut g, 25);
    let at = g.penguins[c as usize].pos;
    let dirs = [v2(1.0, 0.0), v2(0.0, 1.0), v2(-1.0, 0.0), v2(0.0, -1.0)];
    foes_at(&mut g, &dirs.map(|d| at + d * 9.0));
    calm(&mut g, 10);
    let at = g.penguins[c as usize].pos;
    let shields = kinds(&g, Kind::Shield);
    for d in dirs {
        let covered = shields.iter().any(|&s| s.dist(at) > 1.5 && (s - at).norm().dot(d) > 0.5);
        assert!(covered, "no shield toward {d:?}: {shields:?} round {at:?}");
    }
}

#[test]
fn a_slinger_finds_cover_behind_a_wall() {
    let mut g = game();
    let door = g.igloos[home(&g, 0) as usize].door;
    let t = (0..32)
        .map(|k| (door + Vec2::from_angle(k as f32 / 32.0 * 6.283) * 8.0).tile())
        .find(|&t| g.placement_problem(0, BuildKind::Palisade, t).is_none() && g.grid.walkable(t.0 - 2, t.1) && g.grid.walkable(t.0 - 1, t.1))
        .expect("room for a wall");
    g.place_finished_for_test(0, BuildKind::Palisade, t);
    let place = v2(t.0 as f32 - 1.5, t.1 as f32 + 0.5);
    let spot = g.cover_near(place, v2(1.0, 0.0), &[]).expect("cover next to the wall");
    let (fx, fy) = (spot + v2(1.0, 0.0)).tile();
    assert!(g.blocks_snowballs(fx, fy), "the wall is right in front of {spot:?}");
    assert!(g.grid.walkable_at(spot));
    // Facing the other way, the wall is no cover from there.
    let away = g.cover_near(place, v2(-1.0, 0.0), &[]);
    assert!(away.is_none_or(|s| s.dist(spot) > 0.5), "cover on the wrong side of the wall");
}

#[test]
fn buildings_describe_what_they_do() {
    // A camp fills its own slots with workers who mine a crystal.
    let camp = BuildKind::PackerYard.describe();
    assert!(camp.contains("slot") && camp.contains("crystal") && !camp.contains("sent here"), "{camp}");
    // Nothing stores wood, and towers man themselves.
    assert!(!BuildKind::Store.describe().contains("wood"));
    assert!(BuildKind::Tower.describe().contains("by themselves"));
}
