//! Wildlife: where seals lurk, lone penguins being snatched, groups being
//! safe, snowballs driving seals off, and the client staying in step.

use sim::game::*;
use sim::map::Map;
use sim::math::v2;
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};

fn game(map_name: &str, players: usize, seed: u64) -> Game {
    let path = format!("{}/../../assets/maps/fixtures/{map_name}.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..players).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    // The old rules: a frozen penguin thaws back into its tribe in seconds,
    // where the new core leaves it neutral ice for minutes.
    Game::new_with(map, setups, seed, CoreRules::default().map_start())
}

fn home(g: &Game, player: u8) -> usize {
    g.igloos.iter().position(|i| i.owner == player).unwrap()
}

fn workers(g: &Game, player: u8) -> Vec<u16> {
    g.penguins.iter().filter(|p| p.owner == player && p.kind == Kind::Worker).map(|p| p.id).collect()
}

/// Run for `secs` of game time, keeping the client's view up to date and
/// collecting every event along the way.
fn run(g: &mut Game, secs: u32, view: &mut (SnapshotEncoder, WorldView)) -> Vec<Event> {
    let mut all = Vec::new();
    for _ in 0..secs * sim::TICK_RATE {
        g.tick();
        let ev = std::mem::take(&mut g.events);
        view.1.apply(&view.0.encode(g, &ev)).unwrap();
        all.extend(ev);
    }
    all
}

/// Send everyone but `keeps` indoors so nothing gets in the way.
fn quiet(g: &mut Game, keeps: &[u16]) {
    for p in &mut g.penguins {
        if !keeps.contains(&p.id) {
            p.inside = Some(p.home);
        }
    }
}

/// Make `p` stand still at its present position.
fn hold(g: &mut Game, p: u16) {
    let pos = g.penguins[p as usize].pos;
    g.penguins[p as usize].order = Order::Hold { pos };
}

/// Put seal 0 on the ice just east of `p`'s igloo door, three tiles from
/// it, fresh out of hiding. Seals never spawn that near a territory, so it
/// is ours alone to watch.
fn park_seal(g: &mut Game, player: u8) {
    let door = g.igloos[home(g, player)].door;
    let s = &mut g.seals[0];
    s.pos = door + v2(4.0, 0.0);
    s.state = SealState::Lurking;
    s.hp = SEAL_HP;
    s.cooldown = 0;
    s.struck = None;
}

#[test]
fn seals_spawn_in_the_same_spots_and_outside_igloo_land() {
    // Spot choice is a pure function of the map, not of the game seed.
    let a = game("trio", 2, 3);
    let b = game("trio", 2, 77);
    assert!(a.seals.len() >= 2 && a.seals.len() <= 8, "{} seals", a.seals.len());
    for (x, y) in a.seals.iter().zip(&b.seals) {
        assert_eq!(x.pos, y.pos, "same map, same seal spots");
        assert_eq!(x.state, SealState::Lurking);
    }
    for s in &a.seals {
        for ig in &a.igloos {
            assert!(s.pos.dist(ig.center) >= 14.0, "seal {:?} too near igloo {}", s.pos, ig.id);
        }
    }
    // A different map gets its own hunting grounds, still out of the fire.
    let c = game("crystal-six", 2, 3);
    assert!(c.seals.len() >= 2, "{} seals on crystal-six", c.seals.len());
    for s in &c.seals {
        assert!(c.igloos.iter().all(|ig| s.pos.dist(ig.center) >= 14.0));
    }
}

#[test]
fn a_lone_penguin_gets_snatched_and_brought_back_frozen() {
    let mut g = game("trio", 2, 3);
    let ig = home(&g, 0);
    let mut v = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 0)));
    let w = workers(&g, 0)[0];
    // The victim stands close by the door, one tile from the parked seal;
    // everyone else keeps out of the way.
    g.penguins[w as usize].pos = g.igloos[ig].door + v2(3.0, 0.0);
    park_seal(&mut g, 0);
    quiet(&mut g, &[w]);
    hold(&mut g, w);
    // The seal waits until it is sure, then strikes. Check it as soon as
    // the snatch lands, before the thaw.
    let mut events = Vec::new();
    for s in 0..15 {
        events.extend(run(&mut g, 1, &mut v));
        if s == 14 || events.iter().any(|e| matches!(e, Event::SealSnatched { .. })) {
            break;
        }
    }
    assert!(events.iter().any(|e| matches!(e, Event::SealSurfaced { seal: 0 })), "it came up for him");
    assert!(events.iter().any(|e| matches!(e, Event::SealSnatched { seal: 0, .. })), "and took him");
    assert_eq!(g.seals[0].state, SealState::Gone, "the seal dove with its catch");
    assert!(g.seals[0].struck.is_some(), "no catch recorded");
    let p = &g.penguins[w as usize];
    assert!(p.cold && p.act == Act::Frozen, "warmth {}, act {:?}", p.warmth, p.act);
    assert!(p.carried_by.is_none() && p.order == Order::None, "no one dragged it anywhere");

    // Left out on the ice it stays frozen: nobody owns it, and the thaw is slow.
    run(&mut g, 20, &mut v);
    assert!(g.penguins[w as usize].act == Act::Frozen && g.penguins[w as usize].carried_by.is_none());
    check_view(&g, &v.1);
}

