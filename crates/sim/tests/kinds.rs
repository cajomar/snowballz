//! The unit kinds behave as their stats table promises.

use sim::game::*;
use sim::map::{Ground, Map};
use sim::math::{v2, Vec2};

/// How much height difference still counts as level ground.
const LEVEL: f32 = 0.05;

fn map() -> Map {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    Map::load_file(std::path::Path::new(&path)).unwrap()
}

fn game() -> Game {
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    Game::new_with(map(), setups, 7, CoreRules::default().map_start())
}

/// Igloos train nobody (camps do), so turn one of the igloo's workers into
/// `kind` by hand.
fn hatch(g: &mut Game, ig: usize, kind: Kind) {
    let Some(w) = g.penguins.iter().find(|p| p.owner == 0 && p.home == ig as u8 && p.kind == Kind::Worker).map(|p| p.id) else { return };
    let max = g.max_warmth_of(kind);
    let p = &mut g.penguins[w as usize];
    p.kind = kind;
    p.max_warmth = max;
    p.warmth = max;
    p.snowballs = kind.stats().ammo;
}

fn home(g: &Game, owner: u8) -> usize {
    g.igloos.iter().position(|i| i.owner == owner).unwrap()
}

/// Open ground where `from` and a throw away stand level (height could
/// otherwise stretch the ranges and hide the kinds' own difference).
fn level_pair(g: &Game) -> (Vec2, Vec2) {
    for y in 3..g.map.h - 5 {
        for x in 3..g.map.w - 5 {
            let from = v2(x as f32 + 0.5, y as f32 + 0.5);
            let to = from + v2(7.0, 0.0);
            if !g.grid.walkable(x, y) || !g.grid.walkable(x + 7, y) {
                continue;
            }
            if (g.map.height_at(from) - g.map.height_at(to)).abs() > LEVEL {
                continue;
            }
            if g.igloos.iter().all(|ig| ig.door.dist(from) > 12.0) {
                return (from, to);
            }
        }
    }
    panic!("no level open ground");
}

/// Open ground with a level 20-tile march, clear of every igloo door, so
/// neither slope nor a fight changes the kinds' own pace.
fn march_pair(g: &Game) -> (Vec2, Vec2) {
    for y in 3..g.map.h - 5 {
        for x in 3..g.map.w - 24 {
            let from = v2(x as f32 + 0.5, y as f32 + 0.5);
            let to = from + v2(20.0, 0.0);
            if !g.grid.walkable(x, y) || !g.grid.walkable(x, y + 1) || !g.grid.walkable(x + 20, y) {
                continue;
            }
            if (g.map.height_at(from) - g.map.height_at(to)).abs() > LEVEL {
                continue;
            }
            if g.igloos.iter().all(|ig| ig.door.dist(from) > 12.0 && ig.door.dist(to) > 12.0) {
                return (from, to);
            }
        }
    }
    panic!("no level 20-tile corridor");
}

fn ski_distance(kind: Kind, ground: Ground) -> f32 {
    let mut g = game();
    let (w, h) = (g.map.w as i32, g.map.h as i32);
    let mut corridor = None;
    'find: for y in 2..h - 2 {
        for x in 2..w - 10 {
            if (0..=8).all(|dx| g.grid.walkable(x + dx, y) && g.map.ground_at(x + dx, y) == ground) {
                corridor = Some((x, y));
                break 'find;
            }
        }
    }
    let (x, y) = corridor.expect("map has a clear terrain corridor");
    let from = v2(x as f32 + 0.5, y as f32 + 0.5);
    let to = from + v2(8.0, 0.0);
    let ig = home(&g, 0);
    let id = if kind == Kind::Worker {
        g.penguins.iter().find(|p| p.owner == 0 && p.kind == kind).unwrap().id
    } else {
        g.igloos[ig].fish = 100;
        g.igloos[ig].wood = 100;
        g.igloos[ig].tier = kind.tier();
        hatch(&mut g, ig, kind);
        g.penguins.iter().find(|p| p.owner == 0 && p.kind == kind).unwrap().id
    };
    g.igloos[ig].occupants.retain(|&u| u != id);
    let p = &mut g.penguins[id as usize];
    p.pos = from;
    p.inside = None;
    p.order = Order::None;
    g.command(0, Command::Move { units: vec![id], target: to, run: false, queue: false });
    for _ in 0..30 {
        g.penguins[id as usize].warmth = g.penguins[id as usize].max_warmth;
        g.tick();
    }
    from.dist(g.penguins[id as usize].pos)
}

