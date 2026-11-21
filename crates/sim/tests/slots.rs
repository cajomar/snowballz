//! Slots you set, and newcomers from the sea (the `armies` rule): igloos
//! recruit, draft and guard nothing; buildings with slots fill as many as
//! the tribe wants, from idle workers or, with nobody idle, newcomers who
//! swim in from the sea for fish. No Landing, no wild colonies, no Roosts,
//! no fish upkeep.

use sim::game::*;
use sim::map::Map;
use sim::math::Vec2;
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};

fn game() -> Game {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let mut g = Game::new_with(map, setups, 7, CoreRules::default());
    g.weather.calm = true;
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

fn home(g: &Game) -> u8 {
    g.igloos.iter().position(|i| i.owner == 0).unwrap() as u8
}

fn mine(g: &Game) -> usize {
    g.penguins.iter().filter(|p| p.owner == 0 && p.act != Act::Frozen).count()
}

/// Somewhere near home a building of this kind fits.
fn site(g: &Game, kind: BuildKind) -> (i32, i32) {
    let door = g.igloos[home(g) as usize].door;
    (4..14)
        .flat_map(|r| (0..16).map(move |k| (door + Vec2::from_angle(k as f32 / 16.0 * 6.283) * r as f32).tile()))
        .find(|&t| g.placement_problem(0, kind, t).is_none())
        .expect("room to build")
}

#[test]
fn no_upkeep() {
    let mut g = game();
    assert!(!g.sea.is_empty(), "somewhere to swim in from");
    // Nobody eats: with no fishing, the fish stays where it is.
    let ig = home(&g) as usize;
    g.igloos[ig].fish_want = 0;
    g.igloos[ig].fish = 50;
    run(&mut g, 130);
    assert_eq!(g.igloos[ig].fish, 50, "no upkeep");
    // And an igloo drafts and trains nobody.
    run(&mut g, 2);
    assert!(g.penguins.iter().filter(|p| p.owner == 0).all(|p| p.kind != Kind::Snowballer));
}

#[test]
fn a_slot_with_nobody_idle_calls_a_newcomer_from_the_sea_for_fish() {
    let mut g = game();
    let ig = home(&g) as usize;
    // Everyone busy fishing at home; then a Sushi House wants two more.
    run(&mut g, 3);
    assert!(!g.penguins.iter().any(|p| p.owner == 0 && p.kind == Kind::Worker && p.order == Order::None && p.fishery.is_none()), "nobody idle");
    let b = g.place_finished_for_test(0, BuildKind::Bunkhouse, site(&g, BuildKind::Bunkhouse));
    g.command(0, Command::SetWant { building: b, want: 1 });
    g.igloos[ig].fish = 0;
    let before = mine(&g);
    run(&mut g, 10);
    assert_eq!(mine(&g), before, "no fish, nobody comes");
    g.igloos[ig].fish = SEA_RECRUIT_FISH + Kind::Snowballer.stats().fish;
    // (Tick by tick: the fishers keep bringing fish in.)
    let mut swam = Vec::new();
    for _ in 0..30 * sim::TICK_RATE {
        let fish = g.fish_of(0);
        g.tick();
        for e in std::mem::take(&mut g.events) {
            if let Event::SwamIn { penguin } = e {
                assert!(g.fish_of(0) + SEA_RECRUIT_FISH as u32 <= fish + 2, "the newcomer was paid for");
                swam.push(penguin);
            }
        }
    }
    assert_eq!(swam.len(), 1, "one newcomer swam in");
    let p = &g.penguins[swam[0] as usize];
    assert_eq!((p.owner, p.kind), (0, Kind::Snowballer), "and it was drafted into the bunk");
    assert_eq!(g.camp_filled(b), 1);
    // The client hears it swim in.
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    let evs = view.apply(&SnapshotEncoder::new().encode(&g, &[Event::SwamIn { penguin: swam[0] }])).unwrap();
    assert!(evs.iter().any(|e| matches!(e, Event::SwamIn { .. })));
}

#[test]
fn an_idle_worker_is_called_before_the_sea() {
    let mut g = game();
    let ig = home(&g) as usize;
    g.igloos[ig].fish = 100;
    run(&mut g, 3);
    // One fisher less wanted at home: that worker is idle now.
    g.command(0, Command::SetIglooFishers { igloo: ig as u8, want: 1 });
    run(&mut g, 2);
    let idle: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Worker && p.order == Order::None && p.fishery.is_none()).map(|p| p.id).collect();
    assert!(!idle.is_empty(), "wanting fewer fishers let one go");
    let b = g.place_finished_for_test(0, BuildKind::Bunkhouse, site(&g, BuildKind::Bunkhouse));
    g.command(0, Command::SetWant { building: b, want: 1 });
    let before = mine(&g);
    let events = run(&mut g, 20);
    assert!(!events.iter().any(|e| matches!(e, Event::SwamIn { .. })), "nobody swam in");
    assert_eq!(mine(&g), before);
    assert_eq!(g.camp_filled(b), 1, "the idle worker was drafted");
}

