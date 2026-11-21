//! What the world renderers draw each frame, independent of 2D/3D.

use crate::gpu::Gpu;
use sim::game::{BuildKind, Carry, Kind, PowerKind};
use sim::map::{Map, ResourceKind};
use sim::math::Vec2;

use crate::effects::Effects;
use crate::gfx::Batch;
use crate::mathx::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anim {
    Stand,
    Walk,
    Carry,
    Throw,
    Scoop,
    Fish,
    Mine,
    Unload,
    /// A builder swinging a hammer.
    Hammer,
    /// A chopper swinging an axe.
    Chop,
    /// Warming up by a fire: sat down.
    Sit,
    /// Arms up and hopping: trained, thawed, or on the winning side.
    Cheer,
    /// Rocked back by a snowball.
    Flinch,
    /// Flat on its back after a Skier's charge.
    Knocked,
    /// A Warmer blowing hot air on a friend.
    Warm,
}

impl Anim {
    pub fn name(self) -> &'static str {
        match self {
            Anim::Stand => "stand",
            Anim::Walk => "walk",
            Anim::Carry => "carry",
            Anim::Throw => "throw",
            Anim::Scoop => "scoop",
            Anim::Fish => "fish",
            Anim::Mine | Anim::Unload => "unload",
            Anim::Hammer => "hammer",
            Anim::Chop => "chop",
            Anim::Sit => "sit",
            Anim::Cheer => "cheer",
            Anim::Flinch => "flinch",
            Anim::Knocked => "knocked",
            Anim::Warm => "warm",
        }
    }
    /// Frames per second and whether the animation loops.
    pub fn timing(self) -> (f32, bool) {
        match self {
            Anim::Stand => (2.0, true),
            Anim::Walk | Anim::Carry => (13.0, true),
            Anim::Throw => (12.0, false),
            Anim::Scoop => (8.0, true),
            Anim::Fish => (5.0, true),
            Anim::Mine => (7.0, true),
            Anim::Unload => (8.0, false),
            Anim::Hammer | Anim::Chop => (7.0, true),
            Anim::Sit => (2.0, true),
            Anim::Cheer => (9.0, true),
            Anim::Flinch => (14.0, false),
            Anim::Knocked => (8.0, true),
            Anim::Warm => (8.0, true),
        }
    }
    /// Continuous frame position for `t` seconds into the animation.
    pub fn frame(self, t: f32, frames: usize) -> f32 {
        let (fps, looping) = self.timing();
        let f = t * fps;
        if looping {
            f % frames as f32
        } else {
            f.min(frames as f32 - 1.0)
        }
    }
}

#[derive(Clone, Debug)]
pub struct PenguinDraw {
    /// Match to the view's penguin id: used to find a Warmer's target.
    pub id: u16,
    pub pos: Vec2,
    pub z: f32,
    pub facing: f32,
    pub kind: Kind,
    pub anim: Anim,
    pub anim_t: f32,
    pub color: [u8; 3],
    /// 0..1 icy tint for cold penguins.
    pub cold: f32,
    pub frozen: bool,
    /// Belongs to no tribe: recruitable, drawn grey with a ring.
    pub wild: bool,
    pub selected: bool,
    pub mine: bool,
    pub power: Option<PowerKind>,
    pub carry: Carry,
    /// Snowballs in hand.
    pub ammo: u8,
    /// 0..1 white flash after being hit.
    pub flash: f32,
    /// Under a War Cry: running and throwing faster.
    pub shouting: bool,
    /// Warmer: the penguin it is warming or thawing.
    pub tend: Option<u16>,
}

#[derive(Clone, Debug)]
pub struct IglooDraw {
    pub center: Vec2,
    pub door: Vec2,
    pub z: f32,
    pub color: Option<[u8; 3]>,
    /// Igloo, Snow Hall or Ice Palace.
    pub tier: u8,
    /// Health 0..1; a collapsed igloo is a ruin.
    pub hp: f32,
    /// Color of the invader and progress 0..1.
    pub takeover: Option<([u8; 3], f32)>,
    pub selected: bool,
}

