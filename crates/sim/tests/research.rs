//! Research at the igloos: what it costs, what it needs, and what it does.

use sim::ai::AiLevel;
use sim::game::*;
use sim::map::Map;
use sim::math::Vec2;
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};

fn game() -> Game {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let mut g = Game::new_with(map, setups, 5, sim::game::CoreRules { tech: true, ..Default::default() }.map_start());
    g.weather.calm = true;
    g
}

fn home(g: &Game, owner: u8) -> u8 {
    g.igloos.iter().position(|i| i.owner == owner).unwrap() as u8
}

/// Tick `n` times and collect the events.
fn run(g: &mut Game, n: u32) -> Vec<Event> {
    let mut all = Vec::new();
    for _ in 0..n {
        g.tick();
        all.extend(std::mem::take(&mut g.events));
    }
    all
}

/// Another igloo for the same tribe, taken from the neutral ones.
fn second_igloo(g: &mut Game) -> u8 {
    let first = home(g, 0);
    let other = g.igloos.iter().position(|i| i.id != first && i.owner == NEUTRAL).unwrap();
    g.igloos[other].owner = 0;
    other as u8
}

/// The level of `branch` the tribe is working towards.
fn level_of(g: &Game, branch: u8) -> &'static Level {
    next_level(g.levels_of(0), branch).expect("a level to learn")
}

/// Pay for one level of a branch at the home igloo and wait for it.
fn learn(g: &mut Game, branch: u8) -> Vec<Event> {
    let ig = home(g, 0);
    let level = level_of(g, branch);
    let before = g.levels_of(0)[branch as usize];
    g.igloos[ig as usize].fish = level.fish + 20;
    g.igloos[ig as usize].wood = level.wood;
    g.igloos[ig as usize].crystal = level.crystal;
    g.command(0, Command::StartResearch { igloo: ig, branch });
    assert!(g.igloos[ig as usize].research.is_some(), "{branch} research did not start");
    let ev = run(g, level.ticks as u32);
    assert_eq!(g.levels_of(0)[branch as usize], before + 1, "the level was not learnt");
    ev
}

#[test]
fn research_costs_fish_wood_and_crystal() {
    // The igloo pays the fish (wood is paid in fish too); the tribe pools the crystal.
    let mut g = game();
    let ig = home(&g, 0);
    let other = second_igloo(&mut g);
    let level = &BRANCH_TABLE[SNOWCRAFT as usize].levels[0];
    let (fish, _) = price(level.fish, level.wood);
    g.igloos[ig as usize].fish = fish + 5;
    g.igloos[other as usize].crystal = level.crystal;
    g.command(0, Command::StartResearch { igloo: ig, branch: SNOWCRAFT });
    assert_eq!(g.igloos[ig as usize].fish, 5, "the fish were not charged");
    assert_eq!(g.igloos[other as usize].crystal, 0, "the crystal was not paid");
    let r = g.igloos[ig as usize].research.expect("research started");
    assert_eq!(r.branch, SNOWCRAFT);
    assert_eq!(r.total, level.ticks);

    // The work takes its time, and then the tribe knows the trick.
    let ev = run(&mut g, level.ticks as u32 - 1);
    assert!(!ev.iter().any(|e| matches!(e, Event::ResearchDone { .. })), "finished early");
    assert_eq!(g.levels_of(0)[SNOWCRAFT as usize], 0, "learnt too early");
    let ev = run(&mut g, 1);
    assert!(
        ev.iter().any(|e| matches!(e, Event::ResearchDone { player, branch, level }
            if *player == 0 && *branch == SNOWCRAFT && *level == 1)),
        "the work was not reported: {ev:?}"
    );
    assert_eq!(g.levels_of(0)[SNOWCRAFT as usize], 1);
    assert!(g.igloos[ig as usize].research.is_none(), "the work is still running");
}

