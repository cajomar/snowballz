//! Warmers: the job is to warm a cold friend.

use sim::game::*;
use sim::map::Map;
use sim::math::{v2, Vec2};
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};

/// The map's start gives each tribe only two soldiers; these tests want more.
fn soldiers(g: &mut Game, n: usize) {
    for owner in 0..2u8 {
        let ids: Vec<u16> = g.penguins.iter().filter(|p| p.owner == owner && p.kind == Kind::Worker).map(|p| p.id).take(n).collect();
        for id in ids {
            let max = g.max_warmth_of(Kind::Snowballer);
            let p = &mut g.penguins[id as usize];
            p.kind = Kind::Snowballer;
            p.max_warmth = max;
            p.warmth = max;
            p.snowballs = Kind::Snowballer.stats().ammo;
        }
    }
}

fn game() -> Game {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2)
        .map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None })
        .collect();
    let mut g = Game::new_with(map, setups, 3, CoreRules::default().map_start());
    g.weather.calm = true;
    soldiers(&mut g, 8);
    g
}

/// Open snowy spots: walkable, snow on them, far from every igloo door.
fn open_spots(g: &Game) -> Vec<Vec2> {
    let mut out = Vec::new();
    for y in 0..g.map.h {
        for x in 0..g.map.w {
            if !g.grid.walkable(x, y) {
                continue;
            }
            let c = v2(x as f32 + 0.5, y as f32 + 0.5);
            if g.snow_at(c) == 0 {
                continue;
            }
            if g.igloos.iter().any(|ig| ig.door.dist(c) < 20.0) {
                continue;
            }
            out.push(c);
        }
    }
    out
}

#[test]
fn warmer_warms_a_cold_friend() {
    let mut g = game();
    let mut v = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 0)));
    let spots = open_spots(&g);
    // The friend in the quiet; the control as far away from it as possible.
    let f = spots[0];
    let c = *spots.iter().max_by(|a, b| a.dist(f).total_cmp(&b.dist(f))).unwrap();
    // The Warmer stands beside the friend, on the snow.
    let wspot = spots
        .iter()
        .find(|p| (0.5..=WARMER_REACH).contains(&p.dist(f)))
        .copied()
        .unwrap_or(f);

    let sbs: Vec<u16> = g
        .penguins
        .iter()
        .filter(|p| p.owner == 0 && p.kind == Kind::Snowballer)
        .map(|p| p.id)
        .collect();
    let (f_id, c_id, w_id) = (sbs[0], sbs[1], sbs[2]);
    for (id, spot) in [(f_id, f), (c_id, c)] {
        let p = &mut g.penguins[id as usize];
        p.pos = spot;
        p.warmth = COLD;
        p.cold = true;
        p.snowballs = 0;
    }
    let w = &mut g.penguins[w_id as usize];
    w.kind = Kind::Warmer;
    w.pos = wspot;
    w.snowballs = 0;

    // Ten seconds: the Warmer tends the friend, the control stays out in the cold.
    for _ in 0..10 * sim::TICK_RATE {
        for p in &mut g.penguins {
            // Bystanders keep out of it.
            if p.id != f_id && p.id != c_id && p.id != w_id {
                p.snowballs = 0;
                if p.inside.is_none() {
                    p.pos = v2(1.5, 1.5);
                }
            }
            // A cold penguin would walk home; keep the two in place.
            if p.id == f_id {
                p.pos = f;
            } else if p.id == c_id {
                p.pos = c;
            }
        }
        g.tick();
        let ev = std::mem::take(&mut g.events);
        v.1.apply(&v.0.encode(&g, &ev)).unwrap();
    }

    let f_w = g.penguins[f_id as usize].warmth;
    let c_w = g.penguins[c_id as usize].warmth;
    assert!(f_w > c_w, "friend {f_w} vs control {c_w}");
    assert!(f_w > COLD, "friend still cold: {f_w}");
    assert_eq!(v.1.penguins[f_id as usize].warmth, f_w);
}

