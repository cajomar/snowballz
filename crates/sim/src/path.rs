//! Grid pathfinding (A* with octile distance) and line-of-sight checks.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::math::{v2, Vec2};

pub type Tile = (i32, i32);

pub struct Grid {
    pub w: i32,
    pub h: i32,
    pub walk: Vec<bool>,
    /// Movement cost multiplier per tile (ice is quicker to cross).
    pub cost: Vec<f32>,
}

/// How far a straight walk keeps from blocked ground.
const LINE_MARGIN: f32 = 0.2;
/// How far a straight walk keeps from a tree's tile. A pine's boughs reach
/// about 0.65 tiles from the trunk, a tile edge is 0.5 from it, so 0.3 more
/// keeps penguins from brushing through the branches.
pub const TREE_MARGIN: f32 = 0.3;

/// True if a straight walk from `a` to `b` stays `TREE_MARGIN` clear of
/// every tree tile.
pub fn clear_of_trees(map: &crate::map::Map, a: Vec2, b: Vec2) -> bool {
    let d = b - a;
    let steps = (d.len() / 0.25).ceil() as i32;
    let tree = |x: f32, y: f32| {
        let (tx, ty) = v2(x, y).tile();
        map.obstacle_at(tx, ty) == crate::map::Obstacle::Tree
    };
    for i in 0..=steps {
        let p = a + d * (i as f32 / steps.max(1) as f32);
        for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0), (0.0, 0.0)] {
            if tree(p.x + dx * TREE_MARGIN, p.y + dy * TREE_MARGIN) {
                return false;
            }
        }
    }
    true
}

impl Grid {
    pub fn walkable(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h && self.walk[(y * self.w + x) as usize]
    }

    pub fn walkable_at(&self, p: Vec2) -> bool {
        let (x, y) = p.tile();
        self.walkable(x, y)
    }

    /// True if a penguin can walk in a straight line from `a` to `b`.
    pub fn line_clear(&self, a: Vec2, b: Vec2) -> bool {
        let d = b - a;
        let len = d.len();
        let steps = (len / 0.25).ceil() as i32;
        let side = v2(-d.y, d.x).norm() * LINE_MARGIN;
        for i in 0..=steps {
            let p = a + d * (i as f32 / steps.max(1) as f32);
            // Check a little to each side so paths don't graze corners.
            if !self.walkable_at(p + side) || !self.walkable_at(p - side) {
                return false;
            }
        }
        true
    }

    /// Walking distance in tiles from `from` to every tile (infinite where
    /// it can't be reached), with diagonals costing a little more.
    pub fn distances(&self, from: Tile) -> Vec<f32> {
        let mut dist = vec![f32::INFINITY; (self.w * self.h) as usize];
        let Some(start) = self.nearest_walkable(from, 6) else { return dist };
        // Costs in tenths so the heap can order plain integers.
        let mut heap = BinaryHeap::new();
        dist[(start.1 * self.w + start.0) as usize] = 0.0;
        heap.push(std::cmp::Reverse((0u32, start)));
        while let Some(std::cmp::Reverse((d, t))) = heap.pop() {
            if d as f32 / 10.0 > dist[(t.1 * self.w + t.0) as usize] {
                continue;
            }
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let n = (t.0 + dx, t.1 + dy);
                    let diag = dx != 0 && dy != 0;
                    if (dx == 0 && dy == 0) || !self.walkable(n.0, n.1) {
                        continue;
                    }
                    if diag && (!self.walkable(t.0 + dx, t.1) || !self.walkable(t.0, t.1 + dy)) {
                        continue;
                    }
                    let nd = d + if diag { 14 } else { 10 };
                    let i = (n.1 * self.w + n.0) as usize;
                    if (nd as f32) / 10.0 < dist[i] {
                        dist[i] = nd as f32 / 10.0;
                        heap.push(std::cmp::Reverse((nd, n)));
                    }
                }
            }
        }
        dist
    }

    /// Closest walkable tile to `t` within `max_r` tiles (spiral search).
    pub fn nearest_walkable(&self, t: Tile, max_r: i32) -> Option<Tile> {
        if self.walkable(t.0, t.1) {
            return Some(t);
        }
        for r in 1..=max_r {
            let mut best: Option<(i32, Tile)> = None;
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let c = (t.0 + dx, t.1 + dy);
                    if self.walkable(c.0, c.1) {
                        let d = dx * dx + dy * dy;
                        if best.is_none_or(|(bd, _)| d < bd) {
                            best = Some((d, c));
                        }
                    }
                }
            }
            if let Some((_, c)) = best {
                return Some(c);
            }
        }
        None
    }
}

