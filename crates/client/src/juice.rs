//! Game feel: hit flashes, camera shake and the end-of-game tally.
//!
//! All of it is counted on this client from events the server already sends
//! (and from snapshots), so nothing here can change the simulation.

use sim::math::Vec2;
use sim::proto::{PenguinView, WorldView};
use sim::rng::Rng;

/// How long a white hit flash lasts, in seconds.
const FLASH_SECS: f32 = 0.22;
/// Seconds between army-size samples on the end screen.
pub const SAMPLE_SECS: f32 = 5.0;

/// Hit flashes and camera shake.
pub struct Juice {
    /// Flash seconds left, per penguin id.
    flash: Vec<f32>,
    /// Cheer seconds left, per penguin id (trained, thawed, won).
    cheer: Vec<f32>,
    /// Shake amplitude in fractions of the viewport height, how much is
    /// left of it and how much it started with.
    amp: f32,
    left: f32,
    total: f32,
    /// Wobble phase, so the shake curves instead of sliding.
    phase: f32,
    rng: Rng,
    /// Whether camera shake is on at all (a setting).
    shake_on: bool,
}

impl Juice {
    pub fn new() -> Juice {
        Juice { flash: Vec::new(), cheer: Vec::new(), amp: 0.0, left: 0.0, total: 0.0, phase: 0.0, rng: Rng::new(3), shake_on: true }
    }

    /// Turn camera shake on or off (settings).
    pub fn set_shake(&mut self, on: bool) {
        self.shake_on = on;
    }

    pub fn update(&mut self, dt: f32) {
        for f in self.flash.iter_mut().chain(self.cheer.iter_mut()) {
            *f = (*f - dt).max(0.0);
        }
        self.left = (self.left - dt).max(0.0);
        self.phase += dt * 46.0;
    }

    /// Make a penguin cheer for `secs`.
    pub fn cheer(&mut self, id: u16, secs: f32) {
        if self.cheer.len() <= id as usize {
            self.cheer.resize(id as usize + 1, 0.0);
        }
        self.cheer[id as usize] = self.cheer[id as usize].max(secs);
    }

    pub fn cheering(&self, id: u16) -> bool {
        self.cheer.get(id as usize).is_some_and(|&c| c > 0.0)
    }

    /// 0..1 white flash for a penguin, for the renderers.
    pub fn flash_of(&self, id: u16) -> f32 {
        self.flash.get(id as usize).map_or(0.0, |&f| (f / FLASH_SECS).clamp(0.0, 1.0))
    }

    /// Flash every penguin within `radius` of a snowball impact.
    pub fn flash_near(&mut self, penguins: &[PenguinView], pos: Vec2, radius: f32) {
        if self.flash.len() < penguins.len() {
            self.flash.resize(penguins.len(), 0.0);
        }
        for p in penguins {
            if !p.inside && p.pos.dist(pos) <= radius {
                self.flash[p.id as usize] = FLASH_SECS;
            }
        }
    }

    /// Shake the camera. `amp` is a fraction of the viewport height.
    pub fn shake(&mut self, amp: f32, secs: f32) {
        if !self.shake_on {
            return;
        }
        let now = if self.left > 0.0 { self.amp * self.left / self.total.max(0.001) } else { 0.0 };
        // A stronger kick wins; weaker ones don't pile onto it.
        if amp <= now {
            return;
        }
        self.amp = amp;
        self.left = secs;
        self.total = secs;
        self.phase = self.rng.range(0.0, std::f32::consts::TAU);
    }

    /// Current camera offset, in fractions of the viewport height.
    pub fn shake_offset(&self) -> [f32; 2] {
        // Off kills even a shake already in flight.
        if !self.shake_on || self.left <= 0.0 {
            return [0.0, 0.0];
        }
        let k = self.left / self.total.max(0.001);
        let a = self.amp * k * k;
        [self.phase.sin() * a, (self.phase * 1.7).cos() * a * 0.7]
    }
}

/// One player's tallies, as the end screen shows them.
#[derive(Clone, Copy, Default)]
pub struct Tally {
    pub trained: u32,
    pub thrown: u32,
    pub hits: u32,
    pub frozen: u32,
    pub captured: u32,
    pub prisoners: u32,
}

/// A deed worth counting on the end screen.
#[derive(Clone, Copy)]
pub enum Feat {
    Trained,
    Thrown,
    Hit,
    Frozen,
    Captured,
    Prisoner,
}

/// What happened in the match, counted from events, plus the army graph.
pub struct Stats {
    pub tally: Vec<Tally>,
    /// Army size per player, one row every [`SAMPLE_SECS`].
    pub army: Vec<Vec<u16>>,
    /// Turning points in order: game second and what happened.
    pub story: Vec<(u32, String)>,
    next: f32,
}

impl Stats {
    pub fn new(players: usize) -> Stats {
        Stats { tally: vec![Tally::default(); players], army: Vec::new(), story: Vec::new(), next: 0.0 }
    }

