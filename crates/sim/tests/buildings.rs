//! Buildings: placing, building, what each one does, and losing them.

use sim::game::*;
use sim::map::{Map, Obstacle};
use sim::math::{v2, Vec2};
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};

fn game(map_name: &str, players: usize) -> Game {
    let path = format!("{}/../../assets/maps/fixtures/{map_name}.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..players).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    Game::new_with(map, setups, 3, sim::game::CoreRules::default().map_start())
}

/// No logs in reach: a fire's stoker has nothing to fetch, so fires burn
/// down on their own, which these tests watch.
fn felled(g: &mut Game) {
    for o in g.map.obstacle.iter_mut().filter(|o| **o == Obstacle::Tree) {
        *o = Obstacle::None;
    }
}

fn home(g: &Game, player: u8) -> usize {
    g.igloos.iter().position(|i| i.owner == player).unwrap()
}

fn workers(g: &Game, player: u8) -> Vec<u16> {
    g.penguins.iter().filter(|p| p.owner == player && p.kind == Kind::Worker).map(|p| p.id).collect()
}

/// A free spot for a building of this kind a few tiles out from the door.
fn site(g: &Game, player: u8, kind: BuildKind) -> (i32, i32) {
    let door = g.igloos[home(g, player)].door;
    for r in 3..10 {
        for k in 0..16 {
            let p = door + Vec2::from_angle(k as f32 / 16.0 * 6.283) * r as f32;
            let t = p.tile();
            if g.placement_problem(player, kind, t).is_none() {
                return t;
            }
        }
    }
    panic!("nowhere to build");
}

fn run(g: &mut Game, secs: u32, view: &mut (SnapshotEncoder, WorldView)) {
    for _ in 0..secs * sim::TICK_RATE {
        g.tick();
        let ev = std::mem::take(&mut g.events);
        view.1.apply(&view.0.encode(g, &ev)).unwrap();
    }
}

#[test]
fn walls_stop_snowballs_and_crumble_into_snow() {
    let mut g = game("trio", 2);
    let mut v = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 0)));
    let t = site(&g, 0, BuildKind::Palisade);
    let row: Vec<(i32, i32)> = (-2..=2).map(|d| (t.0 + d, t.1)).collect();
    g.command(0, Command::Build { units: workers(&g, 0), kind: BuildKind::Palisade, tiles: row.clone() });
    let n = g.buildings.len();
    assert!(n >= 3, "placed {n} wall segments");
    run(&mut g, 60, &mut v);
    assert!(g.buildings.iter().all(|b| b.done()), "workers moved along the wall");

    // Player 1 penguin north of the wall, player 0 thrower south of it.
    let wall = g.buildings[0].tile;
    let behind = v2(wall.0 as f32 + 0.5, wall.1 as f32 - 0.4);
    let thrower = g.penguins.iter().find(|p| p.owner == 1 && p.kind == Kind::Snowballer).unwrap().id;
    let target = g.penguins.iter().find(|p| p.owner == 0 && p.kind == Kind::Snowballer).unwrap().id;
    g.penguins[target as usize].pos = behind;
    let tw = g.penguins[target as usize].warmth;
    let mut wall_hits = 0;
    for _ in 0..40 {
        let tp = &mut g.penguins[thrower as usize];
        tp.pos = v2(wall.0 as f32 + 0.5, wall.1 as f32 + 4.5);
        tp.snowballs = MAX_SNOWBALLS;
        g.penguins[target as usize].pos = behind;
        // Bystanders keep out of it.
        for p in &mut g.penguins {
            if p.id != thrower {
                p.snowballs = 0;
            }
            if p.id != thrower && p.id != target && p.inside.is_none() {
                p.pos = v2(1.5, 1.5);
            }
        }
        g.tick();
        for e in std::mem::take(&mut g.events) {
            match e {
                Event::Landed { hit: HitKind::Building, .. } => wall_hits += 1,
                _ => {}
            }
        }
    }
    assert!(wall_hits > 0, "throws hit the wall");
    let hp: u32 = g.buildings.iter().map(|b| b.hp as u32).sum();
    assert!(hp < g.buildings.iter().map(|b| b.kind.max_hp() as u32).sum::<u32>(), "the wall took damage");
    assert!(g.penguins[target as usize].warmth >= tw.saturating_sub(1), "the wall protected the penguin");

    // Knock it down; the snow comes back.
    let b0 = g.buildings[0].tile;
    let i = g.map.idx(b0.0, b0.1);
    let before = g.terrain.snow[i];
    g.command(0, Command::Demolish { building: 0 });
    assert!(!g.buildings[0].alive);
    assert!(g.grid.walkable(b0.0, b0.1));
    assert_eq!(g.terrain.snow[i], before + 2);
    run(&mut g, 1, &mut v);
    assert!(!v.1.buildings[0].alive);
}

