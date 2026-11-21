//! Fighting smart, without being told (no stances to set):
//!
//! - **Picking a target.** Finish off whoever is nearly frozen; don't pile
//!   more on someone the snowballs already in the air will freeze; don't
//!   waste throws on someone behind a Shieldbearer's slab.
//! - **Dodging.** A penguin a snowball is thrown at sometimes sees it
//!   coming and sidesteps the direct hit (the splash may still catch it).
//! - **Stepping up.** A soldier in the ranks whose enemies are just out of
//!   reach steps a few paces out to throw, then goes back to its place.
//! - **The hurt fall back.** In the ranks (`armies.rs`), a thrower below
//!   two thirds warmth swaps to the back beside the Warmers, and a fresh one takes
//!   its place at the front.

use super::*;

/// The chance a penguin sees a snowball thrown at it in time to sidestep.
pub const DODGE_CHANCE: f32 = 0.3;
/// How long a sidestep lasts: long enough to clear a direct hit.
const DODGE_TICKS: u16 = secs(0.35);
/// A soldier steps at most this far out of its place in the ranks to get
/// an enemy in reach.
pub const STEP_UP: f32 = 3.0;

impl Game {
    /// How much worse (higher) a target scores than its distance alone says,
    /// for a thrower standing at `from`.
    pub(super) fn target_penalty(&self, q: &Penguin, from: Vec2, direct: u8) -> f32 {
        let left = q.warmth.saturating_sub(q.incoming);
        // Already as good as frozen: throw at someone else if there is anyone.
        let mut score = if left == 0 {
            8.0
        } else if left <= direct {
            // Nearly frozen: one more hit takes a thrower out of the fight.
            -2.0
        } else {
            0.0
        };
        score += 2.0 * left as f32 / q.max_warmth.max(1) as f32;
        // Behind a slab, a direct hit does little.
        if q.kind != Kind::Shield && self.shielded(q.id, from) {
            score += 2.0;
        }
        score
    }

    /// A snowball `ball` with `dmg` is on its way to penguin `t`, thrown from
    /// `from`: it may sidestep.
    pub(super) fn aimed_at(&mut self, ball: u32, t: u16, dmg: u8, from: Vec2) {
        self.aims.push((ball, t, dmg));
        let q = &mut self.penguins[t as usize];
        q.incoming = q.incoming.saturating_add(dmg);
        let ready = q.kind.soldier() && q.kind != Kind::Shield && matches!(q.act, Act::Idle | Act::Walk) && q.dodge_t == 0 && q.stun == 0 && q.inside.is_none() && q.post.is_none() && q.carried_by.is_none() && q.carrying.is_none() && q.owner != NEUTRAL;
        if !ready || !self.rng.chance(DODGE_CHANCE) {
            return;
        }
        let at = q.pos;
        let across = (at - from).norm();
        let side = if self.rng.below(2) == 0 { 1.0 } else { -1.0 };
        let dir = v2(-across.y, across.x) * side;
        // The other way if that side is blocked.
        let dir = if self.grid.walkable_at(at + dir * 0.9) { dir } else { dir * -1.0 };
        if !self.grid.walkable_at(at + dir * 0.9) {
            return;
        }
        let q = &mut self.penguins[t as usize];
        q.dodge_t = DODGE_TICKS;
        q.dodge_dir = dir;
    }

    /// Snowball `ball` has landed: it is no longer on its way to anyone.
    pub(super) fn ball_landed(&mut self, ball: u32) {
        if let Some(k) = self.aims.iter().position(|a| a.0 == ball) {
            let (_, t, dmg) = self.aims.swap_remove(k);
            let q = &mut self.penguins[t as usize];
            q.incoming = q.incoming.saturating_sub(dmg);
        }
    }

    /// One tick of a sidestep. True while it is still stepping.
    pub(super) fn dodging(&mut self, i: usize) -> bool {
        let p = &self.penguins[i];
        if p.dodge_t == 0 {
            return false;
        }
        if !matches!(p.act, Act::Idle | Act::Walk) || p.stun > 0 {
            self.penguins[i].dodge_t = 0;
            return false;
        }
        let dir = p.dodge_dir;
        let speed = self.speed(i, dir) * 1.3;
        let next = p.pos + dir * speed * DT;
        let p = &mut self.penguins[i];
        p.dodge_t -= 1;
        if !self.grid.walkable_at(next) {
            p.dodge_t = 0;
            return false;
        }
        p.pos = next;
        p.vel = dir * speed;
        p.act = Act::Walk;
        // It has left its path; plan afresh afterwards.
        p.goal = None;
        p.path.clear();
        true
    }

    /// A soldier in the ranks at its `place`: the spot it should stand on,
    /// a few paces out if that brings an enemy into reach.
    pub(super) fn stand_at(&self, i: usize, place: Vec2) -> Vec2 {
        let p = &self.penguins[i];
        let eager = p.kind.fights() && !matches!(p.kind, Kind::Shield | Kind::Slinger | Kind::Chieftain) && p.snowballs > 0 && !falls_back(p);
        if !eager {
            return place;
        }
        let range = self.effects_of(p.owner).scale_f(Stat::Range, p.kind.stats().range);
        let Some(&(_, foe)) = self.enemy_fighters_near(i, range + STEP_UP + place.dist(p.pos)).first() else { return place };
        let at = self.penguins[foe as usize].pos;
        // Already in reach and not too far out: stay and throw.
        if p.pos.dist(at) <= range - 0.3 && p.pos.dist(place) <= STEP_UP {
            return p.pos;
        }
        // From the place, step toward it just far enough.
        let d = place.dist(at);
        if d <= range - 0.3 {
            return place;
        }
        let out = d - (range - 0.6);
        if out > STEP_UP {
            return place;
        }
        let spot = place + (at - place).norm() * out;
        if self.grid.walkable_at(spot) {
            spot
        } else {
            place
        }
    }
}

/// A thrower hurt enough to fall back to the rear of the ranks: under two
/// thirds warm, so (with a snowballer's 12 warmth and 3 a hit) after its
/// second hit, while one more still leaves it on its feet.
pub fn falls_back(p: &Penguin) -> bool {
    p.kind.fights() && p.kind != Kind::Shield && (p.warmth as u16) * 3 < p.max_warmth as u16 * 2
}
