//! Watchtowers: a fortress's flanking posts. Up to four throwers stand on
//! the roof, throw farther from up there, and cannot be hit while the
//! tower stands. Enemies have to bring the tower down to reach them.

use super::*;

/// Where on the roof each of the four slots stands, from the tower's centre.
const SLOTS: [(f32, f32); TOWER_SLOTS] = [(-0.42, -0.42), (0.42, -0.42), (-0.42, 0.42), (0.42, 0.42)];
/// A thrower has to be this close to the tower's edge to climb up.
const CLIMB_REACH: f32 = 0.9;

impl Game {
    /// True if a friendly watchtower's roof is where `p` stands.
    pub(super) fn on_tower(&self, owner: u8, p: Vec2) -> bool {
        let team = self.team(owner);
        self.buildings
            .iter()
            .any(|b| b.alive && b.done() && b.kind == BuildKind::Tower && self.team(b.owner) == team && b.center().dist(p) < 0.8)
    }

    /// Penguins standing on this tower's roof.
    pub fn garrison(&self, building: u16) -> impl Iterator<Item = &Penguin> {
        self.penguins.iter().filter(move |p| p.post == Some(building))
    }

    /// The roof slot nobody holds, if any.
    fn free_slot(&self, building: u16) -> Option<usize> {
        let b = &self.buildings[building as usize];
        let c = b.center();
        (0..TOWER_SLOTS).find(|&s| {
            let at = c + v2(SLOTS[s].0, SLOTS[s].1);
            !self.garrison(building).any(|p| p.pos.dist(at) < 0.1)
        })
    }

    /// Send throwers to man a finished watchtower of their own tribe.
    pub(super) fn order_garrison(&mut self, player: u8, units: Vec<u16>, building: u16) {
        let Some(b) = self.buildings.get(building as usize) else { return };
        if !(b.alive && b.done() && b.kind == BuildKind::Tower && self.team(b.owner) == self.team(player)) {
            return;
        }
        for u in units {
            if self.controllable(player, u) && self.penguins[u as usize].kind.fights() {
                self.give_order(u, Order::Garrison { building });
            }
        }
    }

    /// Throwers' job: walk to the tower, then climb onto the roof.
    pub(super) fn garrison_job(&mut self, i: usize, building: u16) {
        let pos = self.penguins[i].pos;
        let b = &self.buildings[building as usize];
        if !(b.alive && b.done() && b.kind == BuildKind::Tower && self.team(b.owner) == self.team(self.penguins[i].owner)) {
            self.penguins[i].order = Order::None;
            return;
        }
        if b.dist_to(pos) > CLIMB_REACH {
            match self.build_spot(building, pos) {
                Some(spot) => {
                    self.travel(i, spot, 0.3);
                }
                None => self.penguins[i].order = Order::None,
            }
            return;
        }
        let Some(slot) = self.free_slot(building) else {
            self.penguins[i].order = Order::None;
            return;
        };
        let at = self.buildings[building as usize].center() + v2(SLOTS[slot].0, SLOTS[slot].1);
        let p = &mut self.penguins[i];
        p.post = Some(building);
        p.pos = at;
        p.goal = None;
        p.path.clear();
        p.act = Act::Idle;
        p.facing = (at - self.buildings[building as usize].center()).angle();
    }

    /// Climb down to the ground beside the tower.
    pub(super) fn leave_post(&mut self, i: usize) {
        let Some(b) = self.penguins[i].post.take() else { return };
        let b = &self.buildings[b as usize];
        let c = b.center();
        let pos = self.penguins[i].pos;
        let away = (pos - c).norm();
        let mut at = c + away * (b.kind.size() as f32 / 2.0 + 0.6);
        if !self.grid.walkable_at(at) {
            at = self.build_spot(b.id, pos).unwrap_or(at);
        }
        let p = &mut self.penguins[i];
        p.pos = at;
        p.act = Act::Idle;
    }

    /// When a tower falls, everyone on it tumbles to the ground.
    pub(super) fn eject_garrison(&mut self, building: u16) {
        for i in 0..self.penguins.len() {
            if self.penguins[i].post == Some(building) {
                self.leave_post(i);
            }
        }
    }

    /// A posted thrower's turn: no walking, just throwing from the roof,
    /// with snowballs handed up every few seconds.
    pub(super) fn update_posted(&mut self, i: usize) {
        let p = &self.penguins[i];
        let alive = p.post.is_some_and(|b| self.buildings[b as usize].alive);
        if !alive || p.cold || p.routed || !matches!(p.order, Order::Garrison { .. }) {
            self.leave_post(i);
            return;
        }
        let cap = self.effects_of(p.owner).ammo(p.kind);
        let p = &mut self.penguins[i];
        p.cooldown = p.cooldown.saturating_sub(1);
        if p.snowballs < cap && (self.tick + i as u32).is_multiple_of(TOWER_RESTOCK) {
            p.snowballs += 1;
        }
        if p.snowballs > 0 && p.cooldown == 0 && (self.tick as usize + i).is_multiple_of(3) {
            if let Some(t) = self.find_target(i) {
                self.start_throw(i, t);
                return;
            }
            if let Some(s) = self.find_seal(i) {
                self.start_seal_throw(i, s);
                return;
            }
            if let Some(at) = self.find_structure(i) {
                self.start_structure_throw(i, at);
                return;
            }
        
        }
        self.penguins[i].act = Act::Idle;
    }
}
