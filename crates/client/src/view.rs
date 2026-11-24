//! Client-side view of a running game: applies snapshots, keeps a smooth
//! render clock, spawns cosmetic effects and builds the [`Scene`].

use sim::game::{Act, BuildKind, Carry, Event, HitKind, Kind, Snowball, NEUTRAL, PLAYER_COLORS};
use sim::map::Ground;
use sim::math::{v2, Vec2};
use sim::proto::{GameInfo, WorldView};

use crate::audio::Sfx;
use crate::effects::{Effects, Particle};
use crate::juice::{Feat, Juice, Stats};
use crate::mathx::v3;
use crate::scene::{Anim, BuildingDraw, IglooDraw, Marker, PenguinDraw, ResourceDraw, SealDraw, Scene};

/// Fog levels (see `GameView::fog`). Ground just out of sight stays plainly
/// dark for `FOG_DARK` updates (0.3 s each), then takes `FOG_REDRAW` more to
/// turn into the hand-drawn old map.
pub const FOG_UNKNOWN: u8 = 1;
pub const FOG_JUST_LOST: u8 = 2;
pub const FOG_DARK: u32 = 160;
pub const FOG_REDRAW: u32 = 90;
pub const FOG_OLD_MAP: u8 = (FOG_JUST_LOST as u32 + FOG_DARK + FOG_REDRAW) as u8;

pub struct GameView {
    pub world: WorldView,
    /// Render clock in fractional ticks (slightly behind the newest snapshot).
    pub clock: f64,
    pub effects: Effects,
    /// Hit flashes and camera shake.
    pub juice: Juice,
    /// Match tallies and the army graph, counted from events here.
    pub stats: Stats,
    /// Camera centre and how far it sees (in tiles), for "near the camera".
    pub camera: Vec2,
    pub view_radius: f32,
    pub flying: Vec<Snowball>,
    pending_impacts: Vec<(f32, Vec2, HitKind, Vec2)>,
    /// Throws waiting for the render clock to reach them (tick, from).
    pending_throws: Vec<(f32, Vec2)>,
    /// Seconds until Warmers blow their next puffs of hot air.
    puff_t: f32,
    /// Positional sounds for the app to play, timed to the render clock.
    pub sounds: Vec<(Sfx, Vec2)>,
    footprint: Vec<(Vec2, f32, bool)>,
    /// Axe strokes heard so far per penguin, to time chopping sounds.
    chop_beat: Vec<u32>,
    /// Render time of the last igloo-hit thud; one per 0.2 s at most.
    last_igloo_thud: f32,
    smoke_t: f32,
    /// Seconds until the next frost sparkles on penguins out in the cold.
    frost_t: f32,
    pub tint_version: u32,
    /// Whole tiles of the Long Night's ring last drawn.
    ring_tiles: i32,
    /// Fog of war per tile for your team: 0 seen now, [`FOG_UNKNOWN`] never
    /// seen, and from [`FOG_JUST_LOST`] up the age of ground seen before.
    /// None when there's nothing to hide.
    pub fog: Option<Vec<u8>>,
    fog_beat: u32,
    /// Seconds until the fog is worked out again.
    fog_t: f32,
    pub time: f32,
    started: bool,
    /// Eased weather for drawing (see [`Scene`]).
    night: f32,
    storm: f32,
    summer: f32,
    thaw: f32,
    snowfall: f32,
}

pub fn player_color(info: &GameInfo, p: u8) -> [u8; 3] {
    info.players.get(p as usize).map_or([235, 235, 235], |pl| PLAYER_COLORS[pl.color as usize % PLAYER_COLORS.len()])
}

impl GameView {
    pub fn new(info: GameInfo) -> GameView {
        let players = info.players.len();
        let mut fog = (info.you != NEUTRAL && std::env::var("SNOWBALLZ_NOFOG").is_err()).then(|| vec![FOG_UNKNOWN; (info.map.w * info.map.h) as usize]);
        // Screenshot helper: pretend most of the map was explored earlier.
        if let (Some(f), true) = (fog.as_mut(), std::env::var("SNOWBALLZ_FOGMEM").is_ok()) {
            for (i, l) in f.iter_mut().enumerate() {
                if (i as i32 % info.map.w) < info.map.w * 2 / 3 {
                    // SNOWBALLZ_FOGMEM=dark shows the freshly lost look instead.
                    *l = if std::env::var("SNOWBALLZ_FOGMEM").is_ok_and(|v| v == "dark") { FOG_JUST_LOST } else { FOG_OLD_MAP };
                }
            }
        }
        GameView {
            world: WorldView::new(info),
            clock: 0.0,
            effects: Effects::new(),
            juice: Juice::new(),
            stats: Stats::new(players),
            camera: Vec2::ZERO,
            view_radius: 40.0,
            flying: Vec::new(),
            pending_impacts: Vec::new(),
            pending_throws: Vec::new(),
            puff_t: 0.0,
            sounds: Vec::new(),
            footprint: Vec::new(),
            chop_beat: Vec::new(),
            last_igloo_thud: -1.0,
            smoke_t: 0.0,
            frost_t: 0.0,
            tint_version: 1,
            ring_tiles: -1,
            fog,
            fog_t: 0.0,
            fog_beat: 0,
            time: 0.0,
            started: false,
            night: 0.0,
            storm: 0.0,
            summer: 0.0,
            thaw: 0.0,
            snowfall: 1.0,
        }
    }

