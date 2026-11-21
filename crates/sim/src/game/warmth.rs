//! Warmth as supply: the cold bites away from home.
//!
//! With `CoreRules::cold` on (off by default for now), penguins left out in
//! the snow lose warmth, unless they can reach heat: a friendly warming
//! hut, the door of a friendly igloo, or the ground of an igloo they are
//! taking over. The cold never freezes a penguin on its own, but a
//! long march wears it down until a few hits will, so every attack wants
//! a heat line behind it.
//!
//! The cold bites faster in the polar night and in blizzards, and slower
//! in midsummer. Penguins at work (gathering, chopping, building) keep
//! half of it out. In shelter the lost warmth slowly returns.

use super::*;

/// Ticks out in the cold that cost a penguin one warmth. Six seconds is
/// slow enough to cross open ground unhurt, quick enough that a long
/// march in the open starts to bite.
pub const EXPOSE_TICKS: u16 = secs(10.0);
/// Working penguins keep this much of the cold out.
pub const WORKING_FACTOR: u16 = 2;
/// The polar night bites this many times faster...
pub const NIGHT_FACTOR: u16 = 2;
/// ...and so does a blizzard.
pub const BLIZZARD_FACTOR: u16 = 2;
/// Midsummer is mild: this many times slower.
pub const SUMMER_FACTOR: u16 = 2;
/// A friendly igloo's door shelters penguins this close.
pub const DOOR_RADIUS: f32 = 4.0;
/// ...and so does the ground this close around an igloo being taken over.
pub const SIEGE_HUDDLE: f32 = 8.0;
/// The cold on its own never takes warmth below this.
pub const EXPOSURE_FLOOR: u8 = COLD + 1;

impl Game {
    /// Is penguin `i` out of the cold? It is sheltered inside an igloo, at
    /// the door of a friendly igloo, in the huddle around an igloo its side
    /// is taking over, or beside a friendly warming hut.
    pub fn sheltered(&self, i: usize) -> bool {
        let Some(p) = self.penguins.get(i) else { return true };
        if p.inside.is_some() {
            return true;
        }
        let team = self.team(p.owner);
        let ours = |ig: &Igloo| ig.owner != NEUTRAL && self.team(ig.owner) == team;
        let sieging = |ig: &Igloo| ig.takeover.as_ref().is_some_and(|t| self.team(t.by) == team);
        if self.igloos.iter().any(|ig| (ours(ig) && p.pos.dist(ig.door) <= DOOR_RADIUS) || (sieging(ig) && p.pos.dist(ig.center) <= SIEGE_HUDDLE)) {
            return true;
        }
        // A friendly warming hut.
        self.nearest_building(p.owner, BuildKind::Sauna, p.pos).is_some_and(|(d, _)| d <= SAUNA_RADIUS)
    }

    /// Ticks of exposure that cost one warmth right now.
    fn exposure_period(&self, working: bool) -> u16 {
        let mut period = EXPOSE_TICKS;
        if self.weather.season == Season::PolarNight {
            period /= NIGHT_FACTOR;
        }
        if self.weather.blizzard > 0 {
            period /= BLIZZARD_FACTOR;
        }
        if self.weather.season == Season::Midsummer {
            period *= SUMMER_FACTOR;
        }
        if working {
            period *= WORKING_FACTOR;
        }
        period
    }

    /// The cold takes a bite out of everyone caught out in the open.
    pub(super) fn update_exposure(&mut self) {
        if !self.core.cold {
            return;
        }
        for i in 0..self.penguins.len() {
            let p = &self.penguins[i];
            if p.inside.is_some() || p.act == Act::Frozen || p.owner == NEUTRAL || p.carried_by.is_some() || p.prison_t > 0 {
                continue;
            }
            let working = matches!(p.order, Order::Gather { .. } | Order::Chop { .. } | Order::Build { .. } | Order::Train { .. } | Order::Tend { .. });
            let period = self.exposure_period(working);
            self.penguins[i].expose_t = self.penguins[i].expose_t.saturating_add(1);
            if self.penguins[i].expose_t < period {
                continue;
            }
            self.penguins[i].expose_t = 0;
            // Back in the warm, the cold gives back what it took.
            if self.sheltered(i) {
                let p = &mut self.penguins[i];
                if p.frost > 0 {
                    p.frost -= 1;
                    p.warmth = (p.warmth + 1).min(p.max_warmth);
                }
                continue;
            }
            let p = &mut self.penguins[i];
            if p.warmth <= EXPOSURE_FLOOR {
                continue;
            }
            p.warmth -= 1;
            p.frost = p.frost.saturating_add(1);
        }
    }
}