#[test]
fn palisades_hold_more_hits_than_walls() {
    let mut g = game("trio", 2);
    let mut v = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 0)));
    let ig = home(&g, 0);
    g.igloos[ig].wood = 10;
    let palisade_site = site(&g, 0, BuildKind::Palisade);
    g.command(0, Command::Build { units: workers(&g, 0), kind: BuildKind::Palisade, tiles: vec![palisade_site] });
    run(&mut g, 25, &mut v);
    let palisade = &g.buildings[0];
    assert!(palisade.done());
    assert_eq!(palisade.hp, BuildKind::Palisade.max_hp());
    assert!(!g.grid.walkable(palisade_site.0, palisade_site.1));
    assert!(g.blocks_snowballs(palisade_site.0, palisade_site.1));
}

#[test]
fn you_may_build_anywhere_there_is_room_even_beside_an_enemy() {
    let g = game("trio", 2);
    // (There used to be a no-build zone round enemy igloos.)
    let theirs = g.igloos[home(&g, 1)].door;
    let found = (3..8).any(|r| (0..16).any(|k| g.placement_problem(0, BuildKind::Palisade, (theirs + Vec2::from_angle(k as f32 / 16.0 * 6.283) * r as f32).tile()).is_none()));
    assert!(found, "no room to build near the enemy igloo");
    // Room is still needed: not on an igloo's own door.
    let door = g.igloos[home(&g, 0)].door;
    assert!(g.placement_problem(0, BuildKind::Palisade, door.tile()).is_some());
}

#[test]
fn fish_and_wood_do_not_warm_penguins_in_an_igloo() {
    let mut g = game("trio", 2);
    g.weather.calm = true;
    let ig = home(&g, 0);
    g.igloos[ig].fish = 80;
    g.igloos[ig].wood = 80;
    let sb = g.penguins.iter().find(|p| p.owner == 0 && p.kind == Kind::Snowballer).unwrap().id;
    let p = &mut g.penguins[sb as usize];
    p.warmth = 1;
    p.cold = true;
    p.order = Order::Enter { igloo: ig as u8 };
    for _ in 0..6 * sim::TICK_RATE {
        g.tick();
        g.events.clear();
    }
    let p = &g.penguins[sb as usize];
    assert!(p.warmth < p.max_warmth, "warmth {}/{}: it should not be fully warm after six seconds", p.warmth, p.max_warmth);
    assert_eq!((g.igloos[ig].fish, g.igloos[ig].wood), (80, 80), "the igloo burned nothing to warm it");
}

#[test]
fn a_campfire_burns_out_and_warms_only_beside_it() {
    let mut g = game("trio", 2);
    felled(&mut g);
    g.weather.calm = true;
    let mut v = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 0)));
    let ig = home(&g, 0);
    g.igloos[ig].wood = 5;
    let t = site(&g, 0, BuildKind::Campfire);
    g.command(0, Command::Build { units: workers(&g, 0), kind: BuildKind::Campfire, tiles: vec![t] });
    assert_eq!(g.igloos[ig].wood, 5, "nothing is paid up front");
    for _ in 0..60 {
        if g.buildings[0].done() {
            break;
        }
        run(&mut g, 1, &mut v);
    }
    let fire = g.buildings[0].clone();
    assert!(fire.done() && fire.lit, "lit as soon as it is built");
    assert_eq!(fire.wood_in as u16, BuildKind::Campfire.wood(), "a log was carried in");
    assert!(v.1.buildings[0].lit);
    let c = fire.center();
    let ids: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Snowballer).map(|p| p.id).take(2).collect();
    let (near, far) = (ids[0], ids[1]);
    for (id, at) in [(near, c + v2(1.0, 0.0)), (far, c + v2(4.5, 0.0))] {
        let p = &mut g.penguins[id as usize];
        p.pos = at;
        p.warmth = 3;
        p.order = Order::Hold { pos: at };
    }
    for _ in 0..7 * sim::TICK_RATE {
        g.penguins[near as usize].pos = c + v2(1.0, 0.0);
        g.penguins[far as usize].pos = c + v2(4.5, 0.0);
        g.tick();
        g.events.clear();
    }
    assert!(g.penguins[near as usize].warmth > 3, "warmed beside it");
    assert!(g.penguins[near as usize].warmth < g.penguins[near as usize].max_warmth, "but only slowly");
    assert_eq!(g.penguins[far as usize].warmth, 3, "not three tiles away");
    // It burns out by itself.
    run(&mut g, 70, &mut v);
    assert!(!g.buildings[0].alive, "burnt out and gone");
    assert!(!v.1.buildings[0].alive);
}