#[derive(Clone, Debug)]
pub struct SealDraw {
    pub pos: Vec2,
    pub z: f32,
    /// Facing angle in map space (the seal noses towards its last target).
    pub facing: f32,
    /// Seconds since it broke the surface, for the rise.
    pub up: f32,
}

#[derive(Clone, Debug)]
pub struct ResourceDraw {
    pub pos: Vec2,
    pub z: f32,
    pub kind: ResourceKind,
    pub amount: u8,
    pub max: u8,
}

#[derive(Clone, Debug)]
pub struct BuildingDraw {
    pub kind: BuildKind,
    pub center: Vec2,
    pub z: f32,
    /// Model orientations (walls turn to join their neighbours; corners get two).
    pub yaws: Vec<f32>,
    pub color: [u8; 3],
    /// 0..1 construction progress.
    pub progress: f32,
    /// Model variant (wall bricks, pile size).
    pub variant: u8,
    pub selected: bool,
    /// Placement preview: Some(can be placed here).
    pub ghost: Option<bool>,
}

#[derive(Clone, Copy, Debug)]
pub enum MarkerKind {
    Move,
    Run,
    Enter,
    Gather,
}

#[derive(Clone, Copy, Debug)]
pub struct Marker {
    pub pos: Vec2,
    pub age: f32,
    pub kind: MarkerKind,
}

pub struct Scene<'a> {
    /// Wall clock seconds (for idle animations).
    pub time: f32,
    pub dt: f32,
    pub penguins: Vec<PenguinDraw>,
    pub igloos: Vec<IglooDraw>,
    pub resources: Vec<ResourceDraw>,
    pub buildings: Vec<BuildingDraw>,
    pub powerups: Vec<(Vec2, f32, PowerKind)>,
    /// Leopard seals surfaced on the ice (hidden ones stay hidden).
    pub seals: Vec<SealDraw>,
    /// Position, height and size of snowballs in flight.
    pub snowballs: Vec<(Vec2, f32, f32)>,
    pub effects: &'a Effects,
    pub markers: Vec<Marker>,
    /// Bumps whenever the per-tile tint (fog of war) changes.
    pub tint_version: u32,
    /// The Long Night's safe ring (centre, radius), once it has begun.
    pub night_ring: Option<(Vec2, f32)>,
    /// Fog of war per tile: 0 seen now, 1 seen before, 2 never seen. None
    /// shows everything (spectators, SNOWBALLZ_NOFOG).
    pub fog: Option<&'a [u8]>,
    /// The map as it is now (trees get chopped, snow wears to ice).
    pub map: &'a Map,
    /// Layers of snow per tile.
    pub snow: &'a [u8],
    /// Changes whenever `map` or `snow` do.
    pub terrain_version: u32,
    /// 0..1, eased: how dark (polar night) and how stormy (blizzard) it is.
    pub night: f32,
    pub storm: f32,
    /// Falling snow relative to normal (none in the thaw).
    pub snowfall: f32,
    /// Screen shake as a camera offset, in fractions of the viewport height.
    pub shake: [f32; 2],
}

/// Implemented by the 2D (lightweight) and 3D (pretty) renderers.
pub trait WorldRenderer {
    fn draw(&mut self, ctx: &mut Gpu, scene: &Scene, batch: &mut Batch);
    /// Physical-pixel screen position of a world point (map coords + height).
    fn world_to_screen(&self, p: Vec3) -> Option<[f32; 2]>;
    fn screen_to_ground(&self, s: [f32; 2]) -> Option<Vec2>;
    fn pan_pixels(&mut self, d: [f32; 2]);
    fn zoom_at(&mut self, factor: f32, anchor: [f32; 2]);
    fn rotate(&mut self, _da: f32) {}
    /// Back to the default north-up view.
    fn reset_rotation(&mut self) {}
    fn focus(&mut self, p: Vec2);
    fn center(&self) -> Vec2;
    /// Approximate on-screen pixels per tile near `p`.
    fn pixels_per_tile(&self, p: Vec2) -> f32;
    fn set_viewport(&mut self, w: f32, h: f32);
}