#[test]
fn research_is_refused_without_the_materials() {
    // Nothing is charged and no work starts when the tribe can't pay.
    let mut g = game();
    let ig = home(&g, 0);
    let level = &BRANCH_TABLE[SNOWCRAFT as usize].levels[0];
    let (fish, _) = price(level.fish, level.wood);
    g.igloos[ig as usize].fish = fish - 1;
    g.command(0, Command::StartResearch { igloo: ig, branch: SNOWCRAFT });
    assert!(g.igloos[ig as usize].research.is_none(), "researched without the fish");
    assert_eq!(g.igloos[ig as usize].fish, fish - 1, "the fish were charged anyway");

    g.igloos[ig as usize].fish = fish;
    g.command(0, Command::StartResearch { igloo: ig, branch: SNOWCRAFT });
    assert!(g.igloos[ig as usize].research.is_some(), "research refused for nothing");
    g.igloos[ig as usize].research = None;

    // The second level wants crystal as well.
    g.players[0].research[SNOWCRAFT as usize] = 1;
    let level = level_of(&g, SNOWCRAFT);
    assert!(level.crystal > 0, "the second level wants crystal");
    g.igloos[ig as usize].tier = TIER_SNOW_HALL;
    g.igloos[ig as usize].fish = price(level.fish, level.wood).0;
    g.igloos[ig as usize].crystal = level.crystal - 1;
    g.command(0, Command::StartResearch { igloo: ig, branch: SNOWCRAFT });
    assert!(g.igloos[ig as usize].research.is_none(), "researched without crystal");

    g.igloos[ig as usize].crystal = level.crystal;
    g.command(0, Command::StartResearch { igloo: ig, branch: SNOWCRAFT });
    assert!(g.igloos[ig as usize].research.is_some(), "research refused for nothing");
}

#[test]
fn the_second_level_needs_a_snow_hall_and_the_third_a_palace() {
    // The grander levels are worked on in a grander igloo.
    for branch in [SNOWCRAFT, ICECRAFT, FISHERY] {
        let mut g = game();
        let ig = home(&g, 0);
        g.players[0].research[branch as usize] = 1;
        let level = level_of(&g, branch);
        assert_eq!(level.tier, TIER_SNOW_HALL, "level two wants a Snow Hall");
        g.igloos[ig as usize].fish = 100;
        g.igloos[ig as usize].wood = 100;
        g.igloos[ig as usize].crystal = 100;
        g.command(0, Command::StartResearch { igloo: ig, branch });
        assert!(g.igloos[ig as usize].research.is_none(), "level two started in a plain igloo");

        g.igloos[ig as usize].tier = TIER_SNOW_HALL;
        g.command(0, Command::StartResearch { igloo: ig, branch });
        assert!(g.igloos[ig as usize].research.is_some(), "no research in a Snow Hall");
        g.igloos[ig as usize].research = None;

        // And the third level wants a palace.
        g.players[0].research[branch as usize] = 2;
        let level = level_of(&g, branch);
        assert_eq!(level.tier, TIER_ICE_PALACE, "level three wants a palace");
        g.command(0, Command::StartResearch { igloo: ig, branch });
        assert!(g.igloos[ig as usize].research.is_none(), "level three started in a Snow Hall");

        g.igloos[ig as usize].tier = TIER_ICE_PALACE;
        g.command(0, Command::StartResearch { igloo: ig, branch });
        assert!(g.igloos[ig as usize].research.is_some(), "no research in a palace");
    }
}

#[test]
fn an_igloo_works_on_one_thing_at_a_time() {
    // One branch is learnt once, and one igloo does one job.
    let mut g = game();
    let ig = home(&g, 0);
    let other = second_igloo(&mut g);
    for igloo in [ig, other] {
        g.igloos[igloo as usize].fish = 100;
        g.igloos[igloo as usize].wood = 100;
    }
    g.command(0, Command::StartResearch { igloo: ig, branch: SNOWCRAFT });
    assert!(g.igloos[ig as usize].research.is_some(), "the research did not start");
    // The same igloo cannot take on another piece of work.
    g.command(0, Command::StartResearch { igloo: ig, branch: ICECRAFT });
    assert!(g.igloos[ig as usize].research.as_ref().unwrap().branch == SNOWCRAFT, "the igloo changed its mind");
    // And the same branch is not learnt twice at once.
    g.command(0, Command::StartResearch { igloo: other, branch: SNOWCRAFT });
    assert!(g.igloos[other as usize].research.is_none(), "the branch was learnt twice at once");
    // A different branch is fine.
    g.command(0, Command::StartResearch { igloo: other, branch: ICECRAFT });
    assert!(g.igloos[other as usize].research.is_some(), "a second branch was refused");
}

