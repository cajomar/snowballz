//! Fishing slots (the `armies` rule, DESIGN.md 0.2). Workers fish only in
//! a fishing slot: every igloo has two, and every Sushi House built by the
//! water has some (2, built up to 4 then 8). Each slot the tribe wants
//! filled calls in the nearest idle worker of its tribe, and a Sushi House
//! with nobody idle to call recruits a new penguin from the sea; a fisher fishes the banks nearest its
//! slot and brings the catch back there, so a Sushi House on a far shore
//! saves the long walk home. A fisher given any other job leaves its slot,
//! and the slot calls someone else. Without armies anyone idle fishes.

use super::*;

/// Fishing slots a Sushi House can be built up to.
pub const FISH_SLOTS: [u8; 3] = [2, 4, 8];
/// Fishing slots every igloo has (never more: build Sushi Houses).
pub const IGLOO_FISH_SLOTS: u8 = 2;
/// The builders' work for each step up (building steps, logs).
pub const FISH_UPGRADE: [(u8, u8); 2] = [(3, 2), (6, 4)];
/// A Sushi House only takes fishers if there is a bank this close to it.
pub const STORE_REACH: f32 = 8.0;

/// The work to build a Sushi House with `slots` fishing slots up to more,
/// if it can be.
pub fn fish_upgrade(slots: u8) -> Option<(u8, u8)> {
    FISH_SLOTS.iter().position(|&s| s == slots).and_then(|k| FISH_UPGRADE.get(k).copied())
}

/// Where a fisher's slot is: an igloo, or a Sushi House.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fishery {
    Igloo(u8),
    House(u16),
}

impl Game {
    /// A fishery that is still standing: its owner, where the catch is set
    /// down, and how many of its slots the tribe wants filled.
    pub fn fishery(&self, f: Fishery) -> Option<(u8, Vec2, u8)> {
        match f {
            Fishery::Igloo(k) => {
                let ig = self.igloos.get(k as usize)?;
                (ig.owner != NEUTRAL).then_some((ig.owner, ig.door, ig.fish_want.min(IGLOO_FISH_SLOTS)))
            }
            Fishery::House(b) => {
                let bd = self.buildings.get(b as usize)?;
                (bd.alive && bd.done() && bd.kind == BuildKind::Store && bd.owner != NEUTRAL).then(|| (bd.owner, bd.center(), bd.want.min(bd.slots)))
            }
        }
    }

    /// Penguins fishing at `f`.
    pub fn fishers(&self, f: Fishery) -> usize {
        self.penguins.iter().filter(|p| p.fishery == Some(f) && p.act != Act::Frozen).count()
    }

    /// Penguins crewing Sushi House `b`.
    pub fn store_crew(&self, b: u16) -> usize {
        self.fishers(Fishery::House(b))
    }

    /// Is there a bank in reach of Sushi House `b`?
    fn store_has_water(&self, b: usize) -> bool {
        let c = self.buildings[b].center();
        self.shores.iter().any(|s| s.pos.dist(c) <= STORE_REACH)
    }

    /// Once a second, with armies: drop fishers whose fishery is gone or who
    /// were given other work, then let every fishery short of fishers call
    /// in the nearest free worker of its tribe.
    pub(super) fn fill_fisheries(&mut self) {
        for i in 0..self.penguins.len() {
            let p = &self.penguins[i];
            let Some(f) = p.fishery else { continue };
            let lost = self.fishery(f).is_none_or(|(owner, _, _)| owner != p.owner);
            // A fisher called away to build keeps its slot, and comes back
            // to it once the site is up (so the slot doesn't call a newcomer).
            let building = p.crew && matches!(p.order, Order::Build { .. });
            if lost || p.kind != Kind::Worker || (p.order != Order::None && !building) {
                self.penguins[i].fishery = None;
            }
        }
        let mut all: Vec<Fishery> = (0..self.igloos.len() as u8).map(Fishery::Igloo).collect();
        all.extend((0..self.buildings.len()).filter(|&b| self.buildings[b].kind == BuildKind::Store && self.store_has_water(b)).map(|b| Fishery::House(b as u16)));
        // Wanted fewer: the last ones called in go back to being idle.
        for &f in &all {
            let Some((_, _, want)) = self.fishery(f) else { continue };
            let mine: Vec<usize> = (0..self.penguins.len()).filter(|&i| self.penguins[i].fishery == Some(f) && self.penguins[i].act != Act::Frozen).collect();
            for &i in mine.iter().skip(want as usize) {
                self.penguins[i].fishery = None;
            }
        }
        for f in all {
            let Some((owner, at, slots)) = self.fishery(f) else { continue };
            if self.fishers(f) >= slots as usize {
                continue;
            }
            let pick = (0..self.penguins.len())
                .filter(|&i| {
                    let p = &self.penguins[i];
                    p.owner == owner
                        && p.kind == Kind::Worker
                        && p.order == Order::None
                        && p.fishery.is_none()
                        && !p.shore_pin
                        && p.is_out()
                        && p.act != Act::Frozen
                        && !p.cold
                        && !p.routed
                })
                .min_by(|&x, &y| self.penguins[x].pos.dist2(at).partial_cmp(&self.penguins[y].pos.dist2(at)).unwrap());
            // Nobody to call: a newcomer swims in from the sea.
            let pick = pick.or_else(|| if self.may_call_sea(owner) { self.call_from_sea(owner, at, 0).map(|id| id as usize) } else { None });
            if let Some(i) = pick {
                let p = &mut self.penguins[i];
                p.fishery = Some(f);
                p.prefer = ResourceKind::Fish;
                p.gather_res = None;
                p.shore = None;
                // An igloo's fishers live there, so the catch goes to its larder.
                if let Fishery::Igloo(k) = f {
                    p.home = k;
                }
            }
        }
    }

    /// Where fisher `i`'s slot is, if it still stands.
    pub(super) fn fishery_of(&self, i: usize) -> Option<(Fishery, Vec2)> {
        let f = self.penguins[i].fishery?;
        let (owner, at, _) = self.fishery(f)?;
        (owner == self.penguins[i].owner).then_some((f, at))
    }
}
