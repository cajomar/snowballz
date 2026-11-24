//! The world renderer: lit terrain with sun shadows, instanced
//! vertex-animated penguins, particles and fog, drawn into a high-range
//! picture that post-processing finishes for the screen.
//!
//! World space here is (map_x, -map_y, height) so the map isn't mirrored in a
//! right-handed world. [`gl`] converts.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

use sim::game::{BuildKind, Carry, Kind, PowerKind};
use sim::map::{Ground, Map, Obstacle, ResourceKind};
use sim::math::{v2, Vec2};
use sim::rng::Rng;

use crate::effects::DecalKind;
use crate::gfx::{self, Batch};
use crate::gpu::{bytes, Gpu, TextureId};
use crate::mathx::{v3, Mat4, Vec3};
use crate::meshgen;
use crate::model::Model;
use crate::post::{Post, PostUniforms, DEPTH};
use crate::scene::{Anim, MarkerKind, Scene, WorldRenderer};

pub fn gl(p: Vec2, z: f32) -> Vec3 {
    v3(p.x, -p.y, z)
}

/// Model yaw for a map-space facing angle (models face -y).
pub fn yaw_for(facing: f32) -> f32 {
    FRAC_PI_2 - facing
}

// A lower sun than straight overhead: long shadows give shape.
pub const SUN: Vec3 = Vec3 { x: -0.52, y: -0.56, z: 0.62 };
const FOV: f32 = 0.70;
const SHADOW_SIZE: u32 = 2048;

/// GL-style clip space (z from -w to w) to wgpu's (0 to w).
const CLIP_FIX: Mat4 = Mat4([1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 0.5, 0., 0., 0., 0.5, 1.]);

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Inst {
    pub pos_yaw: [f32; 4],
    /// scale, frame lerp, alpha, highlight
    pub misc: [f32; 4],
    /// team rgb + cold tint
    pub color: [u8; 4],
    /// Penguins' body language, in radians: lean forward, roll to the
    /// right; then squash (shorter and wider), unused.
    pub tilt: [f32; 4],
}

/// What the renderer remembers of each penguin between frames, to animate
/// it smoothly: the clip it was in (to crossfade out of), how fast it is
/// really going (so feet keep pace with the ground) and how fast it turns.
struct Gait {
    ai: usize,
    frame: usize,
    from: Option<(usize, usize)>,
    fade: f32,
    pos: Vec2,
    facing: f32,
    speed: f32,
    turn: f32,
    stride: f32,
    seen: f32,
}

/// Seconds a penguin takes to blend from one clip into the next.
const CROSSFADE: f32 = 0.16;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WorldUniforms {
    pub viewproj: Mat4,
    pub light: Mat4,
    /// Camera position, and the time in seconds.
    pub eye: [f32; 4],
    /// Sun direction, and 1 when shadows are on.
    pub sun: [f32; 4],
    /// Fog start and depth, a shadow-map texel, unused.
    pub fog: [f32; 4],
    pub fog_color: [f32; 4],
    /// Map size, polar night (0..1), blizzard (0..1).
    pub map: [f32; 4],
    /// x: albedo multiplier (ruins are drawn darker), y: how many of
    /// `lights` are in use.
    pub tint: [f32; 4],
    /// Point lights (fires, windows, lanterns): position and reach.
    pub lights: [[f32; 4]; MAX_LIGHTS],
    /// Their colour times brightness.
    pub light_cols: [[f32; 4]; MAX_LIGHTS],
}

/// The most point lights lighting one frame: the nearest to the camera win.
pub const MAX_LIGHTS: usize = 24;

/// Uniform slots, one per way the world is drawn in a frame.
const SLOT_MAIN: u32 = 0;
const SLOT_SHADOW: u32 = 1;
const SLOT_RUIN: u32 = 2;
const SLOTS: u32 = 3;
const SLOT_STRIDE: u64 = 1024;

const COMMON: &str = r#"
struct World {
    viewproj: mat4x4<f32>,
    light: mat4x4<f32>,
    eye: vec4<f32>,
    sun: vec4<f32>,
    fog: vec4<f32>,
    fog_color: vec4<f32>,
    map: vec4<f32>,
    tint: vec4<f32>,
    lights: array<vec4<f32>, 24>,
    light_cols: array<vec4<f32>, 24>,
}
@group(0) @binding(0) var<uniform> u: World;

// GLSL's smoothstep, which also runs downhill when e0 > e1.
fn ss(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}
fn hash(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 = p3 + dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}
// Distance to the nearest cell border of a jittered grid: ice plate cracks.
fn cracks(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    var d1 = 8.0;
    var d2 = 8.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let g = vec2<f32>(f32(x), f32(y));
            let r = g + vec2<f32>(hash(i + g), hash(i + g + 19.19)) * 0.85 - f;
            let d = dot(r, r);
            if (d < d1) { d2 = d1; d1 = d; } else if (d < d2) { d2 = d; }
        }
    }
    return sqrt(d2) - sqrt(d1);
}
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let w = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2<f32>(1.0, 0.0)), w.x), mix(hash(i + vec2<f32>(0.0, 1.0)), hash(i + vec2<f32>(1.0, 1.0)), w.x), w.y);
}
// Light from the sky: blue from above, the snow's bright bounce from below.
fn sky_light(n: vec3<f32>) -> vec3<f32> {
    let up = n.z * 0.5 + 0.5;
    return mix(vec3<f32>(0.66, 0.68, 0.74), vec3<f32>(0.46, 0.60, 0.88), up);
}
// The sun: warm, and brighter than white, so sunlit snow glows a little.
const SUN_COLOR: vec3<f32> = vec3<f32>(1.0, 0.92, 0.80);
// u.map.z: polar night (0..1) darkens and blues the sun and sky's light
// (not the fires', which is added after).
fn night_tint(col: vec3<f32>) -> vec3<f32> {
    return col * mix(vec3<f32>(1.0), vec3<f32>(0.3, 0.36, 0.56), u.map.z);
}
// Light from the fires, windows and lanterns nearby: a soft pool that fades
// to nothing at the edge of its reach, wrapped a little round the far side.
fn local_diffuse(pos: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    var c = vec3<f32>(0.0);
    let count = i32(u.tint.y);
    for (var k = 0; k < count; k++) {
        let l = u.lights[k];
        let d = l.xyz - pos;
        let dist = length(d);
        let x = clamp(1.0 - (dist * dist) / (l.w * l.w), 0.0, 1.0);
        let wrap = max((dot(n, d / max(dist, 1e-3)) + 0.3) / 1.3, 0.0);
        c += u.light_cols[k].rgb * x * x * wrap;
    }
    return c;
}
// Their glints on shiny things (ice, water, metal).
fn local_spec(pos: vec3<f32>, n: vec3<f32>, v: vec3<f32>, power: f32) -> vec3<f32> {
    var c = vec3<f32>(0.0);
    let count = i32(u.tint.y);
    for (var k = 0; k < count; k++) {
        let l = u.lights[k];
        let d = l.xyz - pos;
        let dist = length(d);
        let x = clamp(1.0 - (dist * dist) / (l.w * l.w), 0.0, 1.0);
        let h = normalize(d / max(dist, 1e-3) + v);
        c += u.light_cols[k].rgb * x * pow(max(dot(n, h), 0.0), power);
    }
    return c;
}
// Land nobody has seen lies under a sea of cloud drifting with the wind:
// billows lit by the sun from the side, shaded blue underneath, with no
// hint of what is below.
fn cloud_height(p: vec2<f32>) -> f32 {
    let t = u.eye.w;
    let q = p * 0.11 + vec2<f32>(t * 0.010, t * 0.004);
    return vnoise(q) * 0.5 + vnoise(q * 2.1 + 3.7) * 0.28 + vnoise(q * 4.3 + 9.1) * 0.14 + vnoise(q * 9.0 + 1.3) * 0.08;
}
fn fow_cloud(p: vec2<f32>) -> vec3<f32> {
    let h = cloud_height(p);
    let e = 0.6;
    let g = vec2<f32>(cloud_height(p + vec2<f32>(e, 0.0)) - h, cloud_height(p + vec2<f32>(0.0, e)) - h) / e;
    let n = normalize(vec3<f32>(-g.x * 9.0, g.y * 9.0, 1.0));
    let lit = max(dot(n, normalize(u.sun.xyz)), 0.0);
    // Deep in a billow it is bluer and darker; tops catch the sun.
    let shade = mix(vec3<f32>(0.46, 0.52, 0.66), vec3<f32>(0.80, 0.84, 0.92), ss(0.25, 0.75, h));
    var c = shade * (0.55 + 0.6 * lit) + SUN_COLOR * pow(lit, 6.0) * 0.12 * ss(0.5, 0.8, h);
    return night_tint(c) * 0.92;
}
// How far a place is under the cloud: `unknown` from the fog map, with a
// billowing edge.
fn cloud_cover(p: vec2<f32>, unknown: f32) -> f32 {
    return ss(0.3, 0.8, unknown * (0.75 + cloud_height(p * 1.7) * 0.6));
}
fn apply_fog(col: vec3<f32>, pos: vec3<f32>) -> vec3<f32> {
    let d = length(u.eye.xyz - pos);
    var f = clamp((d - u.fog.x) / u.fog.y, 0.0, 1.0);
    // u.map.w: blizzard haze, even up close.
    f = max(f * f * 0.9, u.map.w * 0.08);
    return mix(col, u.fog_color.rgb, f);
}
"#;

/// The world's textures: the sun's shadow map, fog of war, ground types.
const TEXTURES: &str = r#"
@group(1) @binding(0) var shadow_map: texture_depth_2d;
@group(1) @binding(1) var shadow_samp: sampler_comparison;
@group(1) @binding(2) var tint_map: texture_2d<f32>;
@group(1) @binding(3) var ground_map: texture_2d<f32>;
@group(1) @binding(4) var lin: sampler;

fn shadow_at(lp: vec4<f32>) -> f32 {
    if (u.sun.w < 0.5) { return 1.0; }
    let p = lp.xyz / lp.w;
    let uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || p.z > 1.0) { return 1.0; }
    // Hardware-filtered PCF over a 3x3 texel area.
    let texel = u.fog.z;
    let z = p.z - 0.002;
    var s = 0.0;
    for (var i = -1; i <= 1; i++) {
        for (var j = -1; j <= 1; j++) {
            s += textureSampleCompareLevel(shadow_map, shadow_samp, uv + vec2<f32>(f32(i), f32(j)) * texel, z);
        }
    }
    return s / 9.0;
}
fn tint_at(mp: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(tint_map, lin, mp / u.map.xy, 0.0);
}
fn ground_at(mp: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(ground_map, lin, mp / u.map.xy, 0.0);
}
// Snow banked against trees and pinnacles: their mask, blurred wide and
// wobbled so banks have no tile corners.
fn drift_at(mp: vec2<f32>) -> f32 {
    let q = mp + vec2<f32>(vnoise(mp * 0.9), vnoise(mp * 0.9 + 5.0)) * 0.8 - 0.4;
    let d = 0.7;
    return (ground_at(q + vec2<f32>(d, d)).a + ground_at(q + vec2<f32>(-d, d)).a
        + ground_at(q + vec2<f32>(d, -d)).a + ground_at(q + vec2<f32>(-d, -d)).a) * 0.25;
}
// The open-water mask, blurred over a tile so holes come out round rather
// than tile-shaped.
fn water_at(mp: vec2<f32>) -> f32 {
    let d = 0.5;
    // Kept partly sharp so even a one-tile pool still shows.
    let soft = (ground_at(mp + vec2<f32>(d, d)).b + ground_at(mp + vec2<f32>(-d, d)).b
        + ground_at(mp + vec2<f32>(d, -d)).b + ground_at(mp + vec2<f32>(-d, -d)).b) * 0.25;
    return mix(ground_at(mp).b, soft, 0.65);
}
"#;

// --- Terrain -----------------------------------------------------------------

const TERRAIN: &str = r#"
struct TOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) pos: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) attr: vec4<f32>,
    @location(3) lpos: vec4<f32>,
}

@vertex
fn vs(@location(0) pos: vec3<f32>, @location(1) nrm: vec3<f32>, @location(2) attr: vec4<f32>) -> TOut {
    var o: TOut;
    o.clip = u.viewproj * vec4<f32>(pos, 1.0);
    o.pos = pos;
    o.nrm = nrm;
    o.attr = attr;
    o.lpos = u.light * vec4<f32>(pos, 1.0);
    return o;
}

// The snow's own relief, in tiles: rolling drifts, and sastrugi (ridges the
// wind carves, stretched across it). `fine` fades the small detail out with
// distance so it never shimmers.
fn snow_relief(p: vec2<f32>, fine: f32) -> f32 {
    // Ridges wander with the wind (a warped frame) and come in patches.
    let q = p + vec2<f32>(vnoise(p * 0.21), vnoise(p * 0.21 + 7.0)) * 3.0;
    let w = vec2<f32>(q.x * 0.8 + q.y * 0.6, q.y * 0.8 - q.x * 0.6);
    let drifts = vnoise(p * 0.13) * 1.3 + vnoise(p * 0.37 + 4.0) * 0.4;
    let patches = ss(0.3, 0.75, vnoise(p * 0.09 + 3.0));
    let ridges = vnoise(vec2<f32>(w.x * 0.6, w.y * 1.7)) * 0.08 + vnoise(vec2<f32>(w.x * 1.5, w.y * 3.6) + 9.0) * 0.03 * fine;
    return drifts + ridges * patches * fine;
}