    pub fn add(&mut self, who: u8, what: Feat) {
        let Some(t) = self.tally.get_mut(who as usize) else { return };
        let n = match what {
            Feat::Trained => &mut t.trained,
            Feat::Thrown => &mut t.thrown,
            Feat::Hit => &mut t.hits,
            Feat::Frozen => &mut t.frozen,
            Feat::Captured => &mut t.captured,
            Feat::Prisoner => &mut t.prisoners,
        };
        *n += 1;
    }

    /// Remember a turning point, such as an igloo changing hands.
    pub fn note(&mut self, secs: u32, text: String) {
        self.story.push((secs, text));
    }

    /// Take an army-size sample every few seconds while the game runs.
    pub fn sample(&mut self, dt: f32, world: &WorldView) {
        self.next -= dt;
        if self.next > 0.0 || self.tally.is_empty() {
            return;
        }
        self.next = SAMPLE_SECS;
        let mut row = vec![0u16; self.tally.len()];
        for p in &world.penguins {
            if let Some(n) = row.get_mut(p.owner as usize) {
                *n += 1;
            }
        }
        self.army.push(row);
        if self.army.len() > 120 {
            self.army.remove(0);
        }
    }

    /// Made-up numbers for the `SNOWBALLZ_ENDSCREEN` screenshot switch.
    pub fn fill_fake(&mut self) {
        let players = self.tally.len().max(1);
        let mut rng = Rng::new(11);
        for (i, t) in self.tally.iter_mut().enumerate() {
            t.trained = 18 + rng.below(30) + i as u32 * 4;
            t.thrown = 150 + rng.below(240);
            t.hits = t.thrown / 2 + rng.below(40);
            t.frozen = 3 + rng.below(14);
            t.captured = 1 + rng.below(5);
            t.prisoners = rng.below(9);
        }
        self.story = vec![(95, "Flipper Bot took Tux Bot's igloo".into()), (210, "Tux Bot's igloo fell in".into())];
        self.army.clear();
        for s in 0..60 {
            let f = s as f32 / 59.0;
            self.army.push(
                (0..players)
                    .map(|i| {
                        let wave = (s as f32 * 0.55 + i as f32 * 2.1).sin() * 6.0;
                        (13.0 + i as f32 * 4.0 + f * 26.0 + wave).max(2.0) as u16
                    })
                    .collect(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tallies_go_to_their_player() {
        let mut s = Stats::new(3);
        s.add(1, Feat::Thrown);
        s.add(1, Feat::Hit);
        s.add(1, Feat::Hit);
        s.add(2, Feat::Captured);
        s.add(0, Feat::Frozen);
        s.add(0, Feat::Trained);
        s.add(0, Feat::Prisoner);
        // Nobody in particular: dropped, not counted.
        s.add(255, Feat::Hit);
        assert_eq!((s.tally[0].trained, s.tally[0].frozen, s.tally[0].prisoners), (1, 1, 1));
        assert_eq!((s.tally[1].thrown, s.tally[1].hits), (1, 2));
        assert_eq!(s.tally[2].captured, 1);
        assert_eq!(s.tally[2].hits, 0);
    }

    #[test]
    fn turning_points_keep_their_order() {
        let mut s = Stats::new(2);
        s.note(40, "one".into());
        s.note(75, "two".into());
        assert_eq!(s.story, vec![(40, "one".to_string()), (75, "two".to_string())]);
    }

    #[test]
    fn fake_stats_are_plausible() {
        let mut s = Stats::new(2);
        s.fill_fake();
        assert!(s.tally.iter().all(|t| t.thrown > t.hits && t.hits > 0 && t.trained > 0));
        assert_eq!(s.army.len(), 60);
        assert!(s.army.iter().all(|row| row.len() == 2 && row.iter().all(|&n| n >= 2)));
    }

    #[test]
    fn a_kick_shakes_then_settles() {
        let mut j = Juice::new();
        assert_eq!(j.shake_offset(), [0.0, 0.0]);
        j.shake(0.01, 0.4);
        let o = j.shake_offset();
        assert!(o[0].abs() + o[1].abs() > 0.0, "kick did nothing");
        assert!(o[0].abs() <= 0.01 && o[1].abs() <= 0.01);
        for _ in 0..40 {
            j.update(1.0 / 60.0);
        }
        assert_eq!(j.shake_offset(), [0.0, 0.0]);
    }

    #[test]
    fn shakes_can_be_switched_off() {
        let mut j = Juice::new();
        j.shake(0.01, 0.4);
        assert!(j.shake_offset()[0].abs() + j.shake_offset()[1].abs() > 0.0, "kick did nothing");
        j.set_shake(false);
        assert_eq!(j.shake_offset(), [0.0, 0.0], "in-flight shake still moving");
        j.shake(0.02, 0.4);
        assert_eq!(j.shake_offset(), [0.0, 0.0], "kick landed while off");
        j.set_shake(true);
        j.shake(0.01, 0.4);
        assert!(j.shake_offset()[0].abs() + j.shake_offset()[1].abs() > 0.0, "still dead after re-enable");
    }

    #[test]
    fn a_bigger_kick_wins_over_a_smaller_one() {
        let mut j = Juice::new();
        j.shake(0.02, 0.5);
        j.shake(0.004, 0.2);
        assert_eq!(j.amp, 0.02);
        j.shake(0.03, 0.5);
        assert_eq!(j.amp, 0.03);
    }
}