/// A frozen enemy, thawed by a Warmer, wakes up in the Warmer's tribe.
#[test]
fn thawed_penguins_join_the_warmer() {
    let mut g = game();
    let mut v = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 0)));
    let spots = open_spots(&g);
    let at = spots[0];
    // The enemy to freeze, a snowballer to freeze it with, a Warmer in reach.
    let enemy = g.penguins.iter().find(|p| p.owner == 1 && p.inside.is_none()).unwrap().id;
    let sbs: Vec<u16> = g
        .penguins
        .iter()
        .filter(|p| p.owner == 0 && p.kind == Kind::Snowballer)
        .map(|p| p.id)
        .collect();
    let (w_id, thrower) = (sbs[0], sbs[1]);
    let wspot = spots
        .iter()
        .find(|p| (0.5..=WARMER_REACH + 0.8).contains(&p.dist(at)))
        .copied()
        .unwrap_or(at + v2(0.0, 2.0));

    // Freeze it the honest way: a snowballer pelts it until it's ice.
    g.penguins[enemy as usize].pos = at;
    g.penguins[enemy as usize].warmth = 1;
    for _ in 0..20 * sim::TICK_RATE {
        for p in &mut g.penguins {
            if p.id != thrower {
                p.snowballs = 0;
            }
        }
        let t = &mut g.penguins[thrower as usize];
        t.snowballs = 5;
        t.warmth = t.max_warmth;
        t.cold = false;
        t.pos = at + v2(3.0, 0.0);
        if g.penguins[enemy as usize].act != Act::Frozen {
            g.penguins[enemy as usize].pos = at;
        }
        g.tick();
        g.events.clear();
        if g.penguins[enemy as usize].act == Act::Frozen {
            break;
        }
    }
    assert_eq!(g.penguins[enemy as usize].act, Act::Frozen);
    assert_eq!(g.penguins[enemy as usize].owner, NEUTRAL, "freezing ends ownership");

    // Send the Warmer to thaw it.
    let w = &mut g.penguins[w_id as usize];
    w.kind = Kind::Warmer;
    w.pos = wspot;
    w.snowballs = 0;
    g.command(0, Command::Thaw { units: vec![w_id], penguin: enemy });

    // Tick until it wakes up.
    let mut thawed = false;
    for _ in 0..30 * sim::TICK_RATE {
        for p in &mut g.penguins {
            // Bystanders keep out of it.
            if p.id != enemy && p.id != w_id {
                p.snowballs = 0;
                if p.inside.is_none() {
                    p.pos = v2(1.5, 1.5);
                }
            }
        }
        g.tick();
        let ev = std::mem::take(&mut g.events);
        if ev.iter().any(|e| matches!(e, Event::Thawed { penguin, by: 0 } if *penguin == enemy)) {
            thawed = true;
        }
        v.1.apply(&v.0.encode(&g, &ev)).unwrap();
    }
    assert!(thawed, "the Warmer never thawed it");
    let p = &g.penguins[enemy as usize];
    assert_eq!(p.owner, 0, "it joins the Warmer's tribe");
    assert!(!p.wild);
    assert_eq!(v.1.penguins[enemy as usize].owner, p.owner);
}

/// A 7x5 open patch: every tile walkable, the four standing spots on snow,
/// far from every igloo door. Returns its center.
fn open_patch(g: &Game) -> Vec2 {
    for y in 0..g.map.h - 4 {
        for x in 0..g.map.w - 6 {
            let mut ok = true;
            for dy in 0..5 {
                for dx in 0..7 {
                    if !g.grid.walkable(x + dx, y + dy) {
                        ok = false;
                    }
                }
            }
            if !ok {
                continue;
            }
            let c = v2(x as f32 + 3.5, y as f32 + 2.5);
            // The patient, the Warmers and the thrower all stand on snow.
            let stand = [c, c + v2(2.0, 0.0), c + v2(0.0, 2.0), c + v2(3.0, 0.0)];
            if stand.iter().any(|p| g.snow_at(*p) == 0) {
                continue;
            }
            if g.igloos.iter().any(|ig| ig.door.dist(c) < 20.0) {
                continue;
            }
            return c;
        }
    }
    panic!("no open patch");
}