@fragment
fn fs(i: TOut) -> @location(0) vec4<f32> {
    var n = normalize(i.nrm);
    let mp = vec2<f32>(i.pos.x, -i.pos.y);
    let cliff = i.attr.y;
    // How many tiles one pixel covers: detail finer than that is left out.
    let px = length(fwidth(mp));
    let fine = ss(0.25, 0.06, px);
    // Ice edges: the per-tile mask, bilinear filtered and wobbled by noise,
    // then cut sharply so floes have crisp, natural outlines.
    let warp = (vnoise(mp * 1.3) - 0.5) * 0.3 + (vnoise(mp * 4.7) - 0.5) * 0.12;
    let g0 = ground_at(mp);
    let m = g0.r + warp;
    let ice = ss(0.47, 0.53, m) * (1.0 - cliff);
    // Open water: the same trick on its own channel.
    let w0 = water_at(mp);
    let wm = w0 + warp * 1.4;
    let water = ss(0.47, 0.53, wm);
    let foam = ss(0.3, 0.47, wm) * (1.0 - water);
    // Snow banks: tilt the normal down towards the ice along the edge.
    let gx = ground_at(mp + vec2<f32>(0.2, 0.0));
    let gy = ground_at(mp + vec2<f32>(0.0, 0.2));
    let grad = vec2<f32>(gx.r, gy.r) - g0.r;
    let bank = ss(0.2, 0.45, m) * (1.0 - ice) * (1.0 - cliff);
    n = normalize(n + vec3<f32>(grad.x, -grad.y, 0.0) * 4.0 * bank);
    // The snow's relief, as a slope: drifts and ridges, trodden paths pressed
    // in, and a lip of snow heaped round each water hole.
    let h0 = snow_relief(mp, fine);
    let e = 0.12;
    var slope = vec2<f32>(snow_relief(mp + vec2<f32>(e, 0.0), fine) - h0, snow_relief(mp + vec2<f32>(0.0, e), fine) - h0) / e;
    let tg = vec2<f32>(gx.g, gy.g) - g0.g;
    let wg = vec2<f32>(water_at(mp + vec2<f32>(0.2, 0.0)), water_at(mp + vec2<f32>(0.0, 0.2))) - w0;
    // Snow drifts up against trees and pinnacles.
    let a0 = drift_at(mp);
    let ag = (vec2<f32>(drift_at(mp + vec2<f32>(0.25, 0.0)), drift_at(mp + vec2<f32>(0.0, 0.25))) - a0) * 0.8;
    slope += -tg * 1.6 + wg * 3.0 * ss(0.12, 0.42, wm) * (1.0 - water) + ag * 2.0;
    n = normalize(n + vec3<f32>(-slope.x, slope.y, 0.0) * (1.0 - ice) * (1.0 - cliff) * (1.0 - water));
    // Wind-packed drift around the ice.
    let rim = ss(0.3, 0.47, m) * (1.0 - ice);

    // Snow: soft drifts, fine grain and wind ripples.
    let drift = vnoise(mp * 0.18);
    let grain = vnoise(mp * 3.1) * 0.6 + vnoise(mp * 11.0) * 0.4;
    let rip = sin(dot(mp, vec2<f32>(1.7, 1.1)) * 3.2 + vnoise(mp * 0.6) * 7.0) * 0.5 + 0.5;
    var snow = vec3<f32>(1.0) * (0.95 + drift * 0.07 + grain * 0.03) - vec3<f32>(0.03, 0.025, 0.01) * rip * (1.0 - drift);
    snow = mix(snow, vec3<f32>(1.0), rim * 0.5);
    // Hollows between drifts hold a little blue.
    snow *= mix(vec3<f32>(0.9, 0.94, 1.0), vec3<f32>(1.0), ss(0.25, 0.8, h0));
    // Trodden snow: flattened, greyer and blotchy with footprints.
    let trod = g0.g * (1.0 - ice);
    let blot = vnoise(mp * 2.3) * 0.6 + vnoise(mp * 7.0) * 0.4;
    snow = mix(snow, vec3<f32>(0.80, 0.85, 0.92) * (0.93 + blot * 0.12), trod * 0.75);

    // Ice: deep blue with lighter frosted patches and cracks.
    let frost = ss(0.35, 0.8, vnoise(mp * 0.55 + 7.0));
    let cw = mp + vec2<f32>(vnoise(mp * 2.0), vnoise(mp * 2.0 + 5.0)) * 0.35;
    let crack = ss(0.03, 0.0, cracks(cw * 0.32)) * 0.6 + ss(0.03, 0.0, cracks(cw * 0.9 + 3.0)) * 0.3;
    // Clear black ice shows deep blue; frosted ice is pale.
    var icec = mix(vec3<f32>(0.13, 0.38, 0.64), vec3<f32>(0.6, 0.8, 0.93), frost * 0.75) + crack * vec3<f32>(0.3, 0.32, 0.32);
    // Frozen bubbles in the clear parts.
    let bc = floor(mp * 6.0);
    let bub = step(0.93, hash(bc + 31.0)) * ss(0.22, 0.1, length(fract(mp * 6.0) - 0.5)) * (1.0 - frost);
    icec += bub * vec3<f32>(0.25, 0.3, 0.3) * fine;
    // Snow blown across the ice in wisps along the wind.
    let wq = mp + vec2<f32>(vnoise(mp * 0.25), vnoise(mp * 0.25 + 3.0)) * 2.5;
    let along = vec2<f32>(wq.x * 0.8 + wq.y * 0.6, wq.y * 0.8 - wq.x * 0.6);
    let dust = ss(0.5, 0.95, vnoise(vec2<f32>(along.x * 0.35, along.y * 1.6)) * 0.75 + vnoise(mp * 0.2 + 11.0) * 0.4);
    icec = mix(icec, vec3<f32>(0.9, 0.94, 1.0), dust * 0.6);
    // The snow bank shades the ice next to it.
    icec *= 0.78 + 0.22 * ss(0.53, 0.75, m);

    // Water: deep blue, lighter where it is shallow near the bank, with
    // drifting ripples; snowy foam laps the edge.
    let t = u.eye.w;
    let rip1 = vnoise(mp * 1.7 + vec2<f32>(t * 0.35, t * 0.2));
    let rip2 = vnoise(mp * 4.3 - vec2<f32>(t * 0.5, t * 0.15));
    // Teal over the shelf just inside the edge, near black where it is deep;
    // the hole's snow wall shades the water on the side towards the sun.
    let deep = mix(vec3<f32>(0.14, 0.42, 0.56), vec3<f32>(0.03, 0.13, 0.27), ss(0.5, 0.9, wm));
    let sun_mp = normalize(vec2<f32>(u.sun.x, -u.sun.y) + 1e-4);
    let wall_shade = max(dot(normalize(wg + 1e-5), -sun_mp), 0.0) * ss(0.68, 0.5, wm);
    let waterc = (deep + (rip1 * 0.6 + rip2 * 0.4 - 0.5) * 0.06) * (1.0 - wall_shade * 0.6);
    let wall = mix(vec3<f32>(0.35, 0.6, 0.78), vec3<f32>(0.75, 0.9, 1.0), clamp(i.pos.z + 0.6, 0.0, 1.0));
    var albedo = mix(mix(snow, icec, ice), wall, cliff);
    albedo = mix(albedo, vec3<f32>(0.95, 0.98, 1.0), foam * (0.55 + 0.35 * sin(t * 1.6 + mp.x * 2.0 + mp.y * 1.3)) * (1.0 - cliff));
    albedo = mix(albedo, waterc, water);
    n = normalize(mix(n, normalize(n + vec3<f32>(rip1 - 0.5, rip2 - 0.5, 0.0) * 0.5), water));
    let sh = shadow_at(i.lpos);
    let L = normalize(u.sun.xyz);
    let diff = max(dot(n, L), 0.0);
    // Shadowed snow picks up the blue of the sky; sunlit snow is a little
    // brighter than white and glows.
    var col = albedo * (sky_light(n) * 0.55 + SUN_COLOR * 0.95 * diff * sh);
    let V = normalize(u.eye.xyz - i.pos);
    let H = normalize(L + V);
    let nh = max(dot(n, H), 0.0);
    let fres = pow(1.0 - max(dot(n, V), 0.0), 4.0);
    // Snow's soft sheen toward the sun, and light scattered through it in shade.
    let snowy = (1.0 - ice) * (1.0 - water) * (1.0 - cliff);
    col += snowy * (pow(nh, 20.0) * 0.12 * sh * SUN_COLOR + (1.0 - sh) * vec3<f32>(0.0, 0.03, 0.08));
    col += ice * (pow(nh, 90.0) * 1.8 * sh + fres * vec3<f32>(0.55, 0.7, 0.95) * 0.4 + (1.0 - frost) * 0.04);
    col += water * (pow(nh, 140.0) * 2.5 * sh + (0.06 + fres) * sky_light(reflect(-V, n)) * 0.55);
    // Sparkles: tiny points that twinkle as the camera moves.
    let cell = floor(mp * 14.0);
    let fc = fract(mp * 14.0) - 0.5;
    let pick = step(0.97, hash(cell + floor(u.eye.xy * 0.6) * 17.0));
    let glint = pick * ss(0.18, 0.0, length(fc)) * pow(nh, 4.0) * sh;
    col += (1.0 - ice) * (1.0 - cliff) * (1.0 - trod) * glint * 2.2;
    col = night_tint(col);
    // Firelight: warm pools on the snow, glints off ice and water.
    col += albedo * local_diffuse(i.pos, n) + (ice * 0.8 + water * 1.6) * local_spec(i.pos, n, V, 80.0);
    let reg = tint_at(mp);
    // Ground you have seen but cannot see now fades to an old hand-drawn
    // map: tea-stained paper, ink outlines along shores and floes, and
    // contour lines. (Memory is white in the tint map, darkness is black.)
    let remembered = ss(0.6, 1.0, reg.b);
    let mem = remembered * reg.a;
    let unknown = ss(0.75, 1.0, reg.a) * (1.0 - remembered);
    if (mem > 0.01) {
        // Just out of sight: plainly dark. Later (reg.r) it is redrawn by hand.
        let lum = dot(col, vec3<f32>(0.3, 0.59, 0.11));
        let dark = col * vec3<f32>(0.34, 0.38, 0.5);
        let paper_n = vnoise(mp * 6.0) * 0.5 + vnoise(mp * 23.0) * 0.5;
        var paper = vec3<f32>(0.89, 0.80, 0.60) * (0.88 + paper_n * 0.14) * (0.62 + lum * 0.5);
        let edge = ss(0.045, 0.0, abs(m - 0.5)) + ss(0.045, 0.0, abs(wm - 0.5));
        let contour = ss(0.05, 0.0, abs(fract(i.pos.z * 2.0) - 0.5) - 0.44) * 0.35;
        let ink = clamp(edge + contour + cliff * 0.5, 0.0, 1.0);
        paper = mix(paper, vec3<f32>(0.24, 0.16, 0.09), ink * 0.8);
        col = mix(col, mix(dark, paper, ss(0.0, 1.0, reg.r)), mem);
    } else if (unknown < 0.01) {
        col = mix(col, col * (reg.rgb * 0.75 + 0.35), reg.a);
    }
    col = mix(col, fow_cloud(mp), cloud_cover(mp, unknown));
    return vec4<f32>(apply_fog(col, i.pos), 1.0);
}
"#;

// --- Models -------------------------------------------------------------------

const MODEL: &str = r#"
struct MOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) pos: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) team: vec4<f32>,
    @location(4) misc: vec4<f32>,
    @location(5) lpos: vec4<f32>,
    // The model's own space, for painting its surface.
    @location(6) mpos: vec3<f32>,
    @location(7) mnrm: vec3<f32>,
}

// How much of a pattern `f` times a tile to keep when one pixel covers `px`
// of the model: fine patterns fade out before they would shimmer.
fn keep(f: f32, px: f32) -> f32 {
    return ss(0.5, 0.2, f * px);
}

// Paint a surface by what it is made of (see MAT_* in model.rs): returns
// the albedo and, in w, how shiny it is.
fn material(mat: u32, albedo: vec3<f32>, p: vec3<f32>, n: vec3<f32>, px: f32) -> vec4<f32> {
    // Project onto the side the face looks out of; on walls v runs up.
    let a = abs(n);
    var uv = p.xy;
    if (a.x > a.y && a.x > a.z) { uv = p.yz; } else if (a.y > a.z) { uv = p.xz; }
    if (mat == 1u) {
        // Packed snow: soft grain, bluer in its pits, and a few glints.
        let g = vnoise(uv * 7.0) * 0.6 + (vnoise(uv * 19.0) - 0.5) * 0.8 * keep(19.0, px);
        let c = albedo * mix(vec3<f32>(0.9, 0.94, 1.0), vec3<f32>(1.03), g);
        let glint = step(0.985, hash(floor(uv * 26.0))) * keep(26.0, px);
        return vec4<f32>(c, 0.15 + glint * 3.0);
    }
    if (mat == 2u) {
        // Wood: long streaks of grain along the board, and darker knots.
        let w = vnoise(vec2<f32>(uv.x * 16.0, uv.y * 1.3)) * 0.55 + vnoise(vec2<f32>(uv.x * 45.0, uv.y * 3.0) + 7.0) * 0.45 * keep(45.0, px);
        let knot = ss(0.82, 0.95, vnoise(uv * 3.1 + 3.0)) * 0.3;
        return vec4<f32>(albedo * (0.74 + 0.4 * w - knot), 0.08);
    }
    if (mat == 3u) {
        // Stone: mottled, with pale flecks.
        let m = vnoise(uv * 2.7) * 0.5 + vnoise(uv * 7.5 + 3.0) * 0.3 + vnoise(uv * 22.0) * 0.2 * keep(22.0, px);
        let fleck = step(0.93, hash(floor(uv * 18.0))) * keep(18.0, px) * 0.15;
        return vec4<f32>(albedo * (0.78 + 0.38 * m) + fleck, 0.12);
    }
    if (mat == 4u) {
        // Cloth: a woven texture and soft folds.
        let weave = (sin(uv.x * 110.0) * sin(uv.y * 110.0)) * keep(18.0, px);
        let fold = vnoise(vec2<f32>(uv.x * 5.0, uv.y * 1.5));
        return vec4<f32>(albedo * (0.86 + 0.04 * weave + 0.2 * fold), 0.05);
    }
    if (mat == 5u) {
        // Dark metal: brushed, and shiny.
        let b = vnoise(vec2<f32>(uv.x * 2.0, uv.y * 40.0)) * keep(40.0, px);
        return vec4<f32>(albedo * (0.85 + 0.3 * b), 0.9);
    }
    if (mat == 6u) {
        // Ice: cloudy inside, with frozen-in streaks.
        let cloud = vnoise(uv * 2.2) * 0.6 + vnoise(uv * 6.0) * 0.4;
        return vec4<f32>(mix(albedo * 0.85, vec3<f32>(0.92, 0.97, 1.0), cloud * 0.35), 1.0);
    }
    return vec4<f32>(albedo, 0.15);
}

