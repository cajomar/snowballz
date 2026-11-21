//! Hordes and elites (the `armies` rule, DESIGN.md 0.1 and 0.2): a Bunkhouse
//! drafts workers into as many snowballers as the tribe wants, and camps
//! are elite housing: each empty slot calls in an idle worker, who mines a
//! crystal and then trains quickly, and the slot calls another when its
//! specialist freezes. Builders build a camp up for more slots.

use sim::game::*;
use sim::map::{Map, ResourceKind};
use sim::math::Vec2;
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};

fn game() -> Game {
    // (A map with crystal near a start.)
    let path = format!("{}/../../assets/maps/aurora-basin.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let mut g = Game::new_with(map, setups, 7, CoreRules::default().map_start());
    g.weather.calm = true;
    for ig in g.igloos.iter_mut().filter(|i| i.owner != NEUTRAL) {
        ig.fish = 500;
    }
    // Player 0 starts by the crystal.
    let near = (0..g.igloos.len()).filter(|&k| g.igloos[k].owner != NEUTRAL).min_by_key(|&k| crystal_dist(&g, g.igloos[k].door) as i32).unwrap();
    let mine = g.igloos.iter().position(|i| i.owner == 0).unwrap();
    if near != mine {
        let (a, b) = (g.igloos[near].owner, g.igloos[mine].owner);
        for p in g.penguins.iter_mut() {
            if p.owner == a {
                p.owner = b;
            } else if p.owner == b {
                p.owner = a;
            }
        }
        g.igloos[near].owner = b;
        g.igloos[mine].owner = a;
    }
    // Some workers to spare, and nobody fishing (so the fish only goes down).
    let home = home(&g);
    for _ in 0..6 {
        g.spawn_for_test(home, Kind::Worker);
    }
    for ig in 0..g.igloos.len() as u8 {
        g.igloos[ig as usize].fish_want = 0;
    }
    g
}

fn crystal_dist(g: &Game, at: Vec2) -> f32 {
    g.resources.iter().filter(|r| r.kind == ResourceKind::Crystal).map(|r| r.pos.dist(at)).fold(f32::MAX, f32::min)
}

fn crystal_left(g: &Game) -> u32 {
    g.resources.iter().filter(|r| r.kind == ResourceKind::Crystal).map(|r| r.amount as u32).sum()
}

fn run(g: &mut Game, secs: u32) {
    for _ in 0..secs * sim::TICK_RATE {
        g.tick();
        g.events.clear();
    }
}

fn home(g: &Game) -> u8 {
    g.igloos.iter().position(|i| i.owner == 0).unwrap() as u8
}

fn count(g: &Game, kind: Kind) -> usize {
    g.penguins.iter().filter(|p| p.owner == 0 && p.kind == kind && p.act != Act::Frozen).count()
}

/// Somewhere near home a building of this kind fits.
fn site(g: &Game, kind: BuildKind) -> (i32, i32) {
    let door = g.igloos[home(g) as usize].door;
    (4..12)
        .flat_map(|r| (0..16).map(move |k| (door + Vec2::from_angle(k as f32 / 16.0 * 6.283) * r as f32).tile()))
        .find(|&t| g.placement_problem(0, kind, t).is_none())
        .expect("room to build")
}

#[test]
fn a_bunkhouse_drafts_idle_workers_into_as_many_snowballers_as_wanted() {
    let mut g = game();
    let before = count(&g, Kind::Snowballer);
    let fish = g.fish_of(0);
    let b = g.place_finished_for_test(0, BuildKind::Bunkhouse, site(&g, BuildKind::Bunkhouse));
    assert_eq!((g.buildings[b as usize].slots, g.buildings[b as usize].want), (BUNK_SLOTS[0], BUNK_SLOTS[0]));
    g.command(0, Command::SetWant { building: b, want: 2 });
    run(&mut g, 25);
    assert_eq!(count(&g, Kind::Snowballer), before + 2, "two drafted, as wanted");
    assert_eq!(g.camp_filled(b), 2);
    assert!(g.fish_of(0) <= fish - 2 * Kind::Snowballer.stats().fish as u32, "each for a snowballer's price");
    // No crystal is needed to be drafted.
    assert_eq!(crystal_left(&g), crystal_left(&game()));
}

#[test]
fn a_camp_slot_calls_in_a_worker_who_mines_a_crystal_and_trains() {
    let mut g = game();
    let fish = g.fish_of(0);
    let crystal = crystal_left(&g);
    let camp = g.place_finished_for_test(0, BuildKind::Workshop, site(&g, BuildKind::Workshop));
    let snowballers = count(&g, Kind::Snowballer);
    run(&mut g, 2);
    let trainee = g.penguins.iter().find(|p| p.order == Order::Train { building: camp }).expect("a trainee called in");
    assert_eq!(trainee.kind, Kind::Worker, "a worker, not a snowballer");
    let t = trainee.id;
    let mut mined = false;
    for _ in 0..120 {
        run(&mut g, 1);
        mined |= g.penguins[t as usize].act == Act::Mine;
        if g.penguins[t as usize].kind == Kind::Shield {
            break;
        }
    }
    assert!(mined, "it went to mine a crystal first");
    assert_eq!(g.penguins[t as usize].kind, Kind::Shield);
    assert_eq!(g.penguins[t as usize].camp, Some(camp));
    assert_eq!(g.penguins[t as usize].carry, Carry::None, "the crystal was used up");
    // (Mined, though it may have grown back since.)
    assert!(crystal_left(&g) <= crystal);
    assert_eq!(count(&g, Kind::Snowballer), snowballers, "no snowballer was taken");
    assert_eq!(count(&g, Kind::Shield), 1, "one slot, one shieldbearer");
    let (cost, _) = sim::game::price(Kind::Shield.stats().fish, Kind::Shield.stats().wood);
    assert!(g.fish_of(0) <= fish - cost as u32, "the training was paid for");
    // Nobody else is called: the slot is full.
    assert!(g.penguins.iter().all(|p| p.order != Order::Train { building: camp }));
}

#[test]
fn no_crystal_no_trainees_and_mined_crystal_grows_back() {
    let mut g = game();
    let full: Vec<u8> = g.resources.iter().map(|r| r.amount).collect();
    for r in g.resources.iter_mut().filter(|r| r.kind == ResourceKind::Crystal) {
        r.amount = 0;
    }
    let camp = g.place_finished_for_test(0, BuildKind::Workshop, site(&g, BuildKind::Workshop));
    run(&mut g, 3);
    assert!(g.penguins.iter().all(|p| p.order != Order::Train { building: camp }), "called in with no crystal to mine");
    run(&mut g, CRYSTAL_REGROW_TICKS / sim::TICK_RATE);
    for (r, &max) in g.resources.iter().zip(&full) {
        if r.kind == ResourceKind::Crystal {
            assert_eq!(r.amount, 1.min(max), "a crystal grew back");
        }
    }
}

#[test]
fn builders_build_a_camp_up_for_more_slots() {
    let mut g = game();
    let camp = g.place_finished_for_test(0, BuildKind::SlingRange, site(&g, BuildKind::SlingRange));
    g.command(0, Command::UpgradeBuilding { building: camp });
    assert_eq!(g.buildings[camp as usize].slots, 1, "not until it is built");
    let (steps, wood) = CAMP_UPGRADE[0];
    assert_eq!(g.buildings[camp as usize].upgrading, Some(BuildingUpgrade { steps, wood, to: 3, into: None }));
    // A second order while it is going changes nothing.
    g.command(0, Command::UpgradeBuilding { building: camp });
    assert_eq!(g.buildings[camp as usize].upgrading.unwrap().to, 3);
    let mut builders = 0;
    for _ in 0..180 {
        run(&mut g, 1);
        builders = builders.max(g.penguins.iter().filter(|p| p.order == Order::Build { building: camp }).count());
        if g.buildings[camp as usize].slots == 3 {
            break;
        }
    }
    assert!(builders > 0, "builders came");
    assert_eq!(g.buildings[camp as usize].slots, 3, "built up to three");
    assert_eq!(g.buildings[camp as usize].upgrading, None);
    assert_eq!(g.wood_of(0), 0, "the logs were fetched, not stored");
    g.command(0, Command::UpgradeBuilding { building: camp });
    for _ in 0..240 {
        run(&mut g, 1);
        if g.buildings[camp as usize].slots == 5 {
            break;
        }
    }
    assert_eq!(g.buildings[camp as usize].slots, 5);
    g.command(0, Command::UpgradeBuilding { building: camp });
    assert_eq!(g.buildings[camp as usize].upgrading, None, "five is the most");
    // The new room isn't filled by itself: only once it is wanted.
    run(&mut g, 20);
    assert_eq!(g.camp_filled(camp), 1);
    g.command(0, Command::SetWant { building: camp, want: 5 });
    run(&mut g, 150);
    assert_eq!(g.camp_filled(camp), 5);
}

#[test]
fn a_frozen_specialist_frees_its_slot_and_thaws_a_worker() {
    let mut g = game();
    let camp = g.place_finished_for_test(0, BuildKind::SkiLodge, site(&g, BuildKind::SkiLodge));
    run(&mut g, 90);
    let ski = g.penguins.iter().find(|p| p.owner == 0 && p.kind == Kind::Skier).expect("a skier").id;
    let enemy = g.penguins.iter().find(|p| p.owner == 1).unwrap().id;
    g.hurt_for_test(ski, 255, enemy);
    assert_eq!(g.camp_filled(camp), 0, "the slot is free");
    run(&mut g, 2);
    let next = g.penguins.iter().find(|p| p.order == Order::Train { building: camp }).expect("someone called in to fill it").id;
    assert_ne!(next, ski);
    // The frozen skier, thawed by its own tribe, is a plain worker again.
    g.thaw_for_test(ski, 0);
    assert_eq!(g.penguins[ski as usize].kind, Kind::Worker);
    assert_eq!(g.penguins[ski as usize].camp, None);
}

#[test]
fn the_client_sees_a_camps_slots_and_training() {
    let mut g = game();
    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    let camp = g.place_finished_for_test(0, BuildKind::Workshop, site(&g, BuildKind::Workshop));
    g.command(0, Command::UpgradeBuilding { building: camp });
    let (mut seen_training, mut seen_building) = (false, false);
    for _ in 0..240 * sim::TICK_RATE {
        g.tick();
        let ev = std::mem::take(&mut g.events);
        view.apply(&enc.encode(&g, &ev)).unwrap();
        seen_training |= view.buildings[camp as usize].training > 0;
        seen_building |= view.buildings[camp as usize].upgrading();
        let b = &view.buildings[camp as usize];
        assert_eq!((b.up_steps, b.up_wood), g.buildings[camp as usize].upgrading.map_or((0, 0), |u| (u.steps, u.wood)));
    }
    assert!(seen_building, "the client never saw it being built up");
    let b = &view.buildings[camp as usize];
    assert_eq!(b.slots, 3);
    assert_eq!(b.filled as usize, g.camp_filled(camp));
    assert!(seen_training, "the client never saw anyone training");
}

#[test]
fn a_camp_pays_when_it_calls_and_calls_nobody_it_cannot_pay_for() {
    let mut g = game();
    for ig in g.igloos.iter_mut().filter(|i| i.owner == 0) {
        ig.fish = 0;
    }
    let camp = g.place_finished_for_test(0, BuildKind::Workshop, site(&g, BuildKind::Workshop));
    run(&mut g, 3);
    assert!(g.penguins.iter().all(|p| p.order != Order::Train { building: camp }), "called in a trainee it can't pay for");
    // With the fish, the call takes it at once, so it is there at the end.
    let (cost, _) = sim::game::price(Kind::Shield.stats().fish, Kind::Shield.stats().wood);
    let ig = home(&g) as usize;
    g.igloos[ig].fish = cost;
    run(&mut g, 2);
    let t = g.penguins.iter().find(|p| p.order == Order::Train { building: camp }).expect("a trainee").id;
    assert!(g.igloos[ig].fish < cost, "paid when called");
    for _ in 0..120 {
        run(&mut g, 1);
        if g.penguins[t as usize].kind == Kind::Shield {
            break;
        }
    }
    assert_eq!(g.penguins[t as usize].kind, Kind::Shield, "trained with the fish paid up front");
}
