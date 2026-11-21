//! How a match ends. Two ways out of a game that would otherwise drag on:
//!
//! * **Concede.** A tribe may give up at any time. Its penguins freeze,
//!   its igloos fall in and its buildings crumble, and it is out.
//! * **The Long Night.** After [`LONG_NIGHT_START`] a ring of killing cold
//!   closes in from the edge of the map toward its middle. Penguins caught
//!   outside the ring freeze, and igloos and buildings out there waste away,
//!   so the last tribes fight over the ground in the middle. Once the ring
//!   has stopped, whoever has the most penguins still standing when
//!   [`LONG_NIGHT_FINAL`] runs out wins.
//!
//! The ring is a pure function of the tick, so clients draw it without
//! being told anything but when the night starts.

use super::*;

/// When the Long Night begins, unless a game says otherwise.
pub const LONG_NIGHT_START: u32 = 35 * 60 * crate::TICK_RATE;
/// How long the ring takes to close.
pub const LONG_NIGHT_CLOSE: u32 = 10 * 60 * crate::TICK_RATE;
/// Once closed, how much longer until the most penguins wins.
pub const LONG_NIGHT_FINAL: u32 = 4 * 60 * crate::TICK_RATE;
/// The ring never closes tighter than this many tiles from the middle.
pub const LONG_NIGHT_MIN_RADIUS: f32 = 12.0;
/// Penguins outside the ring lose a point of warmth this often.
const FREEZE_EVERY: u32 = secs(3.0) as u32;
/// Igloos and buildings outside it lose strength this often.
const WASTE_EVERY: u32 = secs(2.0) as u32;
const IGLOO_WASTE: u16 = 3;

/// Radius of the safe ring at `tick`, or `None` before the night begins.
pub fn night_radius(w: i32, h: i32, start: u32, tick: u32) -> Option<f32> {
    if tick < start {
        return None;
    }
    let outer = (w as f32).hypot(h as f32) / 2.0;
    let f = ((tick - start) as f32 / LONG_NIGHT_CLOSE as f32).min(1.0);
    Some(outer + (LONG_NIGHT_MIN_RADIUS - outer) * f)
}

impl Game {
    /// The safe ring right now, if the Long Night has begun.
    pub fn night_ring(&self) -> Option<(Vec2, f32)> {
        let r = night_radius(self.map.w, self.map.h, self.long_night_at, self.tick)?;
        Some((v2(self.map.w as f32 / 2.0, self.map.h as f32 / 2.0), r))
    }

    /// A tribe gives up: everything it has freezes or falls.
    pub(super) fn concede(&mut self, player: u8) {
        let Some(pl) = self.players.get(player as usize) else { return };
        if pl.out || self.winner.is_some() {
            return;
        }
        for ig in 0..self.igloos.len() {
            if self.igloos[ig].owner == player {
                self.collapse(ig as u8);
            }
        }
        for b in 0..self.buildings.len() {
            if self.buildings[b].alive && self.buildings[b].owner == player {
                self.destroy(b);
            }
        }
        for v in 0..self.penguins.len() {
            let p = &self.penguins[v];
            if p.owner == player && p.act != Act::Frozen {
                let (pos, id) = (p.pos, p.id);
                self.hurt(v as u16, u8::MAX, id, pos);
            }
        }
        self.players[player as usize].out = true;
        self.events.push(Event::PlayerOut { player });
    }

    /// The ring's cold: penguins caught outside freeze a point at a time,
    /// and igloos and buildings out there waste away.
    pub(super) fn update_long_night(&mut self) {
        let Some((mid, r)) = self.night_ring() else { return };
        if self.tick.is_multiple_of(FREEZE_EVERY) {
            for v in 0..self.penguins.len() {
                let p = &self.penguins[v];
                if p.owner == NEUTRAL || p.inside.is_some() || p.act == Act::Frozen || p.carried_by.is_some() || p.pos.dist(mid) <= r {
                    continue;
                }
                let (pos, id) = (p.pos, p.id);
                self.hurt(v as u16, 1, id, pos);
            }
        }
        if self.tick.is_multiple_of(WASTE_EVERY) {
            for ig in 0..self.igloos.len() {
                let g = &self.igloos[ig];
                if g.owner == NEUTRAL || g.hp == 0 || g.center.dist(mid) <= r {
                    continue;
                }
                self.igloos[ig].hp = g.hp.saturating_sub(IGLOO_WASTE);
                if self.igloos[ig].hp == 0 {
                    self.collapse(ig as u8);
                }
            }
            for b in 0..self.buildings.len() {
                if self.buildings[b].alive && self.buildings[b].center().dist(mid) > r {
                    self.damage_building(b, 1);
                }
            }
        }
        // Once the ring has stopped and time is up, the biggest flock wins.
        if self.winner.is_none() && self.tick >= self.long_night_at + LONG_NIGHT_CLOSE + LONG_NIGHT_FINAL {
            let mut best: Option<(usize, u8)> = None;
            let mut tie = false;
            let mut teams: Vec<u8> = self.players.iter().filter(|p| !p.out).map(|p| p.team).collect();
            teams.sort();
            teams.dedup();
            for team in teams {
                let n = self.penguins.iter().filter(|p| p.owner != NEUTRAL && self.team(p.owner) == team && p.act != Act::Frozen).count();
                match best {
                    Some((b, _)) if n < b => {}
                    Some((b, _)) if n == b => tie = true,
                    _ => {
                        best = Some((n, team));
                        tie = false;
                    }
                }
            }
            if let (Some((_, team)), false) = (best, tie) {
                self.winner = Some(team);
                self.events.push(Event::Victory { team });
            }
        }
    }
}
