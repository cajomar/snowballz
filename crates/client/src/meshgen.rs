//! Procedural meshes for things without a Blender source: igloos, fish
//! holes, crystals, ice pinnacles, powerups, snowballs and pine trees.
//!
//! Model space matches the Blender exports: z up, models face -y, one world
//! unit per map tile.

use std::f32::consts::{PI, TAU};

use sim::rng::Rng;

use crate::mathx::{v3, Vec3};
use crate::model::{MeshData, FLAG_EMISSIVE, FLAG_GLOSSY, FLAG_TEAM};

#[derive(Default)]
pub struct Builder {
    pub m: MeshData,
}

pub type Rgba = [u8; 4];

impl Builder {
    pub fn new() -> Builder {
        let mut b = Builder::default();
        b.m.frames.push(Vec::new());
        b
    }

    fn vert(&mut self, p: Vec3, n: Vec3, c: Rgba) -> u16 {
        let f = &mut self.m.frames[0];
        f.push([p.x, p.y, p.z, n.x, n.y, n.z]);
        self.m.colors.push(c);
        (f.len() - 1) as u16
    }

    /// Flat-shaded triangle (counter-clockwise when seen from outside).
    pub fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, col: Rgba) {
        let n = (b - a).cross(c - a).norm();
        let i = self.vert(a, n, col);
        let j = self.vert(b, n, col);
        let k = self.vert(c, n, col);
        self.m.indices.extend_from_slice(&[i, j, k]);
    }

    pub fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, col: Rgba) {
        self.tri(a, b, c, col);
        self.tri(a, c, d, col);
    }

    /// Smooth-shaded vertex grid (rows x cols) from a position function.
    pub fn surface(&mut self, rows: usize, cols: usize, f: impl Fn(f32, f32) -> (Vec3, Rgba)) {
        let base = self.m.frames[0].len() as u16;
        let mut pts = Vec::new();
        for r in 0..=rows {
            for c in 0..=cols {
                pts.push(f(r as f32 / rows as f32, c as f32 / cols as f32));
            }
        }
        let w = cols + 1;
        for r in 0..=rows {
            for c in 0..=cols {
                let p = |rr: usize, cc: usize| pts[rr.min(rows) * w + cc.min(cols)].0;
                let du = p(r, c + 1) - p(r, c.saturating_sub(1));
                let dv = p(r + 1, c) - p(r.saturating_sub(1), c);
                let mut n = du.cross(dv).norm();
                if n.len() < 0.5 {
                    n = (pts[r * w + c].0).norm();
                }
                let (pos, col) = pts[r * w + c];
                self.vert(pos, n, col);
            }
        }
        for r in 0..rows {
            for c in 0..cols {
                let i = base + (r * w + c) as u16;
                let j = i + 1;
                let k = i + w as u16;
                let l = k + 1;
                self.m.indices.extend_from_slice(&[i, j, l, i, l, k]);
            }
        }
    }

    pub fn sphere(&mut self, center: Vec3, r: Vec3, col: Rgba, segs: usize) {
        self.surface(segs / 2, segs, |v, u| {
            let th = v * PI;
            let ph = u * TAU;
            let d = v3(th.sin() * ph.cos(), th.sin() * ph.sin(), th.cos());
            (center + v3(d.x * r.x, d.y * r.y, d.z * r.z), col)
        });
    }

    /// Vertical prism with `sides`, from z0 to z1, optionally pointed.
    #[allow(clippy::too_many_arguments)]
    pub fn prism(&mut self, base: Vec3, axis: Vec3, radius: f32, len: f32, sides: usize, tip: f32, col: Rgba) {
        let axis = axis.norm();
        let t1 = if axis.z.abs() < 0.9 { axis.cross(v3(0.0, 0.0, 1.0)).norm() } else { axis.cross(v3(1.0, 0.0, 0.0)).norm() };
        let t2 = axis.cross(t1);
        let ring = |h: f32, r: f32| -> Vec<Vec3> {
            (0..sides)
                .map(|i| {
                    let a = i as f32 / sides as f32 * TAU;
                    base + axis * h + t1 * (a.cos() * r) + t2 * (a.sin() * r)
                })
                .collect()
        };
        let bottom = ring(0.0, radius);
        let top = ring(len, radius);
        let apex = base + axis * (len + tip);
        for i in 0..sides {
            let j = (i + 1) % sides;
            self.quad(bottom[i], bottom[j], top[j], top[i], col);
            self.tri(top[i], top[j], apex, col);
        }
    }

    /// The finished model, each colour tagged with the material it looks
    /// like, so the shader can give it a surface.
    pub fn finish(mut self) -> MeshData {
        for c in &mut self.m.colors {
            c[3] |= crate::model::guess_material(*c);
        }
        self.m
    }
}

fn shade(c: [u8; 3], k: f32, flags: u8) -> Rgba {
    [
        (c[0] as f32 * k).clamp(0.0, 255.0) as u8,
        (c[1] as f32 * k).clamp(0.0, 255.0) as u8,
        (c[2] as f32 * k).clamp(0.0, 255.0) as u8,
        flags,
    ]
}

pub const IGLOO_RADIUS: f32 = 1.75;
pub const IGLOO_HEIGHT: f32 = 1.5;

