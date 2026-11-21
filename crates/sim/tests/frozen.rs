//! Frozen penguins (DESIGN.md section 0): a frozen penguin's tile is a block
//! of ice that stops walking, and thawing frees it again.

use sim::game::*;
use sim::map::Map;
use sim::math::{v2, Vec2};

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

/// Tick `n` times in midsummer, with nobody throwing.
fn run(g: &mut Game, n: u32) -> Vec<Event> {
    let mut all = Vec::new();
    for _ in 0..n {
        g.weather.season = Season::Midsummer;
        g.weather.season_t = 0;
        for p in &mut g.penguins {
            p.snowballs = 0;
        }
        g.tick();
        all.extend(std::mem::take(&mut g.events));
    }
    all
}

/// The middle of an open, walkable patch, away from igloos.
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
fn someone(g: &Game, owner: u8, kind: Kind) -> u16 {
    g.penguins.iter().find(|p| p.owner == owner && p.kind == kind && p.inside.is_none()).unwrap().id
}

/// Freeze `v` at `at` the real way: an enemy snowballer pelts it.
fn freeze_at(g: &mut Game, v: u16, at: Vec2) {
    let owner = g.penguins[v as usize].owner;
    let foe = (owner + 1) % 2;
    let thrower = someone(g, foe, Kind::Snowballer);
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
    // The new core: a frozen penguin belongs to nobody, and remembers who it was.
    let mut g = game(CoreRules::default().map_start());
    let at = open_ground(&g, 8, 4);
    let v = someone(&g, 0, Kind::Worker);
    freeze_at(&mut g, v, at);
    assert_eq!(g.penguins[v as usize].owner, NEUTRAL, "freezing ends ownership");
    assert_eq!(g.penguins[v as usize].was, 0, "it remembers its tribe");
}

#[test]
fn thawing_unblocks_the_tile() {
    let mut g = game(CoreRules::default().map_start());
    let at = open_ground(&g, 8, 4);
    let v = someone(&g, 0, Kind::Worker);
    freeze_at(&mut g, v, at);
    let (x, y) = g.penguins[v as usize].pos.tile();
    assert!(!g.grid.walkable(x, y), "the frozen tile is a block of ice");
    assert!(g.frozen_at(x, y));
    let ev = run(&mut g, THAW_TICKS as u32 + 5);
    // (It may well freeze again somewhere else once it walks off, still
    // cold: what matters is that it thawed and the tile came free.)
    assert!(ev.iter().any(|e| matches!(e, Event::Thawed { penguin, by: NEUTRAL } if *penguin == v)), "midsummer thaws it out");
    assert!(g.grid.walkable(x, y), "thawing frees the tile");
    assert!(!g.frozen_at(x, y));
}

/// A straight corridor through open ground: a 13x13 walkable square whose
/// middle row is `y0`, at least 12 from every igloo door. Returns the
/// left column of the 3-wide gap the test will block and the row to block.
fn corridor(g: &Game) -> (i32, i32) {
    for y0 in 4..g.grid.h - 8 {
        for cx in 3..g.grid.w - 12 {
            let clear = (y0 - 4..y0 + 9).all(|y| (cx..cx + 13).all(|x| g.grid.walkable(x, y)));
            let c = v2(cx as f32 + 6.5, y0 as f32 + 2.5);
            if clear && g.igloos.iter().all(|ig| ig.door.dist(c) > 12.0) {
                return (cx, y0);
            }
        }
    }
    panic!("no corridor");
}

/// Wall across row `y0`, leaving only the 3-wide gap at `cx..cx+2`.
fn wall_with_gap(g: &mut Game, cx: i32, y0: i32) {
    for x in 0..g.grid.w {
        if !(cx..cx + 3).contains(&x) {
            g.grid.walk[(y0 * g.grid.w + x) as usize] = false;
        }
    }
}

/// A worker ordered straight through the gap, from the row above to the row below.
fn send_mover(g: &mut Game, cx: i32, y0: i32) -> (u16, Vec2) {
    let mover = someone(g, 0, Kind::Worker);
    let start = v2(cx as f32 + 1.5, y0 as f32 - 3.5);
    g.penguins[mover as usize].pos = start;
    g.command(0, Command::Move { units: vec![mover], target: v2(cx as f32 + 1.5, y0 as f32 + 3.5), run: false, queue: false });
    (mover, start)
}

/// Run `n` ticks in polar night: no thawing, no blizzard, and nobody but
/// the mover moves (every other penguin is pinned where it stood). After
/// each tick the mover must still stand on a walkable tile; `check` sees
/// its position.
fn run_pinned(g: &mut Game, mover: u16, pins: &[Vec2], n: u32, check: &mut impl FnMut(Vec2)) {
    for _ in 0..n {
        g.weather.season = Season::PolarNight;
        g.weather.season_t = 0;
        g.weather.blizzard = 0;
        for (p, &pos) in g.penguins.iter_mut().zip(pins) {
            if p.id != mover {
                p.pos = pos;
                p.snowballs = 0;
            }
        }
        g.tick();
        g.events.clear();
        let pos = g.penguins[mover as usize].pos;
        assert!(g.grid.walkable_at(pos), "mover only stands on walkable tiles: {pos:?}");
        check(pos);
    }
}

