//! Frozen penguins in the new core (DESIGN.md section 0). Freezing ends
//! ownership; the frozen stay where they fell as blocks of ice that stop
//! walking and snowballs; left alone they thaw out wild, and a wild penguin
//! joins whichever tribe stands with it.

use super::*;

/// Time for a frozen penguin to thaw out by itself.
pub const THAW_TICKS: u16 = secs(120.0);
/// How close a tribe's penguin must stand to persuade a wild one...
pub const PERSUADE_RANGE: f32 = 1.5;
/// ...and for how long.
pub const PERSUADE_TICKS: u16 = secs(2.0);
/// Persuasion is checked this often.
const PERSUADE_EVERY: u32 = 5;

impl Game {
    /// A penguin has just frozen: with the new core it belongs to nobody
    /// now, and its tile turns into a block of ice.
    pub(super) fn on_frozen(&mut self, v: u16) {
        // Its camp slot is free again: the camp calls someone else in.
        self.penguins[v as usize].camp = None;
        self.penguins[v as usize].fishery = None;
        let p = &mut self.penguins[v as usize];
        p.thaw_t = 0;
        if p.owner != NEUTRAL {
            let (owner, chief) = (p.owner, p.kind == Kind::Chieftain);
            p.was = owner;
            p.owner = NEUTRAL;
            // A tribe that loses its hero raises another later.
            if chief {
                self.chief_down(v);
            }
        }
        let p = &mut self.penguins[v as usize];
        if p.owner == NEUTRAL {
            self.block_frozen(v);
        }
    }

    /// Would penguin `v` be a second Chieftain for `tribe`?
    fn chief_is_surplus(&self, v: u16, tribe: u8) -> bool {
        true
            && tribe != NEUTRAL
            && self.penguins[v as usize].kind == Kind::Chieftain
            && self.has_chieftain(tribe)
    }

    /// Is this tile held by a frozen penguin?
    pub fn frozen_at(&self, x: i32, y: i32) -> bool {
        self.map.in_bounds(x, y) && self.frozen_on_tile[self.map.idx(x, y)] > 0
    }

    fn block_frozen(&mut self, v: u16) {
        let (x, y) = self.penguins[v as usize].pos.tile();
        if !self.map.in_bounds(x, y) || self.penguins[v as usize].blocks.is_some() {
            return;
        }
        let i = self.map.idx(x, y);
        self.penguins[v as usize].blocks = Some(i);
        self.frozen_on_tile[i] += 1;
        if self.frozen_on_tile[i] > 1 {
            return;
        }
        self.frozen_walk[i] = self.grid.walk[i];
        self.grid.walk[i] = false;
        // Anyone else standing there steps aside.
        for q in &mut self.penguins {
            if q.id == v || q.inside.is_some() || q.act == Act::Frozen || q.pos.tile() != (x, y) {
                continue;
            }
            if let Some((nx, ny)) = self.grid.nearest_walkable((x, y), 4) {
                q.pos = v2(nx as f32 + 0.5, ny as f32 + 0.5);
                q.path.clear();
                q.goal = None;
            }
        }
    }

    /// Set whether tile `i` can be walked on, for its own reasons (a
    /// building, a tree). A block of ice on it keeps it shut until it thaws.
    pub(super) fn set_walk(&mut self, i: usize, walk: bool) {
        if self.frozen_on_tile[i] > 0 {
            self.frozen_walk[i] = walk;
        } else {
            self.grid.walk[i] = walk;
        }
    }

    pub(super) fn unblock_frozen(&mut self, v: u16) {
        // Release the tile this penguin blocked, wherever it is now.
        let Some(i) = self.penguins[v as usize].blocks.take() else { return };
        self.frozen_on_tile[i] = self.frozen_on_tile[i].saturating_sub(1);
        if self.frozen_on_tile[i] == 0 {
            self.grid.walk[i] = self.frozen_walk[i];
        }
    }

