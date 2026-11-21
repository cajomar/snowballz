//! The new core (DESIGN.md section 0): a fixed population that changes hands
//! by freezing and thawing.

use sim::game::*;
use sim::map::Map;

fn map(name: &str) -> Map {
    let path = format!("{}/../../assets/maps/{name}.sbmap", env!("CARGO_MANIFEST_DIR"));
    Map::load_file(std::path::Path::new(&path)).unwrap()
}

fn setups() -> Vec<PlayerSetup> {
    (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect()
}

fn game(core: CoreRules) -> Game {
    Game::new_with(map("fixtures/trio"), setups(), 7, core)
}

fn home(g: &Game, owner: u8) -> u8 {
    g.igloos.iter().position(|i| i.owner == owner).unwrap() as u8
}

/// Tick `n` times with nobody throwing.
fn run(g: &mut Game, n: u32) -> Vec<Event> {
    let mut all = Vec::new();
    for _ in 0..n {
        for p in &mut g.penguins {
            p.snowballs = 0;
        }
        g.tick();
        all.extend(std::mem::take(&mut g.events));
    }
    all
}

#[test]
fn the_new_core_is_the_default() {
    assert_eq!(Game::new(map("fixtures/trio"), setups(), 7).core, CoreRules::default());
}

#[test]
fn core_has_no_prisoners() {
    let mut g = game(CoreRules::default().map_start());

    let at = open_ground(&g, 8, 4);
    let v = someone(&g, 0, Kind::Worker, &[]);
    freeze_at(&mut g, v, at);
    assert_eq!(g.penguins[v as usize].owner, NEUTRAL);
    assert!(!g.capturable(&g.penguins[v as usize], g.team(1)), "frozen penguins thaw wild instead of becoming prisoners");
    assert!(g.igloos.iter().all(|ig| ig.prisoners.is_empty()));
}

#[test]
fn workers_found_expansion_igloos_at_map_sites() {
    let mut g = game(CoreRules::default().map_start());
    let home = home(&g, 0);
    let site = g.igloos.iter().find(|ig| ig.owner == NEUTRAL).unwrap().id;
    // (With armies the wood share is paid in fish.)
    g.igloos[home as usize].fish = GREAT_IGLOO_FISH + GREAT_IGLOO_WOOD;
    // (Nobody fishing, so no slot calls a newcomer from the sea with it.)
    g.igloos[home as usize].fish_want = 0;
    for p in &mut g.penguins {
        if p.owner == 0 && p.kind == Kind::Worker {
            p.act = Act::Idle;
            p.carry = Carry::None;
            p.order = Order::Hold { pos: p.pos };
        }
    }
    let worker = someone(&g, 0, Kind::Worker, &[]);

    g.command(0, Command::Enter { units: vec![worker], igloo: site });
    let events = run(&mut g, 45 * sim::TICK_RATE);

    assert_eq!(g.igloos[site as usize].owner, 0, "worker founded the expansion base");
    assert_eq!(g.igloos[home as usize].fish, 0, "founding paid fish at home");
    assert_eq!(g.igloos[home as usize].wood, 0, "founding paid pooled wood");
    assert!(events.iter().any(|e| matches!(e, Event::Captured { igloo, from: NEUTRAL, to: 0 } if *igloo == site)));
}

#[test]
fn expansion_igloos_need_a_worker_and_the_full_cost() {
    let mut g = game(CoreRules::default().map_start());
    let site = g.igloos.iter().find(|ig| ig.owner == NEUTRAL).unwrap().id;
    let fighter = someone(&g, 0, Kind::Snowballer, &[]);
    g.command(0, Command::Enter { units: vec![fighter], igloo: site });
    run(&mut g, 20 * sim::TICK_RATE);
    assert_eq!(g.igloos[site as usize].owner, NEUTRAL, "soldiers cannot found a base");

    let worker = someone(&g, 0, Kind::Worker, &[]);
    g.command(0, Command::Enter { units: vec![worker], igloo: site });
    run(&mut g, 20 * sim::TICK_RATE);
    assert_eq!(g.igloos[site as usize].owner, NEUTRAL, "a worker cannot found without fish and wood");
}

use sim::math::{v2, Vec2};
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};

fn all() -> CoreRules {
    CoreRules::default().map_start()
}

/// The middle of an open, walkable patch `w` x `h` tiles, away from igloos.
fn open_ground(g: &Game, w: i32, h: i32) -> Vec2 {
    for y in 3..g.map.h - h - 3 {
        for x in 3..g.map.w - w - 3 {
            let clear = (0..h).all(|dy| (0..w).all(|dx| g.grid.walkable(x + dx, y + dy)));
            let c = v2(x as f32 + w as f32 / 2.0, y as f32 + h as f32 / 2.0);
            if clear && g.igloos.iter().all(|ig| ig.door.dist(c) > 12.0) {
                return c;
            }
        }
    }
    panic!("no open ground");
}

/// A penguin of `owner`'s that isn't inside, with no orders.
fn someone(g: &Game, owner: u8, kind: Kind, skip: &[u16]) -> u16 {
    g.penguins.iter().find(|p| p.owner == owner && p.kind == kind && p.inside.is_none() && !skip.contains(&p.id)).unwrap().id
}

