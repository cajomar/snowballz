//! Purely cosmetic client-side effects: snow puffs, footprints, smoke...

use sim::game::HitKind;
use sim::map::Map;
use sim::math::{v2, Vec2};
use sim::rng::Rng;

use crate::mathx::{v3, Vec3};

#[derive(Clone, Copy)]
pub struct Particle {
    pub pos: Vec3,
    pub vel: Vec3,
    pub life: f32,
    pub max: f32,
    pub size: f32,
    pub color: [u8; 4],
    pub gravity: f32,
    /// Grows over its lifetime (smoke).
    pub grow: f32,
    /// Shines brighter than white, so it blooms (embers).
    pub glow: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DecalKind {
    Footprint,
    Splat,
}

#[derive(Clone, Copy)]
pub struct Decal {
    pub pos: Vec2,
    pub angle: f32,
    pub size: f32,
    pub life: f32,
    pub max: f32,
    pub kind: DecalKind,
}

/// A ring spreading out over the ground: a War Cry, a Chieftain falling.
#[derive(Clone, Copy)]
pub struct Ring {
    pub pos: Vec2,
    /// How wide the ring is now.
    pub radius: f32,
    /// How wide it grows.
    pub max_radius: f32,
    pub life: f32,
    pub max: f32,
    pub color: [u8; 4],
}

pub struct Effects {
    pub particles: Vec<Particle>,
    pub decals: Vec<Decal>,
    pub rings: Vec<Ring>,
    rng: Rng,
}

const MAX_PARTICLES: usize = 5000;
const MAX_DECALS: usize = 2500;

impl Effects {
    pub fn new() -> Effects {
        Effects { particles: Vec::new(), decals: Vec::new(), rings: Vec::new(), rng: Rng::new(7) }
    }

    pub fn update(&mut self, dt: f32, map: &Map) {
        for p in &mut self.particles {
            p.life -= dt;
            p.vel.z -= p.gravity * dt;
            p.pos = p.pos + p.vel * dt;
            p.size += p.grow * dt;
            let ground = map.height_at(v2(p.pos.x, p.pos.y));
            if p.pos.z < ground && p.gravity > 0.0 {
                p.pos.z = ground;
                p.vel = p.vel * 0.3;
                p.vel.z = 0.0;
            }
        }
        self.particles.retain(|p| p.life > 0.0);
        for d in &mut self.decals {
            d.life -= dt;
        }
        self.decals.retain(|d| d.life > 0.0);
        for r in &mut self.rings {
            r.life -= dt;
            let t = 1.0 - (r.life / r.max).clamp(0.0, 1.0);
            r.radius = 0.4 + t * (r.max_radius - 0.4);
        }
        self.rings.retain(|r| r.life > 0.0);
    }

    fn add(&mut self, p: Particle) {
        if self.particles.len() < MAX_PARTICLES {
            self.particles.push(p);
        }
    }

    fn add_decal(&mut self, d: Decal) {
        if self.decals.len() >= MAX_DECALS {
            self.decals.remove(0);
        }
        self.decals.push(d);
    }

