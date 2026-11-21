//! Heroes: the Chieftain. Every igloo raises one, with an
//! aura that steadies the tribe, a War Cry, and a heavy blow to morale when
//! it freezes. What the client sees is checked against the game.

use sim::game::*;
use sim::map::Map;
use sim::math::{v2, Vec2};
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};
use sim::TICK_RATE;

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

fn game() -> Game {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let mut g = Game::new_with(map, setups, 5, CoreRules::default().map_start());
    g.weather.calm = true;
    soldiers(&mut g, 8);
    // Nobody goes fishing: workers at a pond would wander into the scenes.
    g.shores.clear();
    g.weather.calm = true;
    g
}

fn home(g: &Game, owner: u8) -> u8 {
    g.igloos.iter().position(|i| i.owner == owner).unwrap() as u8
}

fn first(g: &Game, owner: u8, kind: Kind) -> u16 {
    g.penguins.iter().find(|p| p.owner == owner && p.kind == kind).unwrap().id
}

fn chiefs(g: &Game, owner: u8) -> Vec<u16> {
    g.penguins.iter().filter(|p| p.owner == owner && p.kind == Kind::Chieftain).map(|p| p.id).collect()
}

/// Everyone but `keep` puts down their snowballs, so nothing interferes.
fn calm(g: &mut Game, keep: &[u16]) {
    for p in &mut g.penguins {
        if !keep.contains(&p.id) {
            p.snowballs = 0;
        }
    }
}

/// Tick `n` times keeping the rest calm, and collect the events.
fn run(g: &mut Game, n: u32, keep: &[u16]) -> Vec<Event> {
    let mut all = Vec::new();
    for _ in 0..n {
        calm(g, keep);
        g.tick();
        all.extend(std::mem::take(&mut g.events));
    }
    all
}

/// Make the player's home a Snow Hall with plenty in the larder.
fn stock(g: &mut Game, player: u8) -> u8 {
    let ig = home(g, player);
    let igloo = &mut g.igloos[ig as usize];
    igloo.tier = TIER_SNOW_HALL;
    igloo.fish = 500;
    igloo.wood = 500;
    ig
}

/// The player's Chieftain, with plenty in the larder.
fn hatch_chief(g: &mut Game, player: u8) -> u16 {
    stock(g, player);
    *chiefs(g, player).first().expect("every tribe starts with its Chieftain")
}

/// A walkable spot far from either igloo, where the test can stage a scene.
fn arena(g: &Game) -> Vec2 {
    let door = g.igloos[home(g, 0) as usize].door;
    // Open, level snow all around, so no pond, tree, slope or ice (no snow to
    // throw) gets in the way of the scene.
    for r in [14.0f32, 10.0, 18.0, 22.0, 26.0] {
        for k in 0..16 {
            let at = door + Vec2::from_angle(k as f32 / 16.0 * std::f32::consts::TAU) * r;
            let (x, y) = at.tile();
            let flat = |dx: i32, dy: i32| (g.map.height_at(v2((x + dx) as f32 + 0.5, (y + dy) as f32 + 0.5)) - g.map.height_at(at)).abs() < 0.2;
            if (-6..=6).all(|dy| (-6..=6).all(|dx| g.grid.walkable(x + dx, y + dy) && flat(dx, dy) && g.map.ground[g.map.idx(x + dx, y + dy)] == sim::map::Ground::Snow)) {
                return v2(x as f32 + 0.5, y as f32 + 0.5);
            }
        }
    }
    panic!("no open ground for the scene");
}

fn place(g: &mut Game, id: u16, at: Vec2) {
    let p = &mut g.penguins[id as usize];
    p.pos = at;
    p.inside = None;
}

/// Pull everyone not in `keep` well away so they can't see or touch the scene.
fn clear_out(g: &mut Game, keep: &[u16]) {
    for p in &mut g.penguins {
        if !keep.contains(&p.id) {
            p.snowballs = 0;
        }
    }
}

fn freeze(g: &mut Game, v: u16) {
    let p = &mut g.penguins[v as usize];
    p.warmth = 0;
    p.cold = true;
    p.act = Act::Frozen;
    p.act_t = 0;
}

// -------------------------------------------------------------------------
// Hatching

