//! Courage and prisoners.
//!
//! Seeing friends frozen shakes a penguin's courage; seeing enemies frozen
//! steadies it. Huddled groups take shocks better. A penguin with no
//! courage left flees to safety and won't take orders until it calms down.
//!
//! Nobody dies: a frozen penguin can be picked up and carried to an igloo,
//! where it changes sides after a while unless its friends take the igloo
//! back first or it's let go.

use super::*;

pub const MAX_COURAGE: u8 = 10;
/// Fleeing penguins come back to the fight once they're this brave again.
pub const RALLY_COURAGE: u8 = 6;
/// How far a freezing is seen by other penguins.
pub const SHOCK_RADIUS: f32 = 5.0;
/// A penguin with this many friends close by is huddled and loses less courage.
pub const HUDDLE_FRIENDS: usize = 4;
pub const HUDDLE_RADIUS: f32 = 2.5;
/// A Chieftain this close steadies a penguin.
pub const CHIEF_BESIDE_RADIUS: f32 = 4.5;
/// Prisoners change sides after this long in an igloo.
pub const CONVERT_TICKS: u16 = secs(20.0);
/// Carrying a frozen penguin slows you down this much.
pub const CARRY_SPEED: f32 = 0.6;

impl Game {
    /// Penguin `v` of `team` froze at `at`: friends nearby lose heart, enemies take it.
    pub(super) fn morale_shock(&mut self, at: Vec2, team: u8) {
        let mut near = std::mem::take(&mut self.scratch);
        self.buckets.query(at, SHOCK_RADIUS, &mut near);
        for &o in &near {
            let q = &self.penguins[o as usize];
            if q.inside.is_some() || q.act == Act::Frozen || q.owner == NEUTRAL || q.pos.dist(at) > SHOCK_RADIUS {
                continue;
            }
            if self.team(q.owner) == team {
                let loss = if q.kind == Kind::Worker { 4 } else { 3 };
                let loss = if self.huddled(o) { loss / 2 } else { loss };
                // A Chieftain close by steadies them.
                let loss = if self.chief_beside(o as usize) { loss / 2 } else { loss };
                // A Chieftain's presence steadies the whole tribe around it (and itself, a little).
                let loss = if q.kind == Kind::Chieftain || self.chief_near(o as usize) { loss / 2 } else { loss };
                self.lose_courage(o, loss);
            } else {
                let q = &mut self.penguins[o as usize];
                q.courage = (q.courage + 1).min(MAX_COURAGE);
            }
        }
        self.scratch = near;
    }

