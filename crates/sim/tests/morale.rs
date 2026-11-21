//! Courage, fleeing, and carrying frozen enemies off as prisoners.

use sim::game::*;
use sim::map::Map;
use sim::math::v2;
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};

fn game() -> Game {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let mut g = Game::new_with(map, setups, 5, CoreRules::default().map_start());
    soldiers(&mut g, 8);
    g
}

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

fn first(g: &Game, owner: u8, kind: Kind) -> u16 {
    g.penguins.iter().find(|p| p.owner == owner && p.kind == kind).unwrap().id
}

fn home(g: &Game, owner: u8) -> usize {
    g.igloos.iter().position(|i| i.owner == owner).unwrap()
}

fn freeze(g: &mut Game, v: u16, at: sim::math::Vec2) {
    let p = &mut g.penguins[v as usize];
    p.pos = at;
    p.warmth = 0;
    p.cold = true;
    p.act = Act::Frozen;
    p.act_t = 0;
}

/// Put every other snowballer to sleep so nothing interferes.
fn calm(g: &mut Game, keep: &[u16]) {
    for p in &mut g.penguins {
        if !keep.contains(&p.id) {
            p.snowballs = 0;
        }
    }
}

#[test]
fn frozen_enemies_are_carried_home_and_change_sides() {
    let mut g = game();
    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    let ig = home(&g, 0);
    let door = g.igloos[ig].door;
    let victim = first(&g, 1, Kind::Snowballer);
    let carrier = first(&g, 0, Kind::Snowballer);
    let spot = g.grid.nearest_walkable((door + v2(0.0, 5.0)).tile(), 4).unwrap();
    freeze(&mut g, victim, v2(spot.0 as f32 + 0.5, spot.1 as f32 + 0.5));
    g.command(0, Command::Capture { units: vec![carrier], penguin: victim });
    let mut picked = false;
    let mut converted = false;
    for _ in 0..60 * sim::TICK_RATE {
        calm(&mut g, &[]);
        g.tick();
        let ev = std::mem::take(&mut g.events);
        for e in &ev {
            match e {
                Event::PickedUp { penguin, .. } if *penguin == victim => picked = true,
                Event::Converted { penguin, to } if *penguin == victim => {
                    assert_eq!(*to, 0);
                    converted = true;
                }
                _ => {}
            }
        }
        view.apply(&enc.encode(&g, &ev)).unwrap();
        if picked && !converted && g.penguins[victim as usize].inside.is_none() {
            // While carried it stays frozen and travels with the carrier.
            let (v, c) = (&g.penguins[victim as usize], &g.penguins[carrier as usize]);
            assert_eq!(v.act, Act::Frozen);
            assert!(v.pos.dist(c.pos) < 0.5);
            assert!(view.penguins[victim as usize].carried);
        }
        if converted {
            break;
        }
    }
    assert!(picked, "carrier picked the frozen penguin up");
    assert!(converted, "the prisoner changed sides");
    let v = &g.penguins[victim as usize];
    assert_eq!(v.owner, 0);
    assert_eq!(v.home as usize, ig);
}

#[test]
fn freezing_the_carrier_frees_the_prisoner() {
    let mut g = game();
    let ig = home(&g, 0);
    let door = g.igloos[ig].door;
    let victim = first(&g, 1, Kind::Snowballer);
    let carrier = first(&g, 0, Kind::Worker);
    let spot = g.grid.nearest_walkable((door + v2(0.0, 9.0)).tile(), 4).unwrap();
    let at = v2(spot.0 as f32 + 0.5, spot.1 as f32 + 0.5);
    freeze(&mut g, victim, at);
    g.penguins[carrier as usize].pos = at + v2(0.5, 0.0);
    g.command(0, Command::Capture { units: vec![carrier], penguin: victim });
    for _ in 0..10 {
        calm(&mut g, &[]);
        g.tick();
    }
    assert_eq!(g.penguins[carrier as usize].carrying, Some(victim));
    // The carrier gets hit until it's too cold to carry.
    g.penguins[carrier as usize].warmth = 3;
    g.penguins[carrier as usize].cold = true;
    let pos = g.penguins[carrier as usize].pos;
    g.penguins[carrier as usize].warmth = 0;
    g.command(0, Command::Stop { units: vec![] });
    // Simulate the hit through the normal path: a cold carrier drops its load.
    g.penguins[carrier as usize].warmth = 4;
    g.penguins[carrier as usize].cold = false;
    calm(&mut g, &[]);
    // Order the carrier elsewhere: it lets go too.
    g.command(0, Command::Move { units: vec![carrier], target: pos + v2(3.0, 0.0), run: false, queue: false });
    g.tick();
    assert_eq!(g.penguins[carrier as usize].carrying, None);
    assert_eq!(g.penguins[victim as usize].carried_by, None);
    // It thaws where it was dropped, still on its own side.
    for _ in 0..(FROZEN_TICKS as u32 + 20) {
        calm(&mut g, &[]);
        g.tick();
    }
    let v = &g.penguins[victim as usize];
    assert_ne!(v.act, Act::Frozen);
    assert_eq!(v.owner, 1);
}