/// Freeze `v` at `at` the real way: an enemy snowballer pelts it.
fn freeze_at(g: &mut Game, v: u16, at: Vec2) {
    let owner = g.penguins[v as usize].owner;
    let foe = (owner + 1) % 2;
    let thrower = someone(g, foe, Kind::Snowballer, &[]);
    g.penguins[v as usize].pos = at;
    g.penguins[v as usize].warmth = 1;
    g.penguins[v as usize].was = owner;
    for _ in 0..20 * sim::TICK_RATE {
        for p in &mut g.penguins {
            if p.id != thrower {
                p.snowballs = 0;
            }
        }
        g.penguins[thrower as usize].snowballs = 5;
        g.penguins[thrower as usize].pos = at + v2(3.0, 0.0);
        if g.penguins[v as usize].act != Act::Frozen {
            g.penguins[v as usize].pos = at;
        }
        g.tick();
        g.events.clear();
        if g.penguins[v as usize].act == Act::Frozen {
            g.penguins[thrower as usize].pos = at + v2(0.0, 6.0);
            return;
        }
    }
    panic!("never froze");
}

#[test]
fn freezing_ends_ownership() {
    let mut g = game(all());
    let at = open_ground(&g, 8, 4);
    let v = someone(&g, 0, Kind::Worker, &[]);
    freeze_at(&mut g, v, at);
    assert_eq!(g.penguins[v as usize].owner, NEUTRAL);
    assert_eq!(g.penguins[v as usize].was, 0);
    run(&mut g, 30 * sim::TICK_RATE);
    assert_eq!(g.penguins[v as usize].act, Act::Frozen, "still frozen: nobody wakes it");
}

#[test]
fn frozen_penguins_block_the_way_and_stop_snowballs() {
    let mut g = game(all());
    let at = open_ground(&g, 8, 4);
    let v = someone(&g, 0, Kind::Worker, &[]);
    freeze_at(&mut g, v, at);
    let (x, y) = g.penguins[v as usize].pos.tile();
    assert!(!g.grid.walkable(x, y), "the frozen tile is blocked");
    assert!(g.frozen_at(x, y));
    assert!(g.blocks_snowballs(x, y), "the frozen are cover");
    // Nobody else is left standing on it.
    assert!(g.penguins.iter().all(|p| p.id == v || p.inside.is_some() || p.pos.tile() != (x, y)));
}

#[test]
fn frozen_penguins_thaw_out_wild() {
    let mut g = game(all());
    let at = open_ground(&g, 8, 4);
    let v = someone(&g, 0, Kind::Worker, &[]);
    freeze_at(&mut g, v, at);
    let (x, y) = g.penguins[v as usize].pos.tile();
    // Keep everyone else well away so nobody persuades it.
    let ev = run(&mut g, THAW_TICKS as u32 + 5);
    let p = &g.penguins[v as usize];
    assert_ne!(p.act, Act::Frozen);
    assert!(p.wild && p.owner == NEUTRAL);
    assert!(ev.iter().any(|e| matches!(e, Event::Thawed { penguin, by: NEUTRAL } if *penguin == v)));
    assert!(g.grid.walkable(x, y), "the tile opens up again");
    assert_eq!(g.penguins[v as usize].owner, NEUTRAL);
}

#[test]
fn the_seasons_set_the_pace_of_the_thaw() {
    let half = THAW_TICKS as u32 / 2 + 5;
    for (season, thawed) in [(Season::Midsummer, true), (Season::Thaw, true), (Season::DeepWinter, false), (Season::PolarNight, false)] {
        let mut g = game(all());
        let at = open_ground(&g, 8, 4);
        let v = someone(&g, 0, Kind::Worker, &[]);
        freeze_at(&mut g, v, at);
        for _ in 0..half {
            g.weather.season = season;
            g.weather.season_t = 0;
            for p in &mut g.penguins {
                p.snowballs = 0;
            }
            g.tick();
        }
        assert_eq!(g.penguins[v as usize].act != Act::Frozen, thawed, "{season:?}");
    }
}

#[test]
fn wild_penguins_join_whoever_stands_with_them() {
    let mut g = game(all());
    let at = open_ground(&g, 10, 4);
    let v = someone(&g, 0, Kind::Snowballer, &[]);
    freeze_at(&mut g, v, at);
    g.weather.season = Season::Midsummer;
    run(&mut g, THAW_TICKS as u32 / 2 + 5);
    assert!(g.penguins[v as usize].wild);
    // Player 1 stands right next to it; player 0 hangs back 5 tiles away.
    let friend = someone(&g, 1, Kind::Worker, &[]);
    let far = someone(&g, 0, Kind::Worker, &[]);
    let mut ev = Vec::new();
    let here = g.penguins[v as usize].pos;
    for _ in 0..4 * sim::TICK_RATE {
        g.penguins[friend as usize].pos = here + v2(0.8, 0.0);
        g.penguins[far as usize].pos = here + v2(-5.0, 0.0);
        ev.extend(run(&mut g, 1));
    }
    let p = &g.penguins[v as usize];
    assert_eq!(p.owner, 1);
    assert!(!p.wild);
    assert_eq!(g.igloos[p.home as usize].owner, 1, "it moves into its new tribe's igloo");
    assert!(ev.iter().any(|e| matches!(e, Event::Persuaded { penguin, by: 1 } if *penguin == v)));
}

#[test]
fn wild_and_frozen_penguins_are_nobodys_enemy() {
    let g = game(all());
    assert!(!g.hostile(0, NEUTRAL));
    assert!(!g.hostile(NEUTRAL, 1));
}