    pub fn impact(&mut self, pos: Vec2, z: f32, hit: HitKind, dir: Vec2) {
        let n = match hit {
            HitKind::Direct => 34,
            HitKind::Splash => 22,
            HitKind::Tree => 18,
            HitKind::Building => 16,
            HitKind::Miss => 10,
        };
        let back = dir.norm() * -1.0;
        for _ in 0..n {
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            let s = self.rng.range(0.5, 2.6);
            let spray = Vec2::from_angle(a) * s + back * self.rng.range(0.0, 1.5);
            let rise = if hit == HitKind::Tree { self.rng.range(0.5, 2.0) } else { self.rng.range(1.0, 3.5) };
            let start_z = match hit {
                HitKind::Tree => z + self.rng.range(0.4, 1.6),
                HitKind::Building => z + self.rng.range(0.2, 0.7),
                _ => z + 0.1,
            };
            let p = Particle {
                pos: v3(pos.x, pos.y, start_z),
                vel: v3(spray.x, spray.y, rise),
                life: self.rng.range(0.4, 0.9),
                max: 0.9,
                size: self.rng.range(0.05, 0.12),
                color: [250, 252, 255, 255],
                gravity: 9.0,
                grow: 0.0,
                glow: false,
            };
            self.add(p);
        }
        // A quick fat puff on top of the spray gives hits some punch.
        if matches!(hit, HitKind::Direct | HitKind::Splash) {
            let (n, size) = if hit == HitKind::Direct { (4, 0.2) } else { (2, 0.15) };
            for _ in 0..n {
                let a = self.rng.range(0.0, std::f32::consts::TAU);
                let drift = Vec2::from_angle(a) * self.rng.range(0.1, 0.7);
                let up = self.rng.range(0.3, 0.9);
                let p = Particle {
                    pos: v3(pos.x, pos.y, z + 0.25),
                    vel: v3(drift.x, drift.y, up),
                    life: 0.35,
                    max: 0.35,
                    size,
                    color: [245, 250, 255, 210],
                    gravity: 0.0,
                    grow: 0.9,
                    glow: false,
                };
                self.add(p);
            }
        }
        if !matches!(hit, HitKind::Tree | HitKind::Building) {
            let size = if hit == HitKind::Direct { 0.3 } else { 0.22 };
            let angle = self.rng.range(0.0, std::f32::consts::TAU);
            self.add_decal(Decal { pos, angle, size, life: 25.0, max: 25.0, kind: DecalKind::Splat });
        }
    }

    pub fn footprint(&mut self, pos: Vec2, angle: f32) {
        self.add_decal(Decal { pos, angle, size: 0.13, life: 18.0, max: 18.0, kind: DecalKind::Footprint });
    }

    /// Faint grey dust ring left where an igloo collapsed: small dust
    /// patches that fade out over 20 s. Reuses the footprint decal.
    pub fn dust_ring(&mut self, pos: Vec2, radius: f32) {
        for k in 0..12 {
            let a = k as f32 / 12.0 * std::f32::consts::TAU + self.rng.range(-0.1, 0.1);
            let r = radius + self.rng.range(-0.15, 0.15);
            let size = self.rng.range(0.2, 0.4);
            let at = pos + Vec2::from_angle(a) * r;
            self.add_decal(Decal { pos: at, angle: a, size, life: 20.0, max: 20.0, kind: DecalKind::Footprint });
        }
    }

    pub fn smoke(&mut self, pos: Vec3) {
        let p = Particle {
            pos: pos + v3(self.rng.range(-0.08, 0.08), self.rng.range(-0.08, 0.08), 0.0),
            vel: v3(self.rng.range(0.1, 0.35), self.rng.range(-0.1, 0.1), self.rng.range(0.6, 0.9)),
            life: 3.0,
            max: 3.0,
            size: 0.18,
            color: [205, 210, 220, 150],
            gravity: 0.0,
            grow: 0.25,
            glow: false,
        };
        self.add(p);
    }

    /// A penguin's breath, steaming in the cold air.
    pub fn breath(&mut self, pos: Vec3, dir: Vec2) {
        let p = Particle {
            pos,
            vel: v3(dir.x * 0.35, dir.y * 0.35, 0.18),
            life: 0.9,
            max: 0.9,
            size: 0.05,
            color: [235, 242, 255, 90],
            gravity: 0.0,
            grow: 0.22,
            glow: false,
        };
        self.add(p);
    }

    /// A puff of powder left hanging behind a snowball in flight.
    pub fn trail(&mut self, pos: Vec3, size: f32) {
        let p = Particle {
            pos: pos + v3(self.rng.range(-0.02, 0.02), self.rng.range(-0.02, 0.02), self.rng.range(-0.02, 0.02)),
            vel: v3(0.0, 0.0, 0.05),
            life: 0.28,
            max: 0.28,
            size: 0.05 * size,
            color: [240, 246, 255, 120],
            gravity: 0.0,
            grow: 0.12 * size,
            glow: false,
        };
        self.add(p);
    }

