//! Catapult Towers (the `armies` rule): a Watchtower built up. Three workers
//! crew it from the foot of the tower; it lobs big snowballs at the nearest
//! enemy building or igloo in reach, or where its owner aims it; freeze the
//! whole crew and enemy workers can take it over.

use sim::game::*;
use sim::map::Map;
use sim::math::{v2, Vec2};
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};

fn game() -> Game {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let mut g = Game::new_with(map, setups, 7, CoreRules::default());
    g.weather.calm = true;
    for ig in g.igloos.iter_mut().filter(|ig| ig.owner == 0) {
        ig.fish = 200;
        ig.wood = 60;
    }
    g
}

fn run(g: &mut Game, secs: u32) -> Vec<Event> {
    let mut out = Vec::new();
    for _ in 0..secs * sim::TICK_RATE {
        g.tick();
        out.append(&mut g.events);
    }
    out
}

fn home(g: &Game) -> Vec2 {
    g.igloos.iter().find(|i| i.owner == 0).unwrap().door
}

/// A free spot for `kind` about `r` tiles from `from`.
fn spot(g: &Game, kind: BuildKind, from: Vec2, r: f32) -> (i32, i32) {
    (0..64)
        .map(|k| (from + Vec2::from_angle(k as f32 / 64.0 * 6.283) * r).tile())
        .find(|&t| g.placement_problem(0, kind, t).is_none())
        .expect("room to build")
}

/// A finished catapult of player 0 by its home, crewed and ready.
fn catapult(g: &mut Game) -> u16 {
    let t = g.place_finished_for_test(0, BuildKind::Tower, spot(g, BuildKind::Tower, home(g), 6.0));
    g.command(0, Command::UpgradeBuilding { building: t });
    for _ in 0..180 {
        run(g, 1);
        if g.buildings[t as usize].kind == BuildKind::Catapult {
            break;
        }
    }
    assert_eq!(g.buildings[t as usize].kind, BuildKind::Catapult, "the tower was built up");
    for _ in 0..60 {
        run(g, 1);
        if g.crew(t).count() == CATAPULT_CREW as usize {
            break;
        }
    }
    t
}

fn shots(events: &[Event]) -> usize {
    events.iter().filter(|e| matches!(e, Event::Thrown(s) if s.big)).count()
}

#[test]
fn a_watchtower_builds_up_into_a_catapult_and_crews_itself() {
    let mut g = game();
    assert!(!BuildKind::Catapult.offered(), "only by building a tower up");
    let t = g.place_finished_for_test(0, BuildKind::Tower, spot(&g, BuildKind::Tower, home(&g), 6.0));
    // Sentries on the roof first.
    let ig = g.igloos.iter().position(|i| i.owner == 0).unwrap() as u8;
    for _ in 0..4 {
        g.spawn_for_test(ig, Kind::Snowballer);
    }
    run(&mut g, 15);
    assert!(g.garrison(t).count() > 0, "sentries went up");
    // No trees in reach, so the workers can't top the logs back up and the
    // count below is only what the tower took.
    for o in g.map.obstacle.iter_mut().filter(|o| **o == sim::map::Obstacle::Tree) {
        *o = sim::map::Obstacle::None;
    }
    let wood = g.wood_of(0);
    g.command(1, Command::UpgradeBuilding { building: t });
    assert!(g.buildings[t as usize].upgrading.is_none(), "not someone else's");
    g.command(0, Command::UpgradeBuilding { building: t });
    for _ in 0..180 {
        run(&mut g, 1);
        if g.buildings[t as usize].kind == BuildKind::Catapult {
            break;
        }
    }
    let b = &g.buildings[t as usize];
    assert_eq!((b.kind, b.slots, b.want), (BuildKind::Catapult, CATAPULT_CREW, CATAPULT_CREW));
    assert!(g.wood_of(0) + CATAPULT_UPGRADE.1 as u32 <= wood + 1, "the logs went into it");
    assert_eq!(g.garrison(t).count(), 0, "the sentries climbed down");
    run(&mut g, 40);
    assert_eq!(g.crew(t).count(), CATAPULT_CREW as usize, "three workers crew it");
    assert!(g.crew(t).all(|p| p.kind == Kind::Worker));
    // It still sees far.
    let c = g.buildings[t as usize].center();
    assert!(sim::fog::team_eyes(&g, g.team(0)).contains(&(c, sim::fog::TOWER_SIGHT)));
    // The client sees a crewed catapult.
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    view.apply(&SnapshotEncoder::new().encode(&g, &[])).unwrap();
    let v = view.buildings[t as usize];
    assert_eq!((v.kind, v.slots, v.filled, v.want), (BuildKind::Catapult, CATAPULT_CREW, CATAPULT_CREW, CATAPULT_CREW));
}