#[test]
fn clients_see_ownerless_penguins_and_the_thaw() {
    let mut g = game(all());
    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    let at = open_ground(&g, 8, 4);
    let v = someone(&g, 0, Kind::Worker, &[]);
    freeze_at(&mut g, v, at);
    view.apply(&enc.encode(&g, &[])).unwrap();
    assert_eq!(view.penguins[v as usize].owner, NEUTRAL);
    g.weather.season = Season::Midsummer;
    let ev = run(&mut g, THAW_TICKS as u32 / 2 + 5);
    let seen = view.apply(&enc.encode(&g, &ev)).unwrap();
    assert_eq!(view.penguins[v as usize].owner, NEUTRAL);
    assert_ne!(view.penguins[v as usize].act, Act::Frozen);
    assert!(seen.iter().any(|e| matches!(e, Event::Thawed { penguin, by: NEUTRAL } if *penguin == v)));
}

#[test]
fn you_cant_order_other_tribes_penguins() {
    let mut g = game(all());
    let at = open_ground(&g, 8, 4);
    let enemy = someone(&g, 1, Kind::Snowballer, &[]);
    let p = &mut g.penguins[enemy as usize];
    p.pos = at;
    p.inside = None;
    p.order = Order::Hold { pos: at };
    p.cold = false;
    p.routed = false;
    let target = at + v2(3.0, 0.0);
    g.command(0, Command::Move { units: vec![enemy], target, run: true, queue: false });
    assert!(matches!(g.penguins[enemy as usize].order, Order::Hold { .. }), "the move order was ignored");
    run(&mut g, 2 * sim::TICK_RATE);
    assert!(g.penguins[enemy as usize].pos.dist(at) < 1.0, "the enemy penguin stayed put");
}

#[test]
fn queued_moves_visit_each_waypoint() {
    let mut g = game(all());
    let at = open_ground(&g, 10, 10);
    let id = someone(&g, 0, Kind::Snowballer, &[]);
    {
        let p = &mut g.penguins[id as usize];
        p.pos = at;
        p.inside = None;
        p.act = Act::Idle;
        p.cold = false;
        p.routed = false;
        p.snowballs = 0;
        p.order = Order::None;
        p.target = None;
    }
    let spots = [at + v2(-2.0, -2.0), at + v2(2.0, -2.0), at + v2(2.0, 2.0)];
    assert!(spots.iter().all(|&s| g.grid.walkable_at(s)), "pick open waypoints");
    for &spot in &spots {
        g.command(0, Command::Move { units: vec![id], target: spot, run: false, queue: true });
    }

    // Watch the move targets change: they should follow the queue in order.
    let mut seen = Vec::new();
    for _ in 0..40 * sim::TICK_RATE {
        g.tick();
        g.events.clear();
        let order = g.penguins[id as usize].order;
        if let Order::Move { target, .. } = order {
            if seen.last().is_none_or(|&t: &Vec2| t.dist(target) > 0.1) {
                seen.push(target);
            }
        }
        if seen.len() == spots.len() && !matches!(order, Order::Move { .. }) {
            break;
        }
    }
    assert_eq!(seen.len(), 3,
        "the penguin walked {seen:?}, not all three queued waypoints");
    for (got, want) in seen.iter().zip(spots) {
        assert!(got.dist(want) < 0.1, "waypoint {got:?} should be {want:?}");
    }
    assert!(g.penguins[id as usize].pos.dist(spots[2]) < 1.5, "the penguin did not stop at the last waypoint");
}

#[test]
fn a_holding_snowballer_does_not_chase() {
    let mut g = game(all());
    let at = open_ground(&g, 30, 10);
    let reach = Kind::Snowballer.stats().range;
    let holder = someone(&g, 0, Kind::Snowballer, &[]);
    let foe = someone(&g, 1, Kind::Snowballer, &[]);
    // Keep both on one height, so the slope doesn't stretch the foe into range.
    let mut away = reach + 2.5;
    while away < reach + 6.0 && g.map.height_at(at + v2(away, 0.0)) != g.map.height_at(at) {
        away += 0.5;
    }
    for (id, pos, balls) in [(holder, at, 5), (foe, at + v2(away, 0.0), 0)] {
        let p = &mut g.penguins[id as usize];
        p.pos = pos;
        p.inside = None;
        p.act = Act::Idle;
        p.cold = false;
        p.routed = false;
        p.snowballs = balls;
        p.order = Order::Hold { pos };
        p.target = None;
    }
    g.command(0, Command::Hold { units: vec![holder] });
    assert!(matches!(g.penguins[holder as usize].order, Order::Hold { .. }), "the hold order was ignored");

    let mut thrown = 0;
    for _ in 0..3 * sim::TICK_RATE {
        g.penguins[holder as usize].snowballs = 5;
        g.penguins[foe as usize].snowballs = 0;
        g.tick();
        thrown += g.events.drain(..).filter(|e| matches!(e, Event::Thrown(sb) if sb.thrower == holder)).count();
    }
    assert_eq!(thrown, 0, "a holding penguin threw at a foe out of range");
    assert!(g.penguins[holder as usize].pos.dist(at) < 0.5, "the holding penguin walked after the foe");
    assert!(g.penguins[foe as usize].pos.dist(at + v2(away, 0.0)) < 0.5, "the foe walked");

    // In range it still throws without moving.
    g.penguins[foe as usize].pos = at + v2(reach - 1.0, 0.0);
    let mut thrown = 0;
    for _ in 0..2 * sim::TICK_RATE {
        g.penguins[holder as usize].snowballs = 5;
        g.penguins[foe as usize].snowballs = 0;
        g.tick();
        thrown += g.events.drain(..).filter(|e| matches!(e, Event::Thrown(sb) if sb.thrower == holder)).count();
    }
    assert!(thrown > 0, "a holding penguin should throw at a foe in range");
    assert!(g.penguins[holder as usize].pos.dist(at) < 0.5, "the holding penguin moved to throw");

    // A move order ends the hold.
    g.penguins[foe as usize].pos = at + v2(away + 8.0, 0.0);
    g.command(0, Command::Move { units: vec![holder], target: at + v2(0.0, 3.0), run: false, queue: false });
    for _ in 0..sim::TICK_RATE {
        g.penguins[holder as usize].snowballs = 5;
        g.penguins[foe as usize].snowballs = 0;
        g.tick();
        g.events.clear();
    }
    assert!(g.penguins[holder as usize].pos.dist(at) > 1.0, "the move order did not clear the hold");
}