@vertex
fn vs(
    @location(0) color: vec4<f32>,
    @location(1) pos0: vec3<f32>,
    @location(2) nrm0: vec3<f32>,
    @location(3) pos1: vec3<f32>,
    @location(4) nrm1: vec3<f32>,
    @location(5) pos_yaw: vec4<f32>,
    @location(6) misc: vec4<f32>,
    @location(7) team: vec4<f32>,
    @location(8) tilt: vec4<f32>,
) -> MOut {
    var p = mix(pos0, pos1, misc.y) * misc.x;
    var n = mix(nrm0, nrm1, misc.y);
    let mpos = p;
    let mnrm = n;
    // Body language: squash, lean forward (towards -y), roll to the side.
    p = vec3<f32>(p.xy * (1.0 + tilt.z * 0.5), p.z * (1.0 - tilt.z));
    let cl = cos(tilt.x);
    let sl = sin(tilt.x);
    p = vec3<f32>(p.x, p.y * cl - p.z * sl, p.y * sl + p.z * cl);
    n = vec3<f32>(n.x, n.y * cl - n.z * sl, n.y * sl + n.z * cl);
    let cr = cos(tilt.y);
    let sr = sin(tilt.y);
    p = vec3<f32>(p.x * cr + p.z * sr, p.y, p.z * cr - p.x * sr);
    n = vec3<f32>(n.x * cr + n.z * sr, n.y, n.z * cr - n.x * sr);
    let c = cos(pos_yaw.w);
    let s = sin(pos_yaw.w);
    p = vec3<f32>(c * p.x - s * p.y, s * p.x + c * p.y, p.z) + pos_yaw.xyz;
    n = vec3<f32>(c * n.x - s * n.y, s * n.x + c * n.y, n.z);
    var o: MOut;
    o.clip = u.viewproj * vec4<f32>(p, 1.0);
    o.pos = p;
    o.nrm = n;
    o.color = color;
    o.team = team;
    o.misc = misc;
    o.lpos = u.light * vec4<f32>(p, 1.0);
    o.mpos = mpos;
    o.mnrm = mnrm;
    return o;
}

@fragment
fn fs(i: MOut) -> @location(0) vec4<f32> {
    let flags = u32(i.color.a * 255.0 + 0.5);
    let team = f32(flags & 1u);
    let emissive = f32((flags >> 1u) & 1u);
    let glossy = (flags & 4u) != 0u;
    let mat = (flags >> 3u) & 7u;
    let px = length(fwidth(i.mpos));
    var albedo = select(i.color.rgb, i.team.rgb, team > 0.5);
    let surf = material(mat, albedo, i.mpos, normalize(i.mnrm), px);
    albedo = surf.rgb;
    // Ruins are unowned, so their whole batch is drawn darker.
    albedo *= u.tint.x;
    var n = normalize(i.nrm);
    let V = normalize(u.eye.xyz - i.pos);
    if (dot(n, V) < 0.0) { n = -n; }
    let lum = dot(albedo, vec3<f32>(0.3, 0.55, 0.15));
    albedo = mix(albedo, vec3<f32>(0.6, 0.8, 1.0) * (0.45 + lum), i.team.a * 0.75);
    let L = normalize(u.sun.xyz);
    let sh = shadow_at(i.lpos);
    let diff = max(dot(n, L), 0.0);
    // Wrap lighting keeps the dark side of penguins readable; the snow
    // throws a bright fill up from below.
    let wrap = max((dot(n, L) + 0.35) / 1.35, 0.0);
    let bounce = vec3<f32>(0.92, 0.95, 1.0) * max(-n.z, 0.0) * 0.35;
    var col = albedo * (sky_light(n) * 0.5 + bounce + SUN_COLOR * (0.45 * wrap + 0.45 * diff) * sh);
    let H = normalize(L + V);
    let fres = pow(1.0 - max(dot(n, V), 0.0), 5.0);
    let spec = pow(max(dot(n, H), 0.0), select(24.0, 90.0, glossy)) * select(surf.w, 1.6, glossy) * (0.6 + fres);
    col += spec * sh * SUN_COLOR;
    let rim = pow(1.0 - max(dot(n, V), 0.0), 2.5);
    col += rim * mix(vec3<f32>(0.45, 0.6, 0.85) * 0.3, i.team.rgb * 1.4, min(i.misc.w, 1.0));
    col = night_tint(col);
    col += albedo * local_diffuse(i.pos, n) + local_spec(i.pos, n, V, select(24.0, 90.0, glossy)) * (surf.w * 0.5 + 0.1);
    // Glowing parts (fire, lamps) shine past white, so they bloom.
    col = mix(col, albedo * 2.2, emissive * 0.8);
    // Above 1.0 the highlight slot carries a white hit flash.
    col = mix(col, vec3<f32>(1.0), clamp(i.misc.w - 1.0, 0.0, 1.0) * 0.7);
    // Remembered ground is an old map: so is whatever stands on it.
    let reg = tint_at(vec2<f32>(i.pos.x, -i.pos.y));
    let remembered = ss(0.6, 1.0, reg.b);
    let mem = remembered * reg.a;
    let unknown = ss(0.75, 1.0, reg.a) * (1.0 - remembered);
    let l = dot(col, vec3<f32>(0.3, 0.59, 0.11));
    let dark = col * vec3<f32>(0.34, 0.38, 0.5);
    let paper = vec3<f32>(0.89, 0.8, 0.6) * (0.5 + l * 0.65);
    col = mix(col, mix(dark, paper, ss(0.0, 1.0, reg.r)), mem);
    if (unknown < 0.01) { col = mix(col, col * (reg.rgb * 0.75 + 0.35), reg.a * (1.0 - remembered)); }
    let mp = vec2<f32>(i.pos.x, -i.pos.y);
    let cover = cloud_cover(mp, unknown);
    // Whatever stands under the cloud is hidden, not cloud-coloured.
    if (cover > 0.5) { discard; }
    col = mix(col, fow_cloud(mp), cover);
    return vec4<f32>(apply_fog(col, i.pos), i.misc.z);
}
"#;

// --- Effects (particles, decals, rings) ------------------------------------

const FX: &str = r#"
struct FOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) kind: f32,
}

@vertex
fn vs(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>, @location(3) kind: vec4<f32>) -> FOut {
    var o: FOut;
    o.clip = u.viewproj * vec4<f32>(pos, 1.0);
    o.uv = uv;
    o.color = color;
    o.kind = kind.x * 255.0;
    return o;
}

// Soft line segment from a to b, `w` wide.
fn seg(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, w: f32) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let t = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return ss(w, w * 0.4, length(pa - ba * t));
}

@fragment
fn fs(i: FOut) -> @location(0) vec4<f32> {
    let q = i.uv * 2.0 - 1.0;
    let d = length(q);
    var a = 1.0;
    if (i.kind < 0.5) {
        a = ss(1.0, 0.35, d);
    } else if (i.kind < 1.5) {
        a = ss(0.14, 0.0, abs(d - 0.8));
    } else if (i.kind < 2.5) {
        // A three-toed penguin print.
        let h = vec2<f32>(0.5, 0.78);
        a = max(seg(i.uv, h, vec2<f32>(0.5, 0.2), 0.09), max(seg(i.uv, h, vec2<f32>(0.18, 0.3), 0.08), seg(i.uv, h, vec2<f32>(0.82, 0.3), 0.08)));
    } else if (i.kind < 3.5) {
        let ang = atan2(q.y, q.x);
        let wob = 0.72 + 0.12 * sin(ang * 5.0) + 0.08 * sin(ang * 11.0 + 1.3);
        a = ss(wob, wob - 0.35, d) * 0.85;
    } else if (i.kind < 4.5) {
        a = 1.0;
    } else {
        a = ss(1.0, 0.0, d);
    }
    if (i.color.a * a < 0.01) { discard; }
    return vec4<f32>(i.color.rgb, i.color.a * a);
}
"#;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FxVert {
    pos: [f32; 3],
    uv: [f32; 2],
    color: [u8; 4],
    kind: [u8; 4],
}

const FX_DISC: u8 = 0;
const FX_RING: u8 = 1;
const FX_FOOT: u8 = 2;
const FX_SPLAT: u8 = 3;
const FX_BAR: u8 = 4;
const FX_BLOB: u8 = 5;

// --- Sky / water --------------------------------------------------------------

const SKY: &str = r#"
struct SOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) pos: vec2<f32>,
}

@vertex
fn vs(@builtin(vertex_index) v: u32) -> SOut {
    let p = vec2<f32>(f32(v & 1u) * 4.0 - 1.0, f32(v >> 1u) * 4.0 - 1.0);
    var o: SOut;
    o.clip = vec4<f32>(p, 0.9999, 1.0);
    o.pos = p;
    return o;
}

@fragment
fn fs(i: SOut) -> @location(0) vec4<f32> {
    let t = i.pos.y * 0.5 + 0.5;
    let day = mix(vec3<f32>(0.78, 0.84, 0.92), vec3<f32>(0.42, 0.58, 0.82), t);
    let night = mix(vec3<f32>(0.2, 0.24, 0.38), vec3<f32>(0.05, 0.07, 0.18), t);
    var c = mix(day, night, u.map.z);
    // Storms wash the sky out to the fog colour.
    c = mix(c, u.fog_color.rgb, u.map.w * 0.4);
    return vec4<f32>(c, 1.0);
}
"#;

const WATER: &str = r#"
@fragment
fn fs_water(i: TOut) -> @location(0) vec4<f32> {
    let p = i.pos.xy;
    let t = u.eye.w;
    let w1 = vnoise(p * 0.35 + vec2<f32>(t * 0.12, t * 0.05));
    let w2 = vnoise(p * 1.1 - vec2<f32>(t * 0.07, t * 0.15));
    let n = normalize(vec3<f32>((w1 - 0.5) * 0.35 + (w2 - 0.5) * 0.25, (w2 - 0.5) * 0.35, 1.0));
    let V = normalize(u.eye.xyz - i.pos);
    let L = normalize(u.sun.xyz);
    let fres = pow(1.0 - max(dot(n, V), 0.0), 3.0);
    // Clear polar water: teal in the shallows, deeper blue further out.
    let shore = i.attr.x;
    let deep = mix(vec3<f32>(0.07, 0.27, 0.42), vec3<f32>(0.16, 0.48, 0.58), shore);
    var col = mix(deep, vec3<f32>(0.62, 0.74, 0.88), fres * 0.7 + 0.06);
    // Soft caustic-like swell pattern so it reads as water from above.
    let r = mat2x2<f32>(vec2<f32>(0.6, 0.8), vec2<f32>(-0.8, 0.6)) * p;
    let sw = vnoise(r * 0.45 + vec2<f32>(t * 0.2, -t * 0.1)) + vnoise(p * 1.3 - vec2<f32>(t * 0.05, t * 0.13)) * 0.5;
    col += ss(0.7, 1.2, sw) * vec3<f32>(0.04, 0.07, 0.08);
    col += pow(max(dot(reflect(-L, n), V), 0.0), 120.0) * vec3<f32>(1.0, 0.95, 0.85) * 1.2;
    // Lapping foam where the floe meets the sea.
    let q = mat2x2<f32>(vec2<f32>(0.8, -0.6), vec2<f32>(0.6, 0.8)) * p;
    let lap = vnoise(q * 1.6 + vec2<f32>(t * 0.35, 0.0)) * 0.6 + vnoise(q * 4.1 - vec2<f32>(0.0, t * 0.5)) * 0.4;
    let foam = ss(0.8, 1.0, shore) * (0.45 + 0.55 * lap) + ss(0.3, 0.9, shore) * lap * 0.25;
    col = mix(col, vec3<f32>(0.88, 0.94, 0.98), foam * 0.85);
    col = night_tint(col) + local_spec(i.pos, n, V, 60.0) * 1.5;
    return vec4<f32>(apply_fog(col, i.pos), 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy)]
struct TerrainVert {
    pos: [f32; 3],
    nrm: [f32; 3],
    attr: [u8; 4],
}

struct Chunk {
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    n: u32,
}

struct Flake {
    pos: Vec3,
    phase: f32,
}

pub const TREE_VARIANTS: u8 = 3;

/// The model file (`assets/models/<file>.sbm`) made for a kind, if any.
pub fn unit_file(k: Kind) -> &'static str {
    match k {
        Kind::Worker => "unit-worker",
        Kind::Snowballer => "unit-snowballer",
        Kind::Shield => "unit-shieldbearer",
        Kind::Slinger => "unit-slinger",
        Kind::Skier => "unit-skier",
        Kind::Packer => "unit-packer",
        Kind::Warmer => "unit-warmer",
        Kind::Sledder => "unit-sled",
        Kind::Chieftain => "unit-chieftain",
    }
}