#[test]
fn a_chieftain_is_tougher_and_throws_harder() {
    let c = Kind::Chieftain.stats();
    let s = Kind::Snowballer.stats();
    assert!(c.warmth > s.warmth * 2.0, "tougher");
    assert!(c.direct > s.direct && c.splash >= s.splash, "throws harder");
    assert!(c.scale > 1.3, "drawn big");
    let mut g = game();
    let id = hatch_chief(&mut g, 0);
    let snowballer = first(&g, 0, Kind::Snowballer);
    assert!(g.penguins[id as usize].max_warmth > g.penguins[snowballer as usize].max_warmth * 2);
}

// -------------------------------------------------------------------------
// The aura

#[test]
fn the_aura_speeds_courage_recovery() {
    let mut g = game();
    let chief = hatch_chief(&mut g, 0);
    let at = arena(&g);
    let near = first(&g, 0, Kind::Snowballer);
    let far_one = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Snowballer).nth(1).unwrap().id;
    place(&mut g, chief, at);
    place(&mut g, near, at + v2(2.0, 0.0));
    // Out of reach of the aura, but just as far from home.
    place(&mut g, far_one, at + v2(0.0, CHIEF_AURA_RADIUS + 3.0));
    for id in [near, far_one] {
        g.penguins[id as usize].courage = 1;
    }
    // Hold them where they are; the test is about courage, not walking.
    let spots: Vec<(u16, Vec2)> = [chief, near, far_one].iter().map(|&id| (id, g.penguins[id as usize].pos)).collect();
    for _ in 0..3 * TICK_RATE {
        for &(id, pos) in &spots {
            place(&mut g, id, pos);
        }
        calm(&mut g, &[]);
        g.tick();
        g.events.clear();
    }
    let with = g.penguins[near as usize].courage;
    let without = g.penguins[far_one as usize].courage;
    assert!(with >= 3, "near the Chieftain courage came back quickly (got {with})");
    assert!(with > without, "faster than without it ({with} vs {without})");
}

/// A friend freezes beside an observer worker (with or without a Chieftain
/// at the observer's side); returns the observer's courage afterwards.
fn courage_after_a_friend_freezes(with_chief: bool) -> u8 {
    let mut g = game();
    let chief = hatch_chief(&mut g, 0);
    let at = arena(&g);
    let w = first(&g, 0, Kind::Worker);
    let victim = first(&g, 0, Kind::Snowballer);
    let enemy = first(&g, 1, Kind::Snowballer);
    place(&mut g, w, at);
    place(&mut g, victim, at + v2(3.5, 0.0));
    place(&mut g, enemy, at + v2(3.5, 3.5));
    // The Chieftain stands behind the observer, out of the enemy's range.
    place(&mut g, chief, if with_chief { at + v2(-4.5, 0.0) } else { at + v2(-40.0, 0.0) });
    g.penguins[victim as usize].warmth = 1;
    g.penguins[enemy as usize].snowballs = MAX_SNOWBALLS;
    let spots: Vec<(u16, Vec2)> = [w, victim, enemy, chief].iter().map(|&id| (id, g.penguins[id as usize].pos)).collect();
    for _ in 0..10 * TICK_RATE {
        clear_out(&mut g, &[enemy]);
        g.penguins[chief as usize].snowballs = 0;
        g.penguins[enemy as usize].snowballs = MAX_SNOWBALLS;
        // Nobody wanders off.
        for &(id, keep) in &spots {
            if g.penguins[id as usize].act != Act::Frozen {
                place(&mut g, id, keep);
            }
        }
        g.tick();
        g.events.clear();
        if g.penguins[victim as usize].act == Act::Frozen {
            return g.penguins[w as usize].courage;
        }
    }
    panic!("the friend never froze");
}

#[test]
fn the_aura_softens_the_shock_of_a_freezing() {
    let bare = courage_after_a_friend_freezes(false);
    let steadied = courage_after_a_friend_freezes(true);
    assert!(bare < MAX_COURAGE, "the shock cost courage");
    assert!(steadied > bare, "a Chieftain nearby: lost less ({steadied} vs {bare})");
}

// -------------------------------------------------------------------------
// War Cry

/// A Chieftain with three friends close by and an enemy snowballer far off.
fn war_band() -> (Game, u16, Vec<u16>) {
    let mut g = game();
    let chief = hatch_chief(&mut g, 0);
    let at = arena(&g);
    place(&mut g, chief, at);
    let friends: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Snowballer).take(3).map(|p| p.id).collect();
    for (k, &f) in friends.iter().enumerate() {
        place(&mut g, f, at + v2(1.0 + k as f32 * 0.8, 1.0));
    }
    (g, chief, friends)
}