#[test]
fn enemy_igloos_cannot_be_walked_into() {
    let mut g = game(all());
    let theirs = home(&g, 1);
    let raider = someone(&g, 0, Kind::Snowballer, &[]);
    g.penguins[raider as usize].pos = g.igloos[theirs as usize].door + v2(0.0, 1.0);
    g.command(0, Command::Enter { units: vec![raider], igloo: theirs });
    let ev = run(&mut g, 20 * sim::TICK_RATE);
    let started = ev.iter().any(|e| matches!(e, Event::TakeoverStart { igloo, .. } if *igloo == theirs));
    assert!(!started, "an enemy igloo was taken by walking in");
}

/// Turn one of `owner`'s snowballers into a Warmer standing at `at`.
fn warmer(g: &mut Game, owner: u8, at: Vec2, skip: &[u16]) -> u16 {
    let w = someone(g, owner, Kind::Snowballer, skip);
    let p = &mut g.penguins[w as usize];
    p.kind = Kind::Warmer;
    p.snowballs = 0;
    p.pos = at;
    p.order = Order::None;
    w
}

#[test]
fn warmers_warm_cold_friends() {
    for with_warmer in [false, true] {
        let mut g = game(CoreRules::default().map_start());
        let at = open_ground(&g, 8, 4);
        let patient = someone(&g, 0, Kind::Snowballer, &[]);
        let w = if with_warmer { Some(warmer(&mut g, 0, at + v2(3.0, 0.0), &[patient])) } else { None };
        let full = g.penguins[patient as usize].max_warmth;
        for _ in 0..10 * sim::TICK_RATE {
            let p = &mut g.penguins[patient as usize];
            if g.tick == 0 {
                p.warmth = 3;
                p.cold = false;
            }
            p.pos = at;
            p.order = Order::Hold { pos: at };
            run(&mut g, 1);
        }
        // The cold out here bites a little meanwhile, so allow for one.
        let warmed = g.penguins[patient as usize].warmth + 1 >= full;
        assert_eq!(warmed, with_warmer, "warmth {} of {full}", g.penguins[patient as usize].warmth);
        if let Some(w) = w {
            assert_eq!(g.penguins[w as usize].owner, 0);
        }
    }
}

#[test]
fn warmers_thaw_the_frozen_into_their_tribe() {
    let mut g = game(all());
    let at = open_ground(&g, 8, 4);
    let v = someone(&g, 1, Kind::Snowballer, &[]);
    freeze_at(&mut g, v, at);
    assert_eq!(g.penguins[v as usize].owner, NEUTRAL);
    let w = warmer(&mut g, 0, at + v2(0.0, 3.0), &[]);
    let ev = run(&mut g, 8 * sim::TICK_RATE);
    let p = &g.penguins[v as usize];
    assert_ne!(p.act, Act::Frozen, "thawed");
    assert_eq!(p.owner, 0, "joined the Warmer's tribe");
    assert!(!p.wild);
    assert!(ev.iter().any(|e| matches!(e, Event::Thawed { penguin, by: 0 } if *penguin == v)));
    assert_eq!(g.penguins[w as usize].owner, 0);
}

#[test]
fn warmers_can_be_sent_to_thaw_someone() {
    let mut g = game(all());
    let at = open_ground(&g, 14, 4);
    let v = someone(&g, 1, Kind::Worker, &[]);
    freeze_at(&mut g, v, at);
    let w = warmer(&mut g, 0, at + v2(11.0, 0.0), &[]);
    g.command(0, Command::Thaw { units: vec![w], penguin: v });
    run(&mut g, 15 * sim::TICK_RATE);
    assert_eq!(g.penguins[v as usize].owner, 0);
    // And the order goes over the wire.
    let msg = sim::proto::ClientMsg::Cmd(Command::Thaw { units: vec![w, 3], penguin: v });
    let back = sim::proto::ClientMsg::decode(&msg.encode()).unwrap();
    assert!(matches!(back, sim::proto::ClientMsg::Cmd(Command::Thaw { units, penguin }) if units == vec![w, 3] && penguin == v));
}

#[test]
fn warmers_do_not_fight() {
    assert!(!Kind::Warmer.fights(), "warmers throw no snowballs");
    let mut g = game(all());
    let at = level_spot(&g);
    let foe_pos = at + v2(1.0, 0.0);
    let w = warmer(&mut g, 0, at, &[]);
    let foe = someone(&g, 1, Kind::Snowballer, &[]);
    let mut ev = Vec::new();
    for _ in 0..10 * sim::TICK_RATE {
        let p = &mut g.penguins[w as usize];
        p.pos = at;
        p.snowballs = 5;
        p.warmth = p.max_warmth;
        p.cold = false;
        p.inside = None;
        p.order = Order::Hold { pos: at };
        let q = &mut g.penguins[foe as usize];
        q.pos = foe_pos;
        q.snowballs = q.snowballs.max(5);
        q.warmth = q.max_warmth;
        q.cold = false;
        q.inside = None;
        q.order = Order::Hold { pos: foe_pos };
        g.tick();
        ev.extend(std::mem::take(&mut g.events));
    }
    assert!(ev.iter().all(|e| !matches!(e, Event::Thrown(s) if s.thrower == w)), "the warmer threw nothing");
}