#[test]
fn sledders_are_fast() {
    let mut g = game();
    // The sledder is a former worker.
    let ig = g.igloos.iter().position(|i| i.owner == 0).unwrap();
    let igloo = &mut g.igloos[ig];
    igloo.fish = 100;
    igloo.wood = 100;
    igloo.tier = Kind::Sledder.tier();
    hatch(&mut g, ig, Kind::Sledder);
    let sled = g.penguins.iter().find(|p| p.owner == 0 && p.kind == Kind::Sledder).unwrap().id;
    let work = g.penguins.iter().find(|p| p.owner == 0 && p.kind == Kind::Worker).unwrap().id;
    let (from, to) = march_pair(&g);
    g.igloos[ig].occupants.retain(|&o| o != sled && o != work);
    // Side by side at the start of the march.
    let p = &mut g.penguins[sled as usize];
    p.pos = from;
    p.inside = None;
    p.order = Order::None;
    let p = &mut g.penguins[work as usize];
    p.pos = from + v2(0.0, 1.2);
    p.inside = None;
    p.order = Order::None;
    g.command(0, Command::Move { units: vec![sled], target: to, run: false, queue: false });
    g.command(0, Command::Move { units: vec![work], target: to, run: false, queue: false });
    // Keep both warm so cold never sends them home mid-march.
    let mut arrived = false;
    for _ in 0..60 * sim::TICK_RATE {
        g.penguins[sled as usize].warmth = g.max_warmth_of(Kind::Sledder);
        g.penguins[work as usize].warmth = g.max_warmth_of(Kind::Worker);
        g.tick();
        if g.penguins[sled as usize].pos.dist(to) <= 0.2 {
            arrived = true;
            break;
        }
    }
    assert!(arrived, "sledder reached a spot 20 tiles away");
    let w = &g.penguins[work as usize];
    assert!(w.pos.dist(from) > 2.0, "worker actually set off");
    assert!(w.pos.dist(to) > 2.0, "worker still {0:.1} tiles from the spot", w.pos.dist(to));
}

#[test]
fn skiers_are_fast_on_snow_and_slow_on_ice() {
    assert!(ski_distance(Kind::Skier, Ground::Snow) > ski_distance(Kind::Worker, Ground::Snow));
    assert!(ski_distance(Kind::Skier, Ground::Ice) < ski_distance(Kind::Worker, Ground::Ice));
}

/// Level ground for a guarded snowballer, its shield a tile over, and a bare
/// snowballer 10 tiles off, all clear of every igloo door.
fn shield_spot(g: &Game) -> (Vec2, Vec2, Vec2) {
    for y in 3..g.map.h - 5 {
        for x in 3..g.map.w - 13 {
            let guarded = v2(x as f32 + 0.5, y as f32 + 0.5);
            let shield = guarded + v2(1.0, 0.0);
            let bare = guarded + v2(10.0, 0.0);
            if !g.grid.walkable(x, y) || !g.grid.walkable(x + 1, y) || !g.grid.walkable(x + 10, y) {
                continue;
            }
            if (g.map.height_at(guarded) - g.map.height_at(shield)).abs() > LEVEL || (g.map.height_at(guarded) - g.map.height_at(bare)).abs() > LEVEL {
                continue;
            }
            if g.igloos.iter().all(|ig| ig.door.dist(guarded) > 12.0 && ig.door.dist(bare) > 12.0) {
                return (guarded, shield, bare);
            }
        }
    }
    panic!("no level ground for the shield");
}

/// Hold a penguin at `pos`, warm and idle, so it cannot drift or flee.
fn pin(g: &mut Game, id: u16, pos: Vec2) {
    let warm = g.max_warmth_of(g.penguins[id as usize].kind);
    let p = &mut g.penguins[id as usize];
    p.pos = pos;
    p.inside = None;
    p.order = Order::Hold { pos };
    p.warmth = warm;
    p.act = Act::Idle;
    p.act_t = 0;
    p.cold = false;
}