#[test]
fn snowcraft_makes_throws_reach_farther() {
    // Snowcraft level one: snowballs fly farther.
    let mut g = game();
    let ig = home(&g, 0);
    let door = g.igloos[ig as usize].door;
    g.weather.season = Season::DeepWinter;
    g.weather.blizzard = 0;
    let before = g.throw_range(Kind::Snowballer, 0, door, door);
    learn(&mut g, SNOWCRAFT);
    g.weather.blizzard = 0;
    let after = g.throw_range(Kind::Snowballer, 0, door, door);
    assert!(after > before, "the throw did not reach farther ({before} -> {after})");
    assert_eq!(g.effects_of(0).pct(Stat::Range), 115);
    // And the aim is better.
    assert_eq!(g.effects_of(0).pct(Stat::Spread), 80);
}

#[test]
fn icecraft_makes_snow_buildings_tougher() {
    // Icecraft level one: snow buildings are stronger.
    let mut g = game();
    let ig = home(&g, 0);
    let door = g.igloos[ig as usize].door;
    g.igloos[ig as usize].wood = 20;
    // A free spot for a hut a few tiles out from the door.
    let mut site = None;
    for r in 3..10 {
        for k in 0..16 {
            let p = door + Vec2::from_angle(k as f32 / 16.0 * 6.283) * r as f32;
            if g.placement_problem(0, BuildKind::Sauna, p.tile()).is_none() && g.map.resources.iter().all(|r| sim::math::v2(r.x as f32, r.y as f32).dist(p) > 4.0) {
                site = Some(p.tile());
                break;
            }
        }
        if site.is_some() {
            break;
        }
    }
    let site = site.expect("nowhere to build");
    let b = g.place_finished_for_test(0, BuildKind::Sauna, site);
    let hut = g.buildings[b as usize].clone();
    assert_eq!(hut.hp, BuildKind::Sauna.max_hp());

    learn(&mut g, ICECRAFT);
    assert_eq!(g.effects_of(0).pct(Stat::BuildingHp), 150);
    assert_eq!(g.max_hp(&hut), BuildKind::Sauna.max_hp() * 3 / 2, "the hut is no tougher");
    // Only snow buildings get stronger.
    assert_eq!(BuildKind::Store.max_hp_with(g.effects_of(0)), BuildKind::Store.max_hp());
}

#[test]
fn fishery_makes_fishing_faster() {
    // Fishery level one: workers land their fish sooner.
    let mut g = game();
    let worker = g.penguins.iter().find(|p| p.owner == 0 && p.kind == Kind::Worker).unwrap().id;
    let before = fish_time(&mut g, worker);
    learn(&mut g, FISHERY);
    let after = fish_time(&mut g, worker);
    assert!(after < before, "fishing did not get faster ({before} -> {after})");
    assert_eq!(g.effects_of(0).pct(Stat::FishTime), 70);
}

#[test]
fn quick_lessons_make_camps_train_faster() {
    // Fishery level two: camps train penguins sooner.
    let mut g = game();
    assert_eq!(g.effects_of(0).pct(Stat::TrainTime), 100);
    g.players[0].research[FISHERY as usize] = 2;
    assert_eq!(g.effects_of(0).pct(Stat::TrainTime), 70);
}

