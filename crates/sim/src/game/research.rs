//! Research. A tribe learns better ways of doing things at its igloos: three
//! branches of three levels each, every level a few simple stat
//! multipliers from one table. What is learnt belongs to the whole tribe;
//! the work is started at an igloo, paid for in fish, wood and crystal, and
//! takes a while. An igloo researches one thing at a time, and the work is
//! lost if the igloo is captured. The second level needs a Snow Hall and
//! the third an Ice Palace (see [`tiers`](super::tiers)).

use super::*;

/// Snowcraft: throwing better and carrying more.
pub const SNOWCRAFT: u8 = 0;
/// Icecraft: sturdier snow buildings, and getting about on the ice.
pub const ICECRAFT: u8 = 1;
/// Fishery: more from the fish, and faster training and warmth.
pub const FISHERY: u8 = 2;
/// How many branches there are.
pub const BRANCHES: usize = 3;
/// How many levels each branch has.
pub const LEVELS: usize = 3;

/// What a tribe has learnt: the level reached in each branch.
pub type Levels = [u8; BRANCHES];

/// The numbers research can change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Stat {
    /// How far snowballs fly.
    Range = 0,
    /// How wide of the mark they land (smaller is better).
    Spread,
    /// Time to scoop a snowball (smaller is faster).
    ScoopTime,
    /// How wide a snowball splashes.
    Splash,
    /// Strength of snow buildings.
    BuildingHp,
    /// Time for a construction step (smaller is faster).
    BuildTime,
    /// Walking speed on ice.
    IceSpeed,
    /// Damage the thaw does to snow buildings.
    Slump,
    /// Time to land a fish (smaller is faster).
    FishTime,
    /// Time to train at a camp (smaller is faster).
    TrainTime,
    /// Warmth a fish gives at the igloo.
    FishWarmth,
}

const STAT_COUNT: usize = Stat::FishWarmth as usize + 1;

/// One level of one branch.
pub struct Level {
    /// Player-facing name.
    pub name: &'static str,
    /// What it does, in plain words.
    pub blurb: &'static str,
    /// Fish paid from the igloo doing the work.
    pub fish: u16,
    /// Wood paid, pooled from the whole tribe.
    pub wood: u16,
    /// Crystal paid, pooled from the whole tribe.
    pub crystal: u16,
    /// Ticks the work takes.
    pub ticks: u16,
    /// The igloo tier the work needs.
    pub tier: u8,
    /// What it does, as percent of the normal value. Levels add up.
    pub mods: &'static [(Stat, u16)],
}

/// A branch and its levels.
pub struct Branch {
    pub name: &'static str,
    pub levels: [Level; LEVELS],
}

/// Every branch and level.
pub static BRANCH_TABLE: [Branch; BRANCHES] = [
    Branch {
        name: "Snowcraft",
        levels: [
            Level {
                name: "Steady Arm",
                blurb: "Snowballs fly 15% farther and land 20% closer to the mark.",
                fish: 10,
                wood: 4,
                crystal: 0,
                ticks: secs(40.0),
                tier: TIER_IGLOO,
                mods: &[(Stat::Range, 115), (Stat::Spread, 80)],
            },
            Level {
                name: "Quick Scoop",
                blurb: "Penguins scoop a snowball 30% faster.",
                fish: 20,
                wood: 8,
                crystal: 3,
                ticks: secs(60.0),
                tier: TIER_SNOW_HALL,
                mods: &[(Stat::ScoopTime, 70)],
            },
            Level {
                name: "Hard Pack",
                blurb: "Snowballs splash 30% wider.",
                fish: 35,
                wood: 12,
                crystal: 6,
                ticks: secs(80.0),
                tier: TIER_ICE_PALACE,
                mods: &[(Stat::Splash, 130)],
            },
        ],
    },
    Branch {
        name: "Icecraft",
        levels: [
            Level {
                name: "Thick Snow",
                blurb: "Snow buildings are 50% stronger and go up 30% faster.",
                fish: 10,
                wood: 4,
                crystal: 0,
                ticks: secs(40.0),
                tier: TIER_IGLOO,
                mods: &[(Stat::BuildingHp, 150), (Stat::BuildTime, 70)],
            },
            Level {
                name: "Belly Wax",
                blurb: "Penguins slide 25% faster on ice.",
                fish: 20,
                wood: 8,
                crystal: 3,
                ticks: secs(60.0),
                tier: TIER_SNOW_HALL,
                mods: &[(Stat::IceSpeed, 125)],
            },
            Level {
                name: "Frost Walls",
                blurb: "Snow buildings no longer slump in the thaw.",
                fish: 35,
                wood: 12,
                crystal: 6,
                ticks: secs(80.0),
                tier: TIER_ICE_PALACE,
                mods: &[(Stat::Slump, 0)],
            },
        ],
    },
    Branch {
        name: "Fishery",
        levels: [
            Level {
                name: "Deep Holes",
                blurb: "Workers land fish 30% faster.",
                fish: 10,
                wood: 4,
                crystal: 0,
                ticks: secs(40.0),
                tier: TIER_IGLOO,
                mods: &[(Stat::FishTime, 70)],
            },
            Level {
                name: "Quick Lessons",
                blurb: "Camps train penguins 30% faster.",
                fish: 20,
                wood: 8,
                crystal: 3,
                ticks: secs(60.0),
                tier: TIER_SNOW_HALL,
                mods: &[(Stat::TrainTime, 70)],
            },
            Level {
                name: "Fish Stew",
                blurb: "Each fish warms half as much again, so a penguin needs fewer fish.",
                fish: 35,
                wood: 12,
                crystal: 6,
                ticks: secs(80.0),
                tier: TIER_ICE_PALACE,
                mods: &[(Stat::FishWarmth, 150)],
            },
        ],
    },
];

