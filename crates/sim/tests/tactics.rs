//! Penguins fight smart on their own (`game/tactics.rs`): they finish off
//! the nearly frozen and don't pile onto the doomed, sidestep some throws,
//! step out of the ranks to reach an enemy just out of range, and the hurt
//! fall back behind the fresh.

use sim::game::*;
use sim::map::Map;
use sim::math::{v2, Vec2};

fn game(map: &str) -> Game {
    let path = format!("{}/../../assets/maps/{map}.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let mut g = Game::new_with(map, setups, 7, CoreRules::default().map_start());
    g.weather.calm = true;
    g
}

/// A stretch of open level snow, 16 by 9 tiles, far from every igloo.
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

/// One of `owner`'s workers, made a `kind`, stood at `at` and told to hold there.
fn put(g: &mut Game, owner: u8, kind: Kind, at: Vec2) -> u16 {
    let id = g.penguins.iter().find(|p| p.owner == owner && p.kind == Kind::Worker && p.inside.is_none()).or_else(|| g.penguins.iter().find(|p| p.owner == owner && p.kind == Kind::Worker)).expect("a worker").id;
    g.retrain_for_test(id, kind);
    for ig in g.igloos.iter_mut() {
        ig.occupants.retain(|&o| o != id);
    }
    let max = g.max_warmth_of(kind);
    let p = &mut g.penguins[id as usize];
    p.pos = at;
    p.inside = None;
    p.carry = Carry::None;
    p.warmth = max;
    g.command(owner, Command::Hold { units: vec![id] });
    id
}

/// Who `thrower` first picks to throw at, within `secs`.
fn first_target(g: &mut Game, thrower: u16, secs: u32) -> Option<u16> {
    for _ in 0..secs * sim::TICK_RATE {
        g.tick();
        g.events.clear();
        if let Some(t) = g.penguins[thrower as usize].target {
            return Some(t);
        }
    }
    None
}

#[test]
fn a_thrower_finishes_off_the_nearly_frozen() {
    let mut g = game("glacier-crossroads");
    let at = field(&g);
    let me = put(&mut g, 0, Kind::Snowballer, at);
    let fresh = put(&mut g, 1, Kind::Snowballer, at + v2(4.0, 1.5));
    let hurt = put(&mut g, 1, Kind::Snowballer, at + v2(4.0, -1.5));
    // Two hits left in it, and still on its feet (not cold).
    g.penguins[hurt as usize].warmth = COLD + 2;
    let _ = fresh;
    assert_eq!(first_target(&mut g, me, 3), Some(hurt));
}

#[test]
fn nobody_piles_onto_someone_already_as_good_as_frozen() {
    let set_up = |doomed: bool| {
        let mut g = game("glacier-crossroads");
        let at = field(&g);
        let me = put(&mut g, 0, Kind::Snowballer, at);
        let near = put(&mut g, 1, Kind::Snowballer, at + v2(3.0, 0.0));
        let far = put(&mut g, 1, Kind::Snowballer, at + v2(5.0, 1.0));
        if doomed {
            // Snowballs already in the air will freeze the nearer one.
            let w = g.penguins[near as usize].warmth;
            g.aim_for_test(near, w, at);
        }
        (first_target(&mut g, me, 3), near, far)
    };
    let (t, near, _) = set_up(false);
    assert_eq!(t, Some(near), "the nearer one, as a rule");
    let (t, _, far) = set_up(true);
    assert_eq!(t, Some(far), "not the one already as good as frozen");
}

#[test]
fn penguins_sometimes_sidestep_a_snowball_thrown_at_them() {
    let mut g = game("glacier-crossroads");
    let at = field(&g);
    let sling = put(&mut g, 0, Kind::Slinger, at);
    // Out of its own reach, so it only stands there and takes it.
    let target = put(&mut g, 1, Kind::Snowballer, at + v2(8.0, 0.0));
    let mut thrown: Vec<(u32, Vec2)> = Vec::new();
    let (mut landed, mut dodged) = (0, 0);
    for _ in 0..60 * sim::TICK_RATE {
        let max = g.penguins[target as usize].max_warmth;
        let t = &mut g.penguins[target as usize];
        (t.warmth, t.cold, t.courage, t.routed) = (max, false, MAX_COURAGE, false);
        g.tick();
        for e in std::mem::take(&mut g.events) {
            match e {
                Event::Thrown(s) if s.thrower == sling => thrown.push((s.id, g.penguins[target as usize].pos)),
                Event::Landed { id, .. } => {
                    if let Some(k) = thrown.iter().position(|t| t.0 == id) {
                        landed += 1;
                        if g.penguins[target as usize].pos.dist(thrown[k].1) > 0.6 {
                            dodged += 1;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    assert!(landed >= 15, "too few throws to tell: {landed}");
    let share = dodged as f32 / landed as f32;
    assert!(share > 0.1 && share < 0.6, "dodged {dodged} of {landed}");
}

/// Army test set-up: P0's Chieftain on open level ground facing east,
/// with `n` snowballers formed up round him.
fn army(n: usize) -> (Game, u16, Vec<u16>, Vec2) {
    let mut g = game("glacier-crossroads");
    let at = field(&g) + v2(4.0, 0.0);
    let ig = g.igloos.iter().position(|i| i.owner == 0).unwrap() as u8;
    let chief = g.penguins.iter().find(|q| q.owner == 0 && q.kind == Kind::Chieftain && q.home == ig).unwrap().id;
    let soldiers: Vec<u16> = (0..n).map(|_| g.spawn_for_test(ig, Kind::Snowballer)).collect();
    for (k, &s) in soldiers.iter().enumerate() {
        g.penguins[s as usize].pos = at + v2(-1.0, k as f32 - n as f32 / 2.0);
    }
    let c = &mut g.penguins[chief as usize];
    (c.pos, c.facing, c.inside) = (at, 0.0, None);
    for ig in g.igloos.iter_mut() {
        ig.occupants.retain(|&o| o != chief);
    }
    g.command(0, Command::Hold { units: vec![chief] });
    for _ in 0..4 * sim::TICK_RATE {
        g.tick();
        g.events.clear();
    }
    (g, chief, soldiers, at)
}

#[test]
fn the_hurt_fall_back_behind_the_fresh() {
    let (mut g, chief, soldiers, _) = army(6);
    let c = &g.penguins[chief as usize];
    let (at, ahead) = (c.pos, Vec2::from_angle(c.facing));
    let fwd = |g: &Game, s: u16| (g.slot_for_test(s).expect("a place in the ranks") - at).dot(ahead);
    let front = soldiers.iter().copied().max_by(|&a, &b| fwd(&g, a).partial_cmp(&fwd(&g, b)).unwrap()).unwrap();
    assert!(fwd(&g, front) > 0.5, "it stood at the front");
    // Two hits taken: under two thirds warm, not yet cold.
    g.penguins[front as usize].warmth = 6;
    assert!(falls_back(&g.penguins[front as usize]) && 6 > COLD);
    for _ in 0..2 * sim::TICK_RATE {
        g.penguins[front as usize].warmth = 6;
        g.tick();
        g.events.clear();
    }
    assert!(fwd(&g, front) < 0.0, "it went to the back ({:.1})", fwd(&g, front));
    let fresh_ahead = soldiers.iter().filter(|&&s| s != front).filter(|&&s| fwd(&g, s) > 0.5).count();
    assert!(fresh_ahead >= 3, "the fresh hold the front ({fresh_ahead})");
}

#[test]
fn a_soldier_steps_out_of_the_ranks_to_reach_an_enemy_just_out_of_range() {
    let (mut g, chief, soldiers, _) = army(3);
    let c = &g.penguins[chief as usize];
    let (at, ahead) = (c.pos, Vec2::from_angle(c.facing));
    let range = Kind::Snowballer.stats().range;
    // A little past the front rank's reach, and holding still there.
    let spot = at + ahead * (1.5 + range + 2.0);
    let foe = put(&mut g, 1, Kind::Snowballer, spot);
    g.penguins[chief as usize].facing = (spot - at).angle();
    let mut threw = false;
    for _ in 0..8 * sim::TICK_RATE {
        let p = &mut g.penguins[foe as usize];
        (p.warmth, p.cold, p.snowballs) = (p.max_warmth, false, 0);
        g.penguins[chief as usize].order = Order::Hold { pos: at };
        g.tick();
        threw |= g.events.iter().any(|e| matches!(e, Event::Thrown(s) if soldiers.contains(&s.thrower)));
        g.events.clear();
    }
    assert!(threw, "nobody stepped up to throw");
    // None strayed more than a few paces from their places.
    for &s in &soldiers {
        if let Some(place) = g.slot_for_test(s) {
            let d = g.penguins[s as usize].pos.dist(place);
            assert!(d <= STEP_UP + 1.0, "soldier {s} is {d:.1} from its place");
        }
    }
}

#[test]
fn a_cold_penguin_with_no_fire_near_tells_its_owner_once() {
    let mut g = game("glacier-crossroads");
    let at = field(&g);
    let me = put(&mut g, 0, Kind::Snowballer, at);
    let mut told = 0;
    for k in 0..6 * sim::TICK_RATE {
        if k == 0 || k == 3 * sim::TICK_RATE {
            // Hit cold twice, warming up in between: told each time.
            let p = &mut g.penguins[me as usize];
            (p.warmth, p.cold) = (COLD, true);
        }
        if k == 2 * sim::TICK_RATE {
            let p = &mut g.penguins[me as usize];
            (p.warmth, p.cold) = (p.max_warmth, false);
        }
        g.tick();
        told += g.events.iter().filter(|e| matches!(e, Event::NoFire { penguin } if *penguin == me)).count();
        g.events.clear();
    }
    assert_eq!(told, 2, "once per cold spell");
    // The client hears of it.
    let mut enc = sim::proto::SnapshotEncoder::new();
    let mut view = sim::proto::WorldView::new(sim::proto::GameInfo::from_game(&g, 0));
    let snap = enc.encode(&g, &[Event::NoFire { penguin: me }]);
    let evs = view.apply(&snap).unwrap();
    assert!(evs.iter().any(|e| matches!(e, Event::NoFire { penguin } if *penguin == me)));
}