#[test]
fn igloos_have_no_guards_and_a_tower_posts_as_many_sentries_as_wanted() {
    let mut g = game();
    let ig = home(&g) as usize;
    for _ in 0..6 {
        g.spawn_for_test(ig as u8, Kind::Snowballer);
    }
    run(&mut g, 5);
    assert!(g.penguins.iter().all(|p| !matches!(p.order, Order::Hold { .. })), "nobody posted at the door");
    let t = g.place_finished_for_test(0, BuildKind::Tower, site(&g, BuildKind::Tower));
    assert_eq!(g.buildings[t as usize].want, TOWER_SENTRIES as u8);
    run(&mut g, 15);
    assert_eq!(g.garrison(t).count(), TOWER_SENTRIES);
    g.command(0, Command::SetWant { building: t, want: 4 });
    run(&mut g, 15);
    assert_eq!(g.garrison(t).count(), 4, "four wanted, four on the roof");
}

#[test]
fn wanting_fewer_fishers_lets_the_last_ones_go() {
    let mut g = game();
    let ig = home(&g);
    run(&mut g, 3);
    assert_eq!(g.fishers(Fishery::Igloo(ig)), 2);
    g.command(0, Command::SetIglooFishers { igloo: ig, want: 0 });
    run(&mut g, 2);
    assert_eq!(g.fishers(Fishery::Igloo(ig)), 0);
}

#[test]
fn the_watchtower_sees_far() {
    let mut g = game();
    let t = g.place_finished_for_test(0, BuildKind::Tower, site(&g, BuildKind::Tower));
    let c = g.buildings[t as usize].center();
    let eyes = sim::fog::team_eyes(&g, g.team(0));
    assert!(eyes.contains(&(c, sim::fog::TOWER_SIGHT)), "a finished tower sees far");
    assert!((BuildKind::Tower.snow(), BuildKind::Tower.wood()) < (8, 6), "cheaper than it was");
}

#[test]
fn one_wall_and_a_sauna_is_a_campfire_built_up() {
    let mut g = game();
    assert!(BuildKind::Palisade.offered());
    assert!(!BuildKind::Sauna.offered() && BuildKind::Campfire.offered(), "no Sauna from scratch");
    let ig = home(&g) as usize;
    g.igloos[ig].fish = 100;
    g.igloos[ig].wood = 40;
    let tile = site(&g, BuildKind::Sauna);
    let f = g.place_finished_for_test(0, BuildKind::Campfire, tile);
    assert!(g.sauna_site(f).is_some());
    // Someone else's command does nothing.
    g.command(1, Command::UpgradeBuilding { building: f });
    assert_eq!(g.buildings[f as usize].kind, BuildKind::Campfire);
    g.command(0, Command::UpgradeBuilding { building: f });
    let b = &g.buildings[f as usize];
    assert_eq!(b.kind, BuildKind::Sauna, "the fire is a Sauna site now");
    assert!(!b.done() && !b.lit, "going up, fire out");
    assert_eq!((b.built, b.wood_in), (BuildKind::Campfire.snow(), BuildKind::Campfire.wood() as u8), "the fire's step and log count");
    let tiles: Vec<(i32, i32)> = b.tiles().collect();
    assert_eq!(tiles.len(), 4);
    assert!(tiles.contains(&tile));
    assert!(tiles.iter().all(|&(x, y)| g.building_at(x, y).map(|b| b.id) == Some(f)), "it holds all four tiles");
    // Its builders finish it.
    for _ in 0..240 {
        run(&mut g, 1);
        if g.buildings[f as usize].done() {
            break;
        }
    }
    assert!(g.buildings[f as usize].done(), "built");
    run(&mut g, 30);
    assert!(g.buildings[f as usize].lit, "and its stoker lights it");
    // The client sees the Sauna where the fire was.
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    view.apply(&SnapshotEncoder::new().encode(&g, &[])).unwrap();
    let v = view.buildings[f as usize];
    assert_eq!((v.kind, v.tile), (BuildKind::Sauna, g.buildings[f as usize].tile));
}

#[test]
fn a_campfire_hemmed_in_is_not_built_up() {
    let mut g = game();
    let tile = site(&g, BuildKind::Sauna);
    let f = g.place_finished_for_test(0, BuildKind::Campfire, (tile.0 + 1, tile.1 + 1));
    // Fires on every side but one corner... and that one too.
    for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
        let t = (tile.0 + 1 + dx, tile.1 + 1 + dy);
        if g.placement_problem(0, BuildKind::Campfire, t).is_none() {
            g.place_finished_for_test(0, BuildKind::Campfire, t);
        }
    }
    assert!(g.sauna_site(f).is_none());
    g.command(0, Command::UpgradeBuilding { building: f });
    assert_eq!(g.buildings[f as usize].kind, BuildKind::Campfire);
}