/// Snow-block igloo with an entrance tunnel facing -y and a team flag.
/// `tier` picks the plain igloo (0), the Snow Hall (1) or the Ice Palace (2).
pub fn igloo_tier(tier: u8) -> MeshData {
    let mut b = Builder::new();
    let mut rng = Rng::new(3);
    let hall = tier >= 1;
    let palace = tier >= 2;
    // A grander igloo is bigger, and the palace is carved from blue ice.
    let snow = if palace { [166, 210, 246] } else { [244, 248, 255] };
    let rows = 7;
    let r0 = IGLOO_RADIUS * if hall { 1.14 } else { 1.0 };
    let height = IGLOO_HEIGHT * if hall { 1.16 } else { 1.0 };
    let dome = |el: f32, az: f32, bulge: f32| -> Vec3 {
        let r = r0 + bulge;
        v3(el.cos() * az.cos() * r, el.cos() * az.sin() * r, el.sin() * (height + bulge))
    };
    // Inner shell shows through the gaps between blocks as mortar lines.
    b.surface(10, 32, |v, u| (dome(v * PI / 2.0, u * TAU, -0.02), shade([196, 210, 230], 1.0, 0)));
    for row in 0..rows {
        let el0 = row as f32 / rows as f32 * PI / 2.0;
        let el1 = (row + 1) as f32 / rows as f32 * PI / 2.0;
        let cols = ((el0.cos() * 26.0).round() as usize).max(3);
        let offset = if row % 2 == 0 { 0.0 } else { 0.5 };
        for c in 0..cols {
            let az0 = (c as f32 + offset) / cols as f32 * TAU;
            let az1 = (c as f32 + 1.0 + offset) / cols as f32 * TAU;
            // Leave a hole for the tunnel at the front (-y).
            let mid = (az0 + az1) / 2.0;
            let front = (mid - 1.5 * PI).abs() < 0.42 || (mid + 0.5 * PI).abs() < 0.42;
            if front && row < 2 {
                continue;
            }
            let gap_az = 0.012 / el0.cos().max(0.3);
            let (a0, a1) = (az0 + gap_az, az1 - gap_az);
            let (e0, e1) = (el0 + 0.012, el1 - 0.012);
            let k = 0.94 + rng.f32() * 0.08;
            let col = shade(snow, k, 0);
            b.surface(2, 3, |v, u| {
                let el = e0 + (e1 - e0) * v;
                let az = a0 + (a1 - a0) * u;
                let bump = 0.05 * ((v * PI).sin() * (u * PI).sin()).max(0.0).sqrt();
                (dome(el, az, bump), col)
            });
        }
    }
    // Entrance tunnel: a half-cylinder of blocks pointing along -y.
    let tr = 0.62;
    let tunnel_len = 1.0;
    let ty0 = -(r0 - 0.35);
    for seg in 0..4 {
        let y0 = ty0 - seg as f32 * tunnel_len / 4.0;
        let y1 = y0 - tunnel_len / 4.0 + 0.02;
        for s in 0..5 {
            let a0 = s as f32 / 5.0 * PI + 0.015;
            let a1 = (s + 1) as f32 / 5.0 * PI - 0.015;
            let k = 0.94 + rng.f32() * 0.08;
            let col = shade(snow, k, 0);
            b.surface(1, 3, |v, u| {
                let a = a0 + (a1 - a0) * u;
                let y = y0 + (y1 - y0) * v;
                (v3(a.cos() * (tr + 0.06), y, a.sin() * (tr * 1.15 + 0.06)), col)
            });
        }
    }
    // Dark doorway.
    let ye = ty0 - tunnel_len - 0.01;
    let dark = [20, 32, 55, 0];
    for s in 0..10 {
        let a0 = s as f32 / 10.0 * PI;
        let a1 = (s + 1) as f32 / 10.0 * PI;
        b.tri(
            v3(0.0, ye, 0.0),
            v3(a1.cos() * tr, ye, a1.sin() * tr * 1.15),
            v3(a0.cos() * tr, ye, a0.sin() * tr * 1.15),
            dark,
        );
    }
    // Snow mound skirt around the base.
    b.surface(2, 32, |v, u| {
        let az = u * TAU;
        let r = r0 + 0.05 + v * 0.45;
        let z = 0.18 * (1.0 - v) * (1.0 - v) - 0.04;
        (v3(az.cos() * r, az.sin() * r, z), shade(snow, 0.97, 0))
    });
    // Chimney vent.
    b.sphere(v3(0.0, 0.25, height - 0.02), v3(0.22, 0.22, 0.08), [70, 80, 100, 0], 10);
    // Flag pole and team flag beside the door.
    let pole = v3(0.95, -r0 - 0.35, 0.0);
    b.prism(pole, v3(0.0, 0.0, 1.0), 0.04, 2.4, 6, 0.0, [120, 90, 60, 0]);
    let fz = 2.35;
    let team = [255, 255, 255, FLAG_TEAM];
    let wave = |x: f32| (x * 4.0).sin() * 0.06;
    let fl = 0.9;
    for i in 0..6 {
        let x0 = i as f32 / 6.0 * fl;
        let x1 = (i + 1) as f32 / 6.0 * fl;
        let p = |x: f32, z: f32| pole + v3(0.04 + x, wave(x), z);
        b.quad(p(x0, fz - 0.55), p(x1, fz - 0.55), p(x1, fz), p(x0, fz), team);
        b.quad(p(x0, fz), p(x1, fz), p(x1, fz - 0.55), p(x0, fz - 0.55), team);
    }
    b.sphere(pole + v3(0.0, 0.0, 2.45), v3(0.08, 0.08, 0.08), [230, 190, 60, 0], 8);
    // A Snow Hall has a second dome at the back and banners by the door.
    if hall {
        let wing = v3(0.0, r0 * 0.68, 0.0);
        block_dome(&mut b, &mut rng, wing, r0 * 0.6, height * 0.6, snow, 4);
        for sx in [-1.0f32, 1.0] {
            let bp = v3(sx * (r0 + 0.42), -r0 * 0.55, 0.0);
            b.prism(bp, v3(0.0, 0.0, 1.0), 0.05, 2.6, 6, 0.0, [120, 90, 60, 0]);
            // The banner cloth wears the owner's colour.
            let c = [255, 255, 255, FLAG_TEAM];
            let top = 2.5;
            let bot = 1.72;
            let w = 0.44;
            b.quad(bp + v3(0.03, 0.0, top), bp + v3(0.03 + w, 0.0, top), bp + v3(0.03 + w, 0.0, bot), bp + v3(0.03, 0.0, bot), c);
            b.quad(bp + v3(0.03, 0.0, bot), bp + v3(0.03 + w, 0.0, bot), bp + v3(0.03 + w, 0.0, top), bp + v3(0.03, 0.0, top), c);
            b.sphere(bp + v3(0.0, 0.0, 2.62), v3(0.09, 0.09, 0.09), [230, 190, 60, 0], 8);
        }
    }
    // The palace is ringed with spires of blue ice.
    if palace {
        for i in 0..6 {
            let a = i as f32 / 6.0 * TAU + 0.5;
            let p = v3(a.cos() * (r0 + 0.3), a.sin() * (r0 + 0.3), 0.0);
            let len = 0.85 + (i % 2) as f32 * 0.4;
            b.prism(p, v3(0.0, 0.0, 1.0), 0.17, len, 5, 0.4, shade([206, 234, 255], 1.0, FLAG_GLOSSY));
        }
        // A crown of ice on the roof.
        for i in 0..3 {
            let a = i as f32 / 3.0 * TAU + 0.9;
            let p = v3(a.cos() * 0.55, a.sin() * 0.55, height - 0.1);
            b.prism(p, v3(0.0, 0.0, 1.0), 0.13, 0.6, 5, 0.45, shade([220, 242, 255], 1.0, FLAG_GLOSSY));
        }
    }
    b.finish()
}

/// A collapsed igloo: a squashed broken dome, a wedge torn out of the back
/// and the doorway buried under snow.
pub fn igloo_ruin() -> MeshData {
    let mut b = Builder::new();
    let mut rng = Rng::new(17);
    let snow = [244, 248, 255];
    let r0 = IGLOO_RADIUS * 1.06;
    let height = 0.58;
    // Ragged cap: how high the broken dome still stands at each azimuth.
    // The back wedge (+y) is gone entirely.
    let cap = |az: f32| -> f32 {
        let d = (az - 0.5 * PI + PI).rem_euclid(TAU) - PI;
        let wedge = ((d.abs() - 0.75) / 0.2).clamp(0.0, 1.0);
        if wedge == 0.0 {
            return 0.0;
        }
        let rag = (az * 3.1).sin() * 0.5 + (az * 5.3).sin() * 0.3 + (az * 9.7).sin() * 0.2;
        wedge * (0.62 + 0.38 * rag).max(0.35)
    };
    // Dome closure: the broken rim undulates and sinks into the ground
    // across the torn wedge.
    let dome = |el: f32, az: f32, bulge: f32| -> Vec3 {
        let r = r0 + bulge;
        let c = cap(az);
        let z = el.sin() * (height + bulge) * c - (1.0 - c.min(1.0)) * 0.25;
        v3(el.cos() * az.cos() * r, el.cos() * az.sin() * r, z)
    };
    // Inner shell shows through the gaps between blocks as mortar lines.
    b.surface(8, 48, |v, u| {
        let el = v * PI / 2.0;
        let az = u * TAU;
        (dome(el, az, -0.02), shade([176, 190, 212], 1.0, 0))
    });
    let rows = 4;
    for row in 0..rows {
        let el0 = row as f32 / rows as f32 * PI / 2.0;
        let el1 = (row + 1) as f32 / rows as f32 * PI / 2.0;
        let cols = ((el0.cos() * 20.0).round() as usize).max(3);
        let offset = if row % 2 == 0 { 0.0 } else { 0.5 };
        for c in 0..cols {
            let az0 = (c as f32 + offset) / cols as f32 * TAU;
            let az1 = (c as f32 + 1.0 + offset) / cols as f32 * TAU;
            if cap((az0 + az1) / 2.0) < 0.1 {
                continue;
            }
            let gap_az = 0.014 / el0.cos().max(0.3);
            let (a0, a1) = (az0 + gap_az, az1 - gap_az);
            let (e0, e1) = (el0 + 0.014, el1 - 0.014);
            let k = 0.92 + rng.f32() * 0.1;
            let col = shade(snow, k, 0);
            b.surface(2, 3, |v, u| {
                let el = e0 + (e1 - e0) * v;
                let az = a0 + (a1 - a0) * u;
                let bump = 0.05 * ((v * PI).sin() * (u * PI).sin()).max(0.0).sqrt();
                (dome(el, az, bump), col)
            });
        }
    }
    // Snow mound skirt around the base.
    b.surface(2, 32, |v, u| {
        let az = u * TAU;
        let r = r0 + 0.05 + v * 0.45;
        let z = 0.18 * (1.0 - v) * (1.0 - v) - 0.04;
        (v3(az.cos() * r, az.sin() * r, z), shade(snow, 0.97, 0))
    });
    // The doorway, buried: a dark fan in the snow in front (-y).
    let dark = [20, 32, 55, 0];
    let fy = -(r0 + 0.55);
    for s in 0..9 {
        let a0 = PI + s as f32 / 9.0 * PI;
        let a1 = PI + (s + 1) as f32 / 9.0 * PI;
        b.tri(
            v3(0.0, fy, 0.02),
            v3(a1.cos() * 0.5, fy + a1.sin() * 0.5, 0.02),
            v3(a0.cos() * 0.5, fy + a0.sin() * 0.5, 0.02),
            dark,
        );
    }
    // Snow lumps spilling over the opening.
    b.sphere(v3(-0.28, fy - 0.24, 0.1), v3(0.16, 0.2, 0.08), shade(snow, 0.98, 0), 7);
    b.sphere(v3(0.24, fy - 0.3, 0.09), v3(0.14, 0.18, 0.07), shade(snow, 0.94, 0), 7);
    b.sphere(v3(0.0, fy - 0.42, 0.08), v3(0.17, 0.2, 0.08), shade(snow, 1.0, 0), 7);
    // Snow blocks knocked loose around the ruin.
    for _ in 0..7 {
        let a = rng.f32() * TAU;
        let rr = r0 + 0.1 + rng.f32() * 0.4;
        b.sphere(
            v3(a.cos() * rr, a.sin() * rr, 0.08),
            v3(0.1 + rng.f32() * 0.08, 0.1 + rng.f32() * 0.08, 0.06 + rng.f32() * 0.05),
            shade(snow, 0.9 + rng.f32() * 0.12, 0),
            6,
        );
    }
    b.finish()
}