    pub fn info(&self) -> &GameInfo {
        &self.world.info
    }

    /// Apply a snapshot; returns its events for the HUD.
    pub fn apply(&mut self, snap: &[u8]) -> Vec<Event> {
        let events = match self.world.apply(snap) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("bad snapshot: {e}");
                return Vec::new();
            }
        };
        // Screenshot helper (see README): pretend every igloo is grander.
        static FORCE_TIER: std::sync::OnceLock<Option<u8>> = std::sync::OnceLock::new();
        if let Some(t) = *FORCE_TIER.get_or_init(|| std::env::var("SNOWBALLZ_FORCE_TIER").ok().and_then(|t| t.parse().ok())) {
            for ig in self.world.igloos.iter_mut() {
                ig.tier = t;
            }
        }
        let target = self.world.tick as f64 - 1.6;
        if !self.started || (self.clock - target).abs() > 6.0 {
            self.clock = target;
            self.started = true;
        }
        for e in &events {
            match e {
                Event::Thrown(s) => {
                    self.pending_throws.push((s.start_tick as f32, s.from));
                    self.flying.push(s.clone());
                    if let Some(owner) = self.world.penguins.get(s.thrower as usize).map(|p| p.owner) {
                        self.stats.add(owner, Feat::Thrown);
                    }
                }
                Event::Landed { id, pos, hit } => {
                    let ball = self.flying.iter().find(|s| s.id == *id);
                    let (at, dir) = match ball {
                        Some(s) => ((s.start_tick + s.flight as u32) as f32, s.to - s.from),
                        None => (self.world.tick as f32, v2(0.0, 1.0)),
                    };
                    if matches!(hit, HitKind::Direct | HitKind::Splash) {
                        if let Some(owner) = ball.and_then(|s| self.world.penguins.get(s.thrower as usize)).map(|p| p.owner) {
                            self.stats.add(owner, Feat::Hit);
                        }
                    }
                    self.pending_impacts.push((at, *pos, *hit, dir));
                }
                Event::Frozen { penguin } => {
                    if let Some(p) = self.world.penguins.get(*penguin as usize) {
                        let z = self.world.info.map.height_at(p.pos);
                        self.effects.frost_burst(v3(p.pos.x, p.pos.y, z + 0.55));
                        self.sounds.push((Sfx::Freeze, p.pos));
                        self.stats.add(p.owner, Feat::Frozen);
                    // A Chieftain falling is a bigger loss: the tribe's heart goes with it.
                    if p.kind == Kind::Chieftain {
                        self.effects.fall(p.pos);
                    }
                    }
                }
                Event::Imprisoned { penguin, igloo } => {
                    if let (Some(p), Some(ig)) = (self.world.penguins.get(*penguin as usize), self.world.igloos.get(*igloo as usize)) {
                        if p.owner != ig.owner {
                            self.stats.add(ig.owner, Feat::Prisoner);
                        }
                    }
                }
                Event::WarCry { penguin, pos } => {
                    // The shout rings out over the snow.
                    let c = self
                        .world
                        .penguins
                        .get(*penguin as usize)
                        .map(|p| player_color(&self.world.info, p.owner))
                        .unwrap_or([255, 240, 180]);
                    self.effects.shout(*pos, c, sim::game::WAR_CRY_RADIUS);
                    self.sounds.push((Sfx::Shout, *pos));
                }
                Event::Captured { igloo, from, to } => {
                    let c = player_color(&self.world.info, *to);
                    let ig = &self.world.info.igloos[*igloo as usize];
                    let z = self.world.info.map.height_at(ig.center);
                    self.effects.capture_burst(v3(ig.center.x, ig.center.y, z + 0.8), c);
                    if self.on_screen(ig.center) {
                        self.juice.shake(0.014, 0.55);
                    }
                    self.stats.add(*to, Feat::Captured);
                    let who = |i: u8| self.world.info.players.get(i as usize).map_or("Someone", |p| p.name.as_str());
                    let text = match from {
                        f if *f == sim::game::NEUTRAL => format!("{} took an empty igloo", who(*to)),
                        _ => format!("{} took {}'s igloo", who(*to), who(*from)),
                    };
                    self.stats.note(self.world.tick / sim::TICK_RATE, text);
                }
                Event::Collapsed { igloo, owner } => {
                    let who = self.world.info.players.get(*owner as usize).map_or("Someone", |p| p.name.as_str());
                    self.stats.note(self.world.tick / sim::TICK_RATE, format!("{who}'s igloo fell in"));
                    let ig = &self.world.info.igloos[*igloo as usize];
                    let z = self.world.info.map.height_at(ig.center);
                    // The dome caves in: a big spray of snow, then drifting puffs.
                    self.effects.collapse(v3(ig.center.x, ig.center.y, z + 0.9));
                    // Dust settles into a faint grey ring around where the dome stood.
                    self.effects.dust_ring(ig.center, crate::meshgen::IGLOO_RADIUS);
                    self.sounds.push((Sfx::Crumble, ig.center));
                    if self.on_screen(ig.center) {
                        self.juice.shake(0.02, 0.8);
                    }
                }
                Event::Thawed { penguin, .. } => {
                    self.juice.cheer(*penguin, 1.5);
                    // Frost turning to warm sparks as it wakes.
                    if let Some(p) = self.world.penguins.get(*penguin as usize) {
                        let z = self.world.info.map.height_at(p.pos);
                        self.effects.thaw_sparkle(v3(p.pos.x, p.pos.y, z + 0.6));
                        self.sounds.push((Sfx::Chime, p.pos));
                    }
                }
                Event::Trained { penguin, .. } => {
                    self.juice.cheer(*penguin, 1.5);
                    if let Some(p) = self.world.penguins.get(*penguin as usize) {
                        self.stats.add(p.owner, Feat::Trained);
                        let z = self.world.info.map.height_at(p.pos);
                        self.effects.sparkle(v3(p.pos.x, p.pos.y, z + 0.6), [255, 240, 180], 18);
                        self.sounds.push((Sfx::Chime, p.pos));
                    }
                }
                Event::Persuaded { penguin, .. } => {
                    self.juice.cheer(*penguin, 1.5);
                    if let Some(p) = self.world.penguins.get(*penguin as usize) {
                        let z = self.world.info.map.height_at(p.pos);
                        self.effects.sparkle(v3(p.pos.x, p.pos.y, z + 0.6), [255, 240, 180], 24);
                        self.sounds.push((Sfx::Chime, p.pos));
                    }
                }
                Event::TreeFelled { tile } => {
                    let p = v2(tile.0 as f32 + 0.5, tile.1 as f32 + 0.5);
                    let z = self.world.info.map.height_at(p);
                    // Snow shaken off the branches as it comes down.
                    for k in 0..6 {
                        let a = k as f32 * 1.05;
                        let at = p + Vec2::from_angle(a) * 0.35;
                        self.effects.impact(at, z + 0.4 + k as f32 * 0.25, HitKind::Tree, Vec2::from_angle(a));
                    }
                    self.effects.sparkle(v3(p.x, p.y, z + 1.0), [240, 248, 255], 30);
                    self.sounds.push((Sfx::TreeFall, p));
                }
                Event::Built { building } => {
                    if let Some(b) = self.world.buildings.get(*building as usize) {
                        let c = b.center();
                        let z = self.world.info.map.height_at(c);
                        self.effects.sparkle(v3(c.x, c.y, z + 0.8), [255, 245, 210], 30);
                        self.sounds.push((Sfx::Built, c));
                    }
                }
                Event::SwamIn { penguin } => {
                    // A splash where it climbs out of the water.
                    if let Some(p) = self.world.penguins.get(*penguin as usize) {
                        let z = self.world.info.map.height_at(p.pos);
                        self.effects.splash(p.pos, z);
                        self.sounds.push((Sfx::Chime, p.pos));
                    }
                }
                Event::BuildingLost { building } => {
                    if let Some(b) = self.world.buildings.get(*building as usize) {
                        let c = b.center();
                        let z = self.world.info.map.height_at(c);
                        // A watchtower falls in a cloud of snow, like an igloo.
                        if matches!(b.kind, BuildKind::Tower | BuildKind::Catapult) {
                            self.effects.collapse(v3(c.x, c.y, z + 0.9));
                        }
                        for k in 0..8 {
                            let a = k as f32 * 0.785;
                            self.effects.impact(c + Vec2::from_angle(a) * 0.4, z + 0.3 + (k % 3) as f32 * 0.2, HitKind::Tree, Vec2::from_angle(a));
                        }
                        self.sounds.push((Sfx::Crumble, c));
                    }
                }
                Event::PowerupTaken { penguin, .. } => {
                    if let Some(p) = self.world.penguins.get(*penguin as usize) {
                        self.sounds.push((Sfx::Powerup, p.pos));
                    }
                }
                Event::SealSurfaced { seal } => {
                    if let Some(s) = self.world.seals.get(*seal as usize) {
                        let at = s.pos;
                        self.effects.splash(at, self.world.info.map.height_at(at));
                    }
                }
                Event::SealSnatched { seal, .. } => {
                    if let Some(s) = self.world.seals.get(*seal as usize) {
                        let at = s.pos;
                        self.effects.splash(at, self.world.info.map.height_at(at));
                        self.effects.sparkle(v3(at.x, at.y, self.world.info.map.height_at(at) + 0.4), [200, 240, 255], 18);
                    }
                }
                Event::SealDriven { seal, .. } => {
                    if let Some(s) = self.world.seals.get(*seal as usize) {
                        let at = s.pos;
                        self.effects.splash(at, self.world.info.map.height_at(at));
                    }
                }
                _ => {}
            }
        }
        events
    }

    /// Work out what your team sees now, and darken the rest.
    fn update_fog(&mut self) {
        let w = &self.world;
        // Beaten (or the match is won): the server shows everything, so the
        // fog goes too.
        if self.fog.is_some() && (w.out.get(w.info.you as usize).copied().unwrap_or(false) || w.winner.is_some()) {
            self.fog = None;
            // The terrain was tinted by the fog: redraw it clear.
            self.tint_version += 1;
            return;
        }
        let team = w.team(w.info.you);
        // A blizzard shortens everyone's sight rather than hiding the world
        // behind white.
        let blizzard = if w.weather.blizzard > 0 { sim::fog::BLIZZARD_SIGHT } else { 1.0 };
        let mut eyes: Vec<(Vec2, f32)> = w
            .penguins
            .iter()
            .filter(|p| !p.inside && p.owner != NEUTRAL && w.team(p.owner) == team && p.act != Act::Frozen)
            .map(|p| (p.pos, sim::fog::PENGUIN_SIGHT * blizzard))
            .collect();
        for (i, ig) in w.info.igloos.iter().enumerate() {
            let owner = w.igloos[i].owner;
            if owner != NEUTRAL && w.team(owner) == team {
                eyes.push((ig.center, sim::fog::IGLOO_SIGHT));
            }
        }
        for b in &w.buildings {
            if let Some(r) = sim::fog::building_sight(b.kind).filter(|_| b.alive && b.done() && b.owner != NEUTRAL && w.team(b.owner) == team) {
                eyes.push((b.center(), r));
            }
        }
        let seen = sim::fog::visible(&w.info.map, &eyes);
        let Some(fog) = self.fog.as_mut() else { return };
        self.fog_beat += 1;
        let (mut changed, mut aging) = (false, false);
        for (f, &s) in fog.iter_mut().zip(&seen) {
            let level = if s {
                0
            } else if *f == FOG_UNKNOWN {
                FOG_UNKNOWN
            } else if *f == 0 {
                FOG_JUST_LOST
            } else {
                // Lost ground waits dark for a while, then is slowly redrawn as the old map.
                aging |= *f < FOG_OLD_MAP;
                (*f + 1).min(FOG_OLD_MAP)
            };
            changed |= (level == 0) != (*f == 0) || (level == FOG_UNKNOWN) != (*f == FOG_UNKNOWN);
            *f = level;
        }
        // The slow fade is shown a few times a second, not every tick.
        if changed || (aging && self.fog_beat % 3 == 0) {
            self.tint_version += 1;
        }
    }

    /// The Long Night's safe ring (centre, radius) right now, if it has begun.
    pub fn night_ring(&self) -> Option<(Vec2, f32)> {
        let m = &self.world.info.map;
        let r = sim::game::night_radius(m.w, m.h, self.world.info.long_night_at, self.world.tick)?;
        Some((sim::math::v2(m.w as f32 / 2.0, m.h as f32 / 2.0), r))
    }

    /// Has your team ever seen this spot?
    pub fn discovered(&self, at: Vec2) -> bool {
        let Some(fog) = &self.fog else { return true };
        let map = &self.world.info.map;
        let (x, y) = at.tile();
        map.in_bounds(x, y) && fog[map.idx(x, y)] != FOG_UNKNOWN
    }

    /// Can your team see this spot now?
    pub fn in_sight(&self, at: Vec2) -> bool {
        let Some(fog) = &self.fog else { return true };
        let map = &self.world.info.map;
        let (x, y) = at.tile();
        map.in_bounds(x, y) && fog[map.idx(x, y)] == 0
    }

    /// Advance clocks and effects by `dt` seconds.
    pub fn update(&mut self, dt: f32) {
        self.time += dt;
        // What your team can see, a few times a second.
        self.fog_t -= dt;
        if self.fog_t <= 0.0 && self.fog.is_some() {
            self.fog_t = 0.3;
            self.update_fog();
        }
        if self.started {
            self.clock += dt as f64 * sim::TICK_RATE as f64;
            let target = self.world.tick as f64 - 1.6;
            self.clock += (target - self.clock) * (dt as f64 * 2.0).min(1.0);
            self.clock = self.clock.min(self.world.tick as f64 + 0.5);
        }
        let t = self.clock as f32;
        let map = &self.world.info.map;
        let mut i = 0;
        while i < self.pending_impacts.len() {
            if self.pending_impacts[i].0 <= t {
                let (_, pos, hit, dir) = self.pending_impacts.swap_remove(i);
                let z = map.height_at(pos);
                self.effects.impact(pos, z, hit, dir);
                // No igloo hit kind: a Building hit near an igloo's centre is one.
                // It knocks a few chunks of snow loose.
                if hit == HitKind::Building
                    && self.world.info.igloos.iter().any(|ig| ig.center.dist(pos) <= crate::meshgen::IGLOO_RADIUS)
                {
                    self.effects.sparkle(v3(pos.x, pos.y, z + 0.4), [255, 255, 255], 10);
                    // A soft thud, at most one every 0.2 s.
                    if self.time - self.last_igloo_thud >= 0.2 {
                        self.last_igloo_thud = self.time;
                        self.sounds.push((Sfx::IglooHit, pos));
                    }
                }
                match hit {
                    HitKind::Direct => self.juice.flash_near(&self.world.penguins, pos, 0.8),
                    HitKind::Splash => {
                        self.juice.flash_near(&self.world.penguins, pos, 1.0);
                        // A splash is a crowd hit: worth a little shake.
                        if self.on_screen(pos) {
                            self.juice.shake(0.006, 0.3);
                        }
                    }
                    _ => {}
                }
                let sfx = match hit {
                    HitKind::Direct => Sfx::Hit,
                    HitKind::Tree => Sfx::TreeHit,
                    HitKind::Building => Sfx::Splat,
                    HitKind::Splash | HitKind::Miss => Sfx::Splat,
                };
                self.sounds.push((sfx, pos));
            } else {
                i += 1;
            }
        }
        for &(_, from) in self.pending_throws.iter().filter(|th| th.0 <= t) {
            self.sounds.push((Sfx::Throw, from));
        }
        self.pending_throws.retain(|th| th.0 > t);
        self.flying.retain(|s| t < (s.start_tick + s.flight as u32) as f32 + 0.5);

        // Warmers blow hot air at whoever they're tending.
        self.puff_t -= dt;
        if self.puff_t <= 0.0 {
            self.puff_t = 0.15;
            let map = &self.world.info.map;
            let warming: Vec<(Vec2, f32)> = self
                .world
                .penguins
                .iter()
                .filter(|p| p.act == Act::Warm && !p.inside && self.on_screen(p.pos))
                .map(|p| (p.pos, p.facing))
                .collect();
            for (pos, facing) in warming {
                let dir = Vec2::from_angle(facing);
                let z = map.height_at(pos) + 0.6;
                self.effects.hot_air(v3(pos.x + dir.x * 0.4, pos.y + dir.y * 0.4, z), dir);
            }
        }

        // Footprints on snow.
        self.footprint.resize(self.world.penguins.len(), (Vec2::ZERO, 0.0, false));
        for p in &self.world.penguins {
            let (pos, facing) = self.world.penguin_at(p.id, t);
            let fp = &mut self.footprint[p.id as usize];
            if p.inside || fp.0 == Vec2::ZERO || fp.0.dist(pos) > 2.0 {
                *fp = (pos, 0.0, false);
                continue;
            }
            fp.1 += fp.0.dist(pos);
            fp.0 = pos;
            if fp.1 > 0.32 {
                fp.1 = 0.0;
                fp.2 = !fp.2;
                let (x, y) = pos.tile();
                if map.ground_at(x, y) == Ground::Snow {
                    let side = Vec2::from_angle(facing + std::f32::consts::FRAC_PI_2) * if fp.2 { 0.08 } else { -0.08 };
                    self.effects.footprint(pos + side, facing);
                }
            }
        }

        // Axe strokes, in time with the chopping animation.
        self.chop_beat.resize(self.world.penguins.len(), 0);
        for p in &self.world.penguins {
            if p.act != Act::Chop || p.inside {
                continue;
            }
            let beat = ((t - p.act_since as f32) / sim::TICK_RATE as f32 / 0.62).max(0.0) as u32 + 1;
            let seen = &mut self.chop_beat[p.id as usize];
            if *seen != beat {
                *seen = beat;
                self.sounds.push((Sfx::Chop, p.pos));
            }
        }

        // Hammering on building sites.
        for p in &self.world.penguins {
            if p.act != Act::Build || p.inside {
                continue;
            }
            let beat = ((t - p.act_since as f32) / sim::TICK_RATE as f32 / 0.7).max(0.0) as u32 + 1;
            let seen = &mut self.chop_beat[p.id as usize];
            if *seen != beat {
                *seen = beat;
                self.sounds.push((Sfx::Pack, p.pos));
            }
        }

        // Chimney smoke from occupied igloos.
        self.smoke_t += dt;
        if self.smoke_t > 0.15 {
            self.smoke_t = 0.0;
            for (i, ig) in self.world.info.igloos.iter().enumerate() {
                if self.world.igloos[i].occupants > 0 {
                    let z = map.height_at(ig.center) + crate::meshgen::IGLOO_HEIGHT;
                    let back = (ig.center - ig.door).norm() * 0.25;
                    self.effects.smoke(v3(ig.center.x + back.x, ig.center.y + back.y, z));
                }
            }
            // Smoke from lit saunas.
            for b in &self.world.buildings {
                if b.alive && b.done() && b.kind == BuildKind::Campfire && b.lit {
                    let fire = b.center();
                    self.effects.smoke(v3(fire.x, fire.y, map.height_at(fire) + 0.5));
                    for _ in 0..2 {
                        self.effects.ember(v3(fire.x, fire.y, map.height_at(fire) + 0.3));
                    }
                }
                if b.alive && b.done() && b.kind == BuildKind::Sauna && b.lit {
                    let fire = b.center() + v2(0.0, 1.05);
                    self.effects.smoke(v3(fire.x, fire.y, map.height_at(fire) + 0.4));
                    self.effects.ember(v3(fire.x, fire.y, map.height_at(fire) + 0.25));
                }
            }
        }
        // A faint streak of powder behind every snowball in flight.
        {
            let t = self.clock as f32;
            for f in &self.flying {
                if let Some((p, z)) = WorldView::snowball_at(f, t) {
                    self.effects.trail(v3(p.x, p.y, z), if f.big { 2.5 } else { 1.0 });
                }
            }
        }
        // Frost sparkles: penguins out in the cold shimmer faintly.
        self.frost_t += dt;
        if self.frost_t > 0.3 {
            self.frost_t = 0.0;
            let you = self.world.info.you;
            let team = self.world.team(you);
            for p in &self.world.penguins {
                if !p.exposed || p.inside || p.act == Act::Frozen {
                    continue;
                }
                let (pos, _) = self.world.penguin_at(p.id, t);
                // Skip penguins hidden by fog or off screen, like the draw.
                if (p.owner == NEUTRAL || self.world.team(p.owner) != team) && !self.in_sight(pos) {
                    continue;
                }
                if !self.on_screen(pos) {
                    continue;
                }
                // One slow-drifting sparkle per penguin, deterministic.
                let a = p.id as f32 * 2.1 + self.time * 0.7;
                let drift = Vec2::from_angle(a) * (0.08 + (p.id as f32 * 0.73).fract() * 0.1);
                let dir = Vec2::from_angle(a + std::f32::consts::FRAC_PI_2);
                if self.effects.particles.len() < 4800 {
                    self.effects.particles.push(Particle {
                        pos: v3(pos.x + drift.x, pos.y + drift.y, map.height_at(pos) + 0.35),
                        vel: v3(dir.x * 0.05, dir.y * 0.05, 0.12),
                        life: 0.8,
                        max: 0.8,
                        size: 0.05,
                        color: [210, 235, 255, 140],
                        gravity: 0.0,
                        grow: 0.0,
                        glow: false,
                    });
                }
            }
        }
        self.effects.update(dt, &self.world.info.map);
        self.juice.update(dt);
        self.stats.sample(dt, &self.world);

        // The Long Night's ring redraws the tint each time it shrinks a tile.
        let tiles = self.night_ring().map_or(-1, |(_, r)| r as i32);
        if tiles != self.ring_tiles {
            self.ring_tiles = tiles;
            self.tint_version += 1;
        }
        // Ease the light and weather towards the season's.
        let wt = self.world.weather;
        // Polar night is dark all day.
        let night = if wt.season == sim::game::Season::PolarNight { 1.0 } else { 0.0 };
        let storm = if wt.blizzard > 0 { 1.0 } else { 0.0 };
        let snowfall = (wt.season.snowfall() + storm * 1.0).min(3.0);
        let k = (dt * 0.4).min(1.0);
        self.night += (night - self.night) * k;
        let summer = if wt.season == sim::game::Season::Midsummer { 1.0 } else { 0.0 };
        let thaw = if wt.season == sim::game::Season::Thaw { 1.0 } else { 0.0 };
        self.summer += (summer - self.summer) * k;
        self.thaw += (thaw - self.thaw) * k;
        self.storm += (storm - self.storm) * (dt * 0.8).min(1.0);
        self.snowfall += (snowfall - self.snowfall) * k;
    }

    pub fn scene<'a>(
        &'a self,
        dt: f32,
        selected: &[u16],
        selected_igloo: Option<u8>,
        selected_building: Option<u16>,
        markers: &[Marker],
    ) -> Scene<'a> {
        let w = &self.world;
        let info = &w.info;
        let map = &info.map;
        let t = self.clock as f32;
        let you = info.you;
        let mut penguins = Vec::with_capacity(w.penguins.len());
        let team = w.team(you);
        // Workers resting beside a burning fire sit down to warm up.
        let fires: Vec<Vec2> = w.buildings.iter().filter(|b| b.alive && b.done() && b.lit).map(|b| b.center()).collect();
        for p in &w.penguins {
            if p.inside {
                continue;
            }
            let (pos, facing) = w.penguin_at(p.id, t);
            // Only your own team is seen through the fog.
            if (p.owner == NEUTRAL || w.team(p.owner) != team) && !self.in_sight(pos) {
                continue;
            }
            let frozen = p.act == Act::Frozen;
            let anim = match p.act {
                Act::Walk => {
                    if p.kind == Kind::Worker && p.carry == Carry::Fish {
                        Anim::Carry
                    } else {
                        Anim::Walk
                    }
                }
                Act::Throw => Anim::Throw,
                Act::Stunned => Anim::Knocked,
                Act::Warm => Anim::Warm,
                Act::Scoop => Anim::Scoop,
                Act::Fish => Anim::Fish,
                Act::Mine => Anim::Mine,
                Act::Chop => Anim::Chop,
                Act::Build => Anim::Hammer,
                Act::Unload => Anim::Unload,
                Act::Idle if p.kind == Kind::Worker && p.carry == Carry::None && fires.iter().any(|f| f.dist(pos) < 2.2) => Anim::Sit,
                _ => {
                    if p.kind == Kind::Worker && p.carry == Carry::Fish {
                        Anim::Carry
                    } else {
                        Anim::Stand
                    }
                }
            };
            let anim_t = ((t - p.act_since as f32) / sim::TICK_RATE as f32).max(0.0);
            // The winning side celebrates; a fresh hit rocks anyone standing still.
            let won = w.winner.is_some_and(|team| p.owner != NEUTRAL && w.team(p.owner) == team);
            let flash = self.juice.flash_of(p.id);
            let forced = forced_pose().filter(|_| !frozen && p.act == Act::Idle);
            let (anim, mut anim_t) = if let Some(pose) = forced {
                (pose, t / sim::TICK_RATE as f32 + p.id as f32 * 0.21)
            } else if !frozen && matches!(p.act, Act::Idle | Act::Walk | Act::Warm) && (won || self.juice.cheering(p.id)) {
                (Anim::Cheer, t / sim::TICK_RATE as f32 + p.id as f32 * 0.21)
            } else if !frozen && anim == Anim::Stand && flash > 0.0 {
                // The whole six-frame rock, spread over the flash.
                (Anim::Flinch, (1.0 - flash) * 6.0 / Anim::Flinch.timing().0)
            } else {
                (anim, anim_t)
            };
            if anim == Anim::Carry && p.act != Act::Walk {
                anim_t = 0.0;
            }
            if matches!(anim, Anim::Stand | Anim::Walk | Anim::Sit) {
                anim_t += p.id as f32 * 0.137; // desync idle bobbing
            }
            penguins.push(PenguinDraw {
                id: p.id,
                pos,
                // Carried prisoners are held up off the ground.
                z: map.height_at(pos) + if p.carried { 0.35 } else { 0.0 } + if p.posted { sim::game::TOWER_DECK } else { 0.0 },
                facing,
                kind: p.kind,
                anim,
                anim_t,
                // A recruit on its way to its first meal wears no colours yet.
                color: if p.dining { player_color(info, NEUTRAL) } else { player_color(info, p.owner) },
                // Frost: fully frozen is solid ice, a cold penguin is half
                // strength, one out in the cold (exposed) a faint quarter.
                cold: if frozen {
                    1.0
                } else if p.cold {
                    0.55
                } else if p.exposed {
                    0.25
                } else {
                    0.0
                },
                frozen,
                wild: p.owner == NEUTRAL,
                selected: selected.contains(&p.id),
                mine: p.owner == you,
                power: p.power,
                carry: p.carry,
                ammo: p.snowballs,
                flash: self.juice.flash_of(p.id),
                shouting: p.shouting,
                tend: p.tend,
            });
        }
        let igloos = info
            .igloos
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let v = &w.igloos[i];
                IglooDraw {
                    center: s.center,
                    door: s.door,
                    z: map.height_at(s.center),
                    color: (v.owner != NEUTRAL).then(|| player_color(info, v.owner)),
                    tier: v.tier,
                    hp: (v.hp as f32 / sim::game::igloo_max_hp(v.tier) as f32).clamp(0.0, 1.0),
                    takeover: v.takeover.map(|tk| {
                        (player_color(info, tk.by), 1.0 - tk.ticks_left as f32 / tk.total.max(1) as f32)
                    }),
                    selected: selected_igloo == Some(i as u8),
                }
            })
            .collect();
        let resources = info
            .resources
            .iter()
            .enumerate()
            .map(|(i, r)| ResourceDraw {
                pos: r.pos,
                z: map.height_at(r.pos),
                kind: r.kind,
                amount: w.resources.get(i).copied().unwrap_or(0),
                max: r.max,
            })
            .collect();
        let buildings = self.building_draws(selected_building);
        let powerups = w
            .powerups
            .iter()
            .filter(|p| p.holder.is_none())
            .map(|p| (p.pos, map.height_at(p.pos), p.kind))
            .collect();
        // Surfaced seals rise out of their hole; the nose swings to face
        // whatever it is watching.
        let seals = w
            .seals
            .iter()
            .filter(|s| s.state == sim::game::SealState::Surfaced && self.in_sight(s.pos))
            .map(|s| {
                let watcher = w
                    .penguins
                    .iter()
                    .filter(|p| !p.inside)
                    .min_by(|a, b| a.pos.dist2(s.pos).total_cmp(&b.pos.dist2(s.pos)));
                SealDraw {
                    pos: s.pos,
                    z: map.height_at(s.pos),
                    facing: watcher.map_or(0.0, |p| (p.pos - s.pos).angle()),
                    up: ((self.clock - s.since as f64) as f32).max(0.0),
                }
            })
            .collect();
        let snowballs = self
            .flying
            .iter()
            .filter_map(|s| WorldView::snowball_at(s, t).map(|(p, z)| (p, z, if s.big { 4.0 } else if s.kind == Kind::Packer { 2.2 } else { 1.0 })))
            .collect();
        Scene {
            time: self.time,
            dt,
            penguins,
            igloos,
            resources,
            buildings,
            powerups,
            seals,
            snowballs,
            effects: &self.effects,
            markers: markers.to_vec(),
            tint_version: self.tint_version,
            night_ring: self.night_ring(),
            fog: self.fog.as_deref(),
            map,
            snow: &w.snow,
            terrain_version: w.terrain_version,
            night: self.night,
            storm: self.storm,
            summer: self.summer,
            thaw: self.thaw,
            snowfall: self.snowfall,
            shake: self.juice.shake_offset(),
        }
    }

    fn building_draws(&self, selected: Option<u16>) -> Vec<BuildingDraw> {
        let w = &self.world;
        let map = &w.info.map;
        let wall_at = |kind, x: i32, y: i32| w.buildings.iter().any(|b| b.alive && b.kind == kind && b.tile == (x, y));
        w.buildings
            .iter()
            .enumerate()
            .filter(|(_, b)| b.alive)
            .map(|(i, b)| {
                let c = b.center();
                let (x, y) = b.tile;
                let yaws = match b.kind {
                    BuildKind::Palisade => {
                        let across = wall_at(b.kind, x - 1, y) || wall_at(b.kind, x + 1, y);
                        let down = wall_at(b.kind, x, y - 1) || wall_at(b.kind, x, y + 1);
                        match (across, down) {
                            (true, true) => vec![0.0, std::f32::consts::FRAC_PI_2],
                            (false, true) => vec![std::f32::consts::FRAC_PI_2],
                            _ => vec![0.0],
                        }
                    }
                    _ => vec![0.0],
                };
                let variant = match b.kind {
                    BuildKind::Palisade => ((x * 5 + y * 11) % 3) as u8,
                    _ => 0,
                };
                BuildingDraw {
                    kind: b.kind,
                    center: c,
                    z: map.height_at(c),
                    yaws,
                    color: player_color(&w.info, b.owner),
                    progress: b.built as f32 / b.kind.snow() as f32,
                    variant,
                    selected: selected == Some(i as u16),
                    // Staked out but not started: drawn see-through, as it
                    // neither blocks nor can be hit until work begins.
                    ghost: (b.built == 0).then_some(true),
                }
            })
            .collect()
    }

    /// Whether a world point is close enough to the camera to be worth a
    /// screen shake.
    fn on_screen(&self, p: Vec2) -> bool {
        self.camera.dist(p) < self.view_radius + 8.0
    }

    pub fn my_igloos(&self) -> Vec<u8> {
        let you = self.world.info.you;
        self.world.igloos.iter().enumerate().filter(|(_, i)| i.owner == you).map(|(i, _)| i as u8).collect()
    }
}

