//! The rules are fixed (DESIGN.md section 0). What is left to switch is
//! for tests and the Long Night.

/// Fish and wood a worker spends to found an expansion igloo.
pub const GREAT_IGLOO_FISH: u16 = 40;
pub const GREAT_IGLOO_WOOD: u16 = 20;

/// The few switches a game still has.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoreRules {
    /// Igloo tiers and research: igloos are built up into Snow Halls and
    /// Ice Palaces, units need a tier, tribes learn upgrades. Parked: off
    /// by default.
    pub tech: bool,
    /// Penguins slowly lose warmth out in the open. Parked: off by default,
    /// kept as a switch for the Long Night.
    pub cold: bool,
    /// A tribe starts with its Chieftain and two workers (DESIGN.md 0.2)
    /// instead of the map's start.
    pub small_start: bool,
}

impl Default for CoreRules {
    fn default() -> CoreRules {
        CoreRules { tech: false, cold: false, small_start: true }
    }
}

impl CoreRules {
    /// The same rules, but every tribe starts with the people the map gives
    /// it rather than the small start: for setting up a test of something else.
    pub fn map_start(self) -> CoreRules {
        CoreRules { small_start: false, ..self }
    }
}

/// What a price of `fish` and `wood` comes to, as (fish, wood). No wood is
/// stored (DESIGN.md 0.2), so the wood share of anything but a building is
/// paid in fish instead.
pub fn price(fish: u16, wood: u16) -> (u16, u16) {
    (fish + wood, 0)
}