    fn huddled(&self, i: u16) -> bool {
        let p = &self.penguins[i as usize];
        let team = self.team(p.owner);
        let mut n = 0;
        // Small fixed neighbourhood: cheap enough to scan directly.
        let (x0, y0) = p.pos.tile();
        for q in &self.penguins {
            if q.id != i && q.inside.is_none() && q.act != Act::Frozen && self.team(q.owner) == team {
                let (x, y) = q.pos.tile();
                if (x - x0).abs() <= 3 && (y - y0).abs() <= 3 && q.pos.dist(p.pos) <= HUDDLE_RADIUS {
                    n += 1;
                    if n >= HUDDLE_FRIENDS {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// A Chieftain of penguin `i`'s side is close by.
    pub(super) fn chief_beside(&self, i: usize) -> bool {
        let p = &self.penguins[i];
        if p.inside.is_some() {
            return false;
        }
        let team = self.team(p.owner);
        let mut near = Vec::new();
        self.buckets.query(p.pos, CHIEF_BESIDE_RADIUS, &mut near);
        near.iter().any(|&o| {
            let q = &self.penguins[o as usize];
            q.kind == Kind::Chieftain && q.act != Act::Frozen && self.team(q.owner) == team && q.pos.dist(p.pos) <= CHIEF_BESIDE_RADIUS
        })
    }

    pub(super) fn lose_courage(&mut self, i: u16, n: u8) {
        let p = &mut self.penguins[i as usize];
        p.courage = p.courage.saturating_sub(n);
        if p.courage == 0 && !p.routed {
            p.routed = true;
            p.order = Order::None;
            p.goal = None;
            p.path.clear();
            p.target = None;
            if matches!(p.act, Act::Throw | Act::Scoop | Act::Fish | Act::Mine | Act::Chop | Act::Build) {
                p.act = Act::Idle;
            }
            self.drop_captive(i);
            self.events.push(Event::Routed { penguin: i });
        }
    }

    /// Once a second: courage comes back, faster somewhere safe.
    pub(super) fn update_courage(&mut self) {
        for i in 0..self.penguins.len() {
            let p = &self.penguins[i];
            if p.courage >= MAX_COURAGE || p.act == Act::Frozen || p.owner == NEUTRAL {
                continue;
            }
            let safe = p.inside.is_some()
                || p.pos.dist(self.igloos[p.home as usize].door) < 6.0
                || self.nearest_building(p.owner, BuildKind::Sauna, p.pos).is_some_and(|(d, _)| d <= SAUNA_RADIUS);
            let calm = safe || self.chief_beside(i) || self.chief_near(i) || self.tick.is_multiple_of(3 * crate::TICK_RATE);
            let p = &mut self.penguins[i];
            if calm {
                p.courage += 1;
            }
            if p.routed && p.courage >= RALLY_COURAGE {
                p.routed = false;
            }
        }
    }

    // ---------------------------------------------------------------------
    // Prisoners

    /// Could `p` be picked up by someone on `team`?
    pub fn capturable(&self, p: &Penguin, team: u8) -> bool {
        p.act == Act::Frozen && p.inside.is_none() && p.carried_by.is_none() && p.owner != NEUTRAL && self.team(p.owner) != team
    }

    pub(super) fn order_capture(&mut self, player: u8, units: Vec<u16>, penguin: u16) {
        let team = self.team(player);
        let Some(target) = self.penguins.get(penguin as usize) else { return };
        if !self.capturable(target, team) {
            return;
        }
        let at = target.pos;
        let units: Vec<u16> = units.into_iter().filter(|&u| self.controllable(player, u) && self.penguins[u as usize].carrying.is_none()).collect();
        // The closest one carries; the rest escort.
        let Some(&carrier) = units.iter().min_by(|&&a, &&b| {
            self.penguins[a as usize].pos.dist2(at).partial_cmp(&self.penguins[b as usize].pos.dist2(at)).unwrap()
        }) else {
            return;
        };
        self.give_order(carrier, Order::Capture { penguin });
        let escort: Vec<u16> = units.into_iter().filter(|&u| u != carrier).collect();
        if !escort.is_empty() {
            self.command(player, Command::Move { units: escort, target: at, run: false, queue: false });
        }
    }

    /// Carrier job: fetch the frozen penguin, then take it to the nearest igloo.
    pub(super) fn capture_job(&mut self, i: usize, penguin: u16) {
        let me = self.penguins[i].id;
        let team = self.team(self.penguins[i].owner);
        let sled = self.penguins[i].kind == Kind::Sledder;
        if self.penguins[i].carrying.is_none() {
            let target = &self.penguins[penguin as usize];
            if !(self.capturable(target, team) || (sled && self.haulable(target, team))) {
                self.penguins[i].order = Order::None;
                return;
            }
            let at = target.pos;
            if self.travel(i, at, 0.6) {
                // Off the tile it was blocking, onto the sled.
                self.unblock_frozen(penguin);
                self.penguins[i].carrying = Some(penguin);
                let c = &mut self.penguins[penguin as usize];
                c.carried_by = Some(me);
                self.events.push(Event::PickedUp { penguin, by: me });
            }
            return;
        }
        let owner = self.penguins[i].owner;
        let pos = self.penguins[i].pos;
        let Some(ig) = self
            .igloos
            .iter()
            .filter(|ig| ig.owner == owner && ig.takeover.is_none())
            .min_by(|a, b| a.door.dist2(pos).partial_cmp(&b.door.dist2(pos)).unwrap())
            .map(|ig| ig.id)
        else {
            self.drop_captive(me);
            self.penguins[i].order = Order::None;
            return;
        };
        let door = self.igloos[ig as usize].door;
        if self.travel(i, door, 0.6) {
            // A Sled's load thaws on the doorstep and joins the tribe.
            if sled && self.penguins[penguin as usize].owner == NEUTRAL {
                self.haul_home(i, penguin);
                return;
            }
            self.imprison(penguin, ig);
            self.penguins[i].carrying = None;
            self.penguins[i].order = Order::None;
        }
    }

    /// Keep carried penguins in the carrier's arms.
    pub(super) fn follow_carrier(&mut self, i: usize) -> bool {
        let Some(c) = self.penguins[i].carried_by else { return false };
        let carrier = &self.penguins[c as usize];
        if carrier.carrying != Some(self.penguins[i].id) {
            self.penguins[i].carried_by = None;
            return false;
        }
        // In the arms, or riding on a Sled's sled behind it.
        let back = if carrier.kind == Kind::Sledder { 0.85 } else { 0.15 };
        let pos = carrier.pos - Vec2::from_angle(carrier.facing) * back;
        let facing = carrier.facing;
        let p = &mut self.penguins[i];
        p.pos = pos;
        p.facing = facing;
        // The cold keeps: no thawing while being carried.
        p.act_t = 0;
        true
    }

    /// The carrier `c` lets go of whoever it's holding.
    pub(super) fn drop_captive(&mut self, c: u16) {
        if let Some(v) = self.penguins[c as usize].carrying.take() {
            let pos = self.penguins[c as usize].pos;
            let p = &mut self.penguins[v as usize];
            p.carried_by = None;
            if self.grid.walkable_at(pos) {
                p.pos = pos;
            }
            self.events.push(Event::Dropped { penguin: v });
        }
    }

    fn imprison(&mut self, v: u16, ig: u8) {
        let p = &mut self.penguins[v as usize];
        p.carried_by = None;
        p.inside = Some(ig);
        p.act = Act::Inside;
        p.act_t = 0;
        p.order = Order::None;
        p.goal = None;
        p.path.clear();
        p.target = None;
        p.prison_t = CONVERT_TICKS;
        if let Some(pw) = p.power.take() {
            self.unequip(v, pw);
            self.powerups[pw as usize].pos = self.igloos[ig as usize].door;
        }
        self.igloos[ig as usize].prisoners.push(v);
        self.events.push(Event::Imprisoned { penguin: v, igloo: ig });
    }

    /// Prisoners count down to changing sides.
    pub(super) fn update_prisoners(&mut self, ig: usize) {
        let owner = self.igloos[ig].owner;
        let mut k = 0;
        while k < self.igloos[ig].prisoners.len() {
            let v = self.igloos[ig].prisoners[k];
            let p = &mut self.penguins[v as usize];
            p.prison_t = p.prison_t.saturating_sub(1);
            if p.prison_t > 0 || owner == NEUTRAL {
                k += 1;
                continue;
            }
            self.igloos[ig].prisoners.swap_remove(k);
            let p = &mut self.penguins[v as usize];
            p.owner = owner;
            p.home = ig as u8;
            p.cold = false;
            p.routed = false;
            p.courage = RALLY_COURAGE;
            p.warmth = p.max_warmth;
            p.carry = Carry::None;
            self.igloos[ig].occupants.push(v);
            self.events.push(Event::Converted { penguin: v, to: owner });
        }
    }

    /// Let prisoners go: they walk out cold, back to their own side.
    pub(super) fn release_prisoners(&mut self, ig: usize, only_team: Option<u8>) {
        let prisoners = std::mem::take(&mut self.igloos[ig].prisoners);
        let door = self.igloos[ig].door;
        for v in prisoners {
            if only_team.is_some_and(|t| self.team(self.penguins[v as usize].owner) != t) {
                self.igloos[ig].prisoners.push(v);
                continue;
            }
            let jitter = v2(self.rng.range(-0.4, 0.4), self.rng.range(0.0, 0.4));
            let p = &mut self.penguins[v as usize];
            p.inside = None;
            p.pos = door + jitter;
            p.act = Act::Idle;
            p.warmth = p.warmth.max(2);
            p.cold = true;
            p.prison_t = 0;
            self.events.push(Event::Released { penguin: v });
        }
    }

    pub(super) fn release(&mut self, player: u8, igloo: u8) {
        if self.igloos.get(igloo as usize).is_some_and(|ig| ig.owner == player) {
            self.release_prisoners(igloo as usize, None);
        }
    }
}