/// A plain dome of snow blocks, used for the Snow Hall's back wing.
fn block_dome(b: &mut Builder, rng: &mut Rng, center: Vec3, r: f32, h: f32, snow: [u8; 3], rows: usize) {
    b.surface(6, 24, |v, u| {
        let el = v * PI / 2.0;
        let az = u * TAU;
        (center + v3(el.cos() * az.cos() * r, el.cos() * az.sin() * r, el.sin() * h), shade([196, 210, 230], 1.0, 0))
    });
    for row in 0..rows {
        let el0 = row as f32 / rows as f32 * PI / 2.0;
        let el1 = (row + 1) as f32 / rows as f32 * PI / 2.0;
        let cols = ((el0.cos() * 18.0).round() as usize).max(3);
        let offset = if row % 2 == 0 { 0.0 } else { 0.5 };
        for c in 0..cols {
            let az0 = (c as f32 + offset) / cols as f32 * TAU;
            let az1 = (c as f32 + 1.0 + offset) / cols as f32 * TAU;
            let gap = 0.014 / el0.cos().max(0.3);
            let (a0, a1) = (az0 + gap, az1 - gap);
            let (e0, e1) = (el0 + 0.014, el1 - 0.014);
            let k = 0.94 + rng.f32() * 0.08;
            let col = shade(snow, k, 0);
            b.surface(2, 3, |v, u| {
                let el = e0 + (e1 - e0) * v;
                let az = a0 + (a1 - a0) * u;
                let bump = 0.05 * ((v * PI).sin() * (u * PI).sin()).max(0.0).sqrt();
                let rr = r + bump;
                (center + v3(el.cos() * az.cos() * rr, el.cos() * az.sin() * rr, el.sin() * (h + bump)), col)
            });
        }
    }
}

pub fn snowball() -> MeshData {
    let mut b = Builder::new();
    b.sphere(v3(0.0, 0.0, 0.0), v3(0.11, 0.11, 0.11), [255, 255, 255, 0], 10);
    b.finish()
}

/// A fishing hole in the ice; `open` = has fish left.
pub fn fish_hole(open: bool) -> MeshData {
    let mut b = Builder::new();
    let mut rng = Rng::new(11);
    let r = 0.42;
    if open {
        let water = [18, 60, 92, FLAG_GLOSSY];
        b.surface(1, 20, |v, u| {
            let a = u * TAU;
            (v3(a.cos() * r * v, a.sin() * r * v, 0.02), water)
        });
        // Broken ice chunks around the rim.
        for i in 0..9 {
            let a = i as f32 / 9.0 * TAU + rng.range(-0.2, 0.2);
            let p = v3(a.cos() * (r + 0.08), a.sin() * (r + 0.08), 0.0);
            let s = rng.range(0.1, 0.17);
            b.prism(p, v3(rng.range(-0.3, 0.3), rng.range(-0.3, 0.3), 1.0), s, rng.range(0.05, 0.12), 5, 0.03, [214, 236, 250, FLAG_GLOSSY]);
        }
        // A fish tail poking up for good measure.
        b.tri(v3(0.05, 0.0, 0.03), v3(0.14, 0.03, 0.22), v3(0.14, -0.03, 0.22), [110, 140, 160, FLAG_GLOSSY]);
        b.tri(v3(0.05, 0.0, 0.03), v3(0.14, -0.03, 0.22), v3(0.14, 0.03, 0.22), [110, 140, 160, FLAG_GLOSSY]);
    } else {
        // Frozen over: a pale, slightly domed ice lid in a low ring of snow.
        b.surface(3, 20, |v, u| {
            let a = u * TAU;
            let rr = r * v * (1.0 + 0.06 * (a * 3.0).sin());
            let c = shade([158, 200, 228], 0.92 + 0.12 * (1.0 - v), FLAG_GLOSSY);
            (v3(a.cos() * rr, a.sin() * rr, 0.035 + 0.025 * (1.0 - v * v)), c)
        });
        b.surface(2, 24, |v, u| {
            let a = u * TAU;
            let rr = r * (1.0 + 0.06 * (a * 3.0).sin()) + v * 0.16;
            (v3(a.cos() * rr, a.sin() * rr, 0.07 * (v * std::f32::consts::PI).sin() + 0.01), [240, 245, 252, 0])
        });
    }
    b.finish()
}

/// Crystal cluster; `size` 0..1 shrinks as it's mined out.
pub fn crystal(size: f32) -> MeshData {
    let mut b = Builder::new();
    let mut rng = Rng::new(5);
    let base = [190, 80, 230];
    for i in 0..6 {
        let a = i as f32 / 6.0 * TAU + rng.range(-0.3, 0.3);
        let off = if i == 0 { 0.0 } else { 0.18 };
        let p = v3(a.cos() * off, a.sin() * off, -0.05);
        let axis = v3(a.cos() * off * 2.0, a.sin() * off * 2.0, 1.0);
        let len = (if i == 0 { 0.75 } else { rng.range(0.3, 0.5) }) * (0.3 + 0.7 * size);
        let col = shade(base, rng.range(0.85, 1.15), FLAG_EMISSIVE | FLAG_GLOSSY);
        b.prism(p, axis, 0.08 + 0.04 * size, len, 6, 0.12, col);
    }
    b.finish()
}

/// A snowy pine: tiers of drooping, star-shaped branch skirts, each with
/// a ragged cap of snow, on a short trunk. About two tiles tall.
pub fn pine(seed: u64) -> MeshData {
    let mut b = Builder::new();
    let mut rng = Rng::new(seed * 7 + 3);
    let bark = [92, 64, 46];
    b.prism(v3(0.0, 0.0, -0.05), v3(0.0, 0.0, 1.0), 0.075, 0.55, 6, 0.0, shade(bark, 1.0, 0));
    let tiers = 4 + (seed % 2) as usize;
    let lean = v3(rng.range(-0.04, 0.04), rng.range(-0.04, 0.04), 0.0);
    let spin = rng.range(0.0, TAU);
    for t in 0..tiers {
        let f = t as f32 / (tiers - 1) as f32;
        let r = 0.6 - 0.4 * f;
        let rim = 0.38 + 1.12 * f;
        let apex = v3(0.0, 0.0, rim + 0.62 - 0.12 * f) + lean * (f * 4.0);
        let n = 9 - t.min(3);
        let green = [36 + rng.below(10) as u8, 88 + rng.below(16) as u8, 66 + rng.below(10) as u8];
        // Alternate long branch tips and short notches between them.
        let pts: Vec<Vec3> = (0..n * 2)
            .map(|i| {
                let a = i as f32 / (n * 2) as f32 * TAU + spin + t as f32 * 0.6;
                let tip = i % 2 == 0;
                let rr = if tip { r * rng.range(0.92, 1.08) } else { r * 0.62 };
                let z = if tip { rim - 0.08 } else { rim + 0.06 };
                v3(a.cos() * rr, a.sin() * rr, z) + lean * (f * 4.0)
            })
            .collect();
        let under = v3(0.0, 0.0, rim + 0.02) + lean * (f * 4.0);
        for i in 0..n * 2 {
            let (p, q) = (pts[i], pts[(i + 1) % (n * 2)]);
            b.tri(apex, p, q, shade(green, rng.range(0.85, 1.12), 0));
            b.tri(q, p, under, shade(green, 0.55, 0));
        }
        // Snow cap: the upper part of the same skirt, pushed out a little.
        let cover = rng.range(0.58, 0.75);
        let cap: Vec<Vec3> = pts
            .iter()
            .enumerate()
            .map(|(i, &p)| {
                let k = cover * if i % 2 == 0 { rng.range(0.9, 1.15) } else { rng.range(0.75, 1.0) };
                apex + (p - apex) * k + v3(0.0, 0.0, 0.035)
            })
            .collect();
        let top = apex + v3(0.0, 0.0, 0.03);
        for i in 0..n * 2 {
            let (p, q) = (cap[i], cap[(i + 1) % (n * 2)]);
            b.tri(top, p, q, shade([238, 244, 252], rng.range(0.95, 1.03), 0));
        }
    }
    b.finish()
}