pub const STUMP_VARIANTS: u8 = 3;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum M {
    Worker,
    Warrior,
    /// A kind's own model, from tools/penguins.py.
    Unit(u8),
    Tree(u8),
    Igloo(u8),
    Snowball,
    HoleOpen,
    HoleFrozen,
    Crystal(u8),
    Pinnacle(u8),
    Power(u8),
    Ice,
    Stump(u8),
    Log,
    Seal,
    Building(u8, u8),
    Prop(u8),
    Ruin,
}

/// One instanced batch: model, animation index, frame range, instances.
/// What an instance group draws: model, then the animation and frame each
/// end of the blend is taken from (the same animation except while a
/// penguin crossfades from one clip into the next).
type Key = (M, usize, usize, usize, usize);
type InstGroup = (Key, Vec<Inst>);

/// Model for a building kind and variant.
fn building_model(kind: BuildKind, variant: u8) -> M {
    match kind {
        BuildKind::Palisade => M::Building(kind as u8, variant % 3),
        _ => M::Building(kind as u8, 0),
    }
}

struct Pipes {
    terrain: wgpu::RenderPipeline,
    water: wgpu::RenderPipeline,
    model: wgpu::RenderPipeline,
    model_blend: wgpu::RenderPipeline,
    fx: wgpu::RenderPipeline,
    sky: wgpu::RenderPipeline,
    terrain_shadow: wgpu::RenderPipeline,
    model_shadow: wgpu::RenderPipeline,
}

pub struct Render3D {
    // Camera
    target: Vec2,
    dist: f32,
    yaw: f32,
    vw: f32,
    vh: f32,
    proj: Mat4,
    view_proj: Mat4,
    inv_view_proj: Mat4,
    eye: Vec3,
    // Data
    map: Option<Map>,
    pub shadows: bool,
    // GPU
    pipes: Pipes,
    world_layout: wgpu::BindGroupLayout,
    post: Post,
    ubuf: wgpu::Buffer,
    ubind: wgpu::BindGroup,
    shadow_map: wgpu::TextureView,
    no_shadow: wgpu::TextureView,
    shadow_samp: wgpu::Sampler,
    /// The world textures' bind group, and what it was made from.
    world_bind: Option<((TextureId, TextureId, bool), wgpu::BindGroup)>,
    water: Option<Chunk>,
    chunks: Vec<Chunk>,
    tint_tex: Option<TextureId>,
    /// Per-tile ground type (R = ice), filtered for smooth ice edges.
    ground_tex: Option<TextureId>,
    tint_seen: u32,
    pub(crate) models: HashMap<M, Model>,
    inst_buf: wgpu::Buffer,
    fx_vbuf: wgpu::Buffer,
    fx_ibuf: wgpu::Buffer,
    statics: Vec<(M, Inst)>,
    terrain_seen: u32,
    /// Weather from the last scene (see [`Scene`]).
    night: f32,
    storm: f32,
    /// This frame's point lights (see `gather_lights`) and how many.
    lights: ([[f32; 4]; MAX_LIGHTS], [[f32; 4]; MAX_LIGHTS], usize),
    snowfall: f32,
    flakes: Vec<Flake>,
    rng: Rng,
    gaits: HashMap<u16, Gait>,
    last_time: f32,
    /// AO strength, bloom strength, exposure (dev switch for tuning:
    /// SNOWBALLZ_POST="ao bloom exposure").
    tune: [f32; 3],
}

fn shader(gpu: &Gpu, label: &str, parts: &[&str]) -> wgpu::ShaderModule {
    gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(parts.concat().into()) })
}

fn depth_state(write: bool, bias: wgpu::DepthBiasState) -> Option<wgpu::DepthStencilState> {
    Some(wgpu::DepthStencilState {
        format: DEPTH,
        depth_write_enabled: Some(write),
        depth_compare: Some(wgpu::CompareFunction::LessEqual),
        stencil: Default::default(),
        bias,
    })
}

#[allow(clippy::too_many_arguments)]
fn pipeline(
    gpu: &Gpu,
    label: &str,
    module: &wgpu::ShaderModule,
    fs: Option<(&str, wgpu::TextureFormat, Option<wgpu::BlendState>)>,
    layouts: &[&wgpu::BindGroupLayout],
    buffers: &[wgpu::VertexBufferLayout],
    depth: Option<wgpu::DepthStencilState>,
    samples: u32,
) -> wgpu::RenderPipeline {
    let layouts: Vec<Option<&wgpu::BindGroupLayout>> = layouts.iter().map(|l| Some(*l)).collect();
    let buffers: Vec<Option<wgpu::VertexBufferLayout>> = buffers.iter().cloned().map(Some).collect();
    let layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: &layouts, immediate_size: 0 });
    let targets = fs.map(|(_, format, blend)| [Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })]);
    gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState { module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &buffers },
        fragment: fs.map(|(entry, _, _)| wgpu::FragmentState {
            module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: targets.as_ref().unwrap(),
        }),
        primitive: Default::default(),
        depth_stencil: depth,
        multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

fn world_layout(gpu: &Gpu) -> wgpu::BindGroupLayout {
    let tex = |binding: u32, sample_type: wgpu::TextureSampleType| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
        count: None,
    };
    let samp = |binding: u32, ty: wgpu::SamplerBindingType| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(ty),
        count: None,
    };
    gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("world textures"),
        entries: &[
            tex(0, wgpu::TextureSampleType::Depth),
            samp(1, wgpu::SamplerBindingType::Comparison),
            tex(2, wgpu::TextureSampleType::Float { filterable: true }),
            tex(3, wgpu::TextureSampleType::Float { filterable: true }),
            samp(4, wgpu::SamplerBindingType::Filtering),
        ],
    })
}