#[test]
fn shields_protect_their_neighbours() {
    let mut g = game();
    // Training turns the igloo's own workers, so diff the kinds to find the three that changed.
    let ig = g.igloos.iter().position(|i| i.owner == 0).unwrap();
    let igloo = &mut g.igloos[ig];
    igloo.fish = 100;
    igloo.wood = 100;
    igloo.tier = Kind::Shield.tier();
    let before: Vec<Kind> = g.penguins.iter().map(|p| p.kind).collect();
    hatch(&mut g, ig, Kind::Snowballer);
    hatch(&mut g, ig, Kind::Snowballer);
    hatch(&mut g, ig, Kind::Shield);
    let mut snows = Vec::new();
    let mut shield = None;
    for (i, p) in g.penguins.iter().enumerate() {
        if before.get(i) != Some(&p.kind) {
            match p.kind {
                Kind::Snowballer => snows.push(p.id),
                Kind::Shield => shield = Some(p.id),
                _ => panic!("hatched a {p:?}"),
            }
        }
    }
    assert_eq!(snows.len(), 2, "two workers became snowballers");
    let shield = shield.expect("one worker became a shield");
    g.igloos[ig].occupants.retain(|&o| o != snows[0] && o != snows[1] && o != shield);
    // The guarded snowballer stands a tile from its shield; the bare one is
    // 10 tiles away, outside both the shield and the splash.
    let (guarded, _, bare) = shield_spot(&g);
    // One enemy snowball each, thrown straight at the two snowballers. The
    // slab covers whoever is behind it: the shield stands on the thrower's side.
    let thrower = g.penguins.iter().find(|p| p.owner == 1).unwrap().id;
    let from = g.penguins[thrower as usize].pos;
    let shield_pos = guarded + (from - guarded).norm() * 1.0;
    pin(&mut g, snows[0], guarded);
    pin(&mut g, shield, shield_pos);
    pin(&mut g, snows[1], bare);
    for (id, target) in [(90000u32, guarded), (90001, bare)] {
        g.snowballs.push(Snowball { id, from, from_z: 0.0, to: target, to_z: 0.0, start_tick: g.tick, flight: 2, thrower, team: 1, kind: Kind::Snowballer, big: false });
    }
    g.tick();
    pin(&mut g, snows[0], guarded);
    pin(&mut g, shield, shield_pos);
    pin(&mut g, snows[1], bare);
    let guarded_pre = g.penguins[snows[0] as usize].warmth;
    let bare_pre = g.penguins[snows[1] as usize].warmth;
    g.tick();
    let guarded_lost = guarded_pre - g.penguins[snows[0] as usize].warmth;
    let bare_lost = bare_pre - g.penguins[snows[1] as usize].warmth;
    assert_eq!(bare_lost, 3, "the bare snowballer takes the full direct hit");
    assert_eq!(guarded_lost, 2, "the shield covers its neighbour");
    assert!(guarded_lost < bare_lost);
}

#[test]
fn slingers_outrange_snowballers() {
    let g = game();
    let (from, to) = level_pair(&g);
    let sling = g.throw_range(Kind::Slinger, 0, from, to);
    let snow = g.throw_range(Kind::Snowballer, 0, from, to);
    assert!(sling > snow, "slinger reaches {sling} tiles, snowballer only {snow}");
    assert!(snow > 0.0 && sling > 0.0, "both should be able to throw something");
}

#[test]
fn warmers_never_fight() {
    // The stats table decides it: only kinds carrying snowballs fight.
    assert!(!Kind::Warmer.fights(), "warmers throw no snowballs");
    assert!(Kind::Snowballer.fights(), "snowballers fight");
    assert!(!Kind::Worker.fights(), "workers only fish and build");
}

/// How far a lone penguin of this kind gets in five seconds along the march,
/// with the whole corridor laid as `ground`.
fn dash(kind: Kind, ground: sim::map::Ground) -> f32 {
    let mut g = game();
    let (from, to) = march_pair(&g);
    let (x0, y0) = from.tile();
    for x in x0 - 1..=x0 + 21 {
        for dy in -1..=1 {
            let i = g.map.idx(x, y0 + dy);
            g.map.ground[i] = ground;
        }
    }
    let id = g.penguins.iter().find(|p| p.owner == 0 && p.kind == Kind::Worker).unwrap().id;
    let ig = g.igloos.iter().position(|i| i.owner == 0).unwrap();
    g.igloos[ig].occupants.retain(|&o| o != id);
    let p = &mut g.penguins[id as usize];
    p.kind = kind;
    p.pos = from;
    p.inside = None;
    p.order = Order::None;
    g.command(0, Command::Move { units: vec![id], target: to, run: false, queue: false });
    for _ in 0..5 * sim::TICK_RATE {
        g.penguins[id as usize].warmth = g.max_warmth_of(kind);
        g.tick();
    }
    g.penguins[id as usize].pos.dist(from)
}

#[test]
fn skiers_race_over_snow_and_crawl_on_ice() {
    use sim::map::Ground;
    let (skier_snow, skier_ice) = (dash(Kind::Skier, Ground::Snow), dash(Kind::Skier, Ground::Ice));
    let (ball_snow, ball_ice) = (dash(Kind::Snowballer, Ground::Snow), dash(Kind::Snowballer, Ground::Ice));
    assert!(skier_snow > ball_snow * 1.2, "skier {skier_snow:.1} vs snowballer {ball_snow:.1} on snow");
    assert!(skier_ice < skier_snow * 0.8, "skier slows on ice: {skier_ice:.1} vs {skier_snow:.1}");
    assert!(ball_ice > ball_snow, "everyone else slides faster on ice");
    assert!(skier_ice < ball_ice, "skier {skier_ice:.1} vs snowballer {ball_ice:.1} on ice");
}
