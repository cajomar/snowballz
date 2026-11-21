//! The ground changes as it's used: snow wears to ice and comes back,
//! trees get chopped down.

use super::*;

/// The parts of the map that change during a game.
pub struct Terrain {
    /// Layers of snow per tile; 0 means bare ice.
    pub snow: Vec<u8>,
    /// Snow each tile recovers to (0 on the map's own ice).
    pub(super) snow_max: Vec<u8>,
    /// Loads of wood left in each tree.
    pub(super) wood: Vec<u8>,
    /// Foot traffic since the tile last lost a layer.
    pub(super) traffic: Vec<u16>,
    /// Tiles below their natural snow depth, for snowfall.
    pub(super) worn: Vec<u32>,
    pub(super) is_worn: Vec<bool>,
    /// Tiles changed this tick, and every tile changed since the start.
    pub dirty: Vec<u32>,
    is_dirty: Vec<bool>,
    pub touched: Vec<u32>,
    is_touched: Vec<bool>,
    /// Felled trees and the tick each grows back (with armies).
    pub(super) regrow: std::collections::VecDeque<(u32, u32)>,
}

impl Terrain {
    pub(super) fn new(map: &Map) -> Terrain {
        let n = (map.w * map.h) as usize;
        let snow: Vec<u8> = map.ground.iter().map(|&g| initial_snow(g)).collect();
        Terrain {
            snow_max: snow.clone(),
            snow,
            wood: map.obstacle.iter().map(|&o| if o == Obstacle::Tree { TREE_WOOD } else { 0 }).collect(),
            traffic: vec![0; n],
            worn: Vec::new(),
            is_worn: vec![false; n],
            dirty: Vec::new(),
            is_dirty: vec![false; n],
            touched: Vec::new(),
            is_touched: vec![false; n],
            regrow: Default::default(),
        }
    }

    pub(super) fn mark(&mut self, i: usize) {
        if !self.is_dirty[i] {
            self.is_dirty[i] = true;
            self.dirty.push(i as u32);
        }
        if !self.is_touched[i] {
            self.is_touched[i] = true;
            self.touched.push(i as u32);
        }
    }

    pub(super) fn clear_dirty(&mut self) {
        for &i in &self.dirty {
            self.is_dirty[i as usize] = false;
        }
        self.dirty.clear();
    }
}

/// Snow depth of a tile with this ground at the start of a game.
pub fn initial_snow(g: Ground) -> u8 {
    if g == Ground::Snow {
        SNOW_DEPTH
    } else {
        0
    }
}

impl Game {
    /// Layers of snow under a world position.
    pub fn snow_at(&self, p: Vec2) -> u8 {
        let (x, y) = p.tile();
        if self.map.in_bounds(x, y) {
            self.terrain.snow[self.map.idx(x, y)]
        } else {
            0
        }
    }

    pub(super) fn set_snow(&mut self, i: usize, s: u8) {
        let t = &mut self.terrain;
        if t.snow[i] == s {
            return;
        }
        let was_bare = t.snow[i] == 0;
        t.snow[i] = s;
        t.traffic[i] = 0;
        if s < t.snow_max[i] && !t.is_worn[i] {
            t.is_worn[i] = true;
            t.worn.push(i as u32);
        }
        t.mark(i);
        if was_bare != (s == 0) && self.map.ground[i] != Ground::Water {
            self.map.ground[i] = if s == 0 { Ground::Ice } else { Ground::Snow };
            self.grid.cost[i] = if s == 0 { 0.75 } else { 1.0 };
        }
    }

    /// Feet pack the snow down; busy routes turn into ice roads.
    pub(super) fn trample(&mut self, x: i32, y: i32) {
        if !self.map.in_bounds(x, y) {
            return;
        }
        let i = self.map.idx(x, y);
        let s = self.terrain.snow[i];
        if s == 0 {
            return;
        }
        self.terrain.traffic[i] += self.trample_rate();
        if self.terrain.traffic[i] >= TRAMPLE_TICKS {
            self.set_snow(i, s - 1);
        }
    }

    /// Fresh snow slowly covers worn ground again.
    pub(super) fn snowfall(&mut self) {
        let chance = SNOWFALL_CHANCE * self.snowfall_rate();
        let mut k = 0;
        while k < self.terrain.worn.len() {
            let i = self.terrain.worn[k] as usize;
            let (s, max) = (self.terrain.snow[i], self.terrain.snow_max[i]);
            if s >= max {
                self.terrain.is_worn[i] = false;
                self.terrain.worn.swap_remove(k);
                continue;
            }
            if self.rng.chance(chance) {
                self.set_snow(i, s + 1);
            }
            k += 1;
        }
    }

    pub(super) fn fell_tree(&mut self, i: usize) {
        self.map.obstacle[i] = Obstacle::Stump;
        self.set_walk(i, true);
        self.terrain.mark(i);
        self.terrain.regrow.push_back((i as u32, self.tick + TREE_REGROW_TICKS));
    
        let tile = ((i as i32) % self.map.w, (i as i32) / self.map.w);
        self.events.push(Event::TreeFelled { tile });
    }

    /// With armies felled trees grow back (DESIGN.md 0.2), once nothing
    /// stands on the stump: no penguin, building or igloo, and not on a
    /// bank or beside a fishing or crystal spot. A stump that is in use
    /// tries again a little later.
    pub(super) fn regrow_trees(&mut self) {
        while let Some(&(i, at)) = self.terrain.regrow.front() {
            if at > self.tick {
                break;
            }
            self.terrain.regrow.pop_front();
            let i = i as usize;
            if self.map.obstacle[i] != Obstacle::Stump {
                continue;
            }
            let (x, y) = ((i as i32) % self.map.w, (i as i32) / self.map.w);
            let c = v2(x as f32 + 0.5, y as f32 + 0.5);
            let busy = !self.grid.walkable(x, y)
                || self.building_at(x, y).is_some()
                || self.igloo_on(x, y).is_some()
                || (-1..=1).any(|dy| (-1..=1).any(|dx| self.map.ground_at(x + dx, y + dy) == Ground::Water))
                || self.penguins.iter().any(|p| p.pos.dist(c) < 1.2)
                || self.resources.iter().any(|r| r.pos.dist(c) < 2.0);
            if busy {
                self.terrain.regrow.push_back((i as u32, self.tick + TREE_RETRY_TICKS));
                continue;
            }
            self.map.obstacle[i] = Obstacle::Tree;
            self.terrain.wood[i] = TREE_WOOD;
            self.set_walk(i, false);
            self.terrain.mark(i);
        }
    }

    /// Centre of the closest walkable tile with snow on it.
    pub(super) fn nearest_snow(&self, from: Vec2, r: i32) -> Option<Vec2> {
        let (fx, fy) = from.tile();
        let mut best: Option<(i32, (i32, i32))> = None;
        for y in fy - r..=fy + r {
            for x in fx - r..=fx + r {
                if !self.grid.walkable(x, y) || self.terrain.snow[self.map.idx(x, y)] == 0 {
                    continue;
                }
                let d = (x - fx).pow(2) + (y - fy).pow(2);
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, (x, y)));
                }
            }
        }
        best.map(|(_, (x, y))| v2(x as f32 + 0.5, y as f32 + 0.5))
    }

}