#[derive(Copy, Clone)]
struct Node {
    f: f32,
    idx: u32,
}
impl PartialEq for Node {
    fn eq(&self, o: &Self) -> bool {
        self.f == o.f
    }
}
impl Eq for Node {}
impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Node {
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.partial_cmp(&self.f).unwrap_or(Ordering::Equal)
    }
}

/// Reusable A* scratch space.
pub struct PathFinder {
    g: Vec<f32>,
    parent: Vec<u32>,
    stamp: Vec<u32>,
    closed: Vec<u32>,
    gen: u32,
    heap: BinaryHeap<Node>,
}

const SQRT2: f32 = std::f32::consts::SQRT_2;

fn octile(a: Tile, b: Tile) -> f32 {
    let dx = (a.0 - b.0).abs() as f32;
    let dy = (a.1 - b.1).abs() as f32;
    dx.max(dy) + (SQRT2 - 1.0) * dx.min(dy)
}

impl PathFinder {
    pub fn new(n: usize) -> PathFinder {
        PathFinder {
            g: vec![0.0; n],
            parent: vec![0; n],
            stamp: vec![0; n],
            closed: vec![0; n],
            gen: 0,
            heap: BinaryHeap::new(),
        }
    }

    /// Path of tiles from `start` (exclusive) to `goal` (inclusive). If the goal
    /// can't be reached, returns a path to the explored tile nearest to it.
    pub fn find(&mut self, grid: &Grid, start: Tile, goal: Tile, max_nodes: usize) -> Vec<Tile> {
        if start == goal {
            return Vec::new();
        }
        self.gen = self.gen.wrapping_add(1);
        if self.gen == 0 {
            self.stamp.iter_mut().for_each(|s| *s = 0);
            self.closed.iter_mut().for_each(|s| *s = 0);
            self.gen = 1;
        }
        let gen = self.gen;
        let w = grid.w;
        let id = |t: Tile| (t.1 * w + t.0) as u32;
        let tile = |i: u32| ((i as i32) % w, (i as i32) / w);

        self.heap.clear();
        let s = id(start);
        self.g[s as usize] = 0.0;
        self.stamp[s as usize] = gen;
        self.parent[s as usize] = s;
        self.heap.push(Node { f: octile(start, goal), idx: s });

        let mut best = (octile(start, goal), s);
        let mut expanded = 0;
        let goal_id = id(goal);

        while let Some(Node { idx, .. }) = self.heap.pop() {
            if self.closed[idx as usize] == gen {
                continue;
            }
            self.closed[idx as usize] = gen;
            let t = tile(idx);
            let h = octile(t, goal);
            if h < best.0 {
                best = (h, idx);
            }
            if idx == goal_id {
                break;
            }
            expanded += 1;
            if expanded > max_nodes {
                break;
            }
            let g0 = self.g[idx as usize];
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let n = (t.0 + dx, t.1 + dy);
                if !grid.walkable(n.0, n.1) {
                    continue;
                }
                let diag = dx != 0 && dy != 0;
                if diag && (!grid.walkable(t.0 + dx, t.1) || !grid.walkable(t.0, t.1 + dy)) {
                    continue; // no corner cutting
                }
                let ni = id(n);
                if self.closed[ni as usize] == gen {
                    continue;
                }
                let step = if diag { SQRT2 } else { 1.0 };
                let g = g0 + step * grid.cost[ni as usize];
                if self.stamp[ni as usize] != gen || g < self.g[ni as usize] {
                    self.stamp[ni as usize] = gen;
                    self.g[ni as usize] = g;
                    self.parent[ni as usize] = idx;
                    self.heap.push(Node { f: g + octile(n, goal) * 0.8, idx: ni });
                }
            }
        }

        let mut path = Vec::new();
        let mut cur = best.1;
        while cur != s {
            path.push(tile(cur));
            cur = self.parent[cur as usize];
        }
        path.reverse();
        path
    }
}