/// Open ground where `at` and the tile east of it stand level (a height
/// bonus could stretch a Warmer's zero range).
fn level_spot(g: &Game) -> Vec2 {
    for y in 3..g.map.h - 4 {
        for x in 3..g.map.w - 4 {
            if !g.grid.walkable(x, y) || !g.grid.walkable(x + 1, y) {
                continue;
            }
            let a = v2(x as f32 + 0.5, y as f32 + 0.5);
            if g.map.height_at(a) != g.map.height_at(a + v2(1.0, 0.0)) {
                continue;
            }
            if g.igloos.iter().all(|ig| ig.door.dist(a) > 12.0) {
                return a;
            }
        }
    }
    panic!("no level open ground");
}

/// Let a computer player run for `secs`, others asleep.
fn ai_for(g: &mut Game, ai: &mut sim::ai::Ai, secs: u32) -> Vec<Event> {
    let mut cmds = Vec::new();
    let mut all = Vec::new();
    for _ in 0..secs * sim::TICK_RATE {
        ai.update(g, &mut cmds);
        for (pl, c) in cmds.drain(..) {
            g.command(pl, c);
        }
        for p in &mut g.penguins {
            p.snowballs = 0;
        }
        g.tick();
        all.extend(std::mem::take(&mut g.events));
    }
    all
}

#[test]
fn the_ai_thaws_the_frozen_near_home() {
    let mut g = game(all());
    let door = g.igloos[home(&g, 0) as usize].door;
    let v = someone(&g, 1, Kind::Snowballer, &[]);
    // Somewhere walkable near player 0's igloo.
    let (x, y) = g.grid.nearest_walkable((door + v2(0.0, 8.0)).tile(), 6).unwrap();
    freeze_at(&mut g, v, v2(x as f32 + 0.5, y as f32 + 0.5));
    let w = warmer(&mut g, 0, door + v2(2.0, 1.0), &[]);
    let mut ai = sim::ai::Ai::new(0, sim::ai::AiLevel::Normal);
    ai_for(&mut g, &mut ai, 30);
    assert_eq!(g.penguins[v as usize].owner, 0, "the AI's Warmer thawed it");
    assert_eq!(g.penguins[w as usize].owner, 0);
}

#[test]
fn the_ai_persuades_wild_penguins() {
    let mut g = game(all());
    let door = g.igloos[home(&g, 0) as usize].door;
    let v = someone(&g, 1, Kind::Worker, &[]);
    let (x, y) = g.grid.nearest_walkable((door + v2(0.0, 10.0)).tile(), 6).unwrap();
    freeze_at(&mut g, v, v2(x as f32 + 0.5, y as f32 + 0.5));
    // Midsummer all along, or the thawed penguin goes back to its tribe.
    let midsummer = |g: &mut Game| {
        g.weather.season = Season::Midsummer;
        g.weather.season_t = 0;
        g.weather.calm = true;
    };
    for _ in 0..THAW_TICKS as u32 / 2 + 5 {
        midsummer(&mut g);
        g.tick();
    }
    assert!(g.penguins[v as usize].wild);
    let mut ai = sim::ai::Ai::new(0, sim::ai::AiLevel::Normal);
    let mut cmds = Vec::new();
    for _ in 0..90 * sim::TICK_RATE {
        midsummer(&mut g);
        ai.update(&g, &mut cmds);
        for (pl, c) in cmds.drain(..) {
            g.command(pl, c);
        }
        g.tick();
        g.events.clear();
    }
    assert_eq!(g.penguins[v as usize].owner, 0, "the AI took it in");
}

#[test]
fn the_ai_repairs_its_battered_igloo() {
    let mut g = game(all());
    let ig = home(&g, 0);
    let max = igloo_max_hp(g.igloos[ig as usize].tier);
    g.igloos[ig as usize].hp = max / 2;
    g.igloos[ig as usize].fish = 0;
    let start = g.igloos[ig as usize].hp;
    // Park the other tribe far away so the coast is clear, and send the
    // workers off so only the AI can call them back.
    let home_center = g.igloos[ig as usize].center;
    let away = (0..g.map.h)
        .flat_map(|y| (0..g.map.w).map(move |x| (x, y)))
        .filter(|&(x, y)| g.grid.walkable(x, y))
        .map(|(x, y)| v2(x as f32 + 0.5, y as f32 + 0.5))
        .max_by(|a, b| a.dist2(home_center).partial_cmp(&b.dist2(home_center)).unwrap())
        .unwrap();
    assert!(away.dist(home_center) > 10.0);
    for p in &mut g.penguins {
        if p.owner == 1 {
            p.pos = away;
            p.order = Order::Hold { pos: away };
            p.inside = None;
            p.cold = false;
            p.routed = false;
            p.carry = Carry::None;
            p.act = Act::Idle;
        }
    }
    let workers: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Worker).map(|p| p.id).collect();
    for &w in &workers {
        let p = &mut g.penguins[w as usize];
        p.pos = away;
        p.order = Order::None;
        p.inside = None;
        p.cold = false;
        p.routed = false;
        p.carry = Carry::None;
        p.act = Act::Idle;
    }
    let mut ai = sim::ai::Ai::new(0, sim::ai::AiLevel::Normal);
    ai_for(&mut g, &mut ai, 3);
    let door = g.igloos[ig as usize].door;
    let sent = workers
        .iter()
        .filter(|&&w| matches!(g.penguins[w as usize].order, Order::Move { target, .. } if target.dist(door) < 0.01))
        .count();
    assert_eq!(sent, workers.len().min(3), "the AI sent up to three idle workers");
    ai_for(&mut g, &mut ai, 117);
    let hp = g.igloos[ig as usize].hp;
    assert!(hp > start, "the AI's workers patched it up");
    assert!(hp <= max, "hp {hp} went above the igloo's max {max}");
}

