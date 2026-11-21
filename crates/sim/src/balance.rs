//! How even a map is. For each starting igloo this measures what lies within
//! reach (trees, fishing banks, crystal, ice) and how far the nearest rival
//! is on foot. The map generator plants and thins trees to even out wood
//! (`mapgen::balance_trees`), and `tests/balance.rs` keeps every shipped map
//! inside the limits.

use crate::game::{Game, NEUTRAL};
use crate::map::{Ground, ResourceKind};
use crate::math::v2;
use crate::mapgen::{trees_near, WOOD_REACH};

/// One starting igloo's surroundings.
#[derive(Clone, Debug)]
pub struct Start {
    pub trees: usize,
    /// Fishing banks within reach.
    pub banks: usize,
    /// Walking distance to the nearest crystal, infinite if none.
    pub crystal: f32,
    /// Share of the land within reach that is ice.
    pub ice: f32,
    /// Places a fishing outpost could go: banks a walk from home, in spots
    /// at least `OUTPOST_SPACING` apart (the computers build up to two).
    pub outposts: usize,
    pub to_rival: f32,
}

/// A store crews itself from banks this far from the igloo...
pub const OUTPOST_FROM: f32 = 7.0;
pub const OUTPOST_TO: f32 = 22.0;
/// ...and two outposts need to be at least this far apart.
pub const OUTPOST_SPACING: f32 = 16.0;

/// Where fishing outposts could go for an igloo with this door: banks, nearest
/// first, thinned to those far enough from each other.
pub fn outpost_sites(g: &Game, door: crate::math::Vec2) -> Vec<crate::math::Vec2> {
    let mut banks: Vec<crate::math::Vec2> = g.shores.iter().map(|s| s.pos).filter(|p| (OUTPOST_FROM..=OUTPOST_TO).contains(&p.dist(door))).collect();
    banks.sort_by(|a, b| a.dist2(door).partial_cmp(&b.dist2(door)).unwrap());
    let mut sites: Vec<crate::math::Vec2> = Vec::new();
    for b in banks {
        if sites.iter().all(|&o| o.dist(b) >= OUTPOST_SPACING) {
            sites.push(b);
        }
    }
    sites
}

/// Measure every tribe's start in a new game on this map.
pub fn starts(g: &Game) -> Vec<Start> {
    let owned: Vec<_> = g.igloos.iter().filter(|i| i.owner != NEUTRAL).collect();
    let fields: Vec<Vec<f32>> = owned.iter().map(|ig| g.grid.distances(ig.door.tile())).collect();
    // Crystal and igloos sit on blocked ground: take the best neighbouring tile.
    let at = |f: &Vec<f32>, p: crate::math::Vec2| {
        let (x, y) = p.tile();
        let mut best = f32::INFINITY;
        for dy in -2..=2 {
            for dx in -2..=2 {
                if g.map.in_bounds(x + dx, y + dy) {
                    best = best.min(f[g.map.idx(x + dx, y + dy)]);
                }
            }
        }
        best
    };
    owned
        .iter()
        .enumerate()
        .map(|(k, ig)| {
            let r = WOOD_REACH.ceil() as i32;
            let (cx, cy) = ig.center.tile();
            let (mut land, mut ice) = (0.0, 0.0);
            for y in cy - r..=cy + r {
                for x in cx - r..=cx + r {
                    if g.map.in_bounds(x, y) && v2(x as f32 + 0.5, y as f32 + 0.5).dist(ig.center) <= WOOD_REACH && g.map.ground_at(x, y) != Ground::Water {
                        land += 1.0;
                        if g.map.ground_at(x, y) == Ground::Ice {
                            ice += 1.0;
                        }
                    }
                }
            }
            Start {
                trees: trees_near(&g.map, ig.center, WOOD_REACH),
                banks: g.shores.iter().filter(|s| s.pos.dist(ig.center) <= WOOD_REACH).count(),
                crystal: g.resources.iter().filter(|r| r.kind == ResourceKind::Crystal).map(|r| at(&fields[k], r.pos)).fold(f32::INFINITY, f32::min),
                ice: if land > 0.0 { ice / land } else { 0.0 },
                outposts: outpost_sites(g, ig.door).len(),
                to_rival: owned.iter().enumerate().filter(|&(j, _)| j != k).map(|(_, o)| at(&fields[k], o.door)).fold(f32::INFINITY, f32::min),
            }
        })
        .collect()
}