fn depth_texture(gpu: &Gpu, label: &str, w: u32, h: u32) -> wgpu::TextureView {
    gpu.device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

impl Render3D {
    pub fn new(ctx: &mut Gpu, assets: &std::path::Path) -> Result<Render3D, String> {
        let gpu = &*ctx;
        let ulayout = gfx::uniform_layout(gpu, true);
        let world_layout = world_layout(gpu);
        let hdr = gpu.hdr_format;
        let terrain_mod = shader(gpu, "terrain", &[COMMON, TEXTURES, TERRAIN, WATER]);
        let model_mod = shader(gpu, "model", &[COMMON, TEXTURES, MODEL]);
        let fx_mod = shader(gpu, "fx", &[COMMON, FX]);
        let sky_mod = shader(gpu, "sky", &[COMMON, SKY]);

        let terrain_buf = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TerrainVert>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4],
        };
        let frame0 = wgpu::vertex_attr_array![1 => Float32x3, 2 => Float32x3];
        let frame1 = wgpu::vertex_attr_array![3 => Float32x3, 4 => Float32x3];
        let inst = wgpu::vertex_attr_array![5 => Float32x4, 6 => Float32x4, 7 => Unorm8x4, 8 => Float32x4];
        let model_bufs = [
            wgpu::VertexBufferLayout { array_stride: 4, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Unorm8x4] },
            wgpu::VertexBufferLayout { array_stride: 24, step_mode: wgpu::VertexStepMode::Vertex, attributes: &frame0 },
            wgpu::VertexBufferLayout { array_stride: 24, step_mode: wgpu::VertexStepMode::Vertex, attributes: &frame1 },
            wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Inst>() as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &inst },
        ];
        let fx_buf = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<FxVert>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Unorm8x4, 3 => Unorm8x4],
        };
        let none = wgpu::DepthBiasState::default();
        // Slope-scaled bias keeps the sun's shadow off the surface casting it.
        let shadow_bias = wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 };
        let world = [&ulayout, &world_layout];
        let blend = Some(gfx::ALPHA_BLEND);
        let fx_blend = Some(wgpu::BlendState {
            color: gfx::ALPHA_BLEND.color,
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        });
        let pipes = Pipes {
            terrain: pipeline(gpu, "terrain", &terrain_mod, Some(("fs", hdr, None)), &world, std::slice::from_ref(&terrain_buf), depth_state(true, none), gpu.msaa),
            water: pipeline(gpu, "water", &terrain_mod, Some(("fs_water", hdr, None)), &world, std::slice::from_ref(&terrain_buf), depth_state(true, none), gpu.msaa),
            model: pipeline(gpu, "model", &model_mod, Some(("fs", hdr, None)), &world, &model_bufs, depth_state(true, none), gpu.msaa),
            model_blend: pipeline(gpu, "model blend", &model_mod, Some(("fs", hdr, blend)), &world, &model_bufs, depth_state(true, none), gpu.msaa),
            fx: pipeline(gpu, "fx", &fx_mod, Some(("fs", hdr, fx_blend)), &[&ulayout], std::slice::from_ref(&fx_buf), depth_state(false, none), gpu.msaa),
            sky: pipeline(gpu, "sky", &sky_mod, Some(("fs", hdr, None)), &[&ulayout], &[], depth_state(false, none), gpu.msaa),
            terrain_shadow: pipeline(gpu, "terrain shadow", &terrain_mod, None, &[&ulayout], std::slice::from_ref(&terrain_buf), depth_state(true, shadow_bias), 1),
            model_shadow: pipeline(gpu, "model shadow", &model_mod, None, &[&ulayout], &model_bufs, depth_state(true, shadow_bias), 1),
        };
        let ubuf = gpu.buffer("world uniforms", wgpu::BufferUsages::UNIFORM, &vec![0u8; (SLOT_STRIDE * SLOTS as u64) as usize]);
        let ubind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world uniforms"),
            layout: &ulayout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &ubuf,
                    offset: 0,
                    size: std::num::NonZeroU64::new(std::mem::size_of::<WorldUniforms>() as u64),
                }),
            }],
        });
        let shadow_map = depth_texture(gpu, "shadow map", SHADOW_SIZE, SHADOW_SIZE);
        let no_shadow = depth_texture(gpu, "no shadow", 1, 1);
        let shadow_samp = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        let mut models = HashMap::new();
        let mdir = assets.join("models");
        models.insert(M::Worker, Model::load(ctx, &mdir.join("worker.sbm"))?);
        models.insert(M::Warrior, Model::load(ctx, &mdir.join("warrior.sbm"))?);
        // Each kind's own penguin, where tools/penguins.py has made one.
        for k in Kind::ALL {
            let file = unit_file(k);
            let path = mdir.join(format!("{file}.sbm"));
            if crate::assets::exists(&path) {
                match Model::load(ctx, &path) {
                    Ok(m) => {
                        models.insert(M::Unit(k as u8), m);
                    }
                    Err(e) => eprintln!("using the old penguin for {file}: {e}"),
                }
            }
        }
        for i in 0..TREE_VARIANTS {
            models.insert(M::Tree(i), Model::baked_or(ctx, &mdir, &format!("tree-{i}"), &meshgen::pine(i as u64)));
        }
        for t in 0..3u8 {
            models.insert(M::Igloo(t), Model::baked_or(ctx, &mdir, &format!("igloo-{t}"), &meshgen::igloo_tier(t)));
        }
        models.insert(M::Snowball, Model::baked_or(ctx, &mdir, "snowball", &meshgen::snowball()));
        models.insert(M::HoleOpen, Model::baked_or(ctx, &mdir, "hole-open", &meshgen::fish_hole(true)));
        models.insert(M::HoleFrozen, Model::baked_or(ctx, &mdir, "hole-frozen", &meshgen::fish_hole(false)));
        for (i, s) in [0.0, 0.35, 0.7, 1.0].iter().enumerate() {
            models.insert(M::Crystal(i as u8), Model::baked_or(ctx, &mdir, &format!("crystal-{i}"), &meshgen::crystal(*s)));
        }
        for i in 0..3 {
            models.insert(M::Pinnacle(i), Model::baked_or(ctx, &mdir, &format!("pinnacle-{i}"), &meshgen::pinnacle(i as u64 + 1)));
        }
        models.insert(M::Power(0), Model::baked_or(ctx, &mdir, "power-defense", &meshgen::powerup(PowerKind::Defense)));
        models.insert(M::Power(1), Model::baked_or(ctx, &mdir, "power-double", &meshgen::powerup(PowerKind::Double)));
        models.insert(M::Ice, Model::baked_or(ctx, &mdir, "ice", &meshgen::ice_block()));
        for i in 0..STUMP_VARIANTS {
            models.insert(M::Stump(i), Model::baked_or(ctx, &mdir, &format!("stump-{i}"), &meshgen::stump(i as u64)));
        }
        models.insert(M::Log, Model::baked_or(ctx, &mdir, "log", &meshgen::log()));
        models.insert(M::Seal, Model::baked_or(ctx, &mdir, "seal", &meshgen::seal()));
        models.insert(M::Ruin, Model::baked_or(ctx, &mdir, "ruin", &meshgen::igloo_ruin()));
        models.insert(M::Building(BuildKind::Sauna as u8, 0), Model::baked_or(ctx, &mdir, "sauna", &meshgen::hut()));
        models.insert(M::Building(BuildKind::Campfire as u8, 0), Model::baked_or(ctx, &mdir, "campfire", &meshgen::campfire()));
        models.insert(M::Building(BuildKind::Tower as u8, 0), Model::baked_or(ctx, &mdir, "tower", &meshgen::tower()));
        models.insert(M::Building(BuildKind::Catapult as u8, 0), Model::baked_or(ctx, &mdir, "catapult", &meshgen::catapult_tower()));
        models.insert(M::Building(BuildKind::Store as u8, 0), Model::baked_or(ctx, &mdir, "store", &meshgen::store()));
        models.insert(M::Building(BuildKind::SkiLodge as u8, 0), Model::baked_or(ctx, &mdir, "ski-lodge", &meshgen::ski_lodge()));
        models.insert(M::Building(BuildKind::SlingRange as u8, 0), Model::baked_or(ctx, &mdir, "sling-range", &meshgen::sling_range()));
        models.insert(M::Building(BuildKind::PackerYard as u8, 0), Model::baked_or(ctx, &mdir, "packer-yard", &meshgen::packer_yard()));
        models.insert(M::Building(BuildKind::Bunkhouse as u8, 0), Model::baked_or(ctx, &mdir, "bunkhouse", &meshgen::bunkhouse()));
        // A new igloo going up is drawn as the igloo it will be.
        models.insert(M::Building(BuildKind::Igloo as u8, 0), Model::baked_or(ctx, &mdir, "igloo-site", &meshgen::igloo_tier(0)));
        models.insert(M::Building(BuildKind::Workshop as u8, 0), Model::baked_or(ctx, &mdir, "workshop", &meshgen::workshop()));
        models.insert(M::Building(BuildKind::Track as u8, 0), Model::baked_or(ctx, &mdir, "track", &meshgen::track()));
        models.insert(M::Building(BuildKind::Spa as u8, 0), Model::baked_or(ctx, &mdir, "spa", &meshgen::spa()));
        for v in 0..3u8 {
            models.insert(M::Building(BuildKind::Palisade as u8, v), Model::baked_or(ctx, &mdir, &format!("palisade-{v}"), &meshgen::palisade(v as u64)));
        }
        for k in Kind::ALL {
            if let Some(m) = meshgen::prop(k) {
                models.insert(M::Prop(k as u8), Model::baked_or(ctx, &mdir, &format!("prop-{}", k.stats().name.to_lowercase().replace(' ', "-")), &m));
            }
        }

        let inst_buf = ctx.buffer("instances", wgpu::BufferUsages::VERTEX, &[]);
        let fx_vbuf = ctx.buffer("fx vertices", wgpu::BufferUsages::VERTEX, &[]);
        let fx_ibuf = ctx.buffer("fx indices", wgpu::BufferUsages::INDEX, &[]);

        let mut r = Render3D {
            target: v2(50.0, 50.0),
            dist: 26.0,
            yaw: 0.0,
            vw: 1280.0,
            vh: 720.0,
            proj: Mat4::IDENTITY,
            view_proj: Mat4::IDENTITY,
            inv_view_proj: Mat4::IDENTITY,
            eye: v3(0.0, 0.0, 0.0),
            map: None,
            shadows: true,
            pipes,
            world_layout,
            post: Post::new(ctx),
            ubuf,
            ubind,
            shadow_map,
            no_shadow,
            shadow_samp,
            world_bind: None,
            water: None,
            chunks: Vec::new(),
            tint_tex: None,
            ground_tex: None,
            tint_seen: u32::MAX,
            models,
            inst_buf,
            fx_vbuf,
            fx_ibuf,
            statics: Vec::new(),
            terrain_seen: 0,
            night: 0.0,
            storm: 0.0,
            lights: ([[0.0; 4]; MAX_LIGHTS], [[0.0; 4]; MAX_LIGHTS], 0),
            snowfall: 1.0,
            flakes: Vec::new(),
            rng: Rng::new(99),
            gaits: HashMap::new(),
            last_time: 0.0,
            tune: {
                let mut t = [0.7, 0.07, 1.3];
                if let Ok(v) = std::env::var("SNOWBALLZ_POST") {
                    for (i, x) in v.split_whitespace().filter_map(|x| x.parse().ok()).take(3).enumerate() {
                        t[i] = x;
                    }
                }
                t
            },
        };
        r.update_camera();
        Ok(r)
    }

    /// Build terrain meshes and static props for a map.
    pub fn prepare(&mut self, ctx: &mut Gpu, map: &Map) {
        self.chunks.clear();
        const CH: i32 = 32;
        let ice_at = |x: i32, y: i32| -> f32 {
            let mut s = 0.0;
            for (dx, dy) in [(-1, -1), (0, -1), (-1, 0), (0, 0)] {
                let (tx, ty) = ((x + dx).clamp(0, map.w - 1), (y + dy).clamp(0, map.h - 1));
                if map.ground_at(tx, ty) == Ground::Ice {
                    s += 0.25;
                }
            }
            s
        };
        let corner = |x: i32, y: i32| map.corner_height(x, y);
        let normal = |x: i32, y: i32| {
            let dx = corner(x + 1, y) - corner(x - 1, y);
            let dy = corner(x, y + 1) - corner(x, y - 1);
            // gl space: y flipped
            v3(-dx * 0.5, dy * 0.5, 1.0).norm()
        };
        for cy in (0..map.h).step_by(CH as usize) {
            for cx in (0..map.w).step_by(CH as usize) {
                let x1 = (cx + CH).min(map.w);
                let y1 = (cy + CH).min(map.h);
                let w = x1 - cx + 1;
                let mut verts = Vec::new();
                let mut idx: Vec<u16> = Vec::new();
                for y in cy..=y1 {
                    for x in cx..=x1 {
                        let n = normal(x, y);
                        verts.push(TerrainVert {
                            pos: [x as f32, -(y as f32), corner(x, y)],
                            nrm: [n.x, n.y, n.z],
                            attr: [(ice_at(x, y) * 255.0) as u8, 0, 0, 0],
                        });
                    }
                }
                for y in 0..(y1 - cy) {
                    for x in 0..(x1 - cx) {
                        let i = (y * w + x) as u16;
                        let j = i + 1;
                        let k = i + w as u16;
                        let l = k + 1;
                        idx.extend_from_slice(&[i, l, j, i, k, l]);
                    }
                }
                // Ice cliffs where the floe meets the sea.
                let mut skirt = |a: (i32, i32), b: (i32, i32), out: Vec3| {
                    let base = verts.len() as u16;
                    for &(x, y) in &[a, b] {
                        let z = corner(x, y);
                        verts.push(TerrainVert { pos: [x as f32, -(y as f32), z], nrm: [out.x, out.y, out.z], attr: [255, 255, 0, 0] });
                        verts.push(TerrainVert { pos: [x as f32, -(y as f32), -1.2], nrm: [out.x, out.y, out.z], attr: [255, 255, 0, 0] });
                    }
                    idx.extend_from_slice(&[base, base + 1, base + 3, base, base + 3, base + 2]);
                    idx.extend_from_slice(&[base, base + 3, base + 1, base, base + 2, base + 3]);
                };
                for x in cx..x1 {
                    if cy == 0 {
                        skirt((x, 0), (x + 1, 0), v3(0.0, 1.0, 0.0));
                    }
                    if y1 == map.h {
                        skirt((x, map.h), (x + 1, map.h), v3(0.0, -1.0, 0.0));
                    }
                }
                for y in cy..y1 {
                    if cx == 0 {
                        skirt((0, y), (0, y + 1), v3(-1.0, 0.0, 0.0));
                    }
                    if x1 == map.w {
                        skirt((map.w, y), (map.w, y + 1), v3(1.0, 0.0, 0.0));
                    }
                }
                let vbuf = ctx.buffer("terrain", wgpu::BufferUsages::VERTEX, bytes(&verts));
                let ibuf = ctx.buffer("terrain", wgpu::BufferUsages::INDEX, bytes(&idx));
                self.chunks.push(Chunk { vbuf, ibuf, n: idx.len() as u32 });
            }
        }

        // Sea: a grid so the shoreline fringe can fade.
        let (mw, mh) = (map.w as f32, map.h as f32);
        let ext = 220.0;
        let xs = [-ext, -6.0, 0.0, mw, mw + 6.0, mw + ext];
        let ys = [-ext, -6.0, 0.0, mh, mh + 6.0, mh + ext];
        let mut wv = Vec::new();
        let mut wi: Vec<u16> = Vec::new();
        for &y in &ys {
            for &x in &xs {
                let near = x >= 0.0 && x <= mw && y >= 0.0 && y <= mh;
                wv.push(TerrainVert { pos: [x, -y, -0.45], nrm: [0.0, 0.0, 1.0], attr: [if near { 255 } else { 0 }, 0, 0, 0] });
            }
        }
        for y in 0..5u16 {
            for x in 0..5u16 {
                let i = y * 6 + x;
                wi.extend_from_slice(&[i, i + 7, i + 1, i, i + 6, i + 7]);
            }
        }
        self.water = Some(Chunk {
            vbuf: ctx.buffer("sea", wgpu::BufferUsages::VERTEX, bytes(&wv)),
            ibuf: ctx.buffer("sea", wgpu::BufferUsages::INDEX, bytes(&wi)),
            n: wi.len() as u32,
        });

        self.build_statics(map);

        for t in [self.tint_tex.take(), self.ground_tex.take()].into_iter().flatten() {
            ctx.delete_texture(t);
        }
        self.tint_tex = Some(ctx.new_blank(map.w as u32, map.h as u32));
        self.tint_seen = u32::MAX;
        self.ground_tex = Some(ground_texture(ctx, map));
        self.world_bind = None;
        self.terrain_seen = 0;
        self.flakes.clear();
        self.map = Some(map.clone());
        self.target = v2(map.w as f32 / 2.0, map.h as f32 / 2.0);
        self.update_camera();
    }

    /// Trees, stumps and ice pinnacles.
    fn build_statics(&mut self, map: &Map) {
        self.statics.clear();
        for y in 0..map.h {
            for x in 0..map.w {
                let o = map.obstacle_at(x, y);
                if o == Obstacle::None {
                    continue;
                }
                // Seeded per tile so a tree's neighbours stay put when it falls.
                let mut rng = Rng::new(map.idx(x, y) as u64 * 2654435761 + 1234);
                let p = v2(x as f32 + 0.5 + rng.range(-0.15, 0.15), y as f32 + 0.5 + rng.range(-0.15, 0.15));
                let z = map.height_at(p) - 0.05;
                let (m, scale) = match o {
                    Obstacle::Tree => (M::Tree(rng.below(TREE_VARIANTS as u32) as u8), rng.range(0.85, 1.05)),
                    Obstacle::Stump => (M::Stump(rng.below(STUMP_VARIANTS as u32) as u8), rng.range(0.9, 1.2)),
                    _ => (M::Pinnacle(rng.below(3) as u8), rng.range(0.8, 1.2)),
                };
                self.statics.push((m, Inst { tilt: [0.0; 4],
                    pos_yaw: [p.x, -p.y, z, rng.range(0.0, TAU)],
                    misc: [scale, 0.0, 1.0, 0.0],
                    color: [255, 255, 255, 0],
                }));
            }
        }
    }

    /// Follow trees being chopped and snow wearing away.
    fn update_terrain(&mut self, ctx: &mut Gpu, scene: &Scene) {
        if self.terrain_seen == scene.terrain_version {
            return;
        }
        self.terrain_seen = scene.terrain_version;
        self.build_statics(scene.map);
        if let Some(tex) = self.ground_tex {
            ctx.texture_update(tex, &ground_data(scene.map, scene.snow));
        }
        if let Some(m) = &mut self.map {
            m.ground.clone_from(&scene.map.ground);
            m.obstacle.clone_from(&scene.map.obstacle);
        }
    }

    fn update_tint_tex(&mut self, ctx: &mut Gpu, scene: &Scene) {
        if self.tint_seen == scene.tint_version {
            return;
        }
        self.tint_seen = scene.tint_version;
        let (Some(map), Some(tex)) = (&self.map, self.tint_tex) else { return };
        let data = tint_texture(map, scene.fog, scene.night_ring);
        ctx.texture_update(tex, &data);
    }

    fn pitch(&self) -> f32 {
        let t = ((self.dist - 8.0) / 60.0).clamp(0.0, 1.0);
        0.78 + t * 0.32
    }

    fn update_camera(&mut self) {
        let z = self.map.as_ref().map_or(0.0, |m| m.height_at(self.target));
        let target = gl(self.target, z);
        let pitch = self.pitch();
        // Camera sits "south" of the target (towards +map_y) by default.
        let back = v3(self.yaw.sin(), -self.yaw.cos(), 0.0);
        self.eye = target + back * (self.dist * pitch.cos()) + v3(0.0, 0.0, self.dist * pitch.sin());
        let view = Mat4::look_at(self.eye, target, v3(0.0, 0.0, 1.0));
        let proj = Mat4::perspective(FOV, self.vw / self.vh.max(1.0), 0.5, 400.0);
        self.proj = proj;
        self.view_proj = proj * view;
        self.inv_view_proj = self.view_proj.inverse();
    }

    fn light_matrix(&self) -> Mat4 {
        let z = self.map.as_ref().map_or(0.0, |m| m.height_at(self.target));
        let r = (self.dist * 0.95 + 8.0).min(90.0);
        // Snap to shadow texels so edges don't crawl while panning.
        let texel = 2.0 * r / SHADOW_SIZE as f32;
        let sun = SUN.norm();
        let right = sun.cross(v3(0.0, 0.0, 1.0)).norm();
        let up = right.cross(sun);
        let mut c = gl(self.target, z);
        let (a, b) = (c.dot(right), c.dot(up));
        c = c + right * ((a / texel).round() * texel - a) + up * ((b / texel).round() * texel - b);
        let view = Mat4::look_at(c + sun * 80.0, c, v3(0.0, 0.0, 1.0));
        Mat4::ortho(-r, r, -r, r, 1.0, 180.0) * view
    }

    fn visible(&self, p: Vec3, margin: f32) -> bool {
        let c = self.view_proj.transform(p, 1.0);
        if c[3] <= 0.01 {
            return false;
        }
        let m = 1.0 + margin / c[3].max(0.1) * 3.0;
        (c[0] / c[3]).abs() < m && (c[1] / c[3]).abs() < m
    }

    fn uniforms(&self, viewproj: Mat4, light: Mat4, mode: f32, time: f32) -> WorldUniforms {
        let (w, h) = self.map.as_ref().map_or((1.0, 1.0), |m| (m.w as f32, m.h as f32));
        let s = SUN.norm();
        WorldUniforms {
            viewproj: CLIP_FIX * viewproj,
            light: CLIP_FIX * light,
            eye: [self.eye.x, self.eye.y, self.eye.z, time],
            sun: [s.x, s.y, s.z, if self.shadows && mode < 2.5 { 1.0 } else { 0.0 }],
            // Blizzards pull the fog in close and turn it white.
            fog: [(self.dist * 1.6 + 20.0) * (1.0 - 0.3 * self.storm), 140.0 * (1.0 - 0.35 * self.storm), 1.0 / SHADOW_SIZE as f32, mode],
            fog_color: {
                let night = [0.12, 0.15, 0.28];
                let day = [0.72, 0.8, 0.9];
                let storm = [0.86, 0.9, 0.95];
                let c: Vec<f32> = (0..3).map(|i| day[i] + (night[i] - day[i]) * self.night).collect();
                let c: Vec<f32> = (0..3).map(|i| c[i] + (storm[i] - c[i]) * self.storm * (1.0 - self.night * 0.5)).collect();
                [c[0], c[1], c[2], 1.0]
            },
            map: [w, h, if mode < 0.5 { self.night } else { 0.0 }, if mode < 0.5 { self.storm } else { 0.0 }],
            tint: [1.0, if mode < 0.5 { self.lights.2 as f32 } else { 0.0 }, 0.0, 0.0],
            lights: self.lights.0,
            light_cols: self.lights.1,
        }
    }

    /// The fires, windows and lanterns lighting this frame, nearest the
    /// camera first. They glow faintly by day and come into their own at
    /// night and in storms; flames flicker.
    fn gather_lights(&mut self, scene: &Scene) {
        let fire = [1.0, 0.42, 0.12];
        let window = [1.0, 0.5, 0.2];
        let lamp = [1.0, 0.58, 0.28];
        // (where, height, reach, colour, brightness, flickers)
        let mut found: Vec<(Vec2, f32, f32, [f32; 3], f32, bool)> = Vec::new();
        for ig in &scene.igloos {
            if ig.color.is_some() {
                // The door, a step out: the glow spills from the tunnel.
                let out = (ig.door - ig.center).norm();
                found.push((ig.door + out * 0.6, ig.z + 0.7, 6.5, window, 1.2, false));
            }
        }
        for b in scene.buildings.iter().filter(|b| b.ghost.is_none() && b.progress >= 1.0) {
            let (h, reach, col, bright, flick) = match b.kind {
                BuildKind::Campfire => (0.6, 9.0, fire, 2.6, true),
                BuildKind::Sauna => (0.5, 6.0, fire, 1.8, true),
                BuildKind::Spa => (0.6, 5.0, window, 1.2, false),
                BuildKind::Tower | BuildKind::Catapult | BuildKind::Store | BuildKind::Bunkhouse | BuildKind::Workshop | BuildKind::SkiLodge => (1.4, 4.5, lamp, 1.0, false),
                _ => continue,
            };
            found.push((b.center, b.z + h, reach, col, bright, flick));
        }
        let at = self.target;
        found.sort_by(|a, b| a.0.dist(at).partial_cmp(&b.0.dist(at)).unwrap());
        let strength = (0.18 + 0.82 * self.night.max(self.storm * 0.5)) * 1.0;
        let t = scene.time;
        let (mut pos, mut col) = ([[0.0; 4]; MAX_LIGHTS], [[0.0; 4]; MAX_LIGHTS]);
        let n = found.len().min(MAX_LIGHTS);
        for (k, &(p, h, reach, c, bright, flick)) in found.iter().take(n).enumerate() {
            let w = gl(p, h);
            let seed = p.x * 1.7 + p.y * 3.1;
            let f = if flick { 1.0 + 0.14 * (t * 9.0 + seed).sin() + 0.08 * (t * 23.0 + seed * 3.0).sin() } else { 1.0 };
            pos[k] = [w.x, w.y, w.z, reach * (0.97 + 0.03 * f)];
            col[k] = [c[0] * bright * strength * f, c[1] * bright * strength * f, c[2] * bright * strength * f, 0.0];
        }
        self.lights = (pos, col, n);
    }

    /// Penguin instances for this frame grouped by mesh + frame pair.
    #[allow(clippy::type_complexity)]
    fn collect(&mut self, scene: &Scene) -> (Vec<(Key, Vec<Inst>)>, Vec<Inst>, Vec<(Key, Vec<Inst>)>) {
        let mut ghosts: HashMap<Key, Vec<Inst>> = HashMap::new();
        for b in &scene.buildings {
            let Some(ok) = b.ghost else { continue };
            for &yaw in &b.yaws {
                let c = if ok { b.color } else { [255, 60, 50] };
                ghosts.entry((building_model(b.kind, b.variant), 0, 0, 0, 0)).or_default().push(Inst { tilt: [0.0; 4],
                    pos_yaw: [b.center.x, -b.center.y, b.z, yaw],
                    misc: [1.0, 0.0, 0.5, 1.0],
                    color: [c[0], c[1], c[2], if ok { 0 } else { 200 }],
                });
            }
        }
        let now = scene.time;
        let dt = (now - self.last_time).clamp(0.0, 0.1);
        self.last_time = now;
        // Forget penguins not seen for a while.
        self.gaits.retain(|_, g| now - g.seen < 5.0);
        let mut groups: HashMap<Key, Vec<Inst>> = HashMap::new();
        let mut ice = Vec::new();
        let mut add = |key: Key, inst: Inst| groups.entry(key).or_default().push(inst);

        for p in &scene.penguins {
            let pos = gl(p.pos, p.z);
            if !self.visible(pos, 1.0) {
                continue;
            }
            let own = M::Unit(p.kind as u8);
            let (m, model) = match p.kind {
                _ if self.models.contains_key(&own) => (own, &self.models[&own]),
                Kind::Worker => (M::Worker, &self.models[&M::Worker]),
                _ => (M::Warrior, &self.models[&M::Warrior]),
            };
            let scale = p.kind.stats().scale;
            let mut anim = p.anim;
            if matches!(p.carry, Carry::Crystal | Carry::Wood) && anim == Anim::Carry {
                anim = Anim::Walk;
            }
            let ai = model.anim_index(anim.name());
            let frames = model.anims[ai].frames.len();
            let (fps, looping) = anim.timing();
            let base_speed = p.kind.stats().speed;
            let g = self.gaits.entry(p.id).or_insert(Gait {
                ai,
                frame: 0,
                from: None,
                fade: 1.0,
                pos: p.pos,
                facing: p.facing,
                speed: 0.0,
                turn: 0.0,
                stride: p.id as f32 * 1.7,
                seen: now,
            });
            // How fast it is really moving and turning, smoothed.
            if p.pos.dist(g.pos) > 2.0 || now - g.seen > 0.5 {
                g.pos = p.pos;
                g.facing = p.facing;
            }
            if dt > 0.0 {
                let k = 1.0 - (-dt * 10.0).exp();
                g.speed += (p.pos.dist(g.pos) / dt - g.speed) * k;
                g.turn += (sim::math::angle_diff(g.facing, p.facing) / dt - g.turn) * k;
            }
            g.pos = p.pos;
            g.facing = p.facing;
            g.seen = now;
            let pace = (g.speed / base_speed).clamp(0.0, 2.0);
            // Walking feet keep pace with the ground instead of a fixed beat.
            let walking = matches!(anim, Anim::Walk | Anim::Carry);
            if walking {
                g.stride += dt * fps * pace.max(0.45);
            }
            let f = if p.frozen {
                0.0
            } else if walking {
                g.stride % frames as f32
            } else {
                anim.frame(p.anim_t, frames)
            };
            // A new clip blends in from wherever the last one was.
            if ai != g.ai {
                g.from = (model.anims[g.ai].n_verts == model.anims[ai].n_verts).then_some((g.ai, g.frame));
                g.fade = 0.0;
                g.ai = ai;
            }
            g.fade = (g.fade + dt / CROSSFADE).min(1.0);
            let f0 = f.floor() as usize % frames;
            g.frame = f.round() as usize % frames;
            let f1 = if looping { (f0 + 1) % frames } else { (f0 + 1).min(frames - 1) };
            // Body language: lean into the run, roll into turns, waddle from
            // foot to foot, breathe, and squash when a snowball lands.
            let mut tilt = [0.0f32; 4];
            if !p.frozen {
                let cycle = (g.stride / frames as f32) * TAU;
                let moving = if walking { pace.min(1.3) } else { 0.0 };
                tilt[0] = 0.09 * moving;
                tilt[1] = (-g.turn * pace * 0.04).clamp(-0.15, 0.15) + cycle.sin() * 0.06 * moving;
                tilt[2] = (now * 2.1 + p.id as f32).sin() * 0.012 * (1.0 - moving.min(1.0)) + p.flash * 0.14;
            }
            let (key, blend) = match g.from {
                Some((fa, ff)) if g.fade < 1.0 && !p.frozen => ((m, fa, ff.min(model.anims[fa].frames.len() - 1), ai, g.frame), g.fade * g.fade * (3.0 - 2.0 * g.fade)),
                _ => ((m, ai, f0, ai, f1), f.fract()),
            };
            let inst = Inst {
                pos_yaw: [pos.x, pos.y, pos.z, yaw_for(p.facing)],
                misc: [scale, blend, 1.0, if p.flash > 0.0 { 1.0 + p.flash } else if p.selected { 0.9 } else { 0.0 }],
                color: [p.color[0], p.color[1], p.color[2], (p.cold * 255.0) as u8],
                tilt,
            };

            // Packers only hold a snowball up while they have one.
            let prop = M::Prop(p.kind as u8);
            // (A kind's own model carries its gear already.)
            if m != own && self.models.contains_key(&prop) && !(p.kind == Kind::Packer && p.ammo == 0) {
                add((prop, 0, 0, 0, 0), Inst { misc: [scale, 0.0, 1.0, inst.misc[3]], ..inst });
            }
            add(key, inst);
            if p.frozen {
                ice.push(Inst { tilt: [0.0; 4], pos_yaw: [pos.x, pos.y, pos.z, yaw_for(p.facing)], misc: [1.0, 0.0, 0.55, 0.0], color: [255, 255, 255, 0] });
            }
            let above = |h: f32| [pos.x, pos.y, pos.z + h, scene.time * 2.0];
            if p.carry == Carry::Crystal {
                let fwd = Vec2::from_angle(p.facing) * 0.3;
                add((M::Crystal(2), 0, 0, 0, 0), Inst { tilt: [0.0; 4],
                    pos_yaw: [pos.x + fwd.x, pos.y - fwd.y, pos.z + 0.95, 0.0],
                    misc: [0.45, 0.0, 1.0, 0.0],
                    color: [255, 255, 255, 0],
                });
            }
            if p.carry == Carry::Wood {
                // Held across the chest.
                let fwd = Vec2::from_angle(p.facing) * 0.22;
                add((M::Log, 0, 0, 0, 0), Inst { tilt: [0.0; 4],
                    pos_yaw: [pos.x + fwd.x, pos.y - fwd.y, pos.z + 0.55, yaw_for(p.facing)],
                    misc: [1.0, 0.0, 1.0, 0.0],
                    color: [255, 255, 255, 0],
                });
            }
            if let Some(pw) = p.power {
                add((M::Power(pw as u8), 0, 0, 0, 0), Inst { tilt: [0.0; 4], pos_yaw: above(1.25), misc: [0.35, 0.0, 1.0, 0.0], color: [255, 255, 255, 0] });
            }
        }
        for ig in &scene.igloos {
            // The flag fades to blue-gray as the igloo takes damage.
            let base = ig.color.unwrap_or([235, 235, 235]);
            let k = (1.0 - ig.hp) * 0.85;
            let gray = [140, 148, 160];
            let c = [0, 1, 2].map(|i| (base[i] as f32 * (1.0 - k) + gray[i] as f32 * k) as u8);
            let m = if ig.hp <= 0.0 { M::Ruin } else { M::Igloo(ig.tier) };
            let dir = ig.door - ig.center;
            add((m, 0, 0, 0, 0), Inst { tilt: [0.0; 4],
                pos_yaw: [ig.center.x, -ig.center.y, ig.z, yaw_for(dir.angle())],
                misc: [1.0, 0.0, 1.0, if ig.selected { 0.8 } else { 0.0 }],
                color: [c[0], c[1], c[2], 0],
            });
        }
        for r in &scene.resources {
            let m = match r.kind {
                ResourceKind::Fish => {
                    if r.amount > 0 {
                        M::HoleOpen
                    } else {
                        M::HoleFrozen
                    }
                }
                ResourceKind::Crystal => {
                    let f = r.amount as f32 / r.max.max(1) as f32;
                    M::Crystal(if r.amount == 0 { 0 } else { 1 + (f * 2.99) as u8 })
                }
            };
            let yaw = (r.pos.x * 7.3 + r.pos.y * 3.1) % TAU;
            add((m, 0, 0, 0, 0), Inst { tilt: [0.0; 4], pos_yaw: [r.pos.x, -r.pos.y, r.z, yaw], misc: [1.0, 0.0, 1.0, 0.0], color: [255, 255, 255, 0] });
        }
        for &(pos, z, kind) in &scene.powerups {
            let bob = (scene.time * 2.5 + pos.x).sin() * 0.08 + 0.05;
            add((M::Power(kind as u8), 0, 0, 0, 0), Inst { tilt: [0.0; 4],
                pos_yaw: [pos.x, -pos.y, z + bob, scene.time * 1.5],
                misc: [1.0, 0.0, 1.0, 0.0],
                color: [255, 255, 255, 0],
            });
        }
        // Surfaced seals rise out of the ice over the first half second.
        for s in &scene.seals {
            let rise = (s.up / 0.5).clamp(0.0, 1.0);
            add((M::Seal, 0, 0, 0, 0), Inst { tilt: [0.0; 4],
                pos_yaw: [s.pos.x, -s.pos.y, s.z, yaw_for(s.facing)],
                misc: [0.3 + 0.7 * rise, 0.0, 1.0, 0.0],
                color: [255, 255, 255, 0],
            });
        }
        for &(pos, z, size) in &scene.snowballs {
            add((M::Snowball, 0, 0, 0, 0), Inst { tilt: [0.0; 4], pos_yaw: [pos.x, -pos.y, z, 0.0], misc: [size, 0.0, 1.0, 0.0], color: [255, 255, 255, 0] });
        }
        for (m, inst) in &self.statics {
            let p = v3(inst.pos_yaw[0], inst.pos_yaw[1], inst.pos_yaw[2]);
            if self.visible(p, 3.0) {
                add((*m, 0, 0, 0, 0), *inst);
            }
        }
        for b in scene.buildings.iter().filter(|b| b.ghost.is_none()) {
            if !self.visible(v3(b.center.x, -b.center.y, b.z), 3.0) {
                continue;
            }
            // A site shows its stage of construction, in scaffolding, when
            // the model has stages; otherwise it rises out of the snow.
            let m = building_model(b.kind, b.variant);
            let stage = (b.progress < 1.0)
                .then(|| self.models.get(&m).and_then(|model| model.find_anim(&format!("stage-{}", 1 + (b.progress * 3.0) as usize))))
                .flatten();
            let (ai, scale, ghost) = match stage {
                Some(ai) => (ai, 1.0, 0),
                None if b.progress < 1.0 => (0, 0.35 + 0.65 * b.progress, 90),
                None => (0, 1.0, 0),
            };
            for &yaw in &b.yaws {
                add((m, ai, 0, ai, 0), Inst { tilt: [0.0; 4],
                    pos_yaw: [b.center.x, -b.center.y, b.z - 0.03, yaw],
                    misc: [scale, 0.0, 1.0, if b.selected { 0.8 } else { 0.0 }],
                    color: [b.color[0], b.color[1], b.color[2], ghost],
                });
            }
        }
        (groups.into_iter().collect(), ice, ghosts.into_iter().collect())
    }

    /// Draw instance groups whose instances start at `base` in the
    /// instance buffer.
    fn draw_groups<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, groups: &[InstGroup], mut base: u64) {
        let stride = std::mem::size_of::<Inst>() as u64;
        for ((m, ai, f0, aj, f1), insts) in groups {
            let mesh = &self.models[m].anims[*ai];
            let n = insts.len() as u64;
            // Ruins are unowned: no flag, drawn about 0.85x darker.
            let slot = if *m == M::Ruin { SLOT_RUIN } else { SLOT_MAIN };
            pass.set_bind_group(0, &self.ubind, &[slot * SLOT_STRIDE as u32]);
            pass.set_vertex_buffer(0, mesh.colors.slice(..));
            pass.set_vertex_buffer(1, mesh.frames[*f0].slice(..));
            pass.set_vertex_buffer(2, self.models[m].anims[*aj].frames[*f1].slice(..));
            pass.set_vertex_buffer(3, self.inst_buf.slice(base * stride..(base + n) * stride));
            pass.set_index_buffer(mesh.index.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..mesh.n_indices, 0, 0..n as u32);
            base += n;
        }
    }

    fn draw_terrain<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        for c in &self.chunks {
            pass.set_vertex_buffer(0, c.vbuf.slice(..));
            pass.set_index_buffer(c.ibuf.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..c.n, 0, 0..1);
        }
    }

    /// The world textures' bind group, remade when they change.
    fn world_bind(&mut self, ctx: &Gpu) -> Option<()> {
        let key = (self.tint_tex?, self.ground_tex?, self.shadows);
        if self.world_bind.as_ref().is_some_and(|(k, _)| *k == key) {
            return Some(());
        }
        let (tint, ground) = (ctx.tex(key.0)?, ctx.tex(key.1)?);
        let bind = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world textures"),
            layout: &self.world_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(if self.shadows { &self.shadow_map } else { &self.no_shadow }) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.shadow_samp) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&tint.view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&ground.view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&ctx.linear_clamp) },
            ],
        });
        self.world_bind = Some((key, bind));
        Some(())
    }

    fn fx_geometry(&mut self, scene: &Scene) -> (Vec<FxVert>, Vec<u16>) {
        let map = self.map.as_ref().unwrap();
        let mut v: Vec<FxVert> = Vec::new();
        let mut idx: Vec<u16> = Vec::new();
        // Camera-facing basis for billboards.
        let fwd = (gl(self.target, 0.0) - self.eye).norm();
        let right = fwd.cross(v3(0.0, 0.0, 1.0)).norm();
        let up = right.cross(fwd);
        let mut quad = |v: &mut Vec<FxVert>, c: Vec3, ax: Vec3, ay: Vec3, color: [u8; 4], kind: u8| {
            if v.len() + 4 > 65000 {
                return;
            }
            let b = v.len() as u16;
            let k = [kind, 0, 0, 0];
            v.push(FxVert { pos: (c - ax - ay).as_arr(), uv: [0.0, 0.0], color, kind: k });
            v.push(FxVert { pos: (c + ax - ay).as_arr(), uv: [1.0, 0.0], color, kind: k });
            v.push(FxVert { pos: (c + ax + ay).as_arr(), uv: [1.0, 1.0], color, kind: k });
            v.push(FxVert { pos: (c - ax + ay).as_arr(), uv: [0.0, 1.0], color, kind: k });
            idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        };
        let ground = |p: Vec2, lift: f32| gl(p, map.height_at(p) + lift);
        // Decals hug the ground.
        for d in &scene.effects.decals {
            let fade = (d.life / d.max).min(1.0);
            let (kind, color) = match d.kind {
                DecalKind::Footprint => (FX_FOOT, [110, 130, 170, (fade * 70.0) as u8]),
                DecalKind::Splat => (FX_SPLAT, [255, 255, 255, (fade.sqrt() * 230.0) as u8]),
            };
            let a = d.angle;
            let ax = v3(a.cos(), -a.sin(), 0.0) * d.size;
            let ay = v3(-a.sin(), -a.cos(), 0.0) * d.size;
            quad(&mut v, ground(d.pos, 0.03), ay, ax, color, kind);
        }
        let flat = |s: f32| (v3(s, 0.0, 0.0), v3(0.0, s, 0.0));
        // Blob shadows under penguins and snowballs (cheap contact shadows).
        for p in &scene.penguins {
            let (ax, ay) = flat(0.38);
            quad(&mut v, ground(p.pos, 0.025), ax, ay, [20, 40, 80, 70], FX_BLOB);
            // After dark the scarves are hard to read, so every tribe's
            // penguins stand in a ring of their own colour.
            if scene.night > 0.15 && !p.wild && !p.frozen {
                let (ax, ay) = flat(0.42);
                let al = (scene.night.min(1.0) * 190.0) as u8;
                quad(&mut v, ground(p.pos, 0.028), ax, ay, [p.color[0], p.color[1], p.color[2], al], FX_RING);
            }
            // Ownerless penguins are a neutral grey; a faint pale ring marks
            // them as recruitable.
            let wild = p.wild;
            if wild && !p.frozen {
                let (ax, ay) = flat(0.5);
                quad(&mut v, ground(p.pos, 0.03), ax, ay, [210, 210, 220, 90], FX_RING);
            }
            if p.selected {
                let (ax, ay) = flat(0.48);
                // Ring colour says friend or foe: green a friend, red an
                // enemy, white a wild one; a frozen penguin is pale blue
                // whatever its tribe.
                let c = if p.frozen {
                    [150, 205, 255, 230]
                } else if wild {
                    [255, 255, 255, 230]
                } else if p.mine {
                    [120, 255, 140, 230]
                } else {
                    [255, 120, 100, 230]
                };
                quad(&mut v, ground(p.pos, 0.04), ax, ay, c, FX_RING);
            }
            // Penguins under a War Cry glow with it.
            if p.shouting {
                let (ax, ay) = flat(0.62);
                quad(&mut v, ground(p.pos, 0.045), ax, ay, [255, 225, 140, 150], FX_RING);
            }
        }
        // A Warmer's breath: a wavy stream of faint warm air from the
        // Warmer to the penguin it is tending.
        for p in &scene.penguins {
            let (Some(t_id), Kind::Warmer) = (p.tend, p.kind) else {
                continue;
            };
            let Some(t) = scene.penguins.iter().find(|q| q.id == t_id) else {
                continue;
            };
            if t.id == p.id {
                continue;
            }
            let a = ground(p.pos, 0.55);
            let b = ground(t.pos, 0.45);
            let dir = b - a;
            let len = dir.len().max(0.001);
            let perp = v3(dir.y, -dir.x, 0.0) * (1.0 / len);
            const N: u32 = 12;
            for i in 0..N {
                let f = (i as f32 + 0.5) / N as f32;
                let wave = (f * 2.0 * std::f32::consts::TAU - scene.time * 9.0).sin();
                // Tapered: the stream pinches in at both birds.
                let amp = 0.12 * 4.0 * f * (1.0 - f);
                let c = a.lerp(b, f) + perp * (wave * amp);
                let s = 0.09 + 0.05 * f;
                let al = (85.0 + 45.0 * f) as u8;
                quad(&mut v, c, right * s, up * s, [255, 180, 100, al], FX_DISC);
            }
        }
        // War Crys and Chieftains falling: rings racing over the snow.
        for r in &scene.effects.rings {
            let (ax, ay) = flat(r.radius);
            let a = (r.life / r.max).clamp(0.0, 1.0);
            let c = [r.color[0], r.color[1], r.color[2], (r.color[3] as f32 * a) as u8];
            quad(&mut v, ground(r.pos, 0.07), ax, ay, c, FX_RING);
        }
        for &(pos, z, _) in &scene.snowballs {
            let h = z - map.height_at(pos);
            let s = 0.12 + h * 0.02;
            let (ax, ay) = flat(s);
            quad(&mut v, ground(pos, 0.03), ax, ay, [20, 40, 80, (90.0 / (1.0 + h * 0.4)) as u8], FX_BLOB);
        }
        for ig in &scene.igloos {
            // Likewise a tribe's igloo glows in its colour at night.
            if let (Some(c), true) = (ig.color, scene.night > 0.15) {
                let (ax, ay) = flat(2.3);
                let al = (scene.night.min(1.0) * 170.0) as u8;
                quad(&mut v, ground(ig.center, 0.04), ax, ay, [c[0], c[1], c[2], al], FX_RING);
            }
            if ig.selected {
                let (ax, ay) = flat(2.6);
                quad(&mut v, ground(ig.center, 0.05), ax, ay, [255, 220, 120, 200], FX_RING);
            }
            if let Some((c, prog)) = ig.takeover {
                let pulse = 0.5 + 0.5 * (scene.time * 6.0).sin();
                let (ax, ay) = flat(2.9 + pulse * 0.2);
                quad(&mut v, ground(ig.center, 0.06), ax, ay, [c[0], c[1], c[2], (120.0 + 120.0 * prog) as u8], FX_RING);
            }
            // A damaged igloo shows a health bar: green fill over a dark
            // red strip, about twice a penguin's warmth bar wide.
            if ig.hp > 0.0 && ig.hp < 1.0 {
                let w = 2.0;
                let c = ground(ig.center, 2.1);
                quad(&mut v, c, right * (w / 2.0), up * 0.14, [96, 28, 28, 240], FX_BAR);
                quad(&mut v, c - right * (w * (1.0 - ig.hp) / 2.0), right * (w * ig.hp / 2.0), up * 0.14, [110, 220, 120, 245], FX_BAR);
            }
        }
        for mk in &scene.markers {
            let t = mk.age / 0.8;
            let s = 0.9 - t * 0.5;
            let c = match mk.kind {
                MarkerKind::Move => [120, 255, 140],
                MarkerKind::Run => [255, 220, 90],
                MarkerKind::Enter => [255, 120, 100],
                MarkerKind::Gather => [120, 200, 255],
            };
            let (ax, ay) = flat(s);
            quad(&mut v, ground(mk.pos, 0.06), ax, ay, [c[0], c[1], c[2], ((1.0 - t) * 255.0) as u8], FX_RING);
        }
        // Particles and falling snow are billboards.
        for p in &scene.effects.particles {
            let a = (p.life / p.max).min(1.0);
            let c = [p.color[0], p.color[1], p.color[2], (p.color[3] as f32 * a) as u8];
            quad(&mut v, gl(v2(p.pos.x, p.pos.y), p.pos.z), right * p.size, up * p.size, c, FX_DISC);
        }
        for f in &self.flakes {
            quad(&mut v, f.pos, right * 0.05, up * 0.05, [255, 255, 255, 200], FX_DISC);
        }
        (v, idx)
    }

    fn update_snow(&mut self, dt: f32, time: f32) {
        let Some(map) = &self.map else { return };
        let c = self.target;
        let r = (self.dist * 0.9).clamp(12.0, 45.0);
        let want = (300.0 * self.snowfall) as usize;
        self.flakes.truncate(want);
        let wind = 0.3 + self.storm * 2.5;
        while self.flakes.len() < want {
            let p = c + v2(self.rng.range(-r, r), self.rng.range(-r, r));
            let z = self.rng.range(0.0, 16.0);
            self.flakes.push(Flake { pos: gl(p, z), phase: self.rng.range(0.0, TAU) });
        }
        for f in &mut self.flakes {
            f.pos.z -= dt * (1.1 + self.storm * 0.8);
            f.pos.x += (time * 0.9 + f.phase).sin() * dt * 0.4 + dt * wind;
            let mp = v2(f.pos.x, -f.pos.y);
            let ground = map.height_at(mp);
            if f.pos.z < ground || (mp - c).len() > r * 1.5 {
                let p = c + v2(self.rng.range(-r, r), self.rng.range(-r, r));
                f.pos = gl(p, map.height_at(p) + self.rng.range(10.0, 16.0));
            }
        }
    }
}