/// Jagged spires of blue ice; impassable.
pub fn pinnacle(seed: u64) -> MeshData {
    let mut b = Builder::new();
    let mut rng = Rng::new(seed);
    for _ in 0..rng.below(3) + 3 {
        let p = v3(rng.range(-0.3, 0.3), rng.range(-0.3, 0.3), -0.05);
        let axis = v3(rng.range(-0.25, 0.25), rng.range(-0.25, 0.25), 1.0);
        let col = shade([170, 215, 245], rng.range(0.9, 1.1), FLAG_GLOSSY);
        b.prism(p, axis, rng.range(0.1, 0.2), rng.range(0.5, 1.4), 6, rng.range(0.15, 0.35), col);
    }
    b.finish()
}

pub fn powerup(kind: sim::game::PowerKind) -> MeshData {
    let mut b = Builder::new();
    // Snowy pedestal.
    b.prism(v3(0.0, 0.0, -0.02), v3(0.0, 0.0, 1.0), 0.28, 0.12, 10, 0.02, [235, 240, 250, 0]);
    match kind {
        sim::game::PowerKind::Defense => {
            // A big blue gem.
            let c = [70, 150, 255, FLAG_EMISSIVE | FLAG_GLOSSY];
            let top = v3(0.0, 0.0, 0.85);
            let bot = v3(0.0, 0.0, 0.25);
            for i in 0..6 {
                let a0 = i as f32 / 6.0 * TAU;
                let a1 = (i + 1) as f32 / 6.0 * TAU;
                let p0 = v3(a0.cos() * 0.2, a0.sin() * 0.2, 0.6);
                let p1 = v3(a1.cos() * 0.2, a1.sin() * 0.2, 0.6);
                b.tri(p0, p1, top, c);
                b.tri(p1, p0, bot, c);
            }
        }
        sim::game::PowerKind::Double => {
            let c = [255, 200, 70, FLAG_EMISSIVE];
            b.sphere(v3(-0.1, 0.0, 0.38), v3(0.15, 0.15, 0.15), c, 12);
            b.sphere(v3(0.1, 0.0, 0.62), v3(0.15, 0.15, 0.15), c, 12);
        }
    }
    b.finish()
}