/// How many ticks a worker takes to land a fish at the nearest bank.
fn fish_time(g: &mut Game, worker: u16) -> u32 {
    let at = g.penguins[worker as usize].pos;
    let bank = g.shores.iter().min_by(|a, b| a.pos.dist2(at).partial_cmp(&b.pos.dist2(at)).unwrap()).unwrap().tile;
    // Aim at a water tile beside that bank.
    let water = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().map(|&(dx, dy)| (bank.0 + dx, bank.1 + dy)).find(|&(x, y)| g.map.ground_at(x, y) == sim::map::Ground::Water).unwrap();
    g.penguins[worker as usize].carry = Carry::None;
    g.command(0, Command::Fish { units: vec![worker], tile: water });
    // Wait for the worker to reach the bank and start fishing.
    let mut n = 0;
    while g.penguins[worker as usize].act != Act::Fish && n < 1200 {
        g.tick();
        n += 1;
    }
    assert_eq!(g.penguins[worker as usize].act, Act::Fish, "the worker never started fishing");
    // Then count the ticks until the fish is landed.
    let mut t = 0;
    while g.penguins[worker as usize].carry == Carry::None && t < 1200 {
        g.tick();
        t += 1;
    }
    assert_eq!(g.penguins[worker as usize].carry, Carry::Fish, "no fish was landed");
    t
}

#[test]
fn capturing_an_igloo_cancels_the_research() {
    // Half-done work is lost with the old owners.
    let mut g = game();
    let ig = home(&g, 0);
    let level = level_of(&g, SNOWCRAFT);
    g.igloos[ig as usize].fish = 100;
    g.igloos[ig as usize].wood = 100;
    g.command(0, Command::StartResearch { igloo: ig, branch: SNOWCRAFT });
    assert!(g.igloos[ig as usize].research.is_some(), "the research did not start");
    assert_eq!(g.igloos[ig as usize].research.as_ref().unwrap().total, level.ticks);
    // Take the igloo over the usual way: an invader inside and the timer run out.
    let invader = g.penguins.iter().find(|p| p.owner == 1 && p.kind.fights()).unwrap().id;
    g.penguins[invader as usize].inside = Some(ig);
    g.igloos[ig as usize].occupants.push(invader);
    g.igloos[ig as usize].takeover = Some(Takeover { by: 1, ticks_left: 1, total: 100 });
    let ev = run(&mut g, 2);
    assert!(ev.iter().any(|e| matches!(e, Event::Captured { igloo, .. } if *igloo == ig)), "the igloo was not captured");
    assert!(g.igloos[ig as usize].research.is_none(), "the research survived the capture");
    assert_eq!(g.igloos[ig as usize].owner, 1);
    // The new owners have learnt nothing.
    assert_eq!(g.levels_of(1), [0; BRANCHES]);
    assert_eq!(g.levels_of(0), [0; BRANCHES]);
}

#[test]
fn the_client_sees_the_research() {
    // The snapshot carries the levels and the work in progress.
    let mut g = game();
    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    let ig = home(&g, 0);
    let level = level_of(&g, SNOWCRAFT);
    g.igloos[ig as usize].fish = 100;
    g.igloos[ig as usize].wood = 100;
    g.command(0, Command::StartResearch { igloo: ig, branch: SNOWCRAFT });
    let ev = std::mem::take(&mut g.events);
    view.apply(&enc.encode(&g, &ev)).unwrap();
    assert_eq!(view.research[0], g.levels_of(0));
    let (branch, left, total) = view.igloos[ig as usize].research.expect("the research in the view");
    assert_eq!(branch, SNOWCRAFT);
    assert_eq!(total, level.ticks);
    assert!(left <= total);

    // And when it is done.
    run(&mut g, level.ticks as u32);
    let ev = std::mem::take(&mut g.events);
    view.apply(&enc.encode(&g, &ev)).unwrap();
    assert_eq!(view.research[0], g.levels_of(0));
    assert_eq!(view.research[0][SNOWCRAFT as usize], 1);
    assert!(view.igloos[ig as usize].research.is_none(), "the work is still shown as running");
}

#[test]
fn the_ai_researches_once_its_economy_is_steady() {
    // The computer learns tricks of its own, but only when it can afford them.
    let path = format!("{}/../../assets/maps/fixtures/greenwood.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = vec![PlayerSetup { name: "AI".into(), color: 0, team: 0, ai: Some(AiLevel::Normal) }];
    let mut g = Game::new_with(map, setups, 3, sim::game::CoreRules { tech: true, ..Default::default() }.map_start());
    run(&mut g, 300 * sim::TICK_RATE);
    let levels = g.levels_of(0);
    assert!(levels.iter().any(|&l| l > 0), "the AI learnt nothing: {levels:?}");
}
