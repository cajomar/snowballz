//! What each kind of penguin does on its own, so a player gets the good of
//! a Shieldbearer or a Sled without having to know the trick:
//!
//! - **Shieldbearer** steps between the nearest enemy thrower and its
//!   friends; its slab blocks direct hits on whoever stands behind it. In
//!   an army it holds its place in the wall instead (`armies.rs`).
//! - **Slinger** can't throw at anyone close, and backs off when an enemy
//!   gets near: it needs a line in front of it.
//! - **Skier** goes round a fight for the soft targets (Slingers, Warmers,
//!   Sleds, workers) and its charge knocks a thrower flat. It also goes for
//!   Shieldbearers: skis against a slab knock both flat, opening the wall.
//! - **Packer** aims at walls, towers and igloos before penguins.
//! - **Sled** doesn't fight: it fetches frozen penguins, anyone's, and drags
//!   them home, where they thaw into its tribe.
//!
//! (The Warmer's job is in `warmer.rs`.)

use super::*;

/// Slingers can't throw at anyone closer than this...
pub const SLINGER_MIN_RANGE: f32 = 3.0;
/// ...and back off this far when an enemy fighter comes that close.
const SLINGER_BACK_OFF: f32 = 3.0;
/// How far ahead of its friends a Shieldbearer stands, toward the enemy.
const SHIELD_STEP: f32 = 1.2;
/// Enemies this near (and friends this near) put a Shieldbearer to work.
const SHIELD_ENEMY_RANGE: f32 = 10.0;
const SHIELD_FRIEND_RANGE: f32 = 4.0;
/// A Skier looks this far for something soft to go after.
const SKIER_HUNT_RANGE: f32 = 16.0;
/// A charging Skier knocks over a thrower this close...
const CHARGE_REACH: f32 = 0.9;
/// ...who lies there this long, and the Skier needs this long to charge again.
pub const STUN_TICKS: u16 = secs(2.0);
const CHARGE_COOLDOWN: u16 = secs(4.0);
/// A Sled looks this far for someone frozen to fetch...
const SLED_SEARCH: f32 = 24.0;
/// ...but not where enemy fighters are standing over them.
const SLED_DANGER: f32 = 5.0;

impl Game {
    /// The kind's own job, when it has no order of its own to follow. True
    /// if it took the turn.
    pub(super) fn role_job(&mut self, i: usize) -> bool {
        // A player's orders come first, and a move already under way (its
        // own, or answering an alarm) is let finish. A Hold leaves it free.
        if !matches!(self.penguins[i].order, Order::None | Order::Hold { .. }) {
            return false;
        }
        match self.penguins[i].kind {
            Kind::Shield => self.shield_job(i),
            Kind::Slinger => self.slinger_job(i),
            Kind::Skier => self.skier_job(i),
            Kind::Sledder => self.sled_job(i),
            _ => false,
        }
    }

    /// Enemy fighters near `pos`, nearest first, as (distance, id).
    pub(super) fn enemy_fighters_near(&self, i: usize, r: f32) -> Vec<(f32, u16)> {
        let p = &self.penguins[i];
        let mut near = Vec::new();
        self.targets.query(p.pos, r, &mut near);
        let mut out: Vec<(f32, u16)> = near
            .into_iter()
            .filter_map(|o| {
                let q = &self.penguins[o as usize];
                let ok = self.hostile(p.owner, q.owner) && q.kind.fights() && q.inside.is_none() && q.act != Act::Frozen && !q.wild;
                let d = q.pos.dist(p.pos);
                (ok && d <= r).then_some((d, o))
            })
            .collect();
        out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        out
    }