/// A box from `lo` to `hi` (outward-facing, flat shaded).
fn cuboid(b: &mut Builder, lo: Vec3, hi: Vec3, col: Rgba) {
    let p = |i: usize| v3(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
    b.quad(p(4), p(5), p(7), p(6), col); // top
    b.quad(p(0), p(2), p(3), p(1), col); // bottom
    b.quad(p(0), p(1), p(5), p(4), col); // -y
    b.quad(p(2), p(6), p(7), p(3), col); // +y
    b.quad(p(0), p(4), p(6), p(2), col); // -x
    b.quad(p(1), p(3), p(7), p(5), col); // +x
}

/// A small team pennant on a pole standing at `base`.
fn pennant(b: &mut Builder, base: Vec3, h: f32) {
    b.prism(base, v3(0.0, 0.0, 1.0), 0.03, h, 5, 0.0, [120, 90, 60, 0]);
    let team = [255, 255, 255, FLAG_TEAM];
    let top = base + v3(0.0, 0.0, h);
    b.tri(top, top + v3(0.45, 0.0, -0.13), top + v3(0.0, 0.0, -0.27), team);
    b.tri(top, top + v3(0.0, 0.0, -0.27), top + v3(0.45, 0.0, -0.13), team);
}

/// Sauna (2x2 tiles): a low snow-block shelter open at the front with
/// a fire glowing in it and a stack of logs.
pub fn hut() -> MeshData {
    let mut b = Builder::new();
    let mut rng = Rng::new(11);
    let snow = [242, 246, 255];
    let (r, h) = (0.82, 0.78);
    let dome = |el: f32, az: f32| v3(el.cos() * az.cos() * r, el.cos() * az.sin() * r, el.sin() * h);
    let rows = 4;
    for row in 0..rows {
        let el0 = row as f32 / rows as f32 * PI / 2.0;
        let el1 = (row + 1) as f32 / rows as f32 * PI / 2.0;
        let cols = ((el0.cos() * 12.0).round() as usize).max(3);
        for c in 0..cols {
            let az0 = (c as f32 + row as f32 * 0.5) / cols as f32 * TAU;
            let az1 = az0 + TAU / cols as f32;
            let mid = ((az0 + az1) / 2.0).rem_euclid(TAU);
            // Open to the front (-y) for the bottom rows.
            if row < 2 && (mid - 1.5 * PI).abs() < 0.55 {
                continue;
            }
            let col = shade(snow, 0.93 + rng.f32() * 0.08, 0);
            let (a0, a1, e0, e1) = (az0 + 0.02, az1 - 0.02, el0 + 0.02, el1 - 0.02);
            b.surface(1, 2, |v, u| (dome(e0 + (e1 - e0) * v, a0 + (a1 - a0) * u), col));
        }
    }
    // Inside shadow so the opening reads as a hollow.
    b.surface(4, 16, |v, u| (dome(v * PI / 2.0, u * TAU) * 0.96, [60, 70, 95, 0]));
    // Fire: logs and glowing flames just in front of the door.
    let fire = v3(0.0, -r - 0.25, 0.0);
    for k in 0..3 {
        let a = k as f32 / 3.0 * TAU + 0.4;
        b.prism(fire + v3(a.cos() * 0.18, a.sin() * 0.18, 0.04), v3(-a.cos(), -a.sin(), 0.35), 0.035, 0.32, 5, 0.0, shade([96, 66, 44], 1.0, 0));
    }
    b.prism(fire, v3(0.0, 0.0, 1.0), 0.1, 0.12, 6, 0.22, [255, 150, 40, FLAG_EMISSIVE]);
    b.prism(fire + v3(0.02, 0.0, 0.0), v3(0.0, 0.0, 1.0), 0.05, 0.1, 5, 0.18, [255, 230, 120, FLAG_EMISSIVE]);
    // Stones ringing the fire.
    for k in 0..8 {
        let a = k as f32 / 8.0 * TAU;
        b.sphere(fire + v3(a.cos() * 0.24, a.sin() * 0.24, 0.02), v3(0.06, 0.06, 0.05), shade([120, 125, 135], 0.9 + rng.f32() * 0.2, 0), 6);
    }
    // Woodpile beside it.
    for k in 0..4 {
        let z = 0.05 + (k / 2) as f32 * 0.09;
        let x = 0.55 + (k % 2) as f32 * 0.1 + (k / 2) as f32 * 0.05;
        b.prism(v3(x, -0.85, z), v3(0.0, 1.0, 0.0), 0.045, 0.4, 6, 0.0, shade([104, 72, 50], 0.9 + rng.f32() * 0.2, 0));
    }
    pennant(&mut b, v3(-0.75, -0.7, 0.0), 1.2);
    b.finish()
}

/// A campfire (1 tile): a ring of stones, crossed logs and flames.
pub fn campfire() -> MeshData {
    let mut b = Builder::new();
    let mut rng = Rng::new(23);
    for k in 0..8 {
        let a = k as f32 / 8.0 * TAU;
        b.sphere(v3(a.cos() * 0.3, a.sin() * 0.3, 0.03), v3(0.08, 0.08, 0.06), shade([120, 125, 135], 0.9 + rng.f32() * 0.2, 0), 6);
    }
    for k in 0..4 {
        let a = k as f32 / 4.0 * TAU + 0.3;
        b.prism(v3(a.cos() * 0.2, a.sin() * 0.2, 0.06), v3(-a.cos(), -a.sin(), 0.4), 0.04, 0.4, 5, 0.0, shade([96, 66, 44], 1.0, 0));
    }
    b.prism(v3(0.0, 0.0, 0.05), v3(0.0, 0.0, 1.0), 0.13, 0.12, 6, 0.28, [255, 150, 40, FLAG_EMISSIVE]);
    b.prism(v3(0.02, 0.0, 0.05), v3(0.0, 0.0, 1.0), 0.07, 0.1, 5, 0.22, [255, 230, 120, FLAG_EMISSIVE]);
    b.finish()
}

/// A taller snow palisade with staggered packed courses.
pub fn palisade(seed: u64) -> MeshData {
    let mut b = Builder::new();
    let mut rng = Rng::new(seed * 47 + 11);
    let (hw, depth, course) = (0.5, 0.38, 0.38);
    for row in 0..3 {
        let z0 = row as f32 * course;
        let off = if row % 2 == 0 { 0.0 } else { 0.25 };
        let mut x: f32 = -hw - off;
        while x < hw {
            let x0 = x.max(-hw) + 0.012;
            let x1 = (x + 0.5).min(hw) - 0.012;
            cuboid(&mut b, v3(x0, -depth, z0 + 0.01), v3(x1, depth, z0 + course - 0.01), shade([238, 245, 255], 0.92 + rng.f32() * 0.08, 0));
            x += 0.5;
        }
    }
    cuboid(&mut b, v3(-hw, -depth + 0.03, 0.0), v3(hw, depth - 0.03, course * 3.0 - 0.02), [190, 205, 228, 0]);
    b.finish()
}

const FRAC_PI_2_F: f32 = PI / 2.0;

/// Watchtower (2x2): a squat tower of packed snow blocks with a
/// crenellated roof for defenders to stand on, a dark door and a banner.
/// The roof is at height 1.6, where garrisoned penguins stand.
pub fn tower() -> MeshData {
    let mut b = Builder::new();
    tower_body(&mut b);
    pennant(&mut b, v3(0.86 - 0.1, 0.86 - 0.1, 1.6), 0.95);
    b.finish()
}

/// Catapult Tower (2x2): the watchtower with a timber catapult on its roof,
/// its throwing arm cocked back over a cup with a big snowball in it.
pub fn catapult_tower() -> MeshData {
    let mut b = Builder::new();
    tower_body(&mut b);
    let deck = 1.6;
    let wood = [124, 90, 62];
    let dark = shade(wood, 0.7, 0);
    // Two side frames, an A on each side of the arm.
    for x in [-0.32f32, 0.32] {
        cuboid(&mut b, v3(x - 0.05, -0.5, deck), v3(x + 0.05, 0.5, deck + 0.1), dark);
        cuboid(&mut b, v3(x - 0.05, -0.06, deck), v3(x + 0.05, 0.06, deck + 0.62), shade(wood, 0.9, 0));
    }
    // The axle across the tops, and a counterweight box under the short end.
    cuboid(&mut b, v3(-0.36, -0.05, deck + 0.52), v3(0.36, 0.05, deck + 0.62), dark);
    cuboid(&mut b, v3(-0.18, 0.12, deck + 0.18), v3(0.18, 0.42, deck + 0.48), shade(wood, 0.6, 0));
    // The arm, leaning back from the axle, with its cup at the far end.
    let (lo, hi) = (v3(0.0, 0.25, deck + 0.42), v3(0.0, -0.62, deck + 1.05));
    let side = v3(0.05, 0.0, 0.0);
    let up = v3(0.0, 0.035, 0.035);
    b.quad(lo - side + up, hi - side + up, hi + side + up, lo + side + up, shade(wood, 1.05, 0));
    b.quad(lo + side - up, hi + side - up, hi - side - up, lo - side - up, shade(wood, 0.75, 0));
    b.quad(lo - side - up, hi - side - up, hi - side + up, lo - side + up, shade(wood, 0.85, 0));
    b.quad(lo + side + up, hi + side + up, hi + side - up, lo + side - up, shade(wood, 0.85, 0));
    cuboid(&mut b, hi + v3(-0.13, -0.13, -0.05), hi + v3(0.13, 0.13, 0.03), dark);
    cuboid(&mut b, hi + v3(-0.11, -0.11, 0.03), hi + v3(0.11, 0.11, 0.22), [238, 244, 252, 0]);
    b.finish()
}

/// The watchtower's walls, roof and crenellations.
fn tower_body(b: &mut Builder) {
    let mut rng = Rng::new(41);
    let snow = [226, 234, 246];
    let (hw, deck) = (0.86, 1.6);
    // Courses of blocks, each a touch different so the walls read as stacked.
    let courses = 8;
    for k in 0..courses {
        let z0 = k as f32 * deck / courses as f32;
        let inset = 0.015 * (courses - k) as f32 * 0.4;
        let shadeof = 0.9 + rng.f32() * 0.12;
        cuboid(b, v3(-hw + inset, -hw + inset, z0), v3(hw - inset, hw - inset, z0 + deck / courses as f32), shade(snow, shadeof, 0));
    }
    // The floor of the roof, a little wider than the walls.
    cuboid(b, v3(-hw - 0.06, -hw - 0.06, deck - 0.1), v3(hw + 0.06, hw + 0.06, deck), shade(snow, 0.8, 0));
    // Crenellations: three merlons along each side, with gaps to throw through.
    for side in 0..4 {
        let a = side as f32 * FRAC_PI_2_F;
        let (c, s) = (a.cos(), a.sin());
        for m in [-0.58f32, 0.0, 0.58] {
            let (cx, cy) = (c * (hw - 0.04) - s * m, s * (hw - 0.04) + c * m);
            cuboid(b, v3(cx - 0.17, cy - 0.17, deck), v3(cx + 0.17, cy + 0.17, deck + 0.34), shade(snow, 0.95 + rng.f32() * 0.1, 0));
        }
    }
    // A dark doorway at the foot of the front wall.
    cuboid(b, v3(-0.2, -hw - 0.01, 0.0), v3(0.2, -hw + 0.05, 0.55), [48, 56, 74, 0]);
    // Arrow slits.
    for x in [-0.45f32, 0.45] {
        cuboid(b, v3(x - 0.04, -hw - 0.01, 0.95), v3(x + 0.04, -hw + 0.04, 1.3), [48, 56, 74, 0]);
    }
}

/// Fish store (2x2): a plank hut with a rack of fish drying out front.
pub fn store() -> MeshData {
    let mut b = Builder::new();
    let mut rng = Rng::new(23);
    let wood = [124, 90, 62];
    let (hw, hd, wall_h) = (0.62, 0.48, 0.55);
    // Plank walls.
    for k in 0..6 {
        let z0 = k as f32 * wall_h / 6.0;
        let col = shade(wood, 0.85 + rng.f32() * 0.25, 0);
        cuboid(&mut b, v3(-hw, -hd + 0.15, z0 + 0.005), v3(hw, hd + 0.15, z0 + wall_h / 6.0 - 0.005), col);
    }
    // Dark doorway.
    b.quad(v3(-0.16, -hd + 0.149, 0.0), v3(0.16, -hd + 0.149, 0.0), v3(0.16, -hd + 0.149, 0.42), v3(-0.16, -hd + 0.149, 0.42), [30, 25, 22, 0]);
    // Pitched roof under snow.
    let ridge = wall_h + 0.42;
    let (ox, oy) = (hw + 0.1, hd + 0.12);
    let (a, c) = (v3(-ox, -oy + 0.15, wall_h - 0.03), v3(ox, -oy + 0.15, wall_h - 0.03));
    let (d, e) = (v3(-ox, oy + 0.15, wall_h - 0.03), v3(ox, oy + 0.15, wall_h - 0.03));
    let (r0, r1) = (v3(-ox, 0.15, ridge), v3(ox, 0.15, ridge));
    // Plank roof, then a lumpy snow blanket that leaves the eaves showing.
    let plank = shade(wood, 0.75, 0);
    b.quad(a, c, r1, r0, plank);
    b.quad(e, d, r0, r1, plank);
    b.tri(a, r0, d, shade(wood, 0.8, 0));
    b.tri(c, e, r1, shade(wood, 0.8, 0));
    let lift = v3(0.0, 0.0, 0.04);
    let inset = |p: Vec3, toward: Vec3| p + (toward - p) * 0.12 + lift;
    let (sa, sc) = (inset(a, r0), inset(c, r1));
    let (sd, se) = (inset(d, r0), inset(e, r1));
    let (sr0, sr1) = (r0 + lift + v3(0.06, 0.0, 0.0), r1 + lift - v3(0.06, 0.0, 0.0));
    b.quad(sa + v3(0.06, 0.0, 0.0), sc - v3(0.06, 0.0, 0.0), sr1, sr0, [236, 242, 252, 0]);
    b.quad(se - v3(0.06, 0.0, 0.0), sd + v3(0.06, 0.0, 0.0), sr0, sr1, [214, 224, 240, 0]);
    // Ridge beam.
    b.prism(r0 - v3(0.05, 0.0, -0.02), v3(1.0, 0.0, 0.0), 0.035, ox * 2.0 + 0.1, 5, 0.0, shade(wood, 0.7, 0));
    // Drying rack with fish.
    let rack = v3(0.0, -hd - 0.25, 0.0);
    for x in [-0.45f32, 0.45] {
        b.prism(rack + v3(x, 0.0, -0.05), v3(0.0, 0.0, 1.0), 0.03, 0.62, 5, 0.0, shade(wood, 0.9, 0));
    }
    b.prism(rack + v3(-0.47, 0.0, 0.55), v3(1.0, 0.0, 0.0), 0.022, 0.94, 5, 0.0, shade(wood, 0.9, 0));
    for k in 0..5 {
        let x = -0.32 + k as f32 * 0.16;
        let col = shade([150, 170, 190], 0.9 + rng.f32() * 0.2, FLAG_GLOSSY);
        b.sphere(rack + v3(x, 0.0, 0.4), v3(0.035, 0.02, 0.12), col, 6);
        b.tri(rack + v3(x, 0.0, 0.29), rack + v3(x - 0.05, 0.0, 0.23), rack + v3(x + 0.05, 0.0, 0.23), col);
    }
    pennant(&mut b, v3(hw + 0.05, hd + 0.2, 0.0), 1.15);
    b.finish()
}

/// Ski Lodge (2x2): a raised timber shelter with skis by the entrance.
pub fn ski_lodge() -> MeshData {
    let mut b = Builder::new();
    let wood = [124, 90, 62];
    cuboid(&mut b, v3(-0.48, -0.35, 0.12), v3(0.48, 0.42, 0.68), shade(wood, 0.9, 0));
    for x in [-0.52f32, 0.52] {
        b.prism(v3(x, -0.38, 0.0), v3(0.0, 0.0, 1.0), 0.045, 0.88, 5, 0.0, shade(wood, 0.72, 0));
    }
    let a = v3(-0.62, -0.48, 0.65);
    let c = v3(0.62, -0.48, 0.65);
    let d = v3(-0.62, 0.52, 0.65);
    let e = v3(0.62, 0.52, 0.65);
    let r0 = v3(-0.62, 0.02, 1.05);
    let r1 = v3(0.62, 0.02, 1.05);
    b.quad(a, c, r1, r0, shade(wood, 0.75, 0));
    b.quad(e, d, r0, r1, [232, 239, 249, 0]);
    b.prism(r0, v3(1.24, 0.0, 0.0), 0.045, 1.24, 5, 0.0, shade(wood, 0.7, 0));
    b.quad(v3(-0.15, -0.356, 0.12), v3(0.15, -0.356, 0.12), v3(0.15, -0.356, 0.5), v3(-0.15, -0.356, 0.5), [30, 25, 22, 0]);
    for x in [-0.28f32, 0.28] {
        b.prism(v3(x, -0.72, 0.03), v3(0.0, 0.9, 0.0), 0.035, 0.9, 5, 0.08, [170, 205, 225, 0]);
    }
    pennant(&mut b, v3(0.58, 0.48, 0.0), 1.05);
    b.finish()
}

/// Bunkhouse: a long, low snow hut with a plank door, and a stacked bunk
/// frame airing beside it.
pub fn bunkhouse() -> MeshData {
    let mut b = Builder::new();
    let mut rng = Rng::new(43);
    let wood = [124, 90, 62];
    // The hut: a long snow mound, ringed with packed blocks at its foot.
    b.sphere(v3(0.0, 0.1, 0.0), v3(0.82, 0.5, 0.62), shade([236, 242, 250], 1.0, 0), 18);
    for k in 0..14 {
        let a = k as f32 / 14.0 * std::f32::consts::TAU;
        let (x, y) = (a.cos() * 0.8, 0.1 + a.sin() * 0.48);
        cuboid(&mut b, v3(x - 0.09, y - 0.07, 0.0), v3(x + 0.09, y + 0.07, 0.12), shade([226, 236, 248], 0.92 + rng.f32() * 0.1, 0));
    }
    // The door, facing the viewer.
    cuboid(&mut b, v3(-0.17, -0.42, 0.0), v3(0.17, -0.36, 0.42), shade(wood, 0.75, 0));
    cuboid(&mut b, v3(-0.2, -0.43, 0.42), v3(0.2, -0.35, 0.47), shade(wood, 0.6, 0));
    // Bunks: two planks on four posts, a rolled blanket on each.
    let (x0, x1, y0, y1) = (0.35f32, 0.9f32, -0.85f32, -0.55f32);
    for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
        cuboid(&mut b, v3(x - 0.03, y - 0.03, 0.0), v3(x + 0.03, y + 0.03, 0.62), shade(wood, 0.7, 0));
    }
    for (z, col) in [(0.2f32, [178, 64, 52]), (0.5, [70, 110, 170])] {
        cuboid(&mut b, v3(x0, y0, z), v3(x1, y1, z + 0.05), shade(wood, 0.9, 0));
        b.prism(v3(x0 + 0.08, (y0 + y1) / 2.0, z + 0.1), v3(1.0, 0.0, 0.0), 0.06, 0.4, 8, 0.0, shade(col, 1.0, 0));
    }
    pennant(&mut b, v3(-0.9, -0.6, 0.0), 0.95);
    b.finish()
}