    /// A spark flying up from a fire, wandering as it rises and fading out.
    pub fn ember(&mut self, pos: Vec3) {
        let hot = self.rng.range(0.0, 1.0);
        let p = Particle {
            pos: pos + v3(self.rng.range(-0.15, 0.15), self.rng.range(-0.15, 0.15), 0.0),
            vel: v3(self.rng.range(-0.25, 0.25), self.rng.range(-0.25, 0.25), self.rng.range(0.9, 1.8)),
            life: self.rng.range(0.6, 1.4),
            max: 1.4,
            size: self.rng.range(0.025, 0.045),
            color: [255, (140.0 + 90.0 * hot) as u8, (40.0 + 60.0 * hot) as u8, 255],
            gravity: -0.3,
            grow: -0.012,
            glow: true,
        };
        self.add(p);
    }

    /// Water and ice chips kicked up as a seal breaks the surface.
    pub fn splash(&mut self, pos: Vec2, z: f32) {
        for _ in 0..26 {
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            let s = self.rng.range(0.6, 3.4);
            let p = Particle {
                pos: v3(pos.x, pos.y, z + 0.1),
                vel: v3(a.cos() * s, a.sin() * s, self.rng.range(1.8, 4.8)),
                life: self.rng.range(0.5, 1.1),
                max: 1.1,
                size: self.rng.range(0.06, 0.16),
                color: [215, 235, 248, 255],
                gravity: 10.0,
                grow: 0.0,
                glow: false,
            };
            self.add(p);
        }
    }

    pub fn sparkle(&mut self, pos: Vec3, color: [u8; 3], n: usize) {
        for _ in 0..n {
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            let s = self.rng.range(0.3, 2.0);
            let p = Particle {
                pos,
                vel: v3(a.cos() * s, a.sin() * s, self.rng.range(1.5, 4.5)),
                life: self.rng.range(0.8, 1.6),
                max: 1.6,
                size: self.rng.range(0.06, 0.14),
                color: [color[0], color[1], color[2], 255],
                gravity: 4.0,
                grow: 0.0,
                glow: false,
            };
            self.add(p);
        }
    }

    /// A few warm wisps blown out of a Warmer's blowdryer along `dir`.
    pub fn hot_air(&mut self, pos: Vec3, dir: Vec2) {
        for _ in 0..3 {
            let s = self.rng.range(1.2, 2.2);
            let side = self.rng.range(-0.35, 0.35);
            let p = Particle {
                pos,
                vel: v3((dir.x - dir.y * side) * s, (dir.y + dir.x * side) * s, self.rng.range(0.2, 0.8)),
                life: self.rng.range(0.35, 0.6),
                max: 0.6,
                size: self.rng.range(0.05, 0.09),
                color: [255, self.rng.range(170.0, 230.0) as u8, 150, 200],
                gravity: -0.5,
                grow: 0.6,
                glow: false,
            };
            self.add(p);
        }
    }

    /// Ice shards and a puff of frost when a penguin freezes solid.
    pub fn frost_burst(&mut self, pos: Vec3) {
        for k in 0..28 {
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            let s = self.rng.range(1.2, 4.0);
            let rise = self.rng.range(1.0, 4.0);
            let life = self.rng.range(0.5, 1.0);
            let size = self.rng.range(0.05, 0.11);
            let white = k % 3 == 0;
            let p = Particle {
                pos,
                vel: v3(a.cos() * s, a.sin() * s, rise),
                life,
                max: 1.0,
                size,
                color: if white { [255, 255, 255, 255] } else { [150, 215, 255, 255] },
                gravity: 7.0,
                grow: 0.0,
                glow: false,
            };
            self.add(p);
        }
        for _ in 0..3 {
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            let rise = self.rng.range(0.4, 1.0);
            let p = Particle {
                pos,
                vel: v3(a.cos() * 0.5, a.sin() * 0.5, rise),
                life: 0.5,
                max: 0.5,
                size: 0.2,
                color: [190, 230, 255, 170],
                gravity: 0.0,
                grow: 1.0,
                glow: false,
            };
            self.add(p);
        }
    }