#[test]
fn war_cry_buffs_friends_and_rallies_the_fleeing() {
    let (mut g, chief, friends) = war_band();
    let at = g.penguins[chief as usize].pos;
    let runner = friends[0];
    g.penguins[runner as usize].courage = 0;
    g.penguins[runner as usize].routed = true;
    let stranger = first(&g, 0, Kind::Worker);
    place(&mut g, stranger, at + v2(WAR_CRY_RADIUS + 3.0, 0.0));
    let enemy = first(&g, 1, Kind::Snowballer);
    place(&mut g, enemy, at + v2(2.0, -1.0));

    g.command(0, Command::WarCry { unit: chief });
    let ev: Vec<_> = g.events.drain(..).collect();
    assert!(ev.iter().any(|e| matches!(e, Event::WarCry { penguin, .. } if *penguin == chief)), "the shout was reported");
    for &f in &friends {
        assert_eq!(g.penguins[f as usize].cry, WAR_CRY_TICKS, "friend {f} inspired");
    }
    assert!(g.penguins[chief as usize].cry > 0, "the Chieftain too");
    assert!(!g.penguins[runner as usize].routed && g.penguins[runner as usize].courage >= RALLY_COURAGE, "the runner turned back");
    assert_eq!(g.penguins[stranger as usize].cry, 0, "too far away to hear");
    assert_eq!(g.penguins[enemy as usize].cry, 0, "the enemy isn't helped");

    // It wears off.
    run(&mut g, WAR_CRY_TICKS as u32 + 1, &[]);
    assert_eq!(g.penguins[friends[1] as usize].cry, 0);
}

#[test]
fn war_cry_has_a_cooldown() {
    let (mut g, chief, friends) = war_band();
    g.command(0, Command::WarCry { unit: chief });
    assert_eq!(g.events.drain(..).filter(|e| matches!(e, Event::WarCry { .. })).count(), 1);
    assert_eq!(g.penguins[chief as usize].cry_cd, WAR_CRY_COOLDOWN);
    assert_eq!(WAR_CRY_COOLDOWN as u32, 45 * TICK_RATE, "about 45 seconds");

    // Right away, and just before it is ready: nothing.
    run(&mut g, WAR_CRY_COOLDOWN as u32 - 2, &[]);
    g.command(0, Command::WarCry { unit: chief });
    assert!(!g.events.iter().any(|e| matches!(e, Event::WarCry { .. })), "still cooling down");
    assert_eq!(g.penguins[friends[2] as usize].cry, 0);

    run(&mut g, 2, &[]);
    assert_eq!(g.penguins[chief as usize].cry_cd, 0);
    g.command(0, Command::WarCry { unit: chief });
    assert!(g.events.iter().any(|e| matches!(e, Event::WarCry { .. })), "ready again");
}

#[test]
fn war_cry_is_for_chieftains_and_their_owner() {
    let (mut g, chief, friends) = war_band();
    g.command(0, Command::WarCry { unit: friends[0] });
    assert!(g.events.is_empty(), "snowballers can't shout");
    g.command(1, Command::WarCry { unit: chief });
    assert!(g.events.is_empty(), "not your Chieftain");
    freeze(&mut g, chief);
    g.command(0, Command::WarCry { unit: chief });
    assert!(g.events.is_empty(), "a frozen Chieftain can't shout");
}

#[test]
fn war_cry_makes_friends_run_faster() {
    let (mut g, chief, friends) = war_band();
    let at = g.penguins[chief as usize].pos;
    let (shouted, plain) = (friends[0], friends[1]);
    // Same start, same errand, but one has heard the shout.
    place(&mut g, shouted, at + v2(0.0, 2.0));
    place(&mut g, plain, at + v2(0.0, 2.0));
    g.penguins[shouted as usize].cry = WAR_CRY_TICKS;
    let target = at + v2(0.0, 12.0);
    let target = v2(target.x, target.y.min(g.map.h as f32 - 2.0));
    for id in [shouted, plain] {
        g.command(0, Command::Move { units: vec![id], target, run: true, queue: false });
    }
    let start = [g.penguins[shouted as usize].pos, g.penguins[plain as usize].pos];
    run(&mut g, TICK_RATE, &[]);
    let fast = g.penguins[shouted as usize].pos.dist(start[0]);
    let slow = g.penguins[plain as usize].pos.dist(start[1]);
    assert!(fast > slow * 1.15, "the shouted-at friend moved farther ({fast} vs {slow})");
}

