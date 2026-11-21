//! The kinds of penguin and what makes each one different.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Kind {
    /// The gatherer: fishes, chops, mines and builds; doesn't fight.
    Worker = 0,
    /// Basic snowball fighter; can take over igloos.
    Snowballer = 1,
    /// Long range, poor aim, thin-skinned.
    Slinger = 2,
    /// Big, slow snowballs that splash wide and smash buildings.
    Packer = 3,
    /// Belly-sled raider: fast, few snowballs, good at carrying prisoners off.
    Sledder = 4,
    /// Carries an ice shield: friends beside it take less from direct hits.
    Shield = 5,
    /// The tribe's hero: a big crowned emperor penguin. One at a time.
    Chieftain = 6,
    /// The medic: warms cold friends and thaws the frozen into its tribe.
    Warmer = 7,
    /// Fast on snow, slow on bare ice: a flanker and raider.
    Skier = 8,
}

pub struct UnitStats {
    pub name: &'static str,
    pub plural: &'static str,
    pub blurb: &'static str,
    /// For the tooltip: what it is good against, and what to fear.
    pub beats: &'static str,
    pub beaten_by: &'static str,
    pub fish: u16,
    pub wood: u16,
    /// Max warmth relative to the map's snowballer warmth (workers use the map's own).
    pub warmth: f32,
    pub speed: f32,
    pub range: f32,
    /// Aim spread per tile of distance, relative to a snowballer.
    pub spread: f32,
    pub direct: u8,
    pub splash: u8,
    pub splash_radius: f32,
    /// 1 for kinds that throw, 0 for those that don't: a penguin holds the one
    /// snowball it has just scooped, and scoops again after each throw.
    pub ammo: u8,
    pub cooldown: u16,
    /// Drawn this much bigger or smaller.
    pub scale: f32,
    /// The igloo tier this kind can be trained in.
    pub tier: u8,
}