    fn shield_job(&mut self, i: usize) -> bool {
        // In an army its place in the wall is already between friends and foes.
        if self.penguins[i].slot.is_some() && self.penguins[i].order == Order::None {
            return false;
        }
        let Some(&(_, foe)) = self.enemy_fighters_near(i, SHIELD_ENEMY_RANGE).first() else { return false };
        let p = &self.penguins[i];
        let team = self.team(p.owner);
        let mut near = Vec::new();
        self.buckets.query(p.pos, SHIELD_FRIEND_RANGE, &mut near);
        let friends: Vec<Vec2> = near
            .iter()
            .map(|&o| &self.penguins[o as usize])
            .filter(|q| q.id != p.id && q.owner != NEUTRAL && self.team(q.owner) == team && q.kind.fights() && q.kind != Kind::Shield && q.act != Act::Frozen && q.inside.is_none())
            .map(|q| q.pos)
            .collect();
        if friends.is_empty() {
            return false;
        }
        let mid = friends.iter().fold(Vec2::ZERO, |a, &b| a + b) * (1.0 / friends.len() as f32);
        let enemy = self.penguins[foe as usize].pos;
        let spot = mid + (enemy - mid).norm() * SHIELD_STEP;
        if self.penguins[i].pos.dist(spot) > 0.5 && self.grid.walkable_at(spot) {
            self.travel(i, spot, 0.3);
        } else {
            let p = &mut self.penguins[i];
            p.facing = turn_towards(p.facing, (enemy - p.pos).angle(), 10.0 * DT);
            self.idle(i);
        }
        true
    }

    fn slinger_job(&mut self, i: usize) -> bool {
        let Some(&(d, foe)) = self.enemy_fighters_near(i, SLINGER_MIN_RANGE).first() else { return false };
        let p = &self.penguins[i];
        let away = (p.pos - self.penguins[foe as usize].pos).norm();
        let back = p.pos + away * (SLINGER_BACK_OFF + (SLINGER_MIN_RANGE - d));
        // Only somewhere it can stand; else hold and hope.
        if !self.grid.walkable_at(back) {
            return false;
        }
        self.give_order(i as u16, Order::Move { target: back, run: true, auto: true });
        true
    }

    fn skier_job(&mut self, i: usize) -> bool {
        if self.penguins[i].snowballs == 0 || self.penguins[i].charge_cd > 0 {
            return false;
        }
        let p = &self.penguins[i];
        let mut near = Vec::new();
        self.targets.query(p.pos, SKIER_HUNT_RANGE, &mut near);
        // Soft targets first: they can't fight back, or barely.
        let prey = near
            .into_iter()
            .filter_map(|o| {
                let q = &self.penguins[o as usize];
                if !self.hostile(p.owner, q.owner) || q.inside.is_some() || q.post.is_some() || q.act == Act::Frozen || q.wild || q.owner == NEUTRAL {
                    return None;
                }
                let soft = match q.kind {
                    Kind::Slinger | Kind::Warmer | Kind::Sledder => 0.0,
                    // A Shieldbearer standing up: knock it over to open the wall.
                    Kind::Shield if q.stun == 0 => 1.5,
                    Kind::Worker => 3.0,
                    _ => return None,
                };
                Some((q.pos.dist(p.pos) + soft, q.pos))
            })
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let Some((_, at)) = prey else { return false };
        self.give_order(i as u16, Order::Move { target: at, run: true, auto: true });
        true
    }