/// A spot outside an igloo's door, `d` tiles out.
fn outside(g: &Game, ig: u8, d: f32) -> Vec2 {
    let door = g.igloos[ig as usize].door;
    let c = g.igloos[ig as usize].center;
    let at = door + (door - c).norm() * d;
    let (x, y) = g.grid.nearest_walkable(at.tile(), 4).unwrap();
    v2(x as f32 + 0.5, y as f32 + 0.5)
}

#[test]
fn soldiers_pelt_enemy_igloos_until_they_fall_in() {
    let mut g = game(all());
    let theirs = home(&g, 1);
    // Empty their igloo of defenders and park them far away.
    let away = open_ground(&g, 8, 4);
    for p in &mut g.penguins {
        if p.owner == 1 {
            p.pos = away;
            p.snowballs = 0;
            p.inside = None;
        }
    }
    g.igloos[theirs as usize].occupants.clear();
    let spot = outside(&g, theirs, 4.0);
    let raiders: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Snowballer).take(6).map(|p| p.id).collect();
    let start = g.igloos[theirs as usize].hp;
    assert_eq!(start, igloo_max_hp(0));
    let mut ev = Vec::new();
    for _ in 0..180 * sim::TICK_RATE {
        for &r in &raiders {
            let p = &mut g.penguins[r as usize];
            p.pos = spot;
            p.snowballs = p.snowballs.max(p.kind.stats().ammo);
            p.warmth = p.max_warmth;
            p.cold = false;
            p.inside = None;
            p.order = Order::Hold { pos: spot };
        }
        for p in &mut g.penguins {
            if p.owner == 1 {
                p.snowballs = 0;
                p.pos = away;
            }
        }
        g.tick();
        ev.extend(std::mem::take(&mut g.events));
        if g.igloos[theirs as usize].owner == NEUTRAL {
            break;
        }
    }
    assert!(ev.iter().any(|e| matches!(e, Event::Collapsed { igloo, owner: 1 } if *igloo == theirs)), "hp left {}", g.igloos[theirs as usize].hp);
    assert_eq!(g.igloos[theirs as usize].owner, NEUTRAL, "a ruin now");
    // Player 1 had nothing else: knocked out.
    run(&mut g, 20);
    assert!(g.players[1].out);
    assert_eq!(g.winner, Some(0));
}

#[test]
fn penguins_move_out_of_a_collapsed_igloo() {
    let mut g = game(all());
    let theirs = home(&g, 1);
    // Player 1 gets a second igloo: the homeless have somewhere to move.
    let spare = g.igloos.iter().position(|i| i.owner == NEUTRAL).unwrap() as u8;
    g.igloos[spare as usize].owner = 1;
    // Empty their igloo of defenders and park them far away.
    let away = open_ground(&g, 8, 4);
    for p in &mut g.penguins {
        if p.owner == 1 {
            p.pos = away;
            p.snowballs = 0;
            p.inside = None;
        }
    }
    g.igloos[theirs as usize].occupants.clear();
    let spot = outside(&g, theirs, 4.0);
    let raiders: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Snowballer).take(6).map(|p| p.id).collect();
    // One snowball from now.
    g.igloos[theirs as usize].hp = 1;
    let displaced: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 1 && p.home == theirs).map(|p| p.id).collect();
    assert!(!displaced.is_empty());
    let mut ev = Vec::new();
    for _ in 0..180 * sim::TICK_RATE {
        for &r in &raiders {
            let p = &mut g.penguins[r as usize];
            p.pos = spot;
            p.snowballs = p.snowballs.max(p.kind.stats().ammo);
            p.warmth = p.max_warmth;
            p.cold = false;
            p.inside = None;
            p.order = Order::Hold { pos: spot };
        }
        for p in &mut g.penguins {
            if p.owner == 1 {
                p.snowballs = 0;
                p.pos = away;
            }
        }
        g.tick();
        ev.extend(std::mem::take(&mut g.events));
        if g.igloos[theirs as usize].owner == NEUTRAL {
            break;
        }
    }
    assert!(ev.iter().any(|e| matches!(e, Event::Collapsed { igloo, owner: 1 } if *igloo == theirs)), "hp left {}", g.igloos[theirs as usize].hp);
    assert_eq!(g.igloos[theirs as usize].owner, NEUTRAL, "a ruin now");
    // Everyone who lived there now lives at the second igloo.
    for &id in &displaced {
        assert_eq!(g.penguins[id as usize].home, spare);
    }
    assert!(!g.players[1].out, "the tribe still has the second igloo");
}

