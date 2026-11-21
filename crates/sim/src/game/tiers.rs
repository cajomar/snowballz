//! Igloo tiers. A plain igloo can be built up into a Snow Hall and then an
//! Ice Palace: the work is paid for in fish, wood and (for the palace)
//! crystal, takes a while, and the igloo keeps working while it happens.
//! A higher tier holds more penguins, warms its
//! guests faster and takes longer to take over.

use super::*;

/// A plain igloo.
pub const TIER_IGLOO: u8 = 0;
/// Bigger and warmer, and it unlocks the middle ranks of penguin.
pub const TIER_SNOW_HALL: u8 = 1;
/// The top tier: a palace of blue ice.
pub const TIER_ICE_PALACE: u8 = 2;

/// What a tier gives its igloo, and what it costs to reach it.
pub struct TierInfo {
    /// Player-facing name.
    pub name: &'static str,
    /// Fish paid to upgrade into this tier.
    pub fish: u16,
    /// Wood paid to upgrade into this tier.
    pub wood: u16,
    /// Crystal paid to upgrade into this tier.
    pub crystal: u16,
    /// Ticks the work takes.
    pub ticks: u16,
    /// Penguins the igloo can hold.
    pub pop_cap: usize,
    /// Warmth each penguin inside gains per warming tick.
    pub warm: u8,
    /// How much longer a takeover takes, in percent.
    pub takeover: u16,
}

/// The tiers, in order. The cost and the time are for reaching the tier,
/// so the plain igloo (which nobody upgrades into) has none.
pub const TIERS: [TierInfo; 3] = [
    TierInfo {
        name: "Igloo",
        fish: 0,
        wood: 0,
        crystal: 0,
        ticks: 0,
        pop_cap: IGLOO_POP_CAP,
        warm: 1,
        takeover: 100,
    },
    TierInfo {
        name: "Snow Hall",
        fish: 20,
        wood: 10,
        crystal: 0,
        ticks: secs(30.0),
        pop_cap: 70,
        warm: 2,
        takeover: 150,
    },
    TierInfo {
        name: "Ice Palace",
        fish: 40,
        wood: 12,
        crystal: 8,
        ticks: secs(45.0),
        pop_cap: 110,
        warm: 3,
        takeover: 200,
    },
];

/// A running upgrade: how much is left of the work.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Upgrade {
    pub ticks_left: u16,
    pub total: u16,
}

impl Upgrade {
    /// How far along the work is, 0..1.
    pub fn progress(self) -> f32 {
        1.0 - self.ticks_left as f32 / self.total.max(1) as f32
    }
}

/// What a tier gives its igloo.
pub fn tier(tier: u8) -> &'static TierInfo {
    &TIERS[(tier as usize).min(TIERS.len() - 1)]
}

/// The tier above this one, if there is one.
pub fn next_tier(tier: u8) -> Option<&'static TierInfo> {
    let next = tier as usize + 1;
    (next < TIERS.len()).then(|| &TIERS[next])
}

/// How long a takeover of an igloo of this tier takes. A bigger igloo is
/// harder to take.
pub fn takeover_ticks(t: u8, neutral: bool) -> u16 {
    let base = if neutral { NEUTRAL_TAKEOVER_TICKS } else { TAKEOVER_TICKS };
    (base as u32 * tier(t).takeover as u32 / 100) as u16
}

impl Game {
    /// Is an igloo of `igloo_tier` grand enough for `kind`? Without tech
    /// there are no tiers to climb, so every kind is open from the start.
    pub fn tier_met(&self, kind: Kind, igloo_tier: u8) -> bool {
        !self.core.tech || kind.tier() <= igloo_tier
    }

    /// Penguins an igloo of this tier can hold.
    /// Without tiers there is no cap.
    pub fn pop_cap(&self, ig: u8) -> usize {
        if !self.core.tech {
            return usize::MAX;
        }
        tier(self.igloos[ig as usize].tier).pop_cap
    }

    /// Start building an igloo up into the next tier. The materials are
    /// paid up front and the work takes a while; the igloo keeps warming
    /// penguins meanwhile. If it is captured before the work
    /// is done, the upgrade is lost.
    pub(super) fn upgrade_igloo(&mut self, player: u8, ig: u8) {
        if !self.core.tech {
            return;
        }
        let Some(igloo) = self.igloos.get(ig as usize) else { return };
        if igloo.owner != player || igloo.takeover.is_some() || igloo.upgrade.is_some() {
            return;
        }
        let Some(info) = next_tier(igloo.tier) else { return };
        let (fish, wood) = price(info.fish, info.wood);
        if igloo.fish < fish || self.wood_of(player) < wood as u32 || self.crystal_of(player) < info.crystal as u32 {
            return;
        }
        self.igloos[ig as usize].fish -= fish;
        self.spend_wood(player, wood);
        self.spend_crystal(player, info.crystal);
        let igloo = &mut self.igloos[ig as usize];
        igloo.upgrade = Some(Upgrade { ticks_left: info.ticks, total: info.ticks });
    }

    /// Crystal across all of a player's igloos. Like wood for buildings,
    /// the whole tribe pools its wood and crystal for an upgrade.
    pub fn crystal_of(&self, player: u8) -> u32 {
        self.igloos.iter().filter(|ig| ig.owner == player).map(|ig| ig.crystal as u32).sum()
    }

    /// Take crystal from a player's igloos, richest first.
    pub(super) fn spend_crystal(&mut self, player: u8, mut n: u16) {
        while n > 0 {
            let Some(ig) = self.igloos.iter_mut().filter(|ig| ig.owner == player && ig.crystal > 0).max_by_key(|ig| ig.crystal) else { return };
            let take = ig.crystal.min(n);
            ig.crystal -= take;
            n -= take;
        }
    }

    /// Upgrades finish on their own.
    pub(super) fn update_upgrades(&mut self) {
        for ig in 0..self.igloos.len() {
            let Some(mut u) = self.igloos[ig].upgrade else { continue };
            u.ticks_left = u.ticks_left.saturating_sub(1);
            if u.ticks_left > 0 {
                self.igloos[ig].upgrade = Some(u);
                continue;
            }
            let igloo = &mut self.igloos[ig];
            igloo.upgrade = None;
            igloo.tier = (igloo.tier + 1).min(TIERS.len() as u8 - 1);
            // A grander igloo stands up to more.
            igloo.hp = igloo_max_hp(igloo.tier);
            self.events.push(Event::Upgraded { igloo: ig as u8, tier: igloo.tier });
        }
    }
}