    /// A Skier on the move knocks down the first enemy thrower it reaches,
    /// and bounces off a Shieldbearer. Runs every tick for every Skier.
    pub(super) fn skier_charge(&mut self, i: usize) {
        let p = &mut self.penguins[i];
        p.charge_cd = p.charge_cd.saturating_sub(1);
        if p.kind != Kind::Skier || p.act != Act::Walk || p.charge_cd > 0 || p.inside.is_some() || p.cold || p.routed || p.stun > 0 {
            return;
        }
        let (pos, owner, id) = (p.pos, p.owner, p.id);
        let mut near = Vec::new();
        self.targets.query(pos, CHARGE_REACH, &mut near);
        let hit = near.into_iter().find(|&o| {
            let q = &self.penguins[o as usize];
            self.hostile(owner, q.owner) && q.owner != NEUTRAL && q.inside.is_none() && q.post.is_none() && q.act != Act::Frozen && q.stun == 0 && q.pos.dist(pos) <= CHARGE_REACH
        });
        let Some(v) = hit else { return };
        self.penguins[i].charge_cd = CHARGE_COOLDOWN;
        if self.penguins[v as usize].kind == Kind::Shield {
            // Skis against an ice slab: both go down, and the wall has a hole in it.
            self.stun(id);
            self.stun(v);
            self.events.push(Event::Bounced { skier: id, shield: v });
            self.events.push(Event::KnockedDown { penguin: v, by: id });
        } else {
            self.stun(v);
            self.events.push(Event::KnockedDown { penguin: v, by: id });
        }
    }

    /// Knocked flat: drops whatever throw it was making, and lies there.
    pub(super) fn stun(&mut self, v: u16) {
        self.drop_captive(v);
        let p = &mut self.penguins[v as usize];
        p.stun = STUN_TICKS;
        p.act = Act::Stunned;
        p.act_t = 0;
        p.path.clear();
        p.goal = None;
    }

    /// One tick on the ground. True while it is still down.
    pub(super) fn stunned(&mut self, i: usize) -> bool {
        let p = &mut self.penguins[i];
        if p.stun == 0 {
            return false;
        }
        p.stun -= 1;
        if p.stun == 0 {
            p.act = Act::Idle;
            p.act_t = 0;
        }
        true
    }

    /// Penguin `v` is frozen and free for a Sled of `team` to take.
    pub fn haulable(&self, v: &Penguin, team: u8) -> bool {
        v.act == Act::Frozen && v.inside.is_none() && v.carried_by.is_none() && v.post.is_none() && (v.owner == NEUTRAL || self.team(v.owner) != team)
    }

    fn sled_job(&mut self, i: usize) -> bool {
        let p = &self.penguins[i];
        if p.carrying.is_some() || p.cold || p.routed {
            return false;
        }
        let team = self.team(p.owner);
        let pos = p.pos;
        let owner = p.owner;
        // Someone else's Sled already on the way doesn't stop it: first come.
        let taken: Vec<u16> = self.penguins.iter().filter_map(|q| match q.order {
            Order::Capture { penguin } if q.owner == owner && q.id != p.id => Some(penguin),
            _ => None,
        }).collect();
        let mut near = Vec::new();
        self.buckets.query(pos, SLED_SEARCH, &mut near);
        let best = near
            .into_iter()
            .filter(|&o| {
                let q = &self.penguins[o as usize];
                self.haulable(q, team) && !taken.contains(&o) && q.pos.dist(pos) <= SLED_SEARCH
            })
            // Not into a crowd of enemy throwers.
            .filter(|&o| {
                let at = self.penguins[o as usize].pos;
                !self.penguins.iter().any(|e| e.owner != NEUTRAL && self.hostile(owner, e.owner) && e.kind.fights() && e.act != Act::Frozen && e.inside.is_none() && e.pos.dist(at) < SLED_DANGER)
            })
            .min_by(|&a, &b| self.penguins[a as usize].pos.dist2(pos).partial_cmp(&self.penguins[b as usize].pos.dist2(pos)).unwrap());
        let Some(v) = best else { return false };
        self.give_order(i as u16, Order::Capture { penguin: v });
        true
    }

    /// A Sled has brought a frozen penguin to its tribe's door: it thaws
    /// there and joins.
    pub(super) fn haul_home(&mut self, carrier: usize, v: u16) {
        let owner = self.penguins[carrier].owner;
        self.penguins[carrier].carrying = None;
        self.penguins[carrier].order = Order::None;
        self.penguins[v as usize].carried_by = None;
        self.thaw(v, owner);
        self.events.push(Event::Converted { penguin: v, to: owner });
    }
}