#[test]
fn war_cry_makes_friends_throw_faster() {
    let (mut g, chief, friends) = war_band();
    let at = g.penguins[chief as usize].pos;
    // Two snowballers each with a target in range that never goes down.
    let (shouted, plain) = (friends[0], friends[1]);
    let dummies: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 1 && p.kind == Kind::Worker).take(2).map(|p| p.id).collect();
    place(&mut g, shouted, at + v2(-3.0, 0.0));
    place(&mut g, plain, at + v2(3.0, 0.0));
    place(&mut g, dummies[0], at + v2(-3.0, 4.0));
    place(&mut g, dummies[1], at + v2(3.0, 4.0));
    // Both stand their ground, or they would wander off and stop throwing.
    for id in [shouted, plain] {
        let here = g.penguins[id as usize].pos;
        g.command(0, Command::Move { units: vec![id], target: here, run: false, queue: false });
    }
    let mut thrown = [0, 0];
    for _ in 0..8 * TICK_RATE {
        clear_out(&mut g, &[shouted, plain]);
        for &id in &[shouted, plain] {
            let p = &mut g.penguins[id as usize];
            p.snowballs = p.kind.stats().ammo;
            p.cry = if id == shouted { WAR_CRY_TICKS } else { 0 };
        }
        for (k, &d) in dummies.iter().enumerate() {
            // Keep the targets where they are, warm and standing.
            let p = &mut g.penguins[d as usize];
            p.warmth = p.max_warmth;
            p.cold = false;
            p.act = Act::Idle;
            p.pos = at + v2(if k == 0 { -3.0 } else { 3.0 }, 4.0);
        }
        g.tick();
        for e in g.events.drain(..) {
            if let Event::Thrown(sb) = e {
                if sb.thrower == shouted {
                    thrown[0] += 1;
                } else if sb.thrower == plain {
                    thrown[1] += 1;
                }
            }
        }
    }
    assert!(thrown[1] > 3, "the plain thrower threw ({})", thrown[1]);
    assert!(thrown[0] > thrown[1], "the shouted-at friend threw more ({} vs {})", thrown[0], thrown[1]);
}

// -------------------------------------------------------------------------
// Losing the Chieftain

#[test]
fn the_tribe_loses_heart_when_the_chieftain_freezes() {
    let mut g = game();
    let chief = hatch_chief(&mut g, 0);
    let at = arena(&g);
    let enemy = first(&g, 1, Kind::Snowballer);
    let snowballers: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Snowballer).map(|p| p.id).collect();
    let (nearby, distant) = (snowballers[0], snowballers[1]);
    let foe = first(&g, 1, Kind::Worker);
    place(&mut g, chief, at);
    // Beyond the shock radius, but within the fall radius, behind the Chieftain.
    place(&mut g, nearby, at + v2(-8.0, 0.0));
    place(&mut g, distant, at + v2(-(CHIEF_FALL_RADIUS + 4.0), 0.0));
    place(&mut g, enemy, at + v2(3.0, 0.0));
    place(&mut g, foe, at + v2(3.0, -9.0));
    g.penguins[foe as usize].courage = 5;
    g.penguins[chief as usize].warmth = 1;
    // Hold the friends in place (they would otherwise wander).
    let mut frozen = false;
    for _ in 0..10 * TICK_RATE {
        clear_out(&mut g, &[enemy]);
        g.penguins[enemy as usize].snowballs = MAX_SNOWBALLS;
        for (id, pos) in [(nearby, at + v2(-8.0, 0.0)), (distant, at + v2(-(CHIEF_FALL_RADIUS + 4.0), 0.0)), (foe, at + v2(3.0, -9.0)), (enemy, at + v2(3.0, 0.0))] {
            place(&mut g, id, pos);
        }
        g.tick();
        frozen |= g.events.drain(..).any(|e| matches!(e, Event::Frozen { penguin } if penguin == chief));
        if frozen {
            break;
        }
    }
    assert!(frozen, "the Chieftain froze");
    assert!(g.penguins[nearby as usize].courage <= MAX_COURAGE - CHIEF_FALL_LOSS, "the tribe nearby lost heart (courage {})", g.penguins[nearby as usize].courage);
    assert_eq!(g.penguins[distant as usize].courage, MAX_COURAGE, "those far away didn't see");
    assert!(g.penguins[foe as usize].courage > 5, "the enemy took heart");
}