    /// A frozen penguin wakes up, owned by `by` (or wild for [`NEUTRAL`]).
    pub(super) fn thaw(&mut self, v: u16, by: u8) {
        self.unblock_frozen(v);
        // A tribe has one Chieftain: a second one thaws out wild instead.
        let by = if self.chief_is_surplus(v, by) { NEUTRAL } else { by };
        let home = self.nearest_igloo_of(by, self.penguins[v as usize].pos);
        let p = &mut self.penguins[v as usize];
        p.act = Act::Idle;
        p.act_t = 0;
        p.thaw_t = 0;
        p.warmth = p.warmth.max(3);
        p.cold = false;
        p.order = Order::None;
        p.goal = None;
        p.path.clear();
        p.target = None;
        p.owner = by;
        p.wild = by == NEUTRAL;
        p.persuade = None;
        if let Some(h) = home {
            p.home = h;
        }
        // With armies whatever it was is lost in the ice: it always wakes a
        // plain worker (DESIGN.md 0.2), to be drafted or trained again.
        if self.penguins[v as usize].kind != Kind::Worker {
            self.retrain(v, Kind::Worker);
        }
        self.events.push(Event::Thawed { penguin: v, by });
    }

    /// The player's igloo nearest to `at`.
    fn nearest_igloo_of(&self, player: u8, at: Vec2) -> Option<u8> {
        if player == NEUTRAL {
            return None;
        }
        self.igloos
            .iter()
            .filter(|ig| ig.owner == player)
            .min_by(|a, b| a.door.dist2(at).partial_cmp(&b.door.dist2(at)).unwrap())
            .map(|ig| ig.id)
    }

    /// How much closer to thawing out a frozen penguin gets each tick: the
    /// warm seasons hurry it, the polar night stops it.
    fn thaw_rate(&self) -> u16 {
        match self.weather.season {
            Season::Thaw | Season::Midsummer => 2,
            Season::PolarNight => 0,
            Season::DeepWinter => 1,
        }
    }

    /// Ownerless frozen penguins thaw out wild after a while.
    pub(super) fn update_frozen(&mut self) {
        let rate = self.thaw_rate();
        for i in 0..self.penguins.len() {
            let p = &mut self.penguins[i];
            if p.act != Act::Frozen || p.owner != NEUTRAL || p.carried_by.is_some() {
                continue;
            }
            p.thaw_t = p.thaw_t.saturating_add(rate);
            if p.thaw_t >= THAW_TICKS {
                self.thaw(i as u16, NEUTRAL);
            }
        }
    }

    /// A wild penguin joins the tribe whose penguin keeps it company.
    pub(super) fn update_wild(&mut self) {
        let persuade = self.tick.is_multiple_of(PERSUADE_EVERY);
        let mut near = std::mem::take(&mut self.scratch);
        for i in 0..self.penguins.len() {
            let p = &self.penguins[i];
            if !p.wild || p.act == Act::Frozen || p.inside.is_some() {
                continue;
            }
            let pos = p.pos;
            self.buckets.query(pos, PERSUADE_RANGE, &mut near);
            // A tribe penguin in reach persuades the wild one.
            let mut friend: Option<(f32, u8)> = None;
            for &o in &near {
                let q = &self.penguins[o as usize];
                if q.owner == NEUTRAL || q.wild || q.act == Act::Frozen || q.inside.is_some() {
                    continue;
                }
                let d = q.pos.dist(pos);
                if d <= PERSUADE_RANGE && friend.is_none_or(|f| d < f.0) {
                    friend = Some((d, q.owner));
                }
            }
            if !persuade {
                continue;
            }
            let by = friend.map(|f| f.1);
            let p = &mut self.penguins[i];
            p.persuade = match (by, p.persuade) {
                (Some(by), Some((who, t))) if who == by => Some((by, t + PERSUADE_EVERY as u16)),
                (Some(by), _) => Some((by, PERSUADE_EVERY as u16)),
                (None, _) => None,
            };
            if let Some((by, t)) = p.persuade {
                if t >= PERSUADE_TICKS && self.chief_is_surplus(i as u16, by) {
                    self.penguins[i].persuade = None;
                } else if t >= PERSUADE_TICKS {
                    let home = self.nearest_igloo_of(by, pos);
                    let p = &mut self.penguins[i];
                    p.owner = by;
                    p.wild = false;
                    p.persuade = None;
                    if let Some(h) = home {
                        p.home = h;
                    }
                    self.events.push(Event::Persuaded { penguin: i as u16, by });
                }
            }
        }
        self.scratch = near;
    }

}
