//! New penguins come from the Great Sea (the `armies` rule). A slot the tribe wants filled
//! (fishing, a camp's or a Bunkhouse's) with no idle worker to call pays
//! for a newcomer, who swims in to the beach nearest it and walks over to
//! take the place. The fish is the only limit.

use super::*;

/// Fish for a newcomer from the sea.
pub const SEA_RECRUIT_FISH: u16 = 4;
/// The sea is slow to answer: calls go out only one second in this many
/// (the slots are filled once a second).
pub const SEA_CALL_SECONDS: u32 = 4;

/// Water in bodies smaller than this is a fishing pond, not a sea.
pub const POND_TILES: usize = 30;

impl Game {
    /// Bank tiles beside any body of water of at least `min` tiles.
    pub(super) fn water_bodies(&self, min: usize) -> std::collections::HashSet<(i32, i32)> {
        let m = &self.map;
        let mut seen = vec![false; (m.w * m.h) as usize];
        let mut banks = std::collections::HashSet::new();
        for y in 0..m.h {
            for x in 0..m.w {
                if seen[m.idx(x, y)] || m.ground_at(x, y) != Ground::Water {
                    continue;
                }
                let mut body = vec![(x, y)];
                seen[m.idx(x, y)] = true;
                let mut k = 0;
                while k < body.len() {
                    let (bx, by) = body[k];
                    k += 1;
                    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let (nx, ny) = (bx + dx, by + dy);
                        if m.in_bounds(nx, ny) && !seen[m.idx(nx, ny)] && m.ground_at(nx, ny) == Ground::Water {
                            seen[m.idx(nx, ny)] = true;
                            body.push((nx, ny));
                        }
                    }
                }
                if body.len() >= min {
                    for (bx, by) in body {
                        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                            banks.insert((bx + dx, by + dy));
                        }
                    }
                }
            }
        }
        banks
    }

    /// Beaches on the open sea (water bodies too big to be a pond); on a map
    /// with none, the walkable ground along its edges.
    pub(super) fn sea_beaches(&self) -> Vec<Vec2> {
        let big = self.water_bodies(POND_TILES);
        let beaches: Vec<Vec2> = self.shores.iter().filter(|s| big.contains(&s.tile)).map(|s| s.pos).collect();
        if !beaches.is_empty() {
            return beaches;
        }
        let (w, h) = (self.map.w, self.map.h);
        let edge = (0..w).flat_map(|x| [(x, 0), (x, h - 1)]).chain((0..h).flat_map(|y| [(0, y), (w - 1, y)]));
        edge.filter(|&(x, y)| self.grid.walkable(x, y)).map(|(x, y)| v2(x as f32 + 0.5, y as f32 + 0.5)).collect()
    }

    /// Is now a time `player` may call the sea for a slot? (Not while any
    /// worker of the tribe is free to take it.)
    pub(super) fn may_call_sea(&self, player: u8) -> bool {
        true
            && (self.tick / crate::TICK_RATE).is_multiple_of(SEA_CALL_SECONDS)
            && !self.penguins.iter().any(|p| p.owner == player && p.kind == Kind::Worker && p.order == Order::None && p.fishery.is_none() && p.act != Act::Frozen && p.carried_by.is_none())
    }

    /// Pay for a newcomer of `player`'s to swim in to the beach nearest `to`
    /// (by walking). Its new id, or `None` if the tribe can't pay or no beach
    /// can be walked from.
    pub(super) fn call_from_sea(&mut self, player: u8, to: Vec2, extra_fish: u16) -> Option<u16> {
        if self.fish_of(player) < (SEA_RECRUIT_FISH + extra_fish) as u32 {
            return None;
        }
        // The nearest beach by walking, not across the water.
        let walk = self.grid.distances(to.tile());
        let beach = self
            .sea
            .iter()
            .copied()
            .filter_map(|b| {
                let (x, y) = b.tile();
                let d = walk[self.map.idx(x, y)];
                d.is_finite().then_some((d, b))
            })
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap())?
            .1;
        let home = self.igloos.iter().filter(|ig| ig.owner == player).min_by(|a, b| a.door.dist2(to).partial_cmp(&b.door.dist2(to)).unwrap())?.id;
        self.spend_fish(player, SEA_RECRUIT_FISH);
        let id = self.spawn_penguin(home, Kind::Worker);
        let p = &mut self.penguins[id as usize];
        p.pos = beach;
        p.facing = (to - beach).angle();
        self.events.push(Event::SwamIn { penguin: id });
        Some(id)
    }
}