#[test]
fn a_seal_rests_between_snatches() {
    let mut g = game("trio", 2, 3);
    let ig = home(&g, 0);
    let mut v = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 0)));
    let ws = workers(&g, 0);
    let (w, next) = (ws[0], ws[1]);
    g.penguins[w as usize].pos = g.igloos[ig].door + v2(3.0, 0.0);
    park_seal(&mut g, 0);
    quiet(&mut g, &[w]);
    hold(&mut g, w);
    let mut events = Vec::new();
    for _ in 0..15 {
        events.extend(run(&mut g, 1, &mut v));
        if events.iter().any(|e| matches!(e, Event::SealSnatched { .. })) {
            break;
        }
    }
    assert_eq!(events.iter().filter(|e| matches!(e, Event::SealSnatched { .. })).count(), 1, "one snatch");
    // Another penguin takes the first one's place, and the seal is back up
    // at once. It still has to rest before it may take anyone.
    g.penguins[next as usize].pos = g.igloos[ig].door + v2(3.0, 0.0);
    g.penguins[next as usize].inside = None;
    quiet(&mut g, &[next]);
    hold(&mut g, next);
    g.seals[0].state = SealState::Lurking;
    let events = run(&mut g, 30, &mut v);
    assert!(events.iter().all(|e| !matches!(e, Event::SealSnatched { .. })), "it came back too soon");
    // Once the rest is over it hunts again.
    let events = run(&mut g, 10, &mut v);
    assert!(events.iter().any(|e| matches!(e, Event::SealSnatched { .. })), "it never hunts again");
    check_view(&g, &v.1);
}

#[test]
fn a_group_of_three_is_nobody_food() {
    let mut g = game("trio", 2, 3);
    let ig = home(&g, 0);
    let mut v = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 0)));
    let ws: Vec<u16> = workers(&g, 0).into_iter().take(3).collect();
    // A tight huddle one tile short of the parked seal: rungs in the
    // lunge's reach but as a group, not food.
    for (k, &p) in ws.iter().enumerate() {
        g.penguins[p as usize].pos = g.igloos[ig].door + v2(3.0 + k as f32 * 0.3, k as f32 * 0.4);
    }
    park_seal(&mut g, 0);
    quiet(&mut g, &ws);
    for &p in &ws {
        hold(&mut g, p);
    }
    let events = run(&mut g, 12, &mut v);
    assert!(events.iter().all(|e| !matches!(e, Event::SealSnatched { .. })), "nobody snatched");
    assert!(g.seals[0].state == SealState::Lurking, "it stayed under");

    // With the huddle gone the same seal hunts the one left behind.
    quiet(&mut g, &[ws[0]]);
    let events = run(&mut g, 10, &mut v);
    assert!(events.iter().any(|e| matches!(e, Event::SealSnatched { .. })), "the lone one gets taken");
}

#[test]
fn snowballs_drive_a_seal_back_under() {
    let mut g = game("trio", 2, 3);
    let ig = home(&g, 0);
    let mut v = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 0)));
    let throwers: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Snowballer).map(|p| p.id).take(2).collect();
    assert!(throwers.len() == 2, "need two throwers");
    quiet(&mut g, &throwers);
    // Two snowballers opposite the seal: both in range to throw, both out
    // of reach of its lunge, but not a big enough crowd to scare it.
    let door = g.igloos[ig].door;
    g.penguins[throwers[0] as usize].pos = door + v2(1.0, 0.0);
    g.penguins[throwers[1] as usize].pos = door + v2(2.0, -2.0);
    for &t in &throwers {
        hold(&mut g, t);
    }
    park_seal(&mut g, 0);
    let mut driven = false;
    for _ in 0..15 {
        let events = run(&mut g, 1, &mut v);
        if let Some(Event::SealDriven { seal: 0, by }) = events.iter().find(|e| matches!(e, Event::SealDriven { .. })) {
            assert_eq!(*by, 0, "the player who threw gets the credit");
            driven = true;
            break;
        }
    }
    assert!(driven, "snowballs put it under (hp {})", g.seals[0].hp);
    assert_eq!(g.seals[0].state, SealState::Gone);

    // It keeps away for a while, then creeps back.
    let mut seen = false;
    for _ in 0..50 * sim::TICK_RATE {
        if g.seals[0].state == SealState::Lurking {
            seen = true;
            break;
        }
        g.tick();
        let ev = std::mem::take(&mut g.events);
        v.1.apply(&v.0.encode(&g, &ev)).unwrap();
    }
    assert!(seen, "it never came back");
    // The client saw all of that too.
    check_view(&g, &v.1);
}

#[test]
fn the_client_sees_what_the_seals_are_doing() {
    let mut g = game("trio", 2, 3);
    let mut v = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 0)));
    // The first snapshot tells a late joiner about every seal.
    run(&mut g, 2, &mut v);
    check_view(&g, &v.1);

    // A seal strike and the aftermath stay in step on the wire.
    let ig = home(&g, 0);
    let w = workers(&g, 0)[0];
    g.penguins[w as usize].pos = g.igloos[ig].door + v2(3.0, 0.0);
    park_seal(&mut g, 0);
    quiet(&mut g, &[w]);
    hold(&mut g, w);
    let events = run(&mut g, 50, &mut v);
    assert!(events.iter().any(|e| matches!(e, Event::SealSnatched { seal: 0, .. })));
    check_view(&g, &v.1);
}

fn check_view(g: &Game, view: &WorldView) {
    assert_eq!(view.seals.len(), g.seals.len(), "seal count");
    for (a, b) in view.seals.iter().zip(&g.seals) {
        assert_eq!(a.state, b.state);
        assert_eq!(a.hp, b.hp);
        assert!(a.pos.dist(b.pos) < 0.05, "pos {:?} vs {:?}", a.pos, b.pos);
    }
}