#[test]
fn it_wrecks_the_nearest_enemy_building_in_reach_and_nothing_beyond() {
    let mut g = game();
    let t = catapult(&mut g);
    let c = g.buildings[t as usize].center();
    let far = g.place_finished_for_test(1, BuildKind::Palisade, spot(&g, BuildKind::Palisade, c, CATAPULT_RANGE + 3.0));
    let events = run(&mut g, 20);
    assert_eq!(shots(&events), 0, "nothing in reach, nothing thrown");
    assert_eq!(g.buildings[far as usize].hp, BuildKind::Palisade.max_hp());
    let near = g.place_finished_for_test(1, BuildKind::Palisade, spot(&g, BuildKind::Palisade, c, 10.0));
    let events = run(&mut g, 60);
    assert!(shots(&events) >= 3, "it lobbed ({} shots)", shots(&events));
    assert!(!g.buildings[near as usize].alive, "the palisade in reach was wrecked");
    assert!(g.buildings[far as usize].alive && g.buildings[far as usize].hp == BuildKind::Palisade.max_hp(), "the one out of reach wasn't touched");
}

#[test]
fn its_owner_can_aim_it_and_others_cannot() {
    let mut g = game();
    let t = catapult(&mut g);
    let c = g.buildings[t as usize].center();
    let near = g.place_finished_for_test(1, BuildKind::Palisade, spot(&g, BuildKind::Palisade, c, 7.0));
    let aimed = g.place_finished_for_test(1, BuildKind::Palisade, spot(&g, BuildKind::Palisade, c, 12.0));
    let at = g.buildings[aimed as usize].center();
    g.command(1, Command::SetRally { building: t, at });
    assert_eq!(g.buildings[t as usize].rally, None, "not theirs to aim");
    // Aiming at bare ground lets it pick by itself.
    g.command(0, Command::SetRally { building: t, at: c + v2(3.0, 3.0) });
    assert_eq!(g.buildings[t as usize].rally, None);
    g.command(0, Command::SetRally { building: t, at });
    assert_eq!(g.buildings[t as usize].rally, Some(at));
    for _ in 0..60 {
        run(&mut g, 1);
        if !g.buildings[aimed as usize].alive {
            break;
        }
    }
    assert!(!g.buildings[aimed as usize].alive, "the one it was aimed at went down");
    assert_eq!(g.buildings[near as usize].hp, BuildKind::Palisade.max_hp(), "the nearer one was left alone meanwhile");
    // With its target gone it goes back to the nearest.
    run(&mut g, 60);
    assert!(!g.buildings[near as usize].alive);
}

#[test]
fn with_no_crew_it_stops_and_enemy_workers_take_it_over() {
    let mut g = game();
    let t = catapult(&mut g);
    let c = g.buildings[t as usize].center();
    g.place_finished_for_test(1, BuildKind::Palisade, spot(&g, BuildKind::Palisade, c, 9.0));
    // Nobody idle and no fish: the crew can't be replaced.
    for ig in g.igloos.iter_mut().filter(|ig| ig.owner == 0) {
        ig.fish = 0;
    }
    // An enemy worker waits nearby.
    let ig1 = g.igloos.iter().position(|i| i.owner == 1).unwrap() as u8;
    let thief = g.spawn_for_test(ig1, Kind::Worker);
    g.penguins[thief as usize].pos = c + v2(4.0, 0.0);
    g.command(1, Command::Tend { units: vec![thief], building: t });
    run(&mut g, 5);
    assert_eq!(g.buildings[t as usize].owner, 0, "crewed: they can't take it");
    assert_eq!(g.penguins[thief as usize].order, Order::None, "the worker gave up");
    // Freeze the whole crew, and everyone else out of doors (the Chieftain
    // would pelt the thief, and the workers step in).
    let crew: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.is_out()).map(|p| p.id).collect();
    assert!(g.crew(t).all(|p| crew.contains(&p.id)));
    for &id in &crew {
        g.hurt_for_test(id, 255, thief);
    }
    assert!(crew.iter().all(|&id| g.penguins[id as usize].act == Act::Frozen));
    let events = run(&mut g, 15);
    assert_eq!(shots(&events), 0, "no crew, no shots");
    let p = &mut g.penguins[thief as usize];
    (p.pos, p.warmth, p.cold) = (c + v2(4.0, 0.0), p.max_warmth, false);
    g.command(1, Command::Tend { units: vec![thief], building: t });
    let events = run(&mut g, 10);
    assert_eq!(g.buildings[t as usize].owner, 1, "taken over");
    assert!(events.iter().any(|e| matches!(e, Event::TakenOver { building, from: 0, by: 1 } if *building == t)));
    // And the client hears about it.
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    let evs = view.apply(&SnapshotEncoder::new().encode(&g, &[Event::TakenOver { building: t, from: 0, by: 1 }])).unwrap();
    assert!(evs.iter().any(|e| matches!(e, Event::TakenOver { .. })));
}