/// Ice workshop: a work bench with tall slabs of ice stood up beside it.
pub fn workshop() -> MeshData {
    let mut b = Builder::new();
    let mut rng = Rng::new(37);
    let wood = [124, 90, 62];
    // The bench and its legs.
    cuboid(&mut b, v3(-0.8, -0.2, 0.4), v3(0.8, 0.3, 0.48), shade(wood, 0.95, 0));
    for (x, y) in [(-0.72f32, -0.12f32), (0.72, -0.12), (-0.72, 0.22), (0.72, 0.22)] {
        cuboid(&mut b, v3(x - 0.04, y - 0.04, 0.0), v3(x + 0.04, y + 0.04, 0.4), shade(wood, 0.7, 0));
    }
    // A half-cut block on the bench, and a mallet.
    cuboid(&mut b, v3(-0.35, -0.12, 0.48), v3(0.15, 0.2, 0.78), shade([196, 226, 250], 1.0, FLAG_GLOSSY));
    b.prism(v3(0.4, 0.0, 0.5), v3(1.0, 0.2, 0.6), 0.025, 0.35, 5, 0.0, shade(wood, 0.8, 0));
    // Finished shield slabs leaning in a row at the back.
    for k in 0..4 {
        let x = -0.75 + k as f32 * 0.5;
        let h = 0.8 + rng.f32() * 0.2;
        cuboid(&mut b, v3(x - 0.2, 0.6, 0.0), v3(x + 0.2, 0.7, h), shade([200, 230, 252], 0.9 + rng.f32() * 0.15, FLAG_GLOSSY));
    }
    pennant(&mut b, v3(0.9, -0.5, 0.0), 0.95);
    b.finish()
}

/// Sled track: two rails running away from the viewer with a sled waiting on them.
pub fn track() -> MeshData {
    let mut b = Builder::new();
    let wood = [124, 90, 62];
    for x in [-0.4f32, 0.4] {
        cuboid(&mut b, v3(x - 0.05, -0.95, 0.0), v3(x + 0.05, 0.95, 0.1), shade(wood, 0.8, 0));
    }
    // Sleepers.
    for k in 0..6 {
        let y = -0.8 + k as f32 * 0.32;
        cuboid(&mut b, v3(-0.55, y - 0.04, 0.0), v3(0.55, y + 0.04, 0.05), shade(wood, 0.65, 0));
    }
    // The sled: runners, a deck, and a curled front.
    for x in [-0.28f32, 0.28] {
        cuboid(&mut b, v3(x - 0.04, -0.5, 0.1), v3(x + 0.04, 0.2, 0.17), shade([70, 70, 80], 1.0, FLAG_GLOSSY));
        b.prism(v3(x, -0.5, 0.135), v3(0.0, -0.6, 0.8), 0.04, 0.22, 5, 0.0, shade([70, 70, 80], 1.0, FLAG_GLOSSY));
    }
    cuboid(&mut b, v3(-0.34, -0.45, 0.17), v3(0.34, 0.15, 0.24), shade(wood, 1.0, 0));
    // A snowy ramp at the far end.
    b.quad(v3(-0.55, 0.95, 0.0), v3(0.55, 0.95, 0.0), v3(0.55, 0.45, 0.4), v3(-0.55, 0.45, 0.4), [236, 242, 252, 0]);
    pennant(&mut b, v3(0.85, -0.8, 0.0), 0.95);
    b.finish()
}

/// Warming spa: a ring of stones around steaming water.
pub fn spa() -> MeshData {
    let mut b = Builder::new();
    let mut rng = Rng::new(41);
    let n = 12;
    for k in 0..n {
        let a = k as f32 / n as f32 * TAU;
        let c = v3(a.cos() * 0.62, a.sin() * 0.62, 0.12);
        let g = 0.5 + rng.f32() * 0.15;
        b.sphere(c, v3(0.17, 0.17, 0.13), shade([(150.0 * g) as u8 + 40, (145.0 * g) as u8 + 40, (150.0 * g) as u8 + 45], 1.0, 0), 6);
    }
    // Water, glowing a little.
    b.prism(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 0.6, 0.12, 14, 0.0, [96, 176, 220, FLAG_GLOSSY | FLAG_EMISSIVE]);
    // Steam puffs.
    for k in 0..4 {
        let (x, y, z) = (-0.15 + k as f32 * 0.1, 0.1 - (k % 2) as f32 * 0.2, 0.35 + k as f32 * 0.14);
        b.sphere(v3(x, y, z), v3(0.14, 0.14, 0.1), [244, 247, 252, 0], 6);
    }
    pennant(&mut b, v3(0.85, -0.85, 0.0), 0.95);
    b.finish()
}

