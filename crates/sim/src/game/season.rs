//! Seasons and weather. The year turns on a clock everyone can see, and
//! each season changes the ground and the fighting:
//!
//! * Deep winter: heavy snowfall covers worn ground quickly.
//! * Thaw: no snowfall, snow wears away twice as fast, and snow buildings
//!   slowly slump.
//! * Midsummer: fish holes refill twice as fast, very little snow falls.
//! * Polar night: everyone throws shorter in the dark.
//!
//! Blizzards blow in now and then (never in midsummer): throws fall short
//! and penguins out in the open slowly chill, though not enough to send
//! them home. Huts and igloos shelter them, and the chill wears off once
//! the storm has passed.

use super::*;

/// One game day, dawn to dawn.
pub const DAY_TICKS: u32 = 90 * crate::TICK_RATE;

/// Whole days that have gone by.
pub fn day_of(tick: u32) -> u32 {
    tick / DAY_TICKS
}

/// Length of each season.
pub const SEASON_TICKS: u32 = 240 * crate::TICK_RATE;
pub const BLIZZARD_TICKS: u16 = secs(40.0);
/// Throw range lost in a blizzard and in the polar night.
pub const BLIZZARD_RANGE: f32 = 2.0;
pub const NIGHT_RANGE: f32 = 1.5;
/// Blizzards chill penguins in the open by one every this often...
pub const BLIZZARD_CHILL_TICKS: u32 = secs(6.0) as u32;
/// ...but never below this.
pub const BLIZZARD_CHILL_FLOOR: u8 = COLD + 1;
/// Snow buildings lose a point of strength this often in the thaw.
pub const THAW_MELT_TICKS: u32 = secs(30.0) as u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Season {
    DeepWinter = 0,
    Thaw = 1,
    Midsummer = 2,
    PolarNight = 3,
}

impl Season {
    pub fn from_u8(v: u8) -> Season {
        match v % 4 {
            1 => Season::Thaw,
            2 => Season::Midsummer,
            3 => Season::PolarNight,
            _ => Season::DeepWinter,
        }
    }
    pub fn next(self) -> Season {
        Season::from_u8(self as u8 + 1)
    }
    pub fn name(self) -> &'static str {
        match self {
            Season::DeepWinter => "Deep Winter",
            Season::Thaw => "Thaw",
            Season::Midsummer => "Midsummer",
            Season::PolarNight => "Polar Night",
        }
    }
    pub fn blurb(self) -> &'static str {
        match self {
            Season::DeepWinter => "Heavy snowfall covers worn ground.",
            Season::Thaw => "No snowfall; snow wears fast and snow buildings slump.",
            Season::Midsummer => "Fish holes refill twice as fast. Little snow.",
            Season::PolarNight => "Dark: everyone throws shorter.",
        }
    }
    /// Snowfall rate relative to normal.
    pub fn snowfall(self) -> f32 {
        match self {
            Season::DeepWinter => 2.5,
            Season::Thaw => 0.0,
            Season::Midsummer => 0.3,
            Season::PolarNight => 1.0,
        }
    }
    /// Chance per second that a blizzard starts.
    fn blizzard_chance(self) -> f32 {
        match self {
            Season::DeepWinter => 1.0 / 150.0,
            Season::PolarNight => 1.0 / 200.0,
            Season::Thaw => 1.0 / 600.0,
            Season::Midsummer => 0.0,
        }
    }
}

/// The year's state, part of [`Game`].
#[derive(Clone, Copy, Debug)]
pub struct Weather {
    pub season: Season,
    /// Ticks into the current season.
    pub season_t: u32,
    /// Ticks of blizzard left (0: clear).
    pub blizzard: u16,
    /// No blizzard starts by itself (tests that must not depend on the dice).
    /// Not sent to clients.
    pub calm: bool,
}

impl Default for Weather {
    fn default() -> Weather {
        Weather { season: Season::DeepWinter, season_t: 0, blizzard: 0, calm: false }
    }
}

impl Game {
    pub(super) fn update_weather(&mut self) {
        let w = &mut self.weather;
        w.season_t += 1;
        if w.season_t >= SEASON_TICKS {
            w.season_t = 0;
            w.season = w.season.next();
            let season = w.season;
            self.events.push(Event::SeasonChanged { season });
        }
        if self.weather.blizzard > 0 {
            self.weather.blizzard -= 1;
            if self.weather.blizzard == 0 {
                self.events.push(Event::Blizzard { on: false });
            }
        }
        if !self.tick.is_multiple_of(crate::TICK_RATE) {
            return;
        }
        if self.weather.blizzard == 0 && self.rng.chance(self.weather.season.blizzard_chance()) && !self.weather.calm {
            self.weather.blizzard = BLIZZARD_TICKS;
            self.events.push(Event::Blizzard { on: true });
        }
        if self.weather.season == Season::Thaw && self.tick.is_multiple_of(THAW_MELT_TICKS) {
            for b in 0..self.buildings.len() {
                let bd = &self.buildings[b];
                if bd.alive && bd.done() && bd.kind.snowy() && self.effects_of(bd.owner).pct(Stat::Slump) > 0 {
                    self.damage_building(b, 1);
                }
            }
        }
        if self.weather.blizzard == 0 && self.tick.is_multiple_of(BLIZZARD_CHILL_TICKS) {
            for p in &mut self.penguins {
                if p.chill > 0 && p.act != Act::Frozen {
                    p.chill -= 1;
                    p.warmth = (p.warmth + 1).min(p.max_warmth);
                }
            }
        }
        if self.weather.blizzard > 0 && self.tick.is_multiple_of(BLIZZARD_CHILL_TICKS) {
            for i in 0..self.penguins.len() {
                let p = &self.penguins[i];
                if p.inside.is_some() || p.act == Act::Frozen || p.warmth <= BLIZZARD_CHILL_FLOOR {
                    continue;
                }
                let sheltered = p.pos.dist(self.igloos[p.home as usize].door) < 3.0
                    || self.nearest_building(p.owner, BuildKind::Sauna, p.pos).is_some_and(|(d, _)| d <= SAUNA_RADIUS);
                if !sheltered {
                    let p = &mut self.penguins[i];
                    p.warmth -= 1;
                    p.chill += 1;
                }
            }
        }
    }

    /// Throw range lost to the dark and the weather.
    pub(super) fn weather_range_penalty(&self) -> f32 {
        let night = if self.weather.season == Season::PolarNight { NIGHT_RANGE } else { 0.0 };
        let storm = if self.weather.blizzard > 0 { BLIZZARD_RANGE } else { 0.0 };
        night + storm
    }

    /// Snowfall rate relative to normal, blizzards included.
    pub(super) fn snowfall_rate(&self) -> f32 {
        let storm = if self.weather.blizzard > 0 { 4.0 } else { 1.0 };
        self.weather.season.snowfall() * storm
    }

    /// Foot traffic packs snow this many times faster than normal.
    pub(super) fn trample_rate(&self) -> u16 {
        if self.weather.season == Season::Thaw {
            2
        } else {
            1
        }
    }

    /// Fish holes refill this many times faster than normal.
    pub(super) fn fish_rate(&self) -> u16 {
        if self.weather.season == Season::Midsummer {
            2
        } else {
            1
        }
    }
}
