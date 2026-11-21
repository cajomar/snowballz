//! Heroes. Each tribe may have one Chieftain at a time: a big crowned
//! emperor penguin that leads an igloo's army. Penguins near it keep their
//! nerve (see `morale.rs`), and its War Cry rallies and hastens everyone
//! around it. If it freezes the tribe loses heart, and the igloo raises
//! another after a while.

use super::*;

/// Friendly penguins this close to a Chieftain gain courage faster and lose it slower.
pub const CHIEF_AURA_RADIUS: f32 = 6.0;
/// A War Cry reaches penguins this close.
pub const WAR_CRY_RADIUS: f32 = 9.0;
/// How long a War Cry lasts.
pub const WAR_CRY_TICKS: u16 = secs(6.0);
/// Time before the Chieftain can shout again.
pub const WAR_CRY_COOLDOWN: u16 = secs(45.0);
/// Penguins under a War Cry run this much faster.
pub const WAR_CRY_SPEED: f32 = 1.3;
/// Throwing cooldowns run down this many extra ticks per tick under a War Cry.
pub const WAR_CRY_THROW: u16 = 2;
/// The tribe this close to a freezing Chieftain loses heart.
pub const CHIEF_FALL_RADIUS: f32 = 12.0;
pub const CHIEF_FALL_LOSS: u8 = 3;
/// Enemies this close to the freezing take heart.
pub const CHIEF_FALL_CHEER: u8 = 2;

impl Game {
    /// The player has a Chieftain.
    pub fn has_chieftain(&self, player: u8) -> bool {
        self.penguins.iter().any(|p| p.owner == player && p.kind == Kind::Chieftain)
    }

    /// A friendly Chieftain is close to penguin `i` (not counting itself).
    pub(super) fn chief_near(&self, i: usize) -> bool {
        let p = &self.penguins[i];
        if p.inside.is_some() {
            return false;
        }
        let team = self.team(p.owner);
        let mut near = Vec::new();
        self.buckets.query(p.pos, CHIEF_AURA_RADIUS, &mut near);
        near.iter().any(|&o| {
            let q = &self.penguins[o as usize];
            q.kind == Kind::Chieftain && q.id != p.id && q.act != Act::Frozen && self.team(q.owner) == team && q.pos.dist(p.pos) <= CHIEF_AURA_RADIUS
        })
    }

    /// Count down War Cries and their cooldowns.
    pub(super) fn update_heroes(&mut self) {
        for p in &mut self.penguins {
            p.cry = p.cry.saturating_sub(1);
            p.cry_cd = p.cry_cd.saturating_sub(1);
        }
    }

    /// The Chieftain `unit` shouts: friends near it run faster, throw faster
    /// and shake off their fear.
    pub(super) fn war_cry(&mut self, player: u8, unit: u16) {
        let Some(c) = self.penguins.get(unit as usize) else { return };
        if c.kind != Kind::Chieftain || c.owner != player || c.cry_cd > 0 || c.inside.is_some() || c.act == Act::Frozen || c.carried_by.is_some() {
            return;
        }
        let (at, team) = (c.pos, self.team(player));
        // Scan the penguins rather than the neighbour buckets: the buckets are
        // only rebuilt at the start of a tick, and a command can arrive
        // between two ticks, when they no longer hold the true positions.
        let near: Vec<u16> = self
            .penguins
            .iter()
            .filter(|q| {
                q.inside.is_none()
                    && q.act != Act::Frozen
                    && q.owner != NEUTRAL
                    && q.pos.dist(at) <= WAR_CRY_RADIUS
                    && self.players[q.owner as usize].team == team
            })
            .map(|q| q.id)
            .collect();
        for o in near {
            let q = &mut self.penguins[o as usize];
            q.cry = WAR_CRY_TICKS;
            if q.routed {
                q.routed = false;
                q.courage = q.courage.max(RALLY_COURAGE);
            }
        }
        self.penguins[unit as usize].cry_cd = WAR_CRY_COOLDOWN;
        self.events.push(Event::WarCry { penguin: unit, pos: at });
    }

    /// A Chieftain froze at `at`: its tribe nearby loses heart, and the enemy cheers.
    pub(super) fn chieftain_fell(&mut self, at: Vec2, team: u8) {
        let near: Vec<u16> = self
            .penguins
            .iter()
            .filter(|q| {
                q.inside.is_none() && q.act != Act::Frozen && q.owner != NEUTRAL && q.pos.dist(at) <= CHIEF_FALL_RADIUS
            })
            .map(|q| q.id)
            .collect();
        for o in near {
            if self.team(self.penguins[o as usize].owner) == team {
                self.lose_courage(o, CHIEF_FALL_LOSS);
            } else {
                let q = &mut self.penguins[o as usize];
                q.courage = (q.courage + CHIEF_FALL_CHEER).min(MAX_COURAGE);
            }
        }
    }
}