#[test]
fn a_ruin_keeps_nothing() {
    let mut g = game(all());
    let theirs = home(&g, 1);
    g.igloos[theirs as usize].fish = 30;
    g.igloos[theirs as usize].wood = 10;
    // Empty their igloo of defenders and park them far away.
    let away = open_ground(&g, 8, 4);
    for p in &mut g.penguins {
        if p.owner == 1 {
            p.pos = away;
            p.snowballs = 0;
            p.inside = None;
        }
    }
    g.igloos[theirs as usize].occupants.clear();
    let spot = outside(&g, theirs, 4.0);
    let raiders: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Snowballer).take(6).map(|p| p.id).collect();
    for _ in 0..180 * sim::TICK_RATE {
        for &r in &raiders {
            let p = &mut g.penguins[r as usize];
            p.pos = spot;
            p.snowballs = p.snowballs.max(p.kind.stats().ammo);
            p.warmth = p.max_warmth;
            p.cold = false;
            p.inside = None;
            p.order = Order::Hold { pos: spot };
        }
        for p in &mut g.penguins {
            if p.owner == 1 {
                p.snowballs = 0;
                p.pos = away;
                p.order = Order::Hold { pos: away };
            }
        }
        g.tick();
        g.events.clear();
        if g.igloos[theirs as usize].owner == NEUTRAL {
            break;
        }
    }
    let ruin = &g.igloos[theirs as usize];
    assert_eq!(ruin.owner, NEUTRAL, "a ruin now");
    assert_eq!(ruin.fish, 0, "its stores are lost");
    assert_eq!(ruin.wood, 0);
    assert!(ruin.occupants.is_empty(), "nobody left inside");
    assert_eq!(ruin.hp, 0);
}

#[test]
fn idle_workers_patch_up_their_igloo() {
    let mut g = game(CoreRules::default().map_start());
    let ig = home(&g, 0);
    let max = igloo_max_hp(g.igloos[ig as usize].tier);
    g.igloos[ig as usize].hp = max / 2;
    let start = g.igloos[ig as usize].hp;
    // Next to a wall, not inside it.
    let spot = outside(&g, ig, 1.0);
    let workers: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Worker).map(|p| p.id).take(2).collect();
    assert_eq!(workers.len(), 2);
    for _ in 0..60 * sim::TICK_RATE {
        for &w in &workers {
            let p = &mut g.penguins[w as usize];
            p.pos = spot;
            p.inside = None;
            p.cold = false;
            p.routed = false;
            p.act = Act::Idle;
            p.order = Order::Hold { pos: spot };
        }
        for p in &mut g.penguins {
            p.snowballs = 0;
        }
        g.tick();
        let hp = g.igloos[ig as usize].hp;
        assert!(hp <= max, "hp {hp} went above the igloo's max {max}");
        g.events.clear();
    }
    assert!(g.igloos[ig as usize].hp > start, "the workers patched it up");
}

#[test]
fn a_ruined_igloo_is_not_patched_up() {
    let mut g = game(CoreRules::default().map_start());
    let theirs = home(&g, 1);
    g.igloos[theirs as usize].hp = 0;
    g.igloos[theirs as usize].owner = NEUTRAL;
    let spot = outside(&g, theirs, 1.0);
    let workers: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Worker).map(|p| p.id).take(2).collect();
    for _ in 0..10 * sim::TICK_RATE {
        for &w in &workers {
            let p = &mut g.penguins[w as usize];
            p.pos = spot;
            p.inside = None;
            p.cold = false;
            p.routed = false;
            p.act = Act::Idle;
            p.order = Order::Hold { pos: spot };
        }
        for p in &mut g.penguins {
            p.snowballs = 0;
        }
        g.tick();
        g.events.clear();
    }
    assert_eq!(g.igloos[theirs as usize].hp, 0, "ruins stay ruins");
}

#[test]
fn soldiers_leave_their_own_igloos_alone() {
    let mut g = game(all());
    let mine = home(&g, 0);
    let tier = g.igloos[mine as usize].tier;
    // Move player 1's penguins far away so no enemy is around.
    let away = open_ground(&g, 8, 4);
    for p in &mut g.penguins {
        if p.owner == 1 {
            p.pos = away;
            p.snowballs = 0;
            p.inside = None;
        }
    }
    g.igloos[mine as usize].occupants.clear();
    let spot = outside(&g, mine, 4.0);
    // (A tribe starts with few soldiers: bring the squad in.)
    for _ in 0..6 {
        g.spawn_for_test(mine, Kind::Snowballer);
    }
    let squad: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Snowballer).take(6).map(|p| p.id).collect();
    assert_eq!(squad.len(), 6);
    for _ in 0..60 * sim::TICK_RATE {
        for &s in &squad {
            let p = &mut g.penguins[s as usize];
            p.pos = spot;
            p.snowballs = p.snowballs.max(p.kind.stats().ammo);
            p.warmth = p.max_warmth;
            p.cold = false;
            p.inside = None;
            p.order = Order::Hold { pos: spot };
        }
        g.tick();
        g.events.clear();
    }
    let igloo = &g.igloos[mine as usize];
    assert_eq!(igloo.hp, igloo_max_hp(tier), "their own igloo never takes a hit");
    assert_eq!(igloo.owner, 0);
}