/// Sling Range: a practice post, sling frame and distant target.
pub fn sling_range() -> MeshData {
    let mut b = Builder::new();
    let wood = [124, 90, 62];
    for x in [-0.62f32, 0.62] {
        b.prism(v3(x, -0.2, 0.0), v3(0.0, 0.0, 1.0), 0.05, 0.8, 5, 0.0, shade(wood, 0.8, 0));
    }
    b.prism(v3(-0.68, -0.2, 0.8), v3(1.36, 0.0, 0.0), 0.05, 1.36, 5, 0.0, shade(wood, 0.7, 0));
    b.prism(v3(0.0, -0.55, 0.45), v3(0.0, 0.45, 0.35), 0.025, 0.5, 4, 0.0, [85, 70, 58, 0]);
    b.prism(v3(0.0, 0.65, 0.0), v3(0.0, 0.0, 1.0), 0.035, 0.75, 5, 0.0, shade(wood, 0.85, 0));
    b.prism(v3(0.0, 0.65, 0.82), v3(0.0, -1.0, 0.0), 0.28, 0.05, 14, 0.0, [242, 243, 238, 0]);
    b.prism(v3(0.0, 0.59, 0.82), v3(0.0, -1.0, 0.0), 0.12, 0.02, 14, 0.0, [190, 48, 44, 0]);
    pennant(&mut b, v3(0.84, -0.45, 0.0), 0.9);
    b.finish()
}

/// Packer Yard: packed snowballs ready for heavy throwers.
pub fn packer_yard() -> MeshData {
    let mut b = Builder::new();
    let wood = [124, 90, 62];
    cuboid(&mut b, v3(-0.75, -0.62, 0.0), v3(0.75, 0.62, 0.16), shade(wood, 0.72, 0));
    for (x, y, r) in [(-0.38f32, -0.12f32, 0.27f32), (0.22, -0.12, 0.31), (-0.08, 0.28, 0.24)] {
        b.sphere(v3(x, y, 0.22 + r * 0.7), v3(r, r, r * 0.85), [238, 244, 252, 0], 10);
    }
    for x in [-0.72f32, 0.72] {
        b.prism(v3(x, -0.58, 0.0), v3(0.0, 0.0, 1.0), 0.04, 0.85, 5, 0.0, shade(wood, 0.8, 0));
    }
    pennant(&mut b, v3(0.82, 0.5, 0.0), 0.9);
    b.finish()
}

/// What marks out each kind of soldier, modelled around a penguin standing
/// at the origin facing -y (about 1.05 tall). None for plain snowballers.
pub fn prop(kind: sim::game::Kind) -> Option<MeshData> {
    use sim::game::Kind;
    let mut b = Builder::new();
    let team = [255, 255, 255, FLAG_TEAM];
    let wood = [112, 80, 54, 0];
    match kind {
        Kind::Slinger => {
            // Team headband with a feather sticking up at the back.
            b.surface(1, 16, |v, u| {
                let a = u * TAU;
                (v3(a.cos() * 0.215, a.sin() * 0.215, 0.88 + v * 0.06), team)
            });
            let base = v3(0.04, 0.2, 0.92);
            b.prism(base, v3(0.15, 0.5, 1.0), 0.05, 0.38, 4, 0.08, [250, 245, 235, 0]);
            // The sling, hanging from a flipper.
            b.prism(v3(0.36, -0.05, 0.55), v3(0.1, -0.2, -1.0), 0.012, 0.3, 4, 0.0, [130, 100, 70, 0]);
        }
        Kind::Packer => {
            // A huge snowball held up overhead.
            b.sphere(v3(0.0, -0.05, 1.32), v3(0.24, 0.24, 0.22), [250, 252, 255, 0], 12);
            b.surface(1, 12, |v, u| {
                let a = u * TAU;
                (v3(a.cos() * 0.13, a.sin() * 0.13, 1.08 + v * 0.06), team)
            });
        }
        Kind::Sledder => {
            // A plank sled under the belly, curling up at the front.
            let deck = |b: &mut Builder, y0: f32, y1: f32, z0: f32, z1: f32| {
                cuboid(b, v3(-0.22, y0, z0), v3(0.22, y1, z1), wood);
            };
            deck(&mut b, -0.45, 0.42, 0.04, 0.08);
            for x in [-0.2f32, 0.2] {
                cuboid(&mut b, v3(x - 0.025, -0.5, 0.0), v3(x + 0.025, 0.45, 0.04), [80, 60, 45, 0]);
                b.prism(v3(x, -0.5, 0.02), v3(0.0, -0.4, 1.0), 0.025, 0.2, 5, 0.0, [80, 60, 45, 0]);
            }
            cuboid(&mut b, v3(-0.24, 0.3, 0.08), v3(0.24, 0.42, 0.12), team);
        }
        Kind::Skier => {
            // Long skis underfoot and poles in both flippers.
            for x in [-0.16f32, 0.16] {
                b.prism(v3(x, -0.55, 0.02), v3(0.0, 1.0, 0.0), 0.025, 1.0, 5, 0.08, [170, 205, 225, 0]);
                b.prism(v3(x * 2.0, -0.2, 0.0), v3(0.0, 0.0, 1.0), 0.012, 0.9, 4, 0.0, wood);
            }
            b.surface(1, 16, |v, u| {
                let a = u * TAU;
                (v3(a.cos() * 0.215, a.sin() * 0.215, 0.88 + v * 0.06), team)
            });
        }
        Kind::Shield => {
            // A round shield of clear ice on the left flipper, team-coloured rim.
            let c = v3(-0.3, -0.22, 0.48);
            let (r, depth) = (0.3, 0.05);
            let rim = |a: f32, rr: f32, y: f32| c + v3(a.cos() * rr * 0.55, y, a.sin() * rr);
            for k in 0..16 {
                let (a0, a1) = (k as f32 / 16.0 * TAU, (k + 1) as f32 / 16.0 * TAU);
                b.tri(c + v3(0.0, -depth - 0.03, 0.0), rim(a0, r, -depth), rim(a1, r, -depth), [180, 225, 250, FLAG_GLOSSY]);
                b.tri(c, rim(a1, r, 0.0), rim(a0, r, 0.0), [150, 200, 235, FLAG_GLOSSY]);
                b.quad(rim(a0, r, -depth), rim(a0, r, 0.0), rim(a1, r, 0.0), rim(a1, r, -depth), team);
            }
        }
        Kind::Chieftain => {
            // A gold crown, and the golden ear tufts of an emperor penguin.
            let gold = [238, 198, 92, 0];
            let deep = [196, 150, 46, 0];
            let (r, z0, z1) = (0.235f32, 0.86f32, 0.95f32);
            let n = 16;
            let ring = |a: f32, r: f32, z: f32| v3(a.cos() * r, a.sin() * r, z);
            for i in 0..n {
                let (a0, a1) = (i as f32 / n as f32 * TAU, (i + 1) as f32 / n as f32 * TAU);
                b.quad(ring(a0, r, z0), ring(a1, r, z0), ring(a1, r, z1), ring(a0, r, z1), gold);
                b.quad(ring(a0, r, z1), ring(a1, r, z1), ring(a1, r - 0.05, z1 + 0.01), ring(a0, r - 0.05, z1 + 0.01), deep);
                b.quad(ring(a0, r - 0.05, z0), ring(a1, r - 0.05, z0), ring(a1, r, z0), ring(a0, r, z0), deep);
            }
            // Points round the band, the tall one at the front.
            for k in 0..7 {
                let a = k as f32 / 7.0 * TAU;
                let d = v3(a.cos(), a.sin(), 0.0);
                let h = if a.sin() < -0.5 { 0.16 } else { 0.1 };
                b.prism(d * r + v3(0.0, 0.0, z1 - 0.03), v3(0.0, 0.0, 1.0), 0.05, h, 4, 0.07, gold);
            }
            // A gem in the brow, in the tribe's colour.
            b.sphere(v3(0.0, -r, z0 + 0.02), v3(0.05, 0.04, 0.045), team, 8);
            // Golden ear patches.
            for side in [-1.0f32, 1.0] {
                b.sphere(v3(side * 0.175, -0.03, 0.79), v3(0.03, 0.05, 0.09), gold, 8);
            }
        }
        Kind::Warmer => {
            // A pink blowdryer held out in front, and a team-coloured nurse's cap.
            let pink = [240, 120, 170, FLAG_GLOSSY];
            let dark = [70, 60, 70, 0];
            let nozzle = v3(0.24, -0.42, 0.62);
            // Body of the dryer pointing forward, nozzle out front.
            b.prism(v3(0.24, -0.16, 0.62), v3(0.0, -1.0, 0.0), 0.09, 0.26, 10, 0.0, pink);
            b.sphere(v3(0.24, -0.15, 0.62), v3(0.1, 0.06, 0.1), pink, 10);
            b.prism(v3(0.24, -0.4, 0.62), v3(0.0, -1.0, 0.0), 0.06, 0.06, 10, 0.0, dark);
            b.sphere(nozzle, v3(0.05, 0.015, 0.05), dark, 8);
            // The handle, down into the flipper.
            b.prism(v3(0.24, -0.2, 0.6), v3(0.1, 0.35, -1.0), 0.04, 0.2, 6, 0.0, pink);
            // A little cap.
            b.surface(1, 16, |v, u| {
                let a = u * TAU;
                (v3(a.cos() * 0.2, a.sin() * 0.2, 0.92 + v * 0.07), [250, 250, 250, 0])
            });
            b.prism(v3(0.0, -0.2, 0.96), v3(0.0, -0.3, 1.0), 0.035, 0.05, 6, 0.0, team);
        }
        Kind::Worker | Kind::Snowballer => return None,
    }
    Some(b.finish())
}