/// Freeze a penguin across the whole corridor, then send one penguin
/// through: it must never stand on a blocked tile, and it stops short
/// of the frozen wall.
#[test]
fn frozen_penguins_block_the_way() {
    let mut g = game(CoreRules::default().map_start());
    let (cx, y0) = corridor(&g);
    wall_with_gap(&mut g, cx, y0);
    for dx in 0..3 {
        let v = someone(&g, 0, Kind::Worker);
        freeze_at(&mut g, v, v2(cx as f32 + 0.5 + dx as f32, y0 as f32 + 0.5));
        assert!(!g.grid.walkable(cx + dx, y0), "frozen penguin is a block of ice");
        assert!(g.frozen_at(cx + dx, y0));
    }
    let (mover, start) = send_mover(&mut g, cx, y0);
    let pins: Vec<Vec2> = g.penguins.iter().map(|p| p.pos).collect();

    let mut max_row = i32::MIN;
    run_pinned(&mut g, mover, &pins, 30 * sim::TICK_RATE, &mut |pos| {
        let (_, y) = pos.tile();
        assert!(y <= y0 - 1, "mover never stands on the blocked row: {pos:?}");
        max_row = max_row.max(y);
    });
    let now = g.penguins[mover as usize].pos;
    assert!(start.dist(now) > 1.5, "the mover set off: {start:?} -> {now:?}");
    assert!(max_row >= y0 - 1, "the mover reached the wall: farthest row {max_row}");
    assert!(max_row <= y0 - 1, "the mover stops short of the frozen wall");
}

/// The same corridor with the gap left open: the worker crosses it.
/// Shows that the frozen penguins, not the wall, did the blocking.
#[test]
fn open_gap_is_no_block() {
    let mut g = game(CoreRules::default().map_start());
    let (cx, y0) = corridor(&g);
    wall_with_gap(&mut g, cx, y0);
    for dx in 0..3 {
        assert!(g.grid.walkable(cx + dx, y0), "the gap is left open");
    }
    let (mover, _) = send_mover(&mut g, cx, y0);
    let pins: Vec<Vec2> = g.penguins.iter().map(|p| p.pos).collect();

    run_pinned(&mut g, mover, &pins, 30 * sim::TICK_RATE, &mut |_| {});
    let (x, y) = g.penguins[mover as usize].pos.tile();
    assert!(y > y0, "with the gap open the mover crosses: row {y} vs wall row {y0} (x {x})");
}

/// A wall site that player 0 can legally place near its igloo.
fn wall_site(g: &Game) -> (i32, i32) {
    let door = g.igloos.iter().find(|i| i.owner == 0).unwrap().door;
    g.find_site(0, BuildKind::Palisade, door + v2(6.0, 0.0), 12).expect("a place for a wall")
}

#[test]
fn building_does_not_shove_frozen_blocks_around() {
    let mut g = game(CoreRules::default().map_start());
    let at = open_ground(&g, 8, 4);
    let v = someone(&g, 1, Kind::Worker);
    freeze_at(&mut g, v, at);
    let pos = g.penguins[v as usize].pos;
    let (x, y) = pos.tile();

    let builder = someone(&g, 0, Kind::Worker);
    let t = wall_site(&g);
    g.command(0, Command::Build { units: vec![builder], kind: BuildKind::Palisade, tiles: vec![t] });
    assert!(g.buildings.iter().any(|b| b.alive && b.owner == 0), "the wall site went up");
    run(&mut g, 40);
    assert_eq!(g.penguins[v as usize].pos, pos, "the block of ice stayed where it fell");
    assert!(g.frozen_at(x, y) && !g.grid.walkable(x, y));

    // Thawing frees exactly that tile, and leaves no phantom block behind.
    let ev = run(&mut g, THAW_TICKS as u32 + 5);
    assert!(ev.iter().any(|e| matches!(e, Event::Thawed { penguin, .. } if *penguin == v)));
    assert!(!g.frozen_at(x, y) && g.grid.walkable(x, y), "tile free again");
    let stuck = (0..g.map.w).flat_map(|x| (0..g.map.h).map(move |y| (x, y))).filter(|&(x, y)| g.frozen_at(x, y)).count();
    // Others may have frozen meanwhile; a tile counted frozen needs a frozen penguin on it.
    let frozen = g.penguins.iter().filter(|p| p.act == Act::Frozen).count();
    assert!(stuck <= frozen, "{stuck} tiles counted as frozen but only {frozen} penguins are");
}