trait AsArr {
    fn as_arr(&self) -> [f32; 3];
}
impl AsArr for Vec3 {
    fn as_arr(&self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }
}

/// Map-sized texture with R = 255 on ice tiles, linearly filtered so
/// shaders can cut smooth ice outlines out of it.
/// Per-tile ground: R = bare ice, G = how trodden-down the snow is, B = open
/// water, A = something standing there (trees, ice pinnacles) that snow drifts against.
pub fn ground_data(map: &Map, snow: &[u8]) -> Vec<u8> {
    let mut data = vec![0u8; (map.w * map.h * 4) as usize];
    for y in 0..map.h {
        for x in 0..map.w {
            let i = map.idx(x, y);
            if matches!(map.obstacle[i], Obstacle::Tree | Obstacle::Pinnacle) {
                data[i * 4 + 3] = 255;
            }
            if map.ground[i] == Ground::Water {
                data[i * 4 + 2] = 255;
            } else if map.ground[i] == Ground::Ice {
                data[i * 4] = 255;
            } else if let Some(&s) = snow.get(i) {
                let depth = sim::game::SNOW_DEPTH;
                data[i * 4 + 1] = (depth.saturating_sub(s) as u32 * 255 / depth as u32) as u8;
            }
        }
    }
    data
}

pub fn ground_texture(ctx: &mut Gpu, map: &Map) -> TextureId {
    let rgba = ground_data(map, &[]);
    ctx.new_texture(&gfx::Image { w: map.w as u32, h: map.h as u32, rgba }, false, false)
}

