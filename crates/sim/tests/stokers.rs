//! No woodpile (the `armies` rule, DESIGN.md 0.2 and PLAN.md C7 stage 2):
//! nobody chops by themselves and no wood is stored. Builders fetch logs for
//! their own site, every sauna and campfire calls in a stoker who keeps it
//! fed from the trees nearby, other prices pay their wood share in fish,
//! and felled trees grow back.

use sim::game::*;
use sim::map::{Map, Obstacle};
use sim::math::{v2, Vec2};
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};

fn game() -> Game {
    let path = format!("{}/../../assets/maps/fixtures/greenwood.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let mut g = Game::new_with(map, setups, 7, CoreRules::default());
    g.weather.calm = true;
    let home = home(&g);
    for _ in 0..6 {
        g.spawn_for_test(home, Kind::Worker);
    }
    g
}

fn home(g: &Game) -> u8 {
    g.igloos.iter().position(|i| i.owner == 0).unwrap() as u8
}

fn run(g: &mut Game, secs: u32) {
    for _ in 0..secs * sim::TICK_RATE {
        g.tick();
        g.events.clear();
    }
}

fn site(g: &Game, kind: BuildKind) -> (i32, i32) {
    let door = g.igloos[home(g) as usize].door;
    g.find_site(0, kind, door + v2(5.0, 0.0), 14).expect("room to build")
}

#[test]
fn nobody_chops_by_themselves_and_no_wood_is_stored() {
    let mut g = game();
    run(&mut g, 180);
    assert_eq!(g.wood_of(0), 0, "wood was stored");
    assert!(!g.penguins.iter().any(|p| p.owner == 0 && matches!(p.order, Order::Chop { .. })), "someone chopped unasked");

    // Chopping by hand clears the trees, but the logs go nowhere.
    let door = g.igloos[home(&g) as usize].door;
    let tree = g.tree_near(door, 20).expect("a tree near home");
    let workers: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Worker).take(3).map(|p| p.id).collect();
    g.command(0, Command::Chop { units: workers, tile: tree });
    run(&mut g, 60);
    assert_eq!(g.map.obstacle_at(tree.0, tree.1), Obstacle::Stump, "the tree came down");
    assert_eq!(g.wood_of(0), 0, "and no wood was stored");
}

#[test]
fn builders_fetch_the_wood_for_their_site() {
    let mut g = game();
    let t = site(&g, BuildKind::Tower);
    assert!(BuildKind::Tower.wood() > 0);
    g.command(0, Command::Build { units: vec![], kind: BuildKind::Tower, tiles: vec![t] });
    let b = g.buildings.iter().rposition(|b| b.kind == BuildKind::Tower).unwrap();
    for _ in 0..240 {
        run(&mut g, 1);
        if g.buildings[b].done() {
            break;
        }
    }
    assert!(g.buildings[b].done(), "the sauna went up with wood the builders chopped ({}/{} wood)", g.buildings[b].wood_in, BuildKind::Tower.wood());
    assert_eq!(g.wood_of(0), 0);
}

#[test]
fn a_sauna_calls_a_stoker_who_keeps_it_lit() {
    let mut g = game();
    let b = g.place_finished_for_test(0, BuildKind::Sauna, site(&g, BuildKind::Sauna));
    run(&mut g, 3);
    assert_eq!(g.stokers(b), 1, "one stoker came");
    // Lit once the first log is in, and kept lit while the stoker comes and goes.
    let mut first = None;
    let mut out_after = 0;
    for s in 0..240 {
        run(&mut g, 1);
        let lit = g.buildings[b as usize].lit;
        if lit && first.is_none() {
            first = Some(s);
        }
        if first.is_some() && !lit {
            out_after += 1;
        }
    }
    assert!(first.is_some_and(|s| s < 60), "lit at {first:?}");
    assert_eq!(out_after, 0, "the fire went out {out_after} s while the stoker was on the job");

    // The client sees the stoker's slot.
    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    view.apply(&enc.encode(&g, &[])).unwrap();
    let v = view.buildings[b as usize];
    assert_eq!((v.slots, v.filled), (1, 1));

    // Taken off the job by hand, the slot calls someone else.
    let stoker = g.penguins.iter().find(|p| p.order == Order::Tend { building: b }).unwrap().id;
    g.command(0, Command::Move { units: vec![stoker], target: g.igloos[home(&g) as usize].door, run: false, queue: false });
    run(&mut g, 2);
    assert_eq!(g.stokers(b), 1);
    assert!(g.penguins[stoker as usize].order != Order::Tend { building: b });
}

#[test]
fn a_campfire_with_a_stoker_burns_on() {
    let mut g = game();
    let b = g.place_finished_for_test(0, BuildKind::Campfire, site(&g, BuildKind::Campfire));
    g.buildings[b as usize].fuel = CAMPFIRE_TICKS;
    run(&mut g, CAMPFIRE_TICKS as u32 / sim::TICK_RATE * 3);
    assert!(g.buildings[b as usize].alive && g.buildings[b as usize].lit, "the fire burned out with a stoker");
}

#[test]
fn prices_pay_their_wood_in_fish() {
    // Nothing stores wood with armies, so what was paid in wood is fish.
    assert_eq!(sim::game::price(10, 6), (16, 0));
}

#[test]
fn felled_trees_grow_back_once_nothing_stands_there() {
    let mut g = game();
    let door = g.igloos[home(&g) as usize].door;
    let (x, y) = g.tree_near(door, 20).unwrap();
    let w = g.penguins.iter().find(|p| p.owner == 0 && p.kind == Kind::Worker).unwrap().id;
    g.command(0, Command::Chop { units: vec![w], tile: (x, y) });
    for _ in 0..120 {
        run(&mut g, 1);
        if g.map.obstacle_at(x, y) == Obstacle::Stump {
            break;
        }
    }
    assert_eq!(g.map.obstacle_at(x, y), Obstacle::Stump);
    assert!(g.grid.walkable(x, y));
    // Someone stands on the stump when it is due: it waits.
    let stump = v2(x as f32 + 0.5, y as f32 + 0.5);
    let mut view = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 0)));
    let at = g.tick + TREE_REGROW_TICKS;
    while g.tick < at + sim::TICK_RATE * 2 {
        let p = &mut g.penguins[w as usize];
        p.pos = stump;
        p.order = Order::Hold { pos: stump };
        g.tick();
        let ev = std::mem::take(&mut g.events);
        view.1.apply(&view.0.encode(&g, &ev)).unwrap();
    }
    assert_eq!(g.map.obstacle_at(x, y), Obstacle::Stump, "a tree grew under a penguin");
    // It steps off; the tree comes back, and the client sees it.
    g.penguins[w as usize].pos = stump + Vec2::from_angle(0.0) * 4.0;
    g.command(0, Command::Move { units: vec![w], target: door, run: false, queue: false });
    for _ in 0..(TREE_RETRY_TICKS + sim::TICK_RATE * 2) {
        g.tick();
        let ev = std::mem::take(&mut g.events);
        view.1.apply(&view.0.encode(&g, &ev)).unwrap();
    }
    assert_eq!(g.map.obstacle_at(x, y), Obstacle::Tree, "the tree grew back");
    assert!(!g.grid.walkable(x, y));
    assert_eq!(view.1.info.map.obstacle_at(x, y), Obstacle::Tree);
}