/// What's left of a felled pine: a short stump with a snowy, ringed top.
pub fn stump(seed: u64) -> MeshData {
    let mut b = Builder::new();
    let mut rng = Rng::new(seed * 13 + 5);
    let bark = [92, 64, 46];
    let h = rng.range(0.2, 0.3);
    let r = 0.17;
    b.prism(v3(0.0, 0.0, -0.05), v3(0.0, 0.0, 1.0), r, h + 0.05, 7, 0.0, shade(bark, rng.range(0.9, 1.05), 0));
    // Pale cut face, half dusted with snow.
    let top = |i: usize| {
        let a = i as f32 / 7.0 * TAU;
        v3(a.cos() * r, a.sin() * r, h + 0.002)
    };
    let c = v3(0.0, 0.0, h + 0.002);
    for i in 0..7 {
        let col = if i < 3 { shade([238, 244, 252], 1.0, 0) } else { shade([196, 160, 112], rng.range(0.95, 1.05), 0) };
        b.tri(c, top(i), top((i + 1) % 7), col);
    }
    // A few splinters of the trunk lying about.
    let a = rng.range(0.0, TAU);
    b.prism(v3(a.cos() * 0.24, a.sin() * 0.24, 0.02), v3(a.cos(), a.sin(), 0.08), 0.05, 0.3, 5, 0.03, shade(bark, 0.85, 0));
    b.finish()
}

/// A log carried home by a worker; lies along x.
pub fn log() -> MeshData {
    let mut b = Builder::new();
    b.prism(v3(-0.28, 0.0, 0.0), v3(1.0, 0.0, 0.0), 0.075, 0.56, 7, 0.0, shade([104, 72, 50], 1.0, 0));
    // Cut ends.
    for (x, dir) in [(-0.28f32, -1.0f32), (0.28, 1.0)] {
        let ring = |i: usize| {
            let a = i as f32 / 7.0 * TAU * dir;
            v3(x, a.cos() * 0.075, a.sin() * 0.075)
        };
        for i in 0..7 {
            b.tri(v3(x, 0.0, 0.0), ring(i), ring((i + 1) % 7), shade([200, 166, 118], 1.0, 0));
        }
    }
    b.finish()
}

/// A leopard seal surfaced on the ice: long grey-spotted body along -y
/// (head at -y), a big jaw hanging open, small flippers.
pub fn seal() -> MeshData {
    let mut b = Builder::new();
    let hide = [121, 126, 134];
    let belly = [205, 208, 212];
    let dark = [88, 94, 104];
    let gum = [126, 96, 104];
    let teeth = [240, 240, 236];
    // Body: a stretched torso, lighting on the belly side.
    b.sphere(v3(0.0, 0.18, 0.24), v3(0.26, 0.5, 0.26), shade(belly, 1.0, 0), 10);
    b.sphere(v3(0.0, 0.18, 0.34), v3(0.24, 0.48, 0.24), shade(hide, 1.0, 0), 10);
    // Neck and head, jutting forward.
    b.sphere(v3(0.0, -0.42, 0.34), v3(0.19, 0.24, 0.19), shade(hide, 1.05, 0), 9);
    b.sphere(v3(0.0, -0.62, 0.4), v3(0.11, 0.18, 0.09), shade(dark, 1.0, 0), 8);
    // The big lower jaw, wide open: a shallow trough holding fangs.
    cuboid(&mut b, v3(-0.14, -0.78, 0.12), v3(0.14, -0.16, 0.2), shade(gum, 1.0, 0));
    b.sphere(v3(0.0, -0.45, 0.24), v3(0.13, 0.3, 0.05), shade(gum, 0.9, 0), 7);
    for (y, x) in [(-0.68f32, 0.1), (-0.52, -0.1), (-0.66, -0.1), (-0.5, 0.1), (-0.74, 0.06), (-0.76, -0.07)] {
        // Upper fangs from the muzzle, lower fangs out of the jaw.
        let drop = if y < -0.6 { 0.14 } else { 0.05 };
        cuboid(&mut b, v3(x - 0.02, y - 0.02, 0.36), v3(x + 0.02, y + 0.02, 0.36 - drop), shade(teeth, 0.92, 0));
        cuboid(&mut b, v3(x - 0.02, y - 0.02, 0.2), v3(x + 0.02, y + 0.02, 0.2 + drop * 0.7), shade(teeth, 1.0, 0));
    }
    // Grey mottling across the back.
    let spots = [(0.0, -0.3), (0.08, -0.05), (-0.09, -0.12), (0.05, 0.2), (-0.07, 0.3), (0.02, 0.48), (-0.04, 0.6), (0.06, 0.4)];
    for (dx, dy) in spots {
        b.sphere(v3(dx, dy, 0.42 + (dx.abs() + dy.abs()) * 0.12), v3(0.07, 0.09, 0.025), shade(dark, 1.0, 0), 6);
    }
    // Flippers, one tucked along each flank.
    for side in [-1.0f32, 1.0] {
        b.prism(v3(side * 0.24, 0.24, 0.16), v3(side * 0.14, 0.75, 0.02), 0.07, 0.52, 5, 0.1, shade(hide, 0.9, 0));
        b.prism(v3(side * 0.18, -0.36, 0.14), v3(side * 0.3, -0.55, 0.02), 0.05, 0.3, 5, 0.08, shade(hide, 0.95, 0));
    }
    // Blunt tail at the back.
    b.sphere(v3(0.0, 0.74, 0.16), v3(0.08, 0.16, 0.045), shade(dark, 0.95, 0), 6);
    b.finish()
}

/// Translucent block of ice for frozen penguins.
pub fn ice_block() -> MeshData {
    let mut b = Builder::new();
    let c = [190, 230, 255, FLAG_GLOSSY];
    let (x, y, z) = (0.42, 0.42, 1.15);
    let p = |i: f32, j: f32, k: f32| v3(i * x, j * y, k * z);
    // bottom/top and four sides (outward facing)
    b.quad(p(-1., -1., 1.), p(1., -1., 1.), p(1., 1., 1.), p(-1., 1., 1.), c);
    b.quad(p(-1., -1., 0.), p(-1., 1., 0.), p(1., 1., 0.), p(1., -1., 0.), c);
    b.quad(p(-1., -1., 0.), p(1., -1., 0.), p(1., -1., 1.), p(-1., -1., 1.), c);
    b.quad(p(1., 1., 0.), p(-1., 1., 0.), p(-1., 1., 1.), p(1., 1., 1.), c);
    b.quad(p(1., -1., 0.), p(1., 1., 0.), p(1., 1., 1.), p(1., -1., 1.), c);
    b.quad(p(-1., 1., 0.), p(-1., -1., 0.), p(-1., -1., 1.), p(-1., 1., 1.), c);
    b.finish()
}

#[cfg(test)]
mod tests {
    #[test]
    fn igloo_normals_are_sane() {
        for tier in 0..3u8 {
            let m = super::igloo_tier(tier);
            let mut bad = 0;
            for (i, v) in m.frames[0].iter().enumerate() {
                let l = (v[3] * v[3] + v[4] * v[4] + v[5] * v[5]).sqrt();
                if !(0.9..1.1).contains(&l) {
                    if bad < 5 {
                        println!("vert {i} pos {:?} normal {:?} len {l} color {:?}", &v[..3], &v[3..], m.colors[i]);
                    }
                    bad += 1;
                }
            }
            println!("{} verts, {} bad normals", m.frames[0].len(), bad);
            assert_eq!(bad, 0, "tier {tier}");
        }
    }
}