/// Three `kind` soldiers pelt the enemy igloo for 30 s; the hp they take off.
fn igloo_dmg(kind: Kind, seed: u64) -> u16 {
    let mut g = Game::new_with(map("fixtures/trio"), setups(), seed, all());
    let theirs = home(&g, 1);
    // Empty their igloo of defenders and park them far away.
    let away = open_ground(&g, 8, 4);
    for p in &mut g.penguins {
        if p.owner == 1 {
            p.pos = away;
            p.snowballs = 0;
            p.inside = None;
        }
    }
    g.igloos[theirs as usize].occupants.clear();
    // Close enough that even a Packer's 5-tile range reaches the igloo's center.
    let spot = outside(&g, theirs, 2.0);
    let mine = home(&g, 0);
    for _ in 0..3 {
        g.spawn_for_test(mine, Kind::Snowballer);
    }
    let raiders: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Snowballer).take(3).map(|p| p.id).collect();
    assert_eq!(raiders.len(), 3);
    for &r in &raiders {
        let p = &mut g.penguins[r as usize];
        p.kind = kind;
        p.snowballs = kind.stats().ammo;
        p.order = Order::None;
    }
    let start = g.igloos[theirs as usize].hp;
    for _ in 0..30 * sim::TICK_RATE {
        for &r in &raiders {
            let p = &mut g.penguins[r as usize];
            p.pos = spot;
            p.snowballs = p.snowballs.max(kind.stats().ammo);
            p.warmth = p.max_warmth;
            p.cold = false;
            p.inside = None;
            p.order = Order::Hold { pos: spot };
        }
        for p in &mut g.penguins {
            if p.owner == 1 {
                p.snowballs = 0;
                p.pos = away;
            }
        }
        g.tick();
        g.events.clear();
    }
    start - g.igloos[theirs as usize].hp
}

#[test]
fn packers_smash_igloos_faster() {
    // Snowballs fly with some scatter, so add up a few games.
    let total = |k| (7..12).map(|seed| igloo_dmg(k, seed) as u32).sum::<u32>();
    let (snow, pack) = (total(Kind::Snowballer), total(Kind::Packer));
    assert!(pack > snow, "packers took {pack} hp off, snowballers only {snow}");
}

#[test]
fn without_siege_igloos_shrug_off_snowballs() {
    let mut g = game(CoreRules::default().map_start());
    let theirs = home(&g, 1);
    let hp = g.igloos[theirs as usize].hp;
    let spot = outside(&g, theirs, 4.0);
    let raider = someone(&g, 0, Kind::Snowballer, &[]);
    for _ in 0..30 * sim::TICK_RATE {
        let p = &mut g.penguins[raider as usize];
        p.pos = spot;
        p.order = Order::Hold { pos: spot };
        g.tick();
        g.events.clear();
    }
    assert_eq!(g.igloos[theirs as usize].hp, hp);
}

#[test]
fn an_upgraded_igloo_is_sturdier() {
    // A Snow Hall has more hit points than a plain igloo, and the finished
    // work tops the igloo up to the new maximum.
    let mut g = game(CoreRules { tech: true, ..all() });
    let ig = home(&g, 0);
    let plain = igloo_max_hp(TIER_IGLOO);
    let hall = igloo_max_hp(TIER_SNOW_HALL);
    assert!(hall > plain, "the Snow Hall should be sturdier");
    assert_eq!(g.igloos[ig as usize].hp, plain);
    let next = next_tier(TIER_IGLOO).unwrap();
    g.igloos[ig as usize].fish = 100;
    g.igloos[ig as usize].wood = 100;
    g.command(0, Command::Upgrade { igloo: ig });
    let ev = run(&mut g, next.ticks as u32);
    assert!(ev.iter().any(|e| matches!(e, Event::Upgraded { igloo, tier } if *igloo == ig && *tier == TIER_SNOW_HALL)), "upgrade reported");
    assert_eq!(g.igloos[ig as usize].tier, TIER_SNOW_HALL);
    assert_eq!(g.igloos[ig as usize].hp, hall, "the Snow Hall stands up to more");
}

#[test]
fn clients_see_how_battered_an_igloo_is() {
    let mut g = game(all());
    let theirs = home(&g, 1);
    g.igloos[theirs as usize].hp = 7;
    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    view.apply(&enc.encode(&g, &[Event::Collapsed { igloo: theirs, owner: 1 }])).unwrap();
    assert_eq!(view.igloos[theirs as usize].hp, 7);
}

#[test]
fn a_rival_cannot_restart_a_founding_and_keep_the_fish() {
    let mut g = game(CoreRules::default().map_start());
    let (home0, home1) = (home(&g, 0), home(&g, 1));
    let site = g.igloos.iter().find(|ig| ig.owner == NEUTRAL).unwrap().id;
    for h in [home0, home1] {
        // (With armies the wood share is paid in fish.)
        g.igloos[h as usize].fish = GREAT_IGLOO_FISH + GREAT_IGLOO_WOOD;
    }
    let first = someone(&g, 0, Kind::Worker, &[]);
    g.penguins[first as usize].pos = g.igloos[site as usize].door;
    g.command(0, Command::Enter { units: vec![first], igloo: site });
    run(&mut g, 2 * sim::TICK_RATE);
    let started = g.igloos[site as usize].takeover.expect("player 0 is founding it");
    assert_eq!(started.by, 0);
    assert!(g.igloos[home0 as usize].fish < GREAT_IGLOO_FISH, "player 0 paid");

    let rival = someone(&g, 1, Kind::Worker, &[]);
    g.penguins[rival as usize].pos = g.igloos[site as usize].door;
    g.command(1, Command::Enter { units: vec![rival], igloo: site });
    run(&mut g, sim::TICK_RATE / 2);
    let now = g.igloos[site as usize].takeover.expect("still being founded");
    assert_eq!(now.by, 0, "the rival did not take over the founding");
    assert!(now.ticks_left <= started.ticks_left, "and did not restart its clock");
    assert!(g.igloos[home1 as usize].fish >= GREAT_IGLOO_FISH, "and was not charged for trying");
}
