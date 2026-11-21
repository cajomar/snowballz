//! Build the hand-made The Pass map: two valleys, one igloo in each, split by
//! a wall of pinnacles. The valleys meet through one long pass cut through the
//! middle of the wall, exactly three tiles wide; the only other way across is
//! the long way round over the top of the wall.
//! `cargo run -p sim --release --example the_pass` writes
//! `assets/maps/the-pass.sbmap`.

use sim::map::{Ground, Map, MapRules, Obstacle, ResourceKind, ResourceSpawn};
use sim::mapgen::check;
use sim::math::{v2, Vec2};

const S: i32 = 128;
const TAU: f32 = std::f32::consts::TAU;

// The wall of pinnacles, the pass cut through it, and the open way over the top.
const WALL_X0: i32 = 40;
const WALL_X1: i32 = 87;
const PASS_Y: [i32; 3] = [61, 62, 63];
const WAY_Y1: i32 = 29; // rows 0..=WAY_Y1 stay open: the long way round

fn main() {
    let igloos: Vec<(u16, u16)> = vec![(10, 60), (114, 60)];
    let centres: Vec<Vec2> = igloos.iter().map(|&(x, y)| v2(x as f32 + 2.0, y as f32 + 2.0)).collect();
    let powerups: Vec<(u8, u16, u16)> = vec![(0, 20, 18), (1, 107, 18), (0, 20, 110), (1, 107, 110)];

    let in_wall = |x: i32, y: i32| WALL_X0 <= x && x <= WALL_X1 && y >= 0 && y < S;
    let in_pass = |x: i32, y: i32| WALL_X0 <= x && x <= WALL_X1 && PASS_Y.contains(&y);
    let in_way = |x: i32, y: i32| WALL_X0 <= x && x <= WALL_X1 && y <= WAY_Y1;

    let n = (S * S) as usize;
    let ground = vec![Ground::Snow; n];
    let mut obstacle = vec![Obstacle::None; n];
    let mut height = Vec::with_capacity(n);
    for y in 0..S {
        for x in 0..S {
            let i = (y * S + x) as usize;
            if in_wall(x, y) && !in_pass(x, y) && !in_way(x, y) {
                obstacle[i] = Obstacle::Pinnacle;
            }
            // Valleys low, wall high, pass floor and way round in between.
            height.push(if in_pass(x, y) {
                0.6
            } else if in_way(x, y) {
                0.5
            } else if in_wall(x, y) {
                0.9
            } else {
                0.15 + 0.05 * (x as f32 * 0.21).sin() * (y as f32 * 0.17).cos()
            });
        }
    }

    // Trees in the valleys only; keep the yards and powerup spots clear.
    for y in 0..S {
        for x in 0..S {
            let i = (y * S + x) as usize;
            if in_wall(x, y) {
                continue;
            }
            if centres.iter().any(|ct| ct.dist(v2(x as f32 + 0.5, y as f32 + 0.5)) <= 5.0) {
                continue;
            }
            if powerups.iter().any(|&(_, px, py)| (x - px as i32).abs() <= 1 && (y - py as i32).abs() <= 1) {
                continue;
            }
            let h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263);
            if h % 5 != 0 {
                continue;
            }
            obstacle[i] = Obstacle::Tree;
        }
    }

    let mut resources: Vec<ResourceSpawn> = Vec::new();
    // Five fish holes ring each igloo, out on the snow.
    for &ct in &centres {
        for k in 0..5 {
            let dir = Vec2::from_angle(k as f32 * TAU / 5.0);
            for &d in &[7.0f32, 6.0, 8.0, 5.5, 9.0] {
                let (tx, ty) = (ct + dir * d).tile();
                if !((0..S).contains(&tx) && (0..S).contains(&ty)) {
                    continue;
                }
                if obstacle[(ty * S + tx) as usize].blocks() {
                    continue;
                }
                if resources.iter().any(|r| r.x as i32 == tx && r.y as i32 == ty) {
                    continue;
                }
                resources.push(ResourceSpawn { x: tx as u16, y: ty as u16, kind: ResourceKind::Fish, amount: 7 });
                break;
            }
        }
    }
    // The crystals: one on the pass floor, a little nearer the left valley, and
    // one out on the way round, a little nearer the right. Each side gets the
    // closer of one, and neither is handed both.
    resources.push(ResourceSpawn { x: 60, y: 62, kind: ResourceKind::Crystal, amount: 25 });
    resources.push(ResourceSpawn { x: 68, y: 15, kind: ResourceKind::Crystal, amount: 25 });

    let lo = height.iter().cloned().fold(f32::MAX, f32::min);
    let hi = height.iter().cloned().fold(f32::MIN, f32::max);
    let span = (hi - lo).max(0.6);
    let height_raw = height.iter().map(|h| (((h - lo) / span) * 255.0).clamp(0.0, 255.0) as u8).collect();

    let goal = "Take over all enemy igloos.";
    let mut map = Map {
        name: "The Pass".into(),
        description: format!(
            "Two valleys split by a wall of pinnacles. Take the long pass through the middle, three tiles wide, or the longer way round over the top. {goal}"
        ),
        w: S,
        h: S,
        height_raw,
        ground,
        obstacle,
        resources,
        igloos,
        powerups,
        rules: MapRules { workers: 8, snowballers: 10, worker_warmth: 10, snowballer_warmth: 12 },
        height: Vec::new(),
    };
    // Round-trip through the file format so derived data (smoothed heights) is built.
    sim::mapgen::add_ponds(&mut map);
    sim::mapgen::balance_ice(&mut map, 0);
    sim::mapgen::balance_trees(&mut map, 0);
    sim::mapgen::add_ponds(&mut map);
    sim::mapgen::connect_starts(&mut map);
    sim::mapgen::add_home_crystal(&mut map);
    let map = Map::parse(&map.encode()).expect("the pass encodes");
    if let Err(e) = check(&map) {
        panic!("The Pass fails check: {e}");
    }
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps/the-pass.sbmap");
    std::fs::write(&out, map.encode()).unwrap();
    let trees = map.obstacle.iter().filter(|&&o| o == Obstacle::Tree).count();
    let pinnacles = map.obstacle.iter().filter(|&&o| o == Obstacle::Pinnacle).count();
    println!("the-pass.sbmap ({}x{}, {} igloos, {} resources, {} trees, {} pinnacles)", map.w, map.h, map.igloos.len(), map.resources.len(), trees, pinnacles);
}