/// Freeze a foe at `at` the honest way: one snowballer pellets it until it's
/// ice. Returns the frozen penguin's id.
fn freeze_a_foe(g: &mut Game, at: Vec2) -> u16 {
    let enemy = g.penguins.iter().find(|p| p.owner == 1 && p.inside.is_none()).unwrap().id;
    let thrower = g
        .penguins
        .iter()
        .find(|p| p.owner == 0 && p.kind == Kind::Snowballer)
        .unwrap()
        .id;
    g.penguins[enemy as usize].pos = at;
    g.penguins[enemy as usize].warmth = 1;
    for _ in 0..20 * sim::TICK_RATE {
        for p in &mut g.penguins {
            if p.id != thrower {
                p.snowballs = 0;
            }
        }
        let t = &mut g.penguins[thrower as usize];
        t.snowballs = 5;
        t.warmth = t.max_warmth;
        t.cold = false;
        t.pos = at + v2(3.0, 0.0);
        if g.penguins[enemy as usize].act != Act::Frozen {
            g.penguins[enemy as usize].pos = at;
        }
        g.tick();
        g.events.clear();
        if g.penguins[enemy as usize].act == Act::Frozen {
            break;
        }
    }
    assert_eq!(g.penguins[enemy as usize].act, Act::Frozen);
    assert_eq!(g.penguins[enemy as usize].owner, NEUTRAL, "freezing ends ownership");
    enemy
}

/// The ticks it takes `n` Warmers, all in reach, to thaw a frozen foe.
fn thaw_ticks(n: usize) -> u32 {
    let mut g = game();
    let mut v = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 0)));
    let at = open_patch(&g);
    let enemy = freeze_a_foe(&mut g, at);
    let sbs: Vec<u16> = g
        .penguins
        .iter()
        .filter(|p| p.owner == 0 && p.kind == Kind::Snowballer)
        .map(|p| p.id)
        .collect();
    assert!(sbs.len() > n, "not enough snowballers to make Warmers");
    // The first snowballer was the thrower; the next n become Warmers, in reach.
    let warmers: Vec<u16> = sbs[1..1 + n].to_vec();
    for (i, w) in warmers.iter().enumerate() {
        let p = &mut g.penguins[*w as usize];
        p.kind = Kind::Warmer;
        p.snowballs = 0;
        // They walk up fresh, not mid-scoop from the freezing.
        p.act = Act::Idle;
        p.act_t = 0;
        p.pos = if i == 0 { at + v2(2.0, 0.0) } else { at + v2(0.0, 2.0) };
    }
    // separate() nudges the frozen one; put it back where the spots assume.
    g.penguins[enemy as usize].pos = at;
    g.command(0, Command::Thaw { units: warmers.clone(), penguin: enemy });

    // Tick until it wakes up, counting.
    let mut ticks = 0u32;
    for _ in 0..30 * sim::TICK_RATE {
        for p in &mut g.penguins {
            // Bystanders keep out of it.
            if p.id != enemy && !warmers.contains(&p.id) {
                p.snowballs = 0;
                if p.inside.is_none() {
                    p.pos = v2(1.5, 1.5);
                }
            }
        }
        g.tick();
        ticks += 1;
        let ev = std::mem::take(&mut g.events);
        v.1.apply(&v.0.encode(&g, &ev)).unwrap();
        if ev.iter().any(|e| matches!(e, Event::Thawed { penguin, by: 0 } if *penguin == enemy)) {
            break;
        }
    }
    let p = &g.penguins[enemy as usize];
    assert!(!matches!(p.act, Act::Frozen), "no Warmer thawed the enemy in time");
    assert_eq!(p.owner, 0, "it joins the Warmer's tribe");
    assert_eq!(v.1.penguins[enemy as usize].act, p.act);
    assert_eq!(v.1.penguins[enemy as usize].owner, p.owner);
    ticks
}

