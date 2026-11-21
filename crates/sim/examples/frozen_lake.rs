//! Build the hand-made Frozen Lake map: a big ice lake in the centre, three
//! ice causeways from it to the shores, an igloo in each corner, and fish
//! holes around every igloo and out on the open lake.
//! `cargo run -p sim --release --example frozen_lake` writes
//! `assets/maps/frozen-lake.sbmap`.

use sim::map::{Ground, Map, MapRules, Obstacle, ResourceKind, ResourceSpawn};
use sim::mapgen::check;
use sim::math::{v2, Vec2};

const S: i32 = 128;
const TAU: f32 = std::f32::consts::TAU;
const LAKE_R: f32 = 30.0;

fn main() {
    let c = v2(64.0, 64.0);
    let igloos: Vec<(u16, u16)> = vec![(8, 8), (116, 8), (8, 116), (116, 116)];
    let centres: Vec<Vec2> = igloos.iter().map(|&(x, y)| v2(x as f32 + 2.0, y as f32 + 2.0)).collect();
    // Four causeways from the lake, one straight to each corner igloo.
    let dirs: [Vec2; 4] = [v2(-0.7071, -0.7071), v2(0.7071, -0.7071), v2(-0.7071, 0.7071), v2(0.7071, 0.7071)];
    let powerups: Vec<(u8, u16, u16)> = vec![(0, 64, 12), (1, 115, 64), (0, 12, 64), (1, 64, 115)];

    let is_bridge = |x: i32, y: i32| -> bool {
        let q = v2(x as f32 + 0.5, y as f32 + 0.5) - c;
        dirs.iter().any(|d| {
            let along = q.dot(*d);
            let perp = q.x * d.y - q.y * d.x;
            24.0 < along && along < 80.0 && perp.abs() < 2.7
        })
    };

    let n = (S * S) as usize;
    let mut ground = vec![Ground::Snow; n];
    let mut obstacle = vec![Obstacle::None; n];
    let mut height = Vec::with_capacity(n);
    for y in 0..S {
        for x in 0..S {
            let i = (y * S + x) as usize;
            let p = v2(x as f32 + 0.5, y as f32 + 0.5);
            let d = p.dist(c);
            if d < LAKE_R || is_bridge(x, y) {
                ground[i] = Ground::Ice;
            }
            // The causeways are raised over the lake bed; a ridge rings the shore.
            height.push(if is_bridge(x, y) {
                0.3
            } else if d < LAKE_R {
                0.15
            } else if d < LAKE_R + 3.0 {
                0.8
            } else {
                0.45 + 0.15 * (x as f32 * 0.13).sin() * (y as f32 * 0.11).cos()
            });
        }
    }

    // Trees in a ring between the shore and the igloos, thin enough to walk through.
    for y in 0..S {
        for x in 0..S {
            let i = (y * S + x) as usize;
            if ground[i] != Ground::Snow {
                continue;
            }
            let d = v2(x as f32 + 0.5, y as f32 + 0.5).dist(c);
            if !(43.0 < d && d < 57.0) {
                continue;
            }
            let h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263);
            if h % 4 != 0 {
                continue;
            }
            if powerups.iter().any(|&(_, px, py)| (x - px as i32).abs() <= 1 && (y - py as i32).abs() <= 1) {
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
    // Six fish holes on the open lake, and the crystal at its heart.
    for k in 0..6 {
        let (tx, ty) = (c + Vec2::from_angle(k as f32 * TAU / 6.0) * 15.0).tile();
        resources.push(ResourceSpawn { x: tx as u16, y: ty as u16, kind: ResourceKind::Fish, amount: 7 });
    }
    resources.push(ResourceSpawn { x: 64, y: 64, kind: ResourceKind::Crystal, amount: 25 });

    let lo = height.iter().cloned().fold(f32::MAX, f32::min);
    let hi = height.iter().cloned().fold(f32::MIN, f32::max);
    let span = (hi - lo).max(0.6);
    let height_raw = height.iter().map(|h| (((h - lo) / span) * 255.0).clamp(0.0, 255.0) as u8).collect();

    let goal = "Take over all enemy igloos.";
    let mut map = Map {
        name: "Frozen Lake".into(),
        description: format!("A frozen lake with an ice causeway to each of four corner igloos. {goal}"),
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
    let map = Map::parse(&map.encode()).expect("frozen lake encodes");
    if let Err(e) = check(&map) {
        panic!("Frozen Lake fails check: {e}");
    }
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps/frozen-lake.sbmap");
    std::fs::write(&out, map.encode()).unwrap();
    let trees = map.obstacle.iter().filter(|&&o| o == Obstacle::Tree).count();
    println!("frozen-lake.sbmap ({}x{}, {} igloos, {} resources, {} trees)", map.w, map.h, map.igloos.len(), map.resources.len(), trees);
}