#[test]
fn taking_the_igloo_back_frees_prisoners() {
    let mut g = game();
    let ig = home(&g, 0);
    let door = g.igloos[ig].door;
    let victim = first(&g, 1, Kind::Snowballer);
    let carrier = first(&g, 0, Kind::Snowballer);
    freeze(&mut g, victim, door + v2(0.0, 1.0));
    g.penguins[carrier as usize].pos = door + v2(0.5, 1.0);
    g.command(0, Command::Capture { units: vec![carrier], penguin: victim });
    for _ in 0..5 * sim::TICK_RATE {
        calm(&mut g, &[]);
        g.tick();
    }
    assert_eq!(g.igloos[ig].prisoners, vec![victim]);
    // Releasing lets it walk out, cold but still player 1's.
    g.command(0, Command::Release { igloo: ig as u8 });
    assert!(g.igloos[ig].prisoners.is_empty());
    let v = &g.penguins[victim as usize];
    assert!(v.inside.is_none() && v.owner == 1 && v.cold);
}

#[test]
fn seeing_friends_frozen_breaks_courage_and_it_returns() {
    let mut g = game();
    let ig = home(&g, 0);
    let door = g.igloos[ig].door;
    // A lone worker far from home watches three friends freeze.
    let w = first(&g, 0, Kind::Worker);
    let far = g.grid.nearest_walkable((door + v2(14.0, 0.0)).tile(), 6).unwrap();
    let at = v2(far.0 as f32 + 0.5, far.1 as f32 + 0.5);
    g.penguins[w as usize].pos = at;
    let friends: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Snowballer).take(4).map(|p| p.id).collect();
    for (k, &f) in friends.iter().enumerate() {
        g.penguins[f as usize].pos = at + v2(3.5 + k as f32 * 0.3, 1.0);
    }
    calm(&mut g, &[]);
    g.tick();
    let start = g.penguins[w as usize].courage;
    assert_eq!(start, MAX_COURAGE);
    // Hit each friend until it freezes, through the real snowball path.
    let enemy = first(&g, 1, Kind::Snowballer);
    let mut routed = false;
    for &f in &friends {
        g.penguins[f as usize].warmth = 1;
        g.penguins[f as usize].pos = at + v2(3.5, 0.0);
        g.penguins[enemy as usize].pos = at + v2(3.5, 3.5);
        g.penguins[enemy as usize].snowballs = MAX_SNOWBALLS;
        for _ in 0..3 * sim::TICK_RATE {
            for p in &mut g.penguins {
                // (The thrower holds one at a time: keep it supplied.)
                p.snowballs = if p.id == enemy { MAX_SNOWBALLS } else { 0 };
            }
            if !g.penguins[w as usize].routed {
                g.penguins[w as usize].pos = at;
            }
            g.tick();
            for e in std::mem::take(&mut g.events) {
                match e {
                    Event::Routed { penguin } => routed |= penguin == w,
                    _ => {}
                }
            }
            if g.penguins[f as usize].act == Act::Frozen {
                break;
            }
        }
    }
    assert!(routed, "the worker fled (courage {})", g.penguins[w as usize].courage);
    assert!(g.penguins[w as usize].routed);
    // Fleeing penguins ignore orders...
    g.command(0, Command::Move { units: vec![w], target: at, run: false, queue: false });
    assert!(!matches!(g.penguins[w as usize].order, Order::Move { .. }));
    // ...and calm down once safe.
    g.penguins[enemy as usize].pos = v2(1.5, 1.5);
    for _ in 0..40 * sim::TICK_RATE {
        g.tick();
        g.events.clear();
    }
    let p = &g.penguins[w as usize];
    assert!(!p.routed && p.courage >= RALLY_COURAGE, "courage {} routed {}", p.courage, p.routed);
}
