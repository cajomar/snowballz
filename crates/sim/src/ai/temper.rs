//! What a computer player is like this game. Each trait runs from 0 to 1,
//! and 0.5 everywhere plays exactly as the plain rules do; a game rolls
//! fresh traits for every computer (`Game::roll_ai_tempers`), with one or
//! two of them pushed to an extreme so each has a character worth meeting.
//! A title hints at the strongest trait; the rest shows in how it plays.

use super::*;

/// How a computer starts the line, at the end of a game, saying what it
/// was like; the end screen looks for it.
pub const REVEAL: &str = "What I was like this game: ";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Temper {
    /// Marches with smaller groups, against more guards, keeping fewer home.
    pub bold: f32,
    /// Weighs grudges heavily and is slow to forget them.
    pub spite: f32,
    /// Grants longer truces, to more people; a low one breaks truces when it
    /// smells a win.
    pub honour: f32,
    /// Raises more workers and builds its igloos up sooner.
    pub greed: f32,
    /// Puts up more towers.
    pub caution: f32,
    /// Talks more, and taunts more.
    pub chatter: f32,
    /// The fighter it raises most of, unless advised otherwise.
    pub favourite: Option<Kind>,
    /// Rolled for this game, rather than the plain default.
    pub rolled: bool,
}

impl Default for Temper {
    fn default() -> Temper {
        Temper { bold: 0.5, spite: 0.5, honour: 0.5, greed: 0.5, caution: 0.5, chatter: 0.5, favourite: None, rolled: false }
    }
}

/// `base` at 0.5, `base * spread` at 1 and `base / spread` at 0.
pub(super) fn around(t: f32, base: f32, spread: f32) -> f32 {
    base * spread.powf(2.0 * t - 1.0)
}

/// Honour under this breaks truces.
pub(super) const TREACHEROUS: f32 = 0.3;

impl Temper {
    pub fn roll(rng: &mut Rng) -> Temper {
        let mut t = Temper { rolled: true, ..Temper::default() };
        let mut traits = [&mut t.bold, &mut t.spite, &mut t.honour, &mut t.greed, &mut t.caution, &mut t.chatter];
        for x in traits.iter_mut() {
            **x = rng.range(0.25, 0.75);
        }
        // One or two traits at an extreme, at either end.
        for _ in 0..1 + rng.below(2) {
            let i = rng.below(traits.len() as u32) as usize;
            *traits[i] = if rng.chance(0.5) { rng.range(0.0, 0.12) } else { rng.range(0.88, 1.0) };
        }
        if rng.chance(0.6) {
            t.favourite = Some(FAVOURITES[rng.below(FAVOURITES.len() as u32) as usize]);
        }
        t
    }

    /// The trait that stands out most, as a title ("the Reckless").
    pub fn title(&self) -> &'static str {
        let named = [
            (self.bold, "the Reckless", "the Patient"),
            (self.spite, "the Grudge-Holder", "the Forgiving"),
            (self.honour, "the Honourable", "the Treacherous"),
            (self.greed, "the Builder", "the Spartan"),
            (self.caution, "the Cautious", "the Careless"),
            (self.chatter, "the Loudmouth", "the Silent"),
        ];
        let (x, high, low) = named.iter().copied().max_by(|a, b| (a.0 - 0.5).abs().partial_cmp(&(b.0 - 0.5).abs()).unwrap()).unwrap();
        if (x - 0.5).abs() >= 0.3 {
            return if x > 0.5 { high } else { low };
        }
        match self.favourite {
            Some(Kind::Slinger) => "the Slinger",
            Some(Kind::Packer) => "the Packer",
            Some(Kind::Skier) => "the Skier",
            Some(Kind::Shield) => "the Shieldwall",
            _ => "the Steady",
        }
    }

    /// Every trait that leans, in plain words: what it really was.
    pub fn describe(&self) -> String {
        let mut out = Vec::new();
        let mut lean = |x: f32, high: &str, low: &str| {
            if x >= 0.65 {
                out.push(high.to_string());
            } else if x <= 0.35 {
                out.push(low.to_string());
            }
        };
        lean(self.bold, "attacks early with small groups", "waits for a big army before it marches");
        lean(self.spite, "never forgets a slight", "forgives quickly");
        lean(self.honour, "keeps its word and grants long truces", "breaks truces when it smells a win");
        lean(self.greed, "builds a big economy", "keeps few workers");
        lean(self.caution, "builds many towers", "hardly builds towers");
        lean(self.chatter, "talks a lot", "says little");
        if let Some(k) = self.favourite {
            out.push(format!("loves {}", k.stats().plural.to_lowercase()));
        }
        if out.is_empty() {
            "plays it down the middle".to_string()
        } else {
            out.join(", ")
        }
    }
}
