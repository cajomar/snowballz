//! Static map data and the `.sbmap` file format.
//!
//! ```text
//! "SBMAP" 0x02   (0x01 files also carry a u8 region plane, which is skipped)
//! str name, str description            (u16 length + utf8)
//! u16 w, u16 h
//! u8 height[w*h]                         0..255 elevation
//! u8 ground[w*h]                         0 snow, 1 ice, 2 water
//! u8 obstacle[w*h]                       0 none, 1 tree, 2 ice pinnacle, 3 stump
//! u16 n, n * (u16 x, u16 y, u8 kind, u8 amount)   resources (0 fish, 1 crystal)
//! u8 n,  n * (u16 x, u16 y)                       igloos (top-left of 4x4)
//! u8 n,  n * (u8 kind, u16 x, u16 y)              powerups
//! u8 workers, u8 snowballers, u8 worker_warmth, u8 snowballer_warmth
//! u8 victory, u16 gather_amount (unused: every map is conquest now)
//! ```

use crate::math::{v2, Vec2};
use crate::wire::{Reader, Result, WireError, Writer};

/// World units (tiles) of elevation for a height value of 255.
pub const HEIGHT_SCALE: f32 = 3.0;
pub const IGLOO_SIZE: i32 = 4;
/// Highest starting warmth a map may ask for (warmth is a byte and grows).
const MAX_WARMTH_RULE: u8 = 100;
/// Most penguins a map may start with; ids are 16 bits.
const MAX_MAP_PENGUINS: usize = 8000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Ground {
    Snow = 0,
    Ice = 1,
    /// Open water: nobody walks on it, but workers fish from its shore.
    Water = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Obstacle {
    None = 0,
    Tree = 1,
    Pinnacle = 2,
    /// What's left of a chopped-down tree; walkable.
    Stump = 3,
}

impl Obstacle {
    pub fn from_u8(v: u8) -> Obstacle {
        match v {
            1 => Obstacle::Tree,
            2 => Obstacle::Pinnacle,
            3 => Obstacle::Stump,
            _ => Obstacle::None,
        }
    }
    /// True if penguins can't walk through it.
    pub fn blocks(self) -> bool {
        matches!(self, Obstacle::Tree | Obstacle::Pinnacle)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ResourceKind {
    Fish = 0,
    Crystal = 1,
}

impl ResourceKind {
    pub fn from_u8(v: u8) -> ResourceKind {
        if v == 1 {
            ResourceKind::Crystal
        } else {
            ResourceKind::Fish
        }
    }
}

#[derive(Clone, Debug)]
pub struct MapRules {
    pub workers: u8,
    pub snowballers: u8,
    pub worker_warmth: u8,
    pub snowballer_warmth: u8,
}

#[derive(Clone, Debug)]
pub struct ResourceSpawn {
    pub x: u16,
    pub y: u16,
    pub kind: ResourceKind,
    pub amount: u8,
}

#[derive(Clone, Debug)]
pub struct Map {
    pub name: String,
    pub description: String,
    pub w: i32,
    pub h: i32,
    pub height_raw: Vec<u8>,
    pub ground: Vec<Ground>,
    pub obstacle: Vec<Obstacle>,
    pub resources: Vec<ResourceSpawn>,
    pub igloos: Vec<(u16, u16)>,
    pub powerups: Vec<(u8, u16, u16)>,
    pub rules: MapRules,
    /// Smoothed per-tile elevation in world units, derived from `height_raw`.
    pub height: Vec<f32>,
}

impl Map {
    pub fn parse(b: &[u8]) -> Result<Map> {
        let mut r = Reader::new(b);
        let version = match r.raw(6)? {
            b"SBMAP\x01" => 1,
            b"SBMAP\x02" => 2,
            _ => return Err(WireError("not a snowballz map")),
        };
        let name = r.str()?;
        let description = r.str()?;
        let w = r.u16()? as i32;
        let h = r.u16()? as i32;
        let n = (w * h) as usize;
        if n == 0 || w > 1024 || h > 1024 {
            return Err(WireError("bad map size"));
        }
        let height_raw = r.raw(n)?.to_vec();
        let ground = r
            .raw(n)?
            .iter()
            .map(|&g| match g {
                1 => Ground::Ice,
                2 => Ground::Water,
                _ => Ground::Snow,
            })
            .collect();
        // Version 1 maps carried a territory plane; territories are gone.
        if version == 1 {
            r.raw(n)?;
        }
        let obstacle = r.raw(n)?.iter().map(|&o| Obstacle::from_u8(o)).collect();
        let mut resources = Vec::new();
        for _ in 0..r.u16()? {
            resources.push(ResourceSpawn {
                x: r.u16()?,
                y: r.u16()?,
                kind: ResourceKind::from_u8(r.u8()?),
                amount: r.u8()?,
            });
        }
        let mut igloos = Vec::new();
        for _ in 0..r.u8()? {
            igloos.push((r.u16()?, r.u16()?));
        }
        let mut powerups = Vec::new();
        for _ in 0..r.u8()? {
            powerups.push((r.u8()?, r.u16()?, r.u16()?));
        }
        let rules = MapRules {
            workers: r.u8()?,
            snowballers: r.u8()?,
            worker_warmth: r.u8()?.max(5),
            snowballer_warmth: r.u8()?.max(5),
        };
        // The old crystal-race victory; every map is conquest now.
        r.u8()?;
        r.u16()?;
        // Everything below indexes tile arrays, so a map that puts things
        // off the grid (or stuffs it with penguins) is refused here.
        let inside = |x: u16, y: u16, size: i32| x as i32 + size <= w && y as i32 + size <= h;
        if !resources.iter().all(|r| inside(r.x, r.y, 1))
            || !powerups.iter().all(|&(_, x, y)| inside(x, y, 1))
            || !igloos.iter().all(|&(x, y)| inside(x, y, IGLOO_SIZE))
        {
            return Err(WireError("map has things outside the grid"));
        }
        if rules.worker_warmth > MAX_WARMTH_RULE || rules.snowballer_warmth > MAX_WARMTH_RULE {
            return Err(WireError("map warmth out of range"));
        }
        if igloos.len() * (rules.workers as usize + rules.snowballers as usize) > MAX_MAP_PENGUINS {
            return Err(WireError("map starts too many penguins"));
        }
        let mut map = Map {
            name,
            description,
            w,
            h,
            height_raw,
            ground,
            obstacle,
            resources,
            igloos,
            powerups,
            rules,
            height: Vec::new(),
        };
        map.height = map.smooth_heights();
        Ok(map)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.raw(b"SBMAP\x02");
        w.str(&self.name).str(&self.description);
        w.u16(self.w as u16).u16(self.h as u16);
        w.raw(&self.height_raw);
        w.raw(&self.ground.iter().map(|&g| g as u8).collect::<Vec<_>>());
        w.raw(&self.obstacle.iter().map(|&o| o as u8).collect::<Vec<_>>());
        w.u16(self.resources.len() as u16);
        for r in &self.resources {
            w.u16(r.x).u16(r.y).u8(r.kind as u8).u8(r.amount);
        }
        w.u8(self.igloos.len() as u8);
        for &(x, y) in &self.igloos {
            w.u16(x).u16(y);
        }
        w.u8(self.powerups.len() as u8);
        for &(k, x, y) in &self.powerups {
            w.u8(k).u16(x).u16(y);
        }
        let ru = &self.rules;
        w.u8(ru.workers).u8(ru.snowballers);
        w.u8(ru.worker_warmth).u8(ru.snowballer_warmth);
        w.u8(0).u16(0);
        w.buf
    }

    /// The classic maps have stair-stepped elevation; blur it into hills.
    fn smooth_heights(&self) -> Vec<f32> {
        let mut h: Vec<f32> = self
            .height_raw
            .iter()
            .map(|&v| v as f32 / 255.0 * HEIGHT_SCALE)
            .collect();
        for _ in 0..3 {
            let src = h.clone();
            for y in 0..self.h {
                for x in 0..self.w {
                    let mut sum = 0.0;
                    let mut n = 0.0;
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let (xx, yy) = (x + dx, y + dy);
                            if self.in_bounds(xx, yy) {
                                sum += src[(yy * self.w + xx) as usize];
                                n += 1.0;
                            }
                        }
                    }
                    h[(y * self.w + x) as usize] = sum / n;
                }
            }
        }
        h
    }

    /// Fish holes are gone: every one a map still lists becomes a little
    /// pond (a plus of five water tiles) with banks to fish from, and is
    /// dropped from the resource list.
    pub fn holes_to_ponds(&mut self) {
        let holes: Vec<(i32, i32)> = self.resources.iter().filter(|r| r.kind == ResourceKind::Fish).map(|r| (r.x as i32, r.y as i32)).collect();
        if holes.is_empty() {
            return;
        }
        self.resources.retain(|r| r.kind != ResourceKind::Fish);
        for (x, y) in holes {
            for (dx, dy) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (x, y) = (x + dx, y + dy);
                if !self.in_bounds(x, y) || self.igloos.iter().any(|&(ix, iy)| (ix as i32..ix as i32 + 3).contains(&x) && (iy as i32..iy as i32 + 3).contains(&y)) {
                    continue;
                }
                let i = self.idx(x, y);
                self.ground[i] = Ground::Water;
                self.obstacle[i] = Obstacle::None;
                self.height_raw[i] = self.height_raw[i] / 3;
            }
        }
        self.height = self.smooth_heights();
    }

    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h
    }

    pub fn idx(&self, x: i32, y: i32) -> usize {
        (y * self.w + x) as usize
    }

    pub fn ground_at(&self, x: i32, y: i32) -> Ground {
        if self.in_bounds(x, y) {
            self.ground[self.idx(x, y)]
        } else {
            Ground::Snow
        }
    }

    pub fn obstacle_at(&self, x: i32, y: i32) -> Obstacle {
        if self.in_bounds(x, y) {
            self.obstacle[self.idx(x, y)]
        } else {
            Obstacle::None
        }
    }

    /// Height of the tile center at integer coordinates (clamped to the map).
    pub fn tile_height(&self, x: i32, y: i32) -> f32 {
        let x = x.clamp(0, self.w - 1);
        let y = y.clamp(0, self.h - 1);
        self.height[self.idx(x, y)]
    }

    /// Smooth ground height at a world position (bilinear between tile centers).
    pub fn height_at(&self, p: Vec2) -> f32 {
        let fx = p.x - 0.5;
        let fy = p.y - 0.5;
        let x0 = fx.floor();
        let y0 = fy.floor();
        let tx = fx - x0;
        let ty = fy - y0;
        let (x0, y0) = (x0 as i32, y0 as i32);
        let h00 = self.tile_height(x0, y0);
        let h10 = self.tile_height(x0 + 1, y0);
        let h01 = self.tile_height(x0, y0 + 1);
        let h11 = self.tile_height(x0 + 1, y0 + 1);
        let a = h00 + (h10 - h00) * tx;
        let b = h01 + (h11 - h01) * tx;
        a + (b - a) * ty
    }

    /// Height at a grid corner (used for terrain meshes).
    pub fn corner_height(&self, x: i32, y: i32) -> f32 {
        self.height_at(v2(x as f32, y as f32))
    }

    pub fn igloo_center(&self, i: usize) -> Vec2 {
        let (x, y) = self.igloos[i];
        let s = IGLOO_SIZE as f32 / 2.0;
        v2(x as f32 + s, y as f32 + s)
    }

    pub fn load_file(path: &std::path::Path) -> std::io::Result<Map> {
        let b = std::fs::read(path)?;
        Map::parse(&b).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

/// All `.sbmap` files in a directory, sorted by file name.
pub fn list_maps(dir: &std::path::Path) -> Vec<(String, std::path::PathBuf)> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("sbmap") {
                let stem = p.file_stem().unwrap().to_string_lossy().to_string();
                out.push((stem, p));
            }
        }
    }
    out.sort();
    out
}