#[test]
fn a_cold_penguin_with_no_sauna_walks_to_a_campfire() {
    let mut g = game("trio", 2);
    g.weather.calm = true;
    let mut v = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 0)));
    let ig = home(&g, 0);
    g.igloos[ig].wood = 5;
    let t = site(&g, 0, BuildKind::Campfire);
    g.command(0, Command::Build { units: workers(&g, 0), kind: BuildKind::Campfire, tiles: vec![t] });
    run(&mut g, 40, &mut v);
    assert!(g.buildings[0].lit);
    let c = g.buildings[0].center();
    let sb = g.penguins.iter().find(|p| p.owner == 0 && p.kind == Kind::Snowballer && p.inside.is_none()).unwrap().id;
    let spot = g.grid.nearest_walkable((c + v2(6.0, 0.0)).tile(), 3).unwrap();
    let p = &mut g.penguins[sb as usize];
    p.pos = v2(spot.0 as f32 + 0.5, spot.1 as f32 + 0.5);
    p.warmth = 1;
    p.cold = true;
    p.order = Order::None;
    let mut got_there = false;
    for _ in 0..15 * sim::TICK_RATE {
        g.tick();
        g.events.clear();
        got_there |= g.penguins[sb as usize].pos.dist(c) <= 1.9;
    }
    assert!(got_there, "it went to the fire");
    assert!(g.penguins[sb as usize].inside.is_none(), "rather than home");
}

#[test]
fn the_showcase_puts_up_every_building_finished() {
    let mut g = game("trio", 2);
    g.showcase(0, 1.0);
    // (Not an igloo site: it can't go near home, and looks like the igloo.)
    for kind in BuildKind::ALL.into_iter().filter(|k| *k != BuildKind::Igloo) {
        assert!(g.buildings.iter().any(|b| b.kind == kind && b.owner == 0 && b.done()), "no finished {kind:?}");
    }
}

#[test]
fn a_site_with_no_work_done_neither_blocks_nor_draws_fire() {
    // Staking out sites must not be a free wall or a decoy: until the first
    // step is built, people walk through it, snowballs fly over it and
    // nobody throws at it.
    let mut g = game("trio", 2);
    g.weather.calm = true;
    let mut v = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 0)));
    let t = site(&g, 0, BuildKind::Palisade);
    g.command(0, Command::Build { units: vec![], kind: BuildKind::Palisade, tiles: vec![t] });
    // Then the tribe's workers are away, so nobody starts on the site.
    let hands: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Worker).map(|p| p.id).collect();
    for &h in &hands {
        g.penguins[h as usize].kind = Kind::Snowballer;
    }
    let b = g.buildings.iter().position(|b| b.tile == t && b.alive).expect("the site was staked out");
    assert_eq!(g.buildings[b].built, 0);
    let hp = g.buildings[b].hp;
    assert!(g.grid.walkable(t.0, t.1), "a staked site doesn't block the way");
    assert!(!g.blocks_snowballs(t.0, t.1), "nor stop snowballs");

    // An enemy Packer right beside it, with snowballs, leaves it alone.
    let packer = g.penguins.iter().find(|p| p.owner == 1 && p.kind == Kind::Snowballer).unwrap().id;
    let mut hits = 0;
    for _ in 0..5 * sim::TICK_RATE {
        let p = &mut g.penguins[packer as usize];
        p.kind = Kind::Packer;
        p.pos = v2(t.0 as f32 + 0.5, t.1 as f32 + 3.0);
        p.snowballs = MAX_SNOWBALLS;
        p.order = Order::Hold { pos: p.pos };
        // The tribe holds its fire, so the Packer stays on its feet.
        for q in g.penguins.iter_mut().filter(|q| q.owner == 0) {
            q.snowballs = 0;
        }
        g.tick();
        hits += std::mem::take(&mut g.events).iter().filter(|e| matches!(e, Event::Landed { pos, .. } if pos.tile() == t)).count();
    }
    assert_eq!(g.buildings[b].built, 0, "still only staked out");
    assert_eq!(hits, 0, "nobody threw at the staked site");
    assert_eq!(g.buildings[b].hp, hp, "and it took no damage");

    // With the first step built it becomes a real wall.
    for &h in &hands {
        g.penguins[h as usize].kind = Kind::Worker;
    }
    g.command(0, Command::Repair { units: workers(&g, 0), building: b as u16 });
    for _ in 0..60 * sim::TICK_RATE {
        g.tick();
        let ev = std::mem::take(&mut g.events);
        v.1.apply(&v.0.encode(&g, &ev)).unwrap();
        if g.buildings[b].built > 0 {
            break;
        }
    }
    assert!(g.buildings[b].built > 0, "the builders got going");
    assert!(!g.grid.walkable(t.0, t.1), "a started site blocks the way");
    assert!(g.blocks_snowballs(t.0, t.1));
    // And now the same Packer goes for it.
    let mut hits = 0;
    for _ in 0..5 * sim::TICK_RATE {
        let p = &mut g.penguins[packer as usize];
        p.pos = v2(t.0 as f32 + 0.5, t.1 as f32 + 3.0);
        p.snowballs = MAX_SNOWBALLS;
        p.order = Order::Hold { pos: p.pos };
        for q in g.penguins.iter_mut().filter(|q| q.owner == 0) {
            q.snowballs = 0;
        }
        g.tick();
        hits += std::mem::take(&mut g.events).iter().filter(|e| matches!(e, Event::Landed { pos, .. } if pos.tile() == t)).count();
    }
    assert!(hits > 0, "a started wall is fair game");
}
