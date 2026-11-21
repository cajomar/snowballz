//! Warmers: the medics. They warm cold friends and thaw the frozen, who
//! wake up in the Warmer's tribe (DESIGN.md section 0). The blowdryer is
//! just how they look doing it.

use super::*;

/// How far an idle Warmer looks for someone to tend.
pub const WARMER_RANGE: f32 = 6.0;
/// How close it stands to blow hot air on them.
pub const WARMER_REACH: f32 = 1.5;
/// A friend gains a warmth this often while being warmed.
pub const WARMER_TICKS: u16 = secs(0.5);
/// Time to thaw a frozen penguin.
pub const THAW_BY_WARMER_TICKS: u16 = secs(5.0);

impl Game {
    /// Can this Warmer tend penguin `t`? Friends who are cold, or frozen
    /// penguins nobody owns (or its own tribe's frozen).
    fn tendable(&self, w: usize, t: usize) -> bool {
        let (me, q) = (&self.penguins[w], &self.penguins[t]);
        if t == w || q.inside.is_some() || q.carried_by.is_some() || q.wild {
            return false;
        }
        if q.act == Act::Frozen {
            q.owner == NEUTRAL || self.team(q.owner) == self.team(me.owner)
        } else {
            q.owner != NEUTRAL && self.team(q.owner) == self.team(me.owner) && q.warmth < q.max_warmth
        }
    }

    /// The patient an idle Warmer picks: the coldest friend in reach, or
    /// failing that the nearest frozen penguin it may thaw.
    fn pick_patient(&mut self, w: usize) -> Option<u16> {
        let pos = self.penguins[w].pos;
        let mut near = std::mem::take(&mut self.scratch);
        self.buckets.query(pos, WARMER_RANGE, &mut near);
        let mut best: Option<(f32, u16)> = None;
        for &o in &near {
            let q = &self.penguins[o as usize];
            if q.pos.dist(pos) > WARMER_RANGE || !self.tendable(w, o as usize) {
                continue;
            }
            // The living first, coldest first; then the frozen, nearest first.
            let score = if q.act == Act::Frozen { 10.0 + q.pos.dist(pos) * 0.1 } else { q.warmth as f32 / q.max_warmth.max(1) as f32 };
            if best.is_none_or(|(s, _)| score < s) {
                best = Some((score, o));
            }
        }
        self.scratch = near;
        best.map(|(_, o)| o)
    }

    /// A Warmer's own work. Returns false when it has nothing to do, so
    /// the usual soldier behaviour carries on.
    pub(super) fn warmer_job(&mut self, i: usize) -> bool {
        let order = self.penguins[i].order;
        let ordered = match order {
            Order::Thaw { penguin } => Some(penguin),
            Order::None | Order::Hold { .. } => None,
            _ => return false,
        };
        let patient = match ordered.or(self.penguins[i].tend) {
            Some(t) if self.tendable(i, t as usize) => Some(t),
            _ if ordered.is_some() => {
                // The one we were sent to is gone or awake already.
                self.penguins[i].order = Order::None;
                None
            }
            _ if (self.tick as usize + i).is_multiple_of(10) => self.pick_patient(i),
            _ => None,
        };
        self.penguins[i].tend = patient;
        let Some(t) = patient else {
            if self.penguins[i].act == Act::Warm {
                self.penguins[i].act = Act::Idle;
            }
            return false;
        };
        let (tpos, frozen) = (self.penguins[t as usize].pos, self.penguins[t as usize].act == Act::Frozen);
        // A frozen penguin holds its whole tile, so it's reached from the next one.
        let reach = if frozen { WARMER_REACH + 0.8 } else { WARMER_REACH };
        if self.penguins[i].pos.dist(tpos) > reach {
            // Frozen penguins hold their tile, so aim beside them.
            let goal = if frozen {
                self.grid.nearest_walkable(tpos.tile(), 2).map_or(tpos, |(x, y)| v2(x as f32 + 0.5, y as f32 + 0.5))
            } else {
                tpos
            };
            // Patients shuffle about; only plan a new path when they've moved off.
            let goal = match self.penguins[i].goal {
                Some(g0) if g0.dist(goal) < 1.0 => g0,
                _ => goal,
            };
            self.travel(i, goal, 0.3);
            if self.penguins[i].act == Act::Warm {
                self.penguins[i].act = Act::Walk;
            }
            return true;
        }
        let p = &mut self.penguins[i];
        if p.act != Act::Warm {
            p.act = Act::Warm;
            p.act_t = 0;
            p.anim_seq = p.anim_seq.wrapping_add(1);
        }
        p.facing = (tpos - p.pos).angle();
        p.goal = None;
        p.path.clear();
        let (owner, t_act) = (p.owner, p.act_t);
        if frozen {
            if t_act >= THAW_BY_WARMER_TICKS {
                self.thaw(t, owner);
                let p = &mut self.penguins[i];
                p.act = Act::Idle;
                p.tend = None;
                if p.order == (Order::Thaw { penguin: t }) {
                    p.order = Order::None;
                }
            }
        } else if t_act > 0 && t_act.is_multiple_of(WARMER_TICKS) {
            let q = &mut self.penguins[t as usize];
            q.warmth = (q.warmth + 1).min(q.max_warmth);
            if q.warmth >= q.max_warmth {
                q.cold = false;
                q.chill = 0;
                self.penguins[i].tend = None;
                self.penguins[i].act = Act::Idle;
            }
        }
        true
    }

    /// Send Warmers to thaw a frozen penguin.
    pub(super) fn order_thaw(&mut self, player: u8, units: Vec<u16>, penguin: u16) {
        let Some(q) = self.penguins.get(penguin as usize) else { return };
        if q.act != Act::Frozen {
            return;
        }
        for u in units {
            if self.controllable(player, u) && self.penguins[u as usize].kind == Kind::Warmer {
                self.give_order(u, Order::Thaw { penguin });
                self.penguins[u as usize].tend = Some(penguin);
            }
        }
    }
}