/// Per-tile tint: the fog of war darkens what is not in sight (`fog`
/// holds 0 seen now, 1 seen before, 2 never seen, per tile).
/// Per-tile darkening: fog of war, and the deep blue of the Long Night
/// outside the closing ring.
pub fn tint_texture(map: &Map, fog: Option<&[u8]>, ring: Option<(Vec2, f32)>) -> Vec<u8> {
    let mut data = vec![0u8; (map.w * map.h * 4) as usize];
    for y in 0..map.h {
        for x in 0..map.w {
            let i = ((y * map.w + x) * 4) as usize;
            match fog.map_or(0, |f| f[(y * map.w + x) as usize]) {
                0 => {}
                // Never seen: solid black.
                crate::view::FOG_UNKNOWN => {
                    data[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
                    continue;
                }
                // Seen before: red says how far it has been redrawn as the old
                // map, blue marks it as remembered (see the shaders).
                v => {
                    let age = (v - crate::view::FOG_JUST_LOST) as f32 - crate::view::FOG_DARK as f32;
                    let paper = (age / crate::view::FOG_REDRAW as f32).clamp(0.0, 1.0);
                    data[i..i + 4].copy_from_slice(&[(paper * 255.0) as u8, 0, 255, 230]);
                    continue;
                }
            }
            if let Some((mid, r)) = ring {
                if v2(x as f32 + 0.5, y as f32 + 0.5).dist(mid) > r {
                    data[i..i + 4].copy_from_slice(&[10, 22, 80, 150]);
                }
            }
        }
    }
    data
}

impl WorldRenderer for Render3D {

    fn set_viewport(&mut self, w: f32, h: f32) {
        self.vw = w;
        self.vh = h;
        self.update_camera();
    }

    fn draw(&mut self, ctx: &mut Gpu, scene: &Scene, batch: &mut Batch) {
        let _ = batch;
        let Some((fw, fh)) = ctx.frame.as_ref().map(|f| (f.w, f.h)) else { return };
        if self.map.is_none() {
            return;
        }
        self.update_camera();
        // Screen shake: nudge the camera target for this frame only.
        let aim = self.target;
        if scene.shake != [0.0, 0.0] {
            let tiles = scene.shake.map(|f| f * self.vh / self.pixels_per_tile(aim).max(1.0));
            self.target = aim + v2(tiles[0], tiles[1]);
            self.update_camera();
        }
        self.update_tint_tex(ctx, scene);
        self.update_terrain(ctx, scene);
        self.night = scene.night;
        self.storm = scene.storm;
        self.snowfall = scene.snowfall;
        self.gather_lights(scene);
        self.update_snow(scene.dt, scene.time);
        self.post.ensure(ctx, fw, fh);
        if self.world_bind(ctx).is_none() {
            self.target = aim;
            return;
        }
        let (groups, ice, ghosts) = self.collect(scene);
        let ice_group: Vec<InstGroup> = if ice.is_empty() { Vec::new() } else { vec![((M::Ice, 0, 0, 0, 0), ice)] };

        // Every instance this frame, in one buffer: models, ghosts, ice.
        let all: Vec<Inst> = groups.iter().chain(&ghosts).chain(&ice_group).flat_map(|(_, v)| v.iter().copied()).collect();
        let ib = bytes(&all);
        if ib.len() as u64 > self.inst_buf.size() {
            self.inst_buf = ctx.buffer("instances", wgpu::BufferUsages::VERTEX, &vec![0u8; ib.len().next_power_of_two()]);
        }
        ctx.write(&self.inst_buf, 0, ib);
        let n_groups: u64 = groups.iter().map(|(_, v)| v.len() as u64).sum();
        let n_ghosts: u64 = ghosts.iter().map(|(_, v)| v.len() as u64).sum();

        let light = self.light_matrix();
        let main = self.uniforms(self.view_proj, light, 0.0, scene.time);
        let shadow = self.uniforms(light, light, 3.0, scene.time);
        let ruin = WorldUniforms { tint: [0.85, 0.0, 0.0, 0.0], ..main };
        for (slot, u) in [(SLOT_MAIN, main), (SLOT_SHADOW, shadow), (SLOT_RUIN, ruin)] {
            ctx.write(&self.ubuf, slot as u64 * SLOT_STRIDE, bytes(&[u]));
        }

        let (fv, fi) = self.fx_geometry(scene);
        if !fi.is_empty() {
            let (vb, ib) = (bytes(&fv), bytes(&fi));
            if vb.len() as u64 > self.fx_vbuf.size() {
                self.fx_vbuf = ctx.buffer("fx vertices", wgpu::BufferUsages::VERTEX, &vec![0u8; vb.len().next_power_of_two()]);
            }
            if ib.len() as u64 > self.fx_ibuf.size() {
                self.fx_ibuf = ctx.buffer("fx indices", wgpu::BufferUsages::INDEX, &vec![0u8; ib.len().next_power_of_two()]);
            }
            ctx.write(&self.fx_vbuf, 0, vb);
            ctx.write(&self.fx_ibuf, 0, ib);
        }

        let post_u = PostUniforms {
            inv_proj: (CLIP_FIX * self.proj).inverse(),
            proj: CLIP_FIX * self.proj,
            params: [0.6, self.tune[0], self.tune[1], 1.0],
            look: [self.tune[2], 0.22, self.night, scene.time],
        };
        let mut frame = ctx.frame.take().unwrap();
        let f = &mut frame;
        let (color, resolve, depth) = self.post.world_attachments();
        let world_bind = &self.world_bind.as_ref().unwrap().1;
        if self.shadows {
            let mut pass = f.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadows"),
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_map,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.ubind, &[SLOT_SHADOW * SLOT_STRIDE as u32]);
            pass.set_pipeline(&self.pipes.terrain_shadow);
            self.draw_terrain(&mut pass);
            pass.set_pipeline(&self.pipes.model_shadow);
            let stride = std::mem::size_of::<Inst>() as u64;
            let mut base = 0;
            for ((m, ai, f0, aj, f1), insts) in &groups {
                let mesh = &self.models[m].anims[*ai];
                let n = insts.len() as u64;
                pass.set_vertex_buffer(0, mesh.colors.slice(..));
                pass.set_vertex_buffer(1, mesh.frames[*f0].slice(..));
                pass.set_vertex_buffer(2, self.models[m].anims[*aj].frames[*f1].slice(..));
                pass.set_vertex_buffer(3, self.inst_buf.slice(base * stride..(base + n) * stride));
                pass.set_index_buffer(mesh.index.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..mesh.n_indices, 0, 0..n as u32);
                base += n;
            }
        }
        {
            let mut pass = f.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.72, g: 0.8, b: 0.9, a: 1.0 }), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            let main_off = [SLOT_MAIN * SLOT_STRIDE as u32];
            pass.set_bind_group(0, &self.ubind, &main_off);
            pass.set_pipeline(&self.pipes.sky);
            pass.draw(0..3, 0..1);
            pass.set_bind_group(1, world_bind, &[]);
            if let Some(w) = &self.water {
                pass.set_pipeline(&self.pipes.water);
                pass.set_vertex_buffer(0, w.vbuf.slice(..));
                pass.set_index_buffer(w.ibuf.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..w.n, 0, 0..1);
            }
            pass.set_pipeline(&self.pipes.terrain);
            self.draw_terrain(&mut pass);
            pass.set_pipeline(&self.pipes.model);
            self.draw_groups(&mut pass, &groups, 0);
            if !fi.is_empty() {
                pass.set_bind_group(0, &self.ubind, &main_off);
                pass.set_pipeline(&self.pipes.fx);
                pass.set_vertex_buffer(0, self.fx_vbuf.slice(..));
                pass.set_index_buffer(self.fx_ibuf.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..fi.len() as u32, 0, 0..1);
            }
            pass.set_pipeline(&self.pipes.model_blend);
            self.draw_groups(&mut pass, &ghosts, n_groups);
            self.draw_groups(&mut pass, &ice_group, n_groups + n_ghosts);
        }
        let (encoder, out) = (&mut f.encoder, &f.view);
        self.post.run(ctx, encoder, out, &post_u);
        f.drawn = true;
        ctx.frame = Some(frame);
        self.target = aim;
    }

    fn world_to_screen(&self, p: Vec3) -> Option<[f32; 2]> {
        let c = self.view_proj.transform(gl(v2(p.x, p.y), p.z), 1.0);
        if c[3] <= 0.01 {
            return None;
        }
        Some([(c[0] / c[3] * 0.5 + 0.5) * self.vw, (0.5 - c[1] / c[3] * 0.5) * self.vh])
    }

    fn screen_to_ground(&self, s: [f32; 2]) -> Option<Vec2> {
        let map = self.map.as_ref()?;
        let nx = s[0] / self.vw * 2.0 - 1.0;
        let ny = 1.0 - s[1] / self.vh * 2.0;
        let un = |z: f32| {
            let p = self.inv_view_proj.transform(v3(nx, ny, z), 1.0);
            v3(p[0] / p[3], p[1] / p[3], p[2] / p[3])
        };
        let a = un(-1.0);
        let b = un(1.0);
        let dir = (b - a).norm();
        let mut t = 0.0;
        let mut prev = a;
        let max_h = crate::render3d::max_height(map) + 0.5;
        // Skip down to the top of the terrain.
        if a.z > max_h && dir.z < 0.0 {
            t = (a.z - max_h) / -dir.z;
            prev = a + dir * t;
        }
        for _ in 0..800 {
            t += 0.2;
            let p = a + dir * t;
            let h = map.height_at(v2(p.x, -p.y));
            if p.z <= h {
                // Refine between prev and p.
                let (mut lo, mut hi) = (prev, p);
                for _ in 0..12 {
                    let mid = lo.lerp(hi, 0.5);
                    if mid.z <= map.height_at(v2(mid.x, -mid.y)) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                return Some(v2(hi.x, -hi.y));
            }
            prev = p;
            if p.z < -2.0 {
                break;
            }
        }
        if dir.z.abs() > 1e-4 {
            let t = -a.z / dir.z;
            let p = a + dir * t;
            return Some(v2(p.x, -p.y));
        }
        None
    }

    fn pan_pixels(&mut self, d: [f32; 2]) {
        let ppt = self.pixels_per_tile(self.target).max(1.0);
        // Screen right and screen up, in map space.
        let right = v2(self.yaw.cos(), -self.yaw.sin());
        let fwd = v2(-self.yaw.sin(), -self.yaw.cos());
        let pitch_k = 1.0 / self.pitch().sin();
        self.target = self.target - right * (d[0] / ppt) + fwd * (d[1] / ppt * pitch_k);
        if let Some(m) = &self.map {
            self.target = v2(self.target.x.clamp(0.0, m.w as f32), self.target.y.clamp(0.0, m.h as f32));
        }
        self.update_camera();
    }

    fn zoom_at(&mut self, factor: f32, anchor: [f32; 2]) {
        let before = self.screen_to_ground(anchor);
        self.dist = (self.dist * factor).clamp(7.0, 90.0);
        self.update_camera();
        if let (Some(b), Some(a)) = (before, self.screen_to_ground(anchor)) {
            self.target += b - a;
            if let Some(m) = &self.map {
                self.target = v2(self.target.x.clamp(0.0, m.w as f32), self.target.y.clamp(0.0, m.h as f32));
            }
            self.update_camera();
        }
    }

    fn rotate(&mut self, da: f32) {
        self.yaw = (self.yaw + da) % (2.0 * PI);
        self.update_camera();
    }

    fn focus(&mut self, p: Vec2) {
        self.target = p;
        self.update_camera();
    }

    fn reset_rotation(&mut self) {
        self.yaw = 0.0;
        self.update_camera();
    }

    fn center(&self) -> Vec2 {
        self.target
    }

    fn pixels_per_tile(&self, p: Vec2) -> f32 {
        let z = self.map.as_ref().map_or(0.0, |m| m.height_at(p));
        let right = v2(self.yaw.cos(), -self.yaw.sin());
        match (self.world_to_screen(v3(p.x, p.y, z)), self.world_to_screen(v3(p.x + right.x, p.y + right.y, z))) {
            (Some(a), Some(b)) => ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt(),
            _ => 20.0,
        }
    }
}

pub fn max_height(map: &Map) -> f32 {
    map.height.iter().copied().fold(0.0, f32::max)
}