/// A Thaw order for a penguin that isn't frozen does nothing: an idle
/// Warmer stays idle, and a Warmer with an old order keeps it.
#[test]
fn thaw_order_on_an_unfrozen_penguin_does_nothing() {
    let mut g = game();
    let mut v = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 0)));
    let at = open_patch(&g);
    let sbs: Vec<u16> = g
        .penguins
        .iter()
        .filter(|p| p.owner == 0 && p.kind == Kind::Snowballer)
        .map(|p| p.id)
        .collect();
    assert!(sbs.len() >= 3, "not enough snowballers");
    let (w_id, friend, target) = (sbs[0], sbs[1], sbs[2]);

    // An idle Warmer and a warm friend: the order must not start any work.
    let f = &mut g.penguins[friend as usize];
    f.pos = at;
    f.warmth = f.max_warmth;
    f.cold = false;
    f.snowballs = 0;
    let w = &mut g.penguins[w_id as usize];
    w.kind = Kind::Warmer;
    w.pos = at + v2(2.0, 0.0);
    w.snowballs = 0;
    w.order = Order::None;
    g.command(0, Command::Thaw { units: vec![w_id], penguin: friend });
    assert_eq!(g.penguins[w_id as usize].order, Order::None, "idle Warmer stays idle");
    for _ in 0..10 * sim::TICK_RATE {
        for p in &mut g.penguins {
            // Bystanders keep out of it; the friend stays warm and in place.
            if p.id != w_id && p.id != friend {
                p.snowballs = 0;
                if p.inside.is_none() {
                    p.pos = v2(1.5, 1.5);
                }
            } else if p.id == friend {
                p.pos = at;
                p.warmth = p.max_warmth;
                p.cold = false;
                p.snowballs = 0;
            }
        }
        g.tick();
        let ev = std::mem::take(&mut g.events);
        v.1.apply(&v.0.encode(&g, &ev)).unwrap();
        assert!(
            !ev.iter().any(|e| matches!(e, Event::Thawed { penguin, .. } if *penguin == friend)),
            "a penguin that isn't frozen cannot be thawed"
        );
    }
    assert_eq!(g.penguins[w_id as usize].order, Order::None, "idle Warmer stays idle");
    assert_ne!(g.penguins[friend as usize].act, Act::Frozen);
    assert_eq!(g.penguins[friend as usize].owner, 0);

    // A Warmer already sent to a frozen foe keeps that order.
    let foe = freeze_a_foe(&mut g, at);
    g.penguins[foe as usize].pos = at;
    let w = &mut g.penguins[w_id as usize];
    w.pos = at + v2(2.0, 0.0);
    w.act = Act::Idle;
    w.act_t = 0;
    g.command(0, Command::Thaw { units: vec![w_id], penguin: foe });
    assert_eq!(g.penguins[w_id as usize].order, Order::Thaw { penguin: foe });
    let t = &mut g.penguins[target as usize];
    t.pos = at + v2(0.0, 2.0);
    t.warmth = t.max_warmth;
    t.cold = false;
    t.snowballs = 0;
    g.command(0, Command::Thaw { units: vec![w_id], penguin: target });
    assert_eq!(
        g.penguins[w_id as usize].order,
        Order::Thaw { penguin: foe },
        "a Thaw for an unfrozen penguin must not touch the old order"
    );
    // A few seconds on: the Warmer still tends the foe; the target is untouched.
    for _ in 0..3 * sim::TICK_RATE {
        for p in &mut g.penguins {
            if p.id != foe && p.id != w_id {
                p.snowballs = 0;
                if p.inside.is_none() {
                    p.pos = v2(1.5, 1.5);
                }
            }
        }
        g.tick();
        let ev = std::mem::take(&mut g.events);
        v.1.apply(&v.0.encode(&g, &ev)).unwrap();
    }
    assert_eq!(g.penguins[w_id as usize].order, Order::Thaw { penguin: foe });
    assert!(matches!(g.penguins[foe as usize].act, Act::Frozen), "the Warmer is still thawing the foe");
    assert_ne!(g.penguins[target as usize].act, Act::Frozen);
    assert_eq!(g.penguins[target as usize].owner, 0);
    assert_eq!(v.1.penguins[target as usize].act, g.penguins[target as usize].act);
}

/// Two Warmers thaw a frozen penguin no faster than one: each runs its own
/// thaw timer, so a second Warmer adds no speed.
#[test]
fn two_warmers_do_not_thaw_faster() {
    let one = thaw_ticks(1);
    let two = thaw_ticks(2);
    eprintln!("thawed in {one} tick(s) with one Warmer, {two} with two");
    assert!(one > 0);
    assert_eq!(two, one, "the per-Warmer thaw timer gives two Warmers no speed");
}