/// A running piece of research at an igloo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Research {
    pub branch: u8,
    pub ticks_left: u16,
    pub total: u16,
}

impl Research {
    /// How far along the work is, 0..1.
    pub fn progress(self) -> f32 {
        1.0 - self.ticks_left as f32 / self.total.max(1) as f32
    }
}

/// The level that research at `levels` in `branch` would reach next.
pub fn next_level(levels: Levels, branch: u8) -> Option<&'static Level> {
    let reached = *levels.get(branch as usize)? as usize;
    BRANCH_TABLE[branch as usize].levels.get(reached)
}

/// What a tribe's research adds up to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effects([u16; STAT_COUNT]);

/// Add up the research done so far.
pub fn effects(levels: Levels) -> Effects {
    let mut pct = [100u16; STAT_COUNT];
    for (b, &reached) in levels.iter().enumerate() {
        for level in BRANCH_TABLE[b].levels.iter().take(reached as usize) {
            for &(stat, p) in level.mods {
                pct[stat as usize] = (pct[stat as usize] as u32 * p as u32 / 100) as u16;
            }
        }
    }
    Effects(pct)
}

impl Effects {
    /// A stat as percent of normal.
    pub fn pct(self, stat: Stat) -> u16 {
        self.0[stat as usize]
    }
    /// A whole number scaled by a stat, rounded to nearest.
    pub fn scale(self, stat: Stat, n: u32) -> u32 {
        (n * self.pct(stat) as u32 + 50) / 100
    }
    /// A length scaled by a stat.
    pub fn scale_f(self, stat: Stat, x: f32) -> f32 {
        x * self.pct(stat) as f32 / 100.0
    }
    /// Snowballs a penguin of this kind holds: the one it has scooped.
    pub fn ammo(self, kind: Kind) -> u8 {
        kind.stats().ammo
    }
    /// The strength of a building.
    pub fn building_hp(self, kind: BuildKind) -> u8 {
        self.scale(Stat::BuildingHp, kind.max_hp() as u32).min(u8::MAX as u32) as u8
    }
}

impl BuildKind {
    /// Only snow buildings get stronger with Icecraft.
    pub fn max_hp_with(self, fx: Effects) -> u8 {
        if self.snowy() {
            fx.building_hp(self)
        } else {
            self.max_hp()
        }
    }
}

impl Game {
    /// What a player has learnt.
    pub fn levels_of(&self, player: u8) -> Levels {
        self.players.get(player as usize).map_or([0; BRANCHES], |p| p.research)
    }

    /// What a player's research adds up to.
    pub fn effects_of(&self, player: u8) -> Effects {
        effects(self.levels_of(player))
    }

    /// The strength of a building, with its owner's research.
    pub fn max_hp(&self, b: &Building) -> u8 {
        b.kind.max_hp_with(self.effects_of(b.owner))
    }

    /// Is some igloo of this player already working on this branch?
    pub fn researching(&self, player: u8, branch: u8) -> bool {
        self.igloos.iter().any(|ig| ig.owner == player && ig.research.is_some_and(|r| r.branch == branch))
    }

    /// Start a piece of research at an igloo. The materials are paid up
    /// front. The igloo must be a high enough tier, must not be busy, and
    /// the branch must have a level left that nobody is already working on.
    pub(super) fn start_research(&mut self, player: u8, ig: u8, branch: u8) {
        if !self.core.tech {
            return;
        }
        let Some(igloo) = self.igloos.get(ig as usize) else { return };
        if igloo.owner != player || igloo.takeover.is_some() || igloo.research.is_some() || self.researching(player, branch) {
            return;
        }
        let Some(level) = next_level(self.levels_of(player), branch) else { return };
        let (fish, wood) = price(level.fish, level.wood);
        if igloo.tier < level.tier
            || igloo.fish < fish
            || self.wood_of(player) < wood as u32
            || self.crystal_of(player) < level.crystal as u32
        {
            return;
        }
        self.igloos[ig as usize].fish -= fish;
        self.spend_wood(player, wood);
        self.spend_crystal(player, level.crystal);
        self.igloos[ig as usize].research = Some(Research { branch, ticks_left: level.ticks, total: level.ticks });
    }

    /// Research finishes on its own.
    pub(super) fn update_research(&mut self) {
        for ig in 0..self.igloos.len() {
            let Some(mut r) = self.igloos[ig].research else { continue };
            r.ticks_left = r.ticks_left.saturating_sub(1);
            if r.ticks_left > 0 {
                self.igloos[ig].research = Some(r);
                continue;
            }
            self.igloos[ig].research = None;
            let player = self.igloos[ig].owner;
            let Some(p) = self.players.get_mut(player as usize) else { continue };
            let level = &mut p.research[r.branch as usize];
            *level = (*level + 1).min(LEVELS as u8);
            self.events.push(Event::ResearchDone { player, branch: r.branch, level: *level });
        }
    }
}