const STATS: [UnitStats; 9] = [
    UnitStats {
        name: "Worker",
        plural: "workers",
        blurb: "Builds, chops and mines, and fishes when it has a fishing slot at an igloo or Sushi House. Every fighter starts out as a worker.",
        beats: "",
        beaten_by: "",
        fish: 4,
        wood: 0,
        warmth: 1.0,
        speed: 2.3,
        range: 0.0,
        spread: 1.0,
        direct: 0,
        splash: 0,
        splash_radius: 0.0,
        ammo: 0,
        cooldown: 0,
        scale: 1.0,
        tier: 0,
    },
    UnitStats {
        name: "Snowballer",
        plural: "snowballers",
        blurb: "The backbone of an army: cheap, steady, good at everything, best at nothing.",
        beats: "Anything in numbers",
        beaten_by: "Packer splash on a crowd",
        fish: 6,
        wood: 0,
        warmth: 1.0,
        speed: 2.6,
        range: 6.0,
        spread: 1.0,
        direct: 3,
        splash: 1,
        splash_radius: 1.1,
        ammo: 1,
        cooldown: secs(0.35),
        scale: 1.0,
        tier: 0,
    },
    UnitStats {
        name: "Slinger",
        plural: "slingers",
        blurb: "Throws much farther than anyone, but can't hit anyone close and backs away when they come. Needs a line in front.",
        beats: "Towers, crowds, Shieldless lines",
        beaten_by: "Skiers, anyone who gets close",
        fish: 7,
        wood: 1,
        warmth: 0.7,
        speed: 2.5,
        range: 9.0,
        spread: 1.5,
        direct: 2,
        splash: 1,
        splash_radius: 1.0,
        ammo: 1,
        cooldown: secs(0.6),
        scale: 0.95,
        tier: 1,
    },
    UnitStats {
        name: "Packer",
        plural: "packers",
        blurb: "Siege: goes for walls, towers and igloos first. Its huge snowballs smash them and splash wide.",
        beats: "Forts, towers, igloos, tight crowds",
        beaten_by: "Slingers and Skiers (slow, one shot)",
        fish: 9,
        wood: 0,
        warmth: 1.3,
        speed: 2.1,
        range: 5.0,
        spread: 1.2,
        direct: 4,
        splash: 2,
        splash_radius: 1.7,
        ammo: 1,
        cooldown: secs(1.6),
        scale: 1.18,
        tier: 1,
    },
    UnitStats {
        name: "Sled",
        plural: "sleds",
        blurb: "Doesn't fight. Fetches frozen penguins, anyone's, and drags them home, where they thaw into your tribe.",
        beats: "Wins you the frozen after a fight",
        beaten_by: "Skiers, and anyone who throws at it",
        fish: 7,
        wood: 1,
        warmth: 0.9,
        speed: 3.2,
        range: 0.0,
        spread: 1.1,
        direct: 0,
        splash: 0,
        splash_radius: 0.0,
        ammo: 0,
        cooldown: 0,
        scale: 1.0,
        tier: 1,
    },
    UnitStats {
        name: "Shieldbearer",
        plural: "shieldbearers",
        blurb: "Steps in front of its friends: its ice slab blocks snowballs thrown at anyone behind it.",
        beats: "Slingers, Snowballer lines",
        beaten_by: "Packer splash, attacks from the side",
        fish: 8,
        wood: 1,
        warmth: 1.5,
        speed: 2.2,
        range: 5.0,
        spread: 1.1,
        direct: 2,
        splash: 1,
        splash_radius: 1.0,
        ammo: 1,
        cooldown: secs(0.8),
        scale: 1.08,
        tier: 1,
    },
    UnitStats {
        name: "Chieftain",
        plural: "chieftains",
        blurb: "Leads one igloo's army: soldiers with nothing to do follow him and form up around him, so move him to move them. Nobody near him runs away, and his War Cry makes friends faster and quicker to throw. If he freezes, his igloo raises a new one a minute later.",
        beats: "",
        beaten_by: "",
        fish: 40,
        wood: 20,
        warmth: 2.6,
        speed: 2.2,
        range: 6.5,
        spread: 0.9,
        direct: 5,
        splash: 2,
        splash_radius: 1.4,
        ammo: 1,
        cooldown: secs(0.5),
        scale: 1.55,
        tier: 1,
    },
    UnitStats {
        name: "Warmer",
        plural: "warmers",
        blurb: "Doesn't fight. Warms the coldest friend near it, and thaws frozen penguins into your tribe on the spot.",
        beats: "Keeps an army fighting longer",
        beaten_by: "Skiers",
        fish: 9,
        wood: 0,
        warmth: 1.0,
        speed: 2.3,
        range: 0.0,
        spread: 1.0,
        direct: 0,
        splash: 0,
        splash_radius: 0.0,
        ammo: 0,
        cooldown: 0,
        scale: 1.0,
        tier: 0,
    },
    UnitStats {
        name: "Skier",
        plural: "skiers",
        blurb: "Fast on snow, slow on bare ice. Goes round a fight for Slingers, Warmers and Sleds; its charge knocks a thrower flat.",
        beats: "Slingers, Warmers, Sleds, workers",
        beaten_by: "Shieldbearers (it bounces off), Snowballer lines",
        fish: 7,
        wood: 2,
        warmth: 0.9,
        speed: 4.0,
        range: 5.0,
        spread: 1.1,
        direct: 2,
        splash: 1,
        splash_radius: 1.0,
        ammo: 1,
        cooldown: secs(0.5),
        scale: 1.0,
        tier: 1,
    },
];

/// Skiers move this fast on bare ice, compared with snow.
pub const SKIER_ICE_SPEED: f32 = 0.6;

/// Shieldbearers cover friends this close.
pub const SHIELD_RADIUS: f32 = 1.3;
impl Kind {
    pub const ALL: [Kind; 9] = [
        Kind::Worker,
        Kind::Snowballer,
        Kind::Slinger,
        Kind::Packer,
        Kind::Sledder,
        Kind::Shield,
        Kind::Chieftain,
        Kind::Warmer,
        Kind::Skier,
    ];

    pub fn from_u8(v: u8) -> Kind {
        Kind::ALL.get(v as usize).copied().unwrap_or(Kind::Worker)
    }
    pub fn stats(self) -> &'static UnitStats {
        &STATS[self as usize]
    }
    /// The igloo tier needed before this kind can be trained.
    pub fn tier(self) -> u8 {
        self.stats().tier
    }
    /// Throws snowballs (and so can take over igloos).
    pub fn fights(self) -> bool {
        self.stats().ammo > 0
    }
    /// Follows orders as a soldier rather than working.
    pub fn soldier(self) -> bool {
        self != Kind::Worker
    }
}