/// For screenshots: `SNOWBALLZ_POSE=cheer|flinch|sit|hammer|chop|throw` shows that
/// pose on every idle penguin.
fn forced_pose() -> Option<Anim> {
    static POSE: std::sync::OnceLock<Option<Anim>> = std::sync::OnceLock::new();
    *POSE.get_or_init(|| match std::env::var("SNOWBALLZ_POSE").ok()?.as_str() {
        "cheer" => Some(Anim::Cheer),
        "flinch" => Some(Anim::Flinch),
        "sit" => Some(Anim::Sit),
        "hammer" => Some(Anim::Hammer),
        "chop" => Some(Anim::Chop),
        "throw" => Some(Anim::Throw),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::game::{Game, PlayerSetup};
    use sim::map::Map;

    #[test]
    fn a_beaten_player_loses_the_fog() {
        let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
        let map = Map::load_file(std::path::Path::new(&path)).unwrap();
        let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
        let g = Game::new(map, setups, 1);
        let mut v = GameView::new(GameInfo::from_game(&g, 0));
        v.update(1.0);
        assert!(v.fog.is_some(), "a player in the game sees through fog");
        let far = v.world.info.igloos[1].center;
        assert!(!v.in_sight(far), "the far igloo is out of sight at the start");
        let before = v.tint_version;
        v.world.out[0] = true;
        v.update(1.0);
        assert!(v.fog.is_none(), "beaten, the fog is gone");
        assert!(v.in_sight(far) && v.discovered(far));
        assert!(v.tint_version > before, "the terrain is told to redraw");
    }
}