// -------------------------------------------------------------------------
// The computer players

/// A tribe with its Chieftain, workers and six fighters at home.
fn ai_tribe() -> (Game, u8) {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let mut g = Game::new_with(map, setups, 5, CoreRules::default().map_start());
    g.weather.calm = true;
    let ig = home(&g, 0);
    // Turn some workers into fighters by hand, so the test starts from a known tribe.
    let mut turned = 0;
    for p in g.penguins.iter_mut().filter(|p| p.home == ig && p.kind == Kind::Worker) {
        if turned < 6 {
            p.kind = Kind::Snowballer;
            p.snowballs = 3;
            turned += 1;
        }
    }
    // Let the game get going, so the computer has had a look round.
    for _ in 0..20 * TICK_RATE {
        g.tick();
        g.events.clear();
    }
    (g, ig)
}

/// Let the computer player think, and hand back the commands it made.
fn ai_thinks(g: &Game, ai: &mut sim::ai::Ai) -> Vec<Command> {
    let mut cmds = Vec::new();
    ai.update(g, &mut cmds);
    cmds.into_iter().map(|(_, c)| c).collect()
}

#[test]
fn the_ai_war_cries_when_its_army_is_engaged() {
    let (mut g, _) = ai_tribe();
    let chief = hatch_chief(&mut g, 0);
    let at = arena(&g);
    // Three friends and two enemies, all in earshot.
    let friends: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind.fights() && p.id != chief).take(3).map(|p| p.id).collect();
    assert_eq!(friends.len(), 3, "the tribe has an army");
    let foes: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 1 && p.kind.fights()).take(2).map(|p| p.id).collect();
    assert_eq!(foes.len(), 2, "and something to fight");
    let mut ai = sim::ai::Ai::new(0, sim::ai::AiLevel::Normal);
    let mut shouted = false;
    for _ in 0..10 * TICK_RATE {
        place(&mut g, chief, at);
        for (k, &f) in friends.iter().enumerate() {
            place(&mut g, f, at + v2(1.0 + k as f32 * 0.5, 1.0));
        }
        for (k, &f) in foes.iter().enumerate() {
            place(&mut g, f, at + v2(1.0 + k as f32 * 0.5, -1.0));
            g.penguins[f as usize].cold = false;
        }
        for cmd in ai_thinks(&g, &mut ai) {
            if matches!(cmd, Command::WarCry { .. }) {
                shouted = true;
            }
            g.command(0, cmd);
        }
        g.tick();
        g.events.clear();
        if shouted {
            break;
        }
    }
    assert!(shouted, "the AI told its tribe to fight on");
}

// -------------------------------------------------------------------------
// The client's view

#[test]
fn the_client_sees_the_chieftain_and_its_war_cry() {
    let (mut g, chief, friends) = war_band();
    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));

    g.command(0, Command::WarCry { unit: chief });
    let ev: Vec<Event> = g.events.drain(..).collect();
    let got = view.apply(&enc.encode(&g, &ev)).unwrap();
    assert!(got.iter().any(|e| matches!(e, Event::WarCry { penguin, pos } if *penguin == chief && pos.dist(g.penguins[chief as usize].pos) < 1e-3)), "the shout reached the client");

    let c = &view.penguins[chief as usize];
    assert_eq!(c.kind, Kind::Chieftain);
    assert!(c.shouting, "it is shouting");
    assert_eq!(c.cry_cd as u32, (WAR_CRY_COOLDOWN as u32).div_ceil(TICK_RATE));
    assert!(view.penguins[friends[0] as usize].shouting);

    // The flags and the countdown track the game as it wears off.
    for _ in 0..(WAR_CRY_TICKS as u32 + 2 * TICK_RATE) {
        calm(&mut g, &[]);
        g.tick();
        let ev: Vec<Event> = g.events.drain(..).collect();
        view.apply(&enc.encode(&g, &ev)).unwrap();
    }
    for p in &g.penguins {
        let v = &view.penguins[p.id as usize];
        assert_eq!(v.shouting, p.cry > 0, "penguin {} shouting", p.id);
        assert_eq!(v.cry_cd as u32, (p.cry_cd as u32).div_ceil(TICK_RATE), "penguin {} cooldown", p.id);
        assert_eq!(v.kind, p.kind);
        assert_eq!(v.courage, p.courage);
    }
    assert!(!view.penguins[chief as usize].shouting);
}