    /// A ring of small warm-yellow sparks that rise and fade: a penguin thaws.
    pub fn thaw_sparkle(&mut self, pos: Vec3) {
        for k in 0..20 {
            let a = k as f32 / 20.0 * std::f32::consts::TAU + self.rng.range(-0.08, 0.08);
            let s = self.rng.range(0.1, 0.4);
            let p = Particle {
                pos: pos + v3(a.cos() * 0.4, a.sin() * 0.4, self.rng.range(-0.05, 0.05)),
                vel: v3(a.cos() * s, a.sin() * s, self.rng.range(0.7, 1.4)),
                life: 0.8,
                max: 0.8,
                size: self.rng.range(0.05, 0.09),
                color: [255, 215, 115, 255],
                gravity: -0.4,
                grow: 0.0,
                glow: false,
            };
            self.add(p);
        }
    }

    /// A ring of the new owner's colour flung out and up, for a captured igloo.
    pub fn capture_burst(&mut self, pos: Vec3, color: [u8; 3]) {
        for k in 0..60 {
            let a = k as f32 / 60.0 * std::f32::consts::TAU + self.rng.range(-0.05, 0.05);
            let s = self.rng.range(3.5, 6.0);
            let rise = self.rng.range(2.0, 5.0);
            let life = self.rng.range(0.9, 1.5);
            let size = self.rng.range(0.1, 0.18);
            let tint = if k % 4 == 0 { [255, 255, 255] } else { color };
            let p = Particle {
                pos,
                vel: v3(a.cos() * s, a.sin() * s, rise),
                life,
                max: 1.5,
                size,
                color: [tint[0], tint[1], tint[2], 255],
                gravity: 5.0,
                grow: 0.0,
                glow: false,
            };
            self.add(p);
        }
    }

    /// An igloo caves in: a shower of snow blocks, then drifting puffs.
    pub fn collapse(&mut self, pos: Vec3) {
        for k in 0..46 {
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            let s = self.rng.range(0.8, 3.6);
            let life = self.rng.range(0.6, 1.3);
            let p = Particle {
                pos: pos + v3(a.cos() * 0.6, a.sin() * 0.6, self.rng.range(-0.5, 0.5)),
                vel: v3(a.cos() * s, a.sin() * s, self.rng.range(0.5, 3.8)),
                life,
                max: 1.3,
                size: self.rng.range(0.07, 0.18),
                color: if k % 4 == 0 { [208, 220, 238, 255] } else { [252, 253, 255, 255] },
                gravity: 8.0,
                grow: 0.0,
                glow: false,
            };
            self.add(p);
        }
        // Pulverized snow puffs that rise and fatten.
        for k in 0..8 {
            let a = k as f32 / 8.0 * std::f32::consts::TAU + self.rng.range(-0.2, 0.2);
            let drift = Vec2::from_angle(a) * self.rng.range(0.3, 1.1);
            let p = Particle {
                pos: pos + v3(a.cos() * 0.4, a.sin() * 0.4, 0.2),
                vel: v3(drift.x, drift.y, self.rng.range(0.4, 1.0)),
                life: 1.4,
                max: 1.4,
                size: self.rng.range(0.3, 0.5),
                color: [238, 244, 252, 170],
                gravity: -0.2,
                grow: 0.5,
                glow: false,
            };
            self.add(p);
        }
    }

    /// A War Cry: a ring of sound racing out over the snow.
    pub fn shout(&mut self, pos: Vec2, color: [u8; 3], radius: f32) {
        self.rings.push(Ring {
            pos,
            radius: 0.4,
            max_radius: radius,
            life: 0.9,
            max: 0.9,
            color: [color[0], color[1], color[2], 230],
        });
    }

    /// A Chieftain falls: a dark ring and a shower of snow.
    pub fn fall(&mut self, pos: Vec2) {
        self.rings.push(Ring {
            pos,
            radius: 0.4,
            max_radius: 7.0,
            life: 1.2,
            max: 1.2,
            color: [255, 90, 80, 200],
        });
        let z = 0.4;
        self.sparkle(v3(pos.x, pos.y, z), [210, 225, 245], 40);
    }
}
