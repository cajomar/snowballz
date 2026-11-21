//! Fishing slots (the `armies` rule, DESIGN.md 0.2): workers fish only in a
//! slot at an igloo or a Sushi House (2, upgraded to 4 then 8), fish the
//! banks beside it and set the catch down there.
use sim::game::*;
use sim::map::Map;
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};

fn game(core: CoreRules) -> Game {
    let path = format!("{}/../../assets/maps/fixtures/greenwood.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let mut g = Game::new_with(map, setups, 7, core);
    g.weather.calm = true;
    let home = home(&g);
    for _ in 0..8 {
        g.spawn_for_test(home, Kind::Worker);
    }
    g
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

/// A Sushi House a little way from home with a bank right beside it.
fn store_by_water(g: &mut Game) -> u16 {
    let door = g.igloos[home(g) as usize].door;
    let mut banks: Vec<_> = g.shores.iter().map(|s| s.pos).filter(|p| p.dist(door) > 8.0).collect();
    banks.sort_by(|a, b| a.dist(door).partial_cmp(&b.dist(door)).unwrap());
    for bank in banks {
        let (x, y) = bank.tile();
        for (dx, dy) in [(-2, -2), (-2, 0), (0, -2), (0, 0), (-1, -1), (1, 1), (-3, -1), (1, -1), (-1, 1)] {
            let t = (x + dx, y + dy);
            if g.placement_problem(0, BuildKind::Store, t).is_none() {
                return g.place_finished_for_test(0, BuildKind::Store, t);
            }
        }
    }
    panic!("no room for a Sushi House by the water");
}

fn crew(g: &Game, f: Fishery) -> Vec<u16> {
    g.penguins.iter().filter(|p| p.fishery == Some(f)).map(|p| p.id).collect()
}

/// Workers of player 0 fishing without a slot.
fn unslotted_fishing(g: &Game) -> usize {
    g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Worker && p.fishery.is_none() && p.act == Act::Fish).count()
}

#[test]
fn only_workers_in_a_slot_fish_and_the_igloo_has_two() {
    let mut g = game(CoreRules::default());
    let ig = home(&g);
    let at = Fishery::Igloo(ig);
    assert_eq!(g.igloos[ig as usize].fish_want, IGLOO_FISH_SLOTS);
    let mut catches = 0;
    for _ in 0..60 {
        let before: Vec<bool> = g.penguins.iter().map(|p| p.carry == Carry::Fish).collect();
        run(&mut g, 1);
        assert_eq!(unslotted_fishing(&g), 0, "a worker fished without a slot");
        catches += g.penguins.iter().filter(|p| p.fishery == Some(at) && p.carry == Carry::None && before[p.id as usize]).count();
    }
    assert_eq!(g.fishers(at), 2, "the igloo called in its two fishers");
    assert!(catches > 2, "and they brought the catch home");

    // Igloos are never built up: build Sushi Houses for more. You choose
    // how many of its two slots are filled.
    g.command(0, Command::SetIglooFishers { igloo: ig, want: 1 });
    run(&mut g, 2);
    assert_eq!(g.fishers(at), 1, "one fisher, as wanted");
    // Another player can't change it.
    g.command(1, Command::SetIglooFishers { igloo: ig, want: 2 });
    assert_eq!(g.igloos[ig as usize].fish_want, 1);
    g.command(0, Command::SetIglooFishers { igloo: ig, want: 9 });
    assert_eq!(g.igloos[ig as usize].fish_want, IGLOO_FISH_SLOTS, "no more than it has");
    run(&mut g, 2);
    assert_eq!(g.fishers(at), 2);

    // The client sees the slots.
    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    view.apply(&enc.encode(&g, &[])).unwrap();
    let v = &view.igloos[ig as usize];
    assert_eq!((v.fish_want, v.fishers), (IGLOO_FISH_SLOTS, 2));
}

#[test]
fn a_sushi_house_by_the_water_fills_its_slots_and_its_fishers_unload_there() {
    let mut g = game(CoreRules::default());
    let b = store_by_water(&mut g);
    let f = Fishery::House(b);
    let at = g.buildings[b as usize].center();
    assert_eq!(g.buildings[b as usize].slots, FISH_SLOTS[0]);
    run(&mut g, 4);
    assert_eq!(g.store_crew(b), FISH_SLOTS[0] as usize, "the house called in its fishers");
    let mut unloads = 0;
    for _ in 0..120 * sim::TICK_RATE {
        g.tick();
        g.events.clear();
        for p in g.penguins.iter().filter(|p| p.fishery == Some(f) && p.act == Act::Unload) {
            assert!(p.pos.dist(at) < 3.0, "a fisher unloaded at {:?}, away from its house", p.pos);
            unloads += 1;
        }
    }
    assert!(unloads > 0, "the fishers brought fish to the house");
    for id in crew(&g, f) {
        let p = &g.penguins[id as usize];
        assert!(p.pos.dist(at) < STORE_REACH + 3.0, "fisher {id} stays by the house, at {:?}", p.pos);
    }

    // Built up to 4, then 8, and no further: builders do the work, and
    // the new room stays empty until it is wanted.
    for room in [FISH_SLOTS[1], FISH_SLOTS[2]] {
        g.command(0, Command::UpgradeBuilding { building: b });
        assert!(g.buildings[b as usize].upgrading.is_some());
        for _ in 0..240 {
            run(&mut g, 1);
            if g.buildings[b as usize].slots == room {
                break;
            }
        }
        assert_eq!(g.buildings[b as usize].slots, room);
        run(&mut g, 5);
        assert_eq!(g.store_crew(b), FISH_SLOTS[0] as usize, "built up, but nobody more called in");
    }
    g.command(0, Command::UpgradeBuilding { building: b });
    assert_eq!(g.buildings[b as usize].upgrading, None, "eight is the most");
    // Wanting them all fills them, newcomers from the sea if need be.
    let ig = home(&g) as usize;
    g.igloos[ig].fish = 200;
    g.command(0, Command::SetWant { building: b, want: 8 });
    run(&mut g, 30);
    assert_eq!(g.store_crew(b), FISH_SLOTS[2] as usize, "all eight wanted and filled");

    // The client sees the fishers as the house's filled slots.
    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    view.apply(&enc.encode(&g, &[])).unwrap();
    let v = view.buildings[b as usize];
    assert_eq!(v.slots, FISH_SLOTS[2]);
    assert_eq!(v.filled as usize, g.store_crew(b));
}

#[test]
fn fishers_leave_for_other_work_and_the_slot_calls_someone_else() {
    let mut g = game(CoreRules::default());
    let b = store_by_water(&mut g);
    let f = Fishery::House(b);
    run(&mut g, 4);
    let first = crew(&g, f);
    assert_eq!(first.len(), FISH_SLOTS[0] as usize);
    let gone = first[0];
    let target = g.igloos[home(&g) as usize].door;
    g.command(0, Command::Move { units: vec![gone], target, run: false, queue: false });
    run(&mut g, 2);
    assert_eq!(g.penguins[gone as usize].fishery, None, "sent off by hand, it left its slot");
    assert_eq!(g.store_crew(b), FISH_SLOTS[0] as usize, "and someone took its place");

    // A house that falls takes its fishers with it.
    let hp = g.buildings[b as usize].hp;
    g.damage_building(b as usize, hp);
    run(&mut g, 2);
    assert!(crew(&g, f).is_empty(), "no fishers for a fallen house");
}

#[test]
fn a_site_calls_builders_even_from_the_fishers() {
    let mut g = game(CoreRules::default());
    // Only the igloo's two fishers are left as workers.
    let ig = home(&g);
    run(&mut g, 4);
    let fishers = crew(&g, Fishery::Igloo(ig));
    for p in g.penguins.iter_mut().filter(|p| p.owner == 0 && p.kind == Kind::Worker && !fishers.contains(&p.id)) {
        p.kind = Kind::Snowballer;
    }
    let door = g.igloos[ig as usize].door;
    let tile = (3..12)
        .flat_map(|r| (0..16).map(move |k| (door + sim::math::Vec2::from_angle(k as f32 / 16.0 * 6.283) * r as f32).tile()))
        .find(|&t| g.placement_problem(0, BuildKind::Tower, t).is_none())
        .unwrap();
    g.command(0, Command::Build { units: vec![], kind: BuildKind::Tower, tiles: vec![tile] });
    let site = g.buildings.iter().rposition(|b| b.kind == BuildKind::Tower).unwrap() as u16;
    run(&mut g, 2);
    let building = g.penguins.iter().filter(|p| p.order == Order::Build { building: site }).count();
    assert!(building > 0, "nobody came to build");
}

#[test]
fn a_house_away_from_water_has_no_fishers() {
    // Away from the water: the house stands next to home, which has no bank near.
    let mut g = game(CoreRules::default());
    let door = g.igloos[home(&g) as usize].door;
    let dry = (3..16)
        .flat_map(|r| (0..16).map(move |k| (door + sim::math::Vec2::from_angle(k as f32 / 16.0 * 6.283) * r as f32).tile()))
        .find(|&t| {
            let c = sim::math::v2(t.0 as f32 + 1.0, t.1 as f32 + 1.0);
            g.placement_problem(0, BuildKind::Store, t).is_none() && g.shores.iter().all(|s| s.pos.dist(c) > STORE_REACH + 1.0)
        })
        .expect("dry ground near home");
    let b = g.place_finished_for_test(0, BuildKind::Store, dry);
    run(&mut g, 3);
    assert_eq!(g.store_crew(b), 0);
}
