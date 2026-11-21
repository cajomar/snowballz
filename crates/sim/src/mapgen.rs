//! Maps from a short description. A language model (or the keyword fallback,
//! or a person) fills in a [`MapSpec`]: size, igloo layout, how hilly, icy
//! and wooded, and a few placed features. [`generate`] turns that into a
//! playable [`Map`]: every igloo gets the same clearing and fish, and every
//! igloo, fish hole and crystal can be walked to. Deterministic per seed.

use crate::json::{quote, Json};
use crate::map::*;
use crate::math::{v2, Vec2};
use crate::rng::Rng;

pub const MIN_SIZE: i32 = 64;
pub const MAX_SIZE: i32 = 160;
pub const MAX_IGLOOS: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layout {
    /// Igloos in a circle around the middle.
    Ring,
    /// Igloos in the corners (and along the edges if there are many).
    Corners,
    /// Two rows facing each other across the map.
    Sides,
    /// Spread out irregularly.
    Scattered,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FeatureKind {
    Hill,
    Crater,
    Ridge,
    Lake,
    Forest,
    Spires,
    Clearing,
    Crystal,
    /// A snowy landmass ringed by ice.
    Island,
    /// A raised rim around a warm, bare, snowy middle.
    Volcano,
    /// Open water: a bay or inlet workers can fish from the bank.
    Water,
}

/// Something placed on purpose: `x`, `y` and `size` are fractions of the map.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feature {
    pub kind: FeatureKind,
    pub x: f32,
    pub y: f32,
    pub size: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapSpec {
    pub name: String,
    pub description: String,
    pub size: i32,
    pub igloos: usize,
    pub layout: Layout,
    /// 0..1 each.
    pub hills: f32,
    pub ice: f32,
    pub trees: f32,
    pub spires: f32,
    pub features: Vec<Feature>,
    pub workers: u8,
    pub snowballers: u8,
    pub seed: u64,
}

impl Default for MapSpec {
    fn default() -> MapSpec {
        MapSpec {
            name: "New Map".into(),
            description: String::new(),
            size: 120,
            igloos: 4,
            layout: Layout::Ring,
            hills: 0.4,
            ice: 0.2,
            trees: 0.3,
            spires: 0.1,
            features: Vec::new(),
            workers: 8,
            snowballers: 10,
            seed: 1,
        }
    }
}

/// What a language model is told about the spec. Keep in step with
/// [`MapSpec::from_json`].
pub const SPEC_GUIDE: &str = r#"Describe a map for Snowballz, a real-time strategy snowball fight between penguin tribes, as one JSON object:
{
  "name": "short evocative map name (max 24 chars)",
  "description": "one or two sentences shown in the lobby",
  "size": 64-160 (tiles per side; 100 small, 128 medium, 150 large),
  "igloos": 2-10 (one per player; extra igloos start neutral),
  "layout": "ring" | "corners" | "sides" | "scattered",
  "hills": 0-1, "ice": 0-1 (frozen lakes, fast to cross, no snow for ammo),
  "trees": 0-1 (forest cover that blocks snowballs and gives wood),
  "spires": 0-1 (ice pinnacles that block movement),
  "features": [ {"kind": "hill"|"crater"|"ridge"|"lake"|"forest"|"spires"|"clearing"|"crystal"|"island"|"volcano"|"water",
                 "x": 0-1, "y": 0-1, "size": 0-1} ... up to 12 ],
  "workers": 4-15, "snowballers": 4-15 (starting penguins per igloo)
}
x and y are fractions across and down the map; (0.5, 0.5) is the middle. Every match is won by taking all enemy igloos. Crystal is a rare mined resource that pays for the grandest igloos; \"crystal\" features say where it lies (otherwise it is spread through contested ground). \"island\" is a raised snowy landmass ringed by ice, and \"volcano\" is a crater with a raised rim around a warm bare middle with no ice, and \"water\" is open water that penguins fish from the bank (it can't be walked on, so keep it off the igloos). Scattered layouts with high ice make each igloo an island in a frozen sea. Use features to make the map match the request and feel distinct. Reply with the JSON object only."#;

fn clamp01(v: f64) -> f32 {
    (v as f32).clamp(0.0, 1.0)
}

impl MapSpec {
    /// Read a spec, forgiving anything missing or out of range.
    pub fn from_json(j: &Json, seed: u64) -> MapSpec {
        let d = MapSpec::default();
        let num = |k: &str| j.get(k).and_then(Json::as_f64);
        let text = |k: &str, max: usize| -> Option<String> {
            let s: String = j.get(k)?.as_str()?.trim().chars().filter(|c| !c.is_control()).take(max).collect();
            (!s.is_empty()).then_some(s)
        };
        let layout = match j.get("layout").and_then(Json::as_str).unwrap_or("").to_ascii_lowercase().as_str() {
            "corners" => Layout::Corners,
            "sides" | "rows" | "line" | "lines" => Layout::Sides,
            "scattered" | "random" => Layout::Scattered,
            _ => Layout::Ring,
        };
        let features = j
            .get("features")
            .map(Json::as_arr)
            .unwrap_or(&[])
            .iter()
            .take(12)
            .filter_map(|f| {
                let kind = match f.get("kind")?.as_str()?.to_ascii_lowercase().as_str() {
                    "hill" | "hills" | "mountain" => FeatureKind::Hill,
                    "crater" | "valley" | "bowl" => FeatureKind::Crater,
                    "ridge" => FeatureKind::Ridge,
                    "lake" | "ice" | "pond" => FeatureKind::Lake,
                    "forest" | "trees" | "wood" => FeatureKind::Forest,
                    "spires" | "pinnacles" | "rocks" => FeatureKind::Spires,
                    "clearing" | "plain" => FeatureKind::Clearing,
                    "crystal" | "crystals" => FeatureKind::Crystal,
                    "island" | "isle" | "landmass" => FeatureKind::Island,
                    "volcano" | "volcanic" | "vent" => FeatureKind::Volcano,
                    "water" | "sea" | "bay" | "ocean" | "inlet" => FeatureKind::Water,
                    _ => return None,
                };
                let g = |k: &str, def: f64| clamp01(f.get(k).and_then(Json::as_f64).unwrap_or(def));
                Some(Feature { kind, x: g("x", 0.5), y: g("y", 0.5), size: g("size", 0.3) })
            })
            .collect();
        MapSpec {
            name: text("name", 24).unwrap_or(d.name),
            description: text("description", 200).unwrap_or_default(),
            size: num("size").map_or(d.size, |v| (v as i32).clamp(MIN_SIZE, MAX_SIZE)),
            igloos: num("igloos").map_or(d.igloos, |v| (v as usize).clamp(2, MAX_IGLOOS)),
            layout,
            hills: num("hills").map_or(d.hills, clamp01),
            ice: num("ice").map_or(d.ice, clamp01),
            trees: num("trees").map_or(d.trees, clamp01),
            spires: num("spires").map_or(d.spires, clamp01),
            features,
            workers: num("workers").map_or(d.workers, |v| v.clamp(2.0, 20.0) as u8),
            snowballers: num("snowballers").map_or(d.snowballers, |v| v.clamp(2.0, 20.0) as u8),
            seed,
        }
    }

    /// No model to ask: pick a spec from the words in the request.
    pub fn from_words(prompt: &str, seed: u64) -> MapSpec {
        let p = prompt.to_ascii_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| p.contains(w));
        let mut s = MapSpec { seed, ..MapSpec::default() };
        let mut rng = Rng::new(seed);
        s.hills = rng.range(0.2, 0.5);
        s.ice = rng.range(0.1, 0.3);
        s.trees = rng.range(0.1, 0.35);
        s.spires = rng.range(0.0, 0.15);
        if has(&["forest", "wood", "tree", "pine", "taiga"]) {
            s.trees = 0.75;
        }
        if has(&["ice", "frozen", "lake", "glacier", "slippery", "sea"]) {
            s.ice = 0.65;
        }
        if has(&["mountain", "hill", "peak", "highland", "cliff"]) {
            s.hills = 0.85;
        }
        if has(&["flat", "plain", "tundra", "open"]) {
            s.hills = 0.08;
            s.trees = s.trees.min(0.1);
        }
        if has(&["spire", "pinnacle", "rock", "maze", "spike"]) {
            s.spires = 0.6;
        }
        if has(&["crystal", "gem", "mine"]) {
            s.features.push(Feature { kind: FeatureKind::Crystal, x: 0.5, y: 0.5, size: 0.1 });
        }
        if has(&["crater", "bowl", "arena", "colosseum"]) {
            s.features.push(Feature { kind: FeatureKind::Crater, x: 0.5, y: 0.5, size: 0.45 });
        }
        if has(&["volcano", "volcanic"]) {
            s.features.push(Feature { kind: FeatureKind::Volcano, x: 0.5, y: 0.5, size: 0.4 });
        }
        if has(&["island", "archipelago"]) {
            s.ice = 0.8;
            s.layout = Layout::Scattered;
            s.features.push(Feature { kind: FeatureKind::Island, x: 0.5, y: 0.5, size: 0.3 });
        }
        if has(&["duel", "1v1", "two player", "2 player"]) {
            s.igloos = 2;
            s.layout = Layout::Sides;
            s.size = 96;
        }
        if has(&["small", "tiny", "quick"]) {
            s.size = 96;
        }
        if has(&["huge", "large", "big", "epic"]) {
            s.size = 150;
            s.igloos = s.igloos.max(6);
        }
        if let Some(n) = p
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|w| w.parse::<usize>().ok())
            .find(|&n| (2..=MAX_IGLOOS).contains(&n))
        {
            s.igloos = n;
        }
        if has(&["corner"]) {
            s.layout = Layout::Corners;
        }
        s.name = title(prompt);
        s.description = format!("Made from: \"{}\"", prompt.trim().chars().take(120).collect::<String>());
        s
    }

    /// The spec as JSON, in the shape [`SPEC_GUIDE`] describes.
    pub fn to_json(&self) -> String {
        let layout = match self.layout {
            Layout::Ring => "ring",
            Layout::Corners => "corners",
            Layout::Sides => "sides",
            Layout::Scattered => "scattered",
        };
        let features: Vec<String> = self
            .features
            .iter()
            .map(|f| {
                let k = format!("{:?}", f.kind).to_ascii_lowercase();
                format!("{{\"kind\":\"{k}\",\"x\":{:.2},\"y\":{:.2},\"size\":{:.2}}}", f.x, f.y, f.size)
            })
            .collect();
        format!(
            "{{\"name\":{},\"description\":{},\"size\":{},\"igloos\":{},\"layout\":\"{layout}\",\"hills\":{:.2},\"ice\":{:.2},\"trees\":{:.2},\"spires\":{:.2},\"features\":[{}],\"workers\":{},\"snowballers\":{}}}",
            quote(&self.name),
            quote(&self.description),
            self.size,
            self.igloos,
            self.hills,
            self.ice,
            self.trees,
            self.spires,
            features.join(","),
            self.workers,
            self.snowballers
        )
    }
}

/// A map name from the first few words of a request.
fn title(prompt: &str) -> String {
    let skip = ["a", "an", "the", "with", "map", "of", "and", "make", "me", "please", "some", "lots"];
    let words: Vec<String> = prompt
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !skip.contains(&w.to_ascii_lowercase().as_str()))
        .take(3)
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + &c.as_str().to_ascii_lowercase())
        })
        .collect();
    if words.is_empty() {
        "New Map".into()
    } else {
        words.join(" ").chars().take(24).collect()
    }
}

/// Smooth random field in 0..1 with features about `cell` tiles across.
fn value_noise(rng: &mut Rng, w: i32, h: i32, cell: f32) -> Vec<f32> {
    let gw = (w as f32 / cell).ceil() as i32 + 2;
    let gh = (h as f32 / cell).ceil() as i32 + 2;
    let grid: Vec<f32> = (0..gw * gh).map(|_| rng.f32()).collect();
    let mut out = vec![0.0; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / cell, y as f32 / cell);
            let (x0, y0) = (fx as i32, fy as i32);
            let s = |t: f32| t * t * (3.0 - 2.0 * t);
            let (tx, ty) = (s(fx - x0 as f32), s(fy - y0 as f32));
            let g = |i: i32, j: i32| grid[(j * gw + i) as usize];
            let a = g(x0, y0) + (g(x0 + 1, y0) - g(x0, y0)) * tx;
            let b = g(x0, y0 + 1) + (g(x0 + 1, y0 + 1) - g(x0, y0 + 1)) * tx;
            out[(y * w + x) as usize] = a + (b - a) * ty;
        }
    }
    out
}

fn fractal(rng: &mut Rng, w: i32, h: i32, cell: f32) -> Vec<f32> {
    let mut out = vec![0.0; (w * h) as usize];
    let mut total = 0.0;
    for (k, amp) in [1.0, 0.5, 0.25].into_iter().enumerate() {
        let n = value_noise(rng, w, h, cell / (1 << k) as f32);
        for (o, v) in out.iter_mut().zip(n) {
            *o += v * amp;
        }
        total += amp;
    }
    out.iter_mut().for_each(|v| *v /= total);
    out
}

/// The value below which `frac` of `vals` lie.
fn quantile(vals: &[f32], frac: f32) -> f32 {
    let mut v = vals.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f32 * frac.clamp(0.0, 1.0)) as usize]
}

/// Igloo centres (in tiles) for a layout.
fn place_igloos(spec: &MapSpec, rng: &mut Rng) -> Vec<Vec2> {
    let s = spec.size as f32;
    let n = spec.igloos;
    let margin = 9.0 / s;
    let mut pts: Vec<Vec2> = match spec.layout {
        Layout::Ring => {
            let a0 = rng.range(0.0, std::f32::consts::TAU);
            (0..n).map(|i| {
                let a = a0 + i as f32 * std::f32::consts::TAU / n as f32;
                v2(0.5, 0.5) + Vec2::from_angle(a) * 0.36
            }).collect()
        }
        Layout::Corners => {
            let mut c = vec![v2(0.14, 0.14), v2(0.86, 0.86), v2(0.86, 0.14), v2(0.14, 0.86), v2(0.5, 0.12), v2(0.5, 0.88), v2(0.12, 0.5), v2(0.88, 0.5), v2(0.32, 0.32), v2(0.68, 0.68)];
            c.truncate(n);
            c
        }
        Layout::Sides => (0..n)
            .map(|i| {
                let row = i % 2;
                let k = i / 2;
                let per_row = (n + 1 - row) / 2;
                v2((k as f32 + 0.5) / per_row as f32, if row == 0 { 0.14 } else { 0.86 })
            })
            .collect(),
        Layout::Scattered => {
            let mut pts: Vec<Vec2> = Vec::new();
            for _ in 0..n {
                // Best of a few candidates: the one farthest from the rest.
                let best = (0..24)
                    .map(|_| v2(rng.range(0.12, 0.88), rng.range(0.12, 0.88)))
                    .max_by(|a, b| {
                        let d = |p: &Vec2| pts.iter().map(|q| q.dist(*p)).fold(f32::MAX, f32::min);
                        d(a).partial_cmp(&d(b)).unwrap()
                    })
                    .unwrap();
                pts.push(best);
            }
            pts
        }
    };
    for p in &mut pts {
        *p = v2(
            (p.x + rng.range(-0.02, 0.02)).clamp(margin, 1.0 - margin),
            (p.y + rng.range(-0.02, 0.02)).clamp(margin, 1.0 - margin),
        );
    }
    pts.into_iter().map(|p| p * s).collect()
}

struct Grid2 {
    w: i32,
    h: i32,
}

impl Grid2 {
    fn idx(&self, x: i32, y: i32) -> usize {
        (y * self.w + x) as usize
    }
    fn tiles(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        (0..self.h).flat_map(move |y| (0..self.w).map(move |x| (x, y)))
    }
    fn centre(x: i32, y: i32) -> Vec2 {
        v2(x as f32 + 0.5, y as f32 + 0.5)
    }
}

/// Build the map.
pub fn generate(spec: &MapSpec) -> Map {
    let mut rng = Rng::new(spec.seed ^ 0x005E_ED0F_5B0A);
    let s = spec.size.clamp(MIN_SIZE, MAX_SIZE);
    let g = Grid2 { w: s, h: s };
    let n = (s * s) as usize;
    let sf = s as f32;

    // Elevation: rolling noise plus placed hills and craters.
    let base = fractal(&mut rng, s, s, sf / 3.0);
    let mut height: Vec<f32> = base.iter().map(|v| 0.4 + (v - 0.5) * (0.15 + spec.hills * 1.4)).collect();
    // Lakes follow the lie of the land, not the hills and craters placed on it.
    let natural = height.clone();
    for f in &spec.features {
        let c = v2(f.x, f.y) * sf;
        let r = (4.0 + f.size * sf * 0.3).max(4.0);
        let dir = Vec2::from_angle(rng.range(0.0, std::f32::consts::PI));
        for (x, y) in g.tiles() {
            let p = Grid2::centre(x, y) - c;
            let bump = match f.kind {
                FeatureKind::Hill => 0.5 * (-(p.len2()) / (r * r * 0.5)).exp(),
                FeatureKind::Crater => {
                    let d = p.len() / r;
                    // A bowl with a raised rim.
                    if d < 1.0 { -0.35 * (1.0 - d * d) + 0.2 * d.powi(6) } else { 0.2 * (-(d - 1.0) * 4.0).exp() }
                }
                FeatureKind::Ridge => {
                    let along = p.dot(dir);
                    let across = p.dot(v2(-dir.y, dir.x));
                    0.45 * (-(across * across) / 6.0).exp() * (-(along * along) / (r * r * 1.5)).exp()
                }
                FeatureKind::Island => {
                    // A plateau that falls away at the edge.
                    let d = p.len() / r;
                    0.45 * (1.0 - d.powi(4)).max(0.0)
                }
                FeatureKind::Volcano => {
                    // A bowl inside a rim that stands well above the ground around it.
                    let d = p.len() / r;
                    -0.15 * (1.0 - d * d).max(0.0) + 0.5 * (-((d - 0.9) / 0.15).powi(2)).exp()
                }
                _ => 0.0,
            };
            height[g.idx(x, y)] += bump;
        }
    }

    // Ice: frozen lakes, more likely on low ground.
    let lakes = fractal(&mut rng, s, s, sf / 5.0);
    let score: Vec<f32> = (0..n).map(|i| lakes[i] + (0.45 - natural[i]) * 0.6).collect();
    let cut = quantile(&score, 1.0 - spec.ice * 0.45);
    let mut ground: Vec<Ground> = score.iter().map(|&v| if spec.ice > 0.02 && v > cut { Ground::Ice } else { Ground::Snow }).collect();
    for f in spec.features.iter().filter(|f| f.kind == FeatureKind::Lake) {
        let c = v2(f.x, f.y) * sf;
        let r = 4.0 + f.size * sf * 0.25;
        for (x, y) in g.tiles() {
            let p = Grid2::centre(x, y);
            let wobble = (lakes[g.idx(x, y)] - 0.5) * r * 0.6;
            if p.dist(c) < r + wobble {
                let i = g.idx(x, y);
                ground[i] = Ground::Ice;
                height[i] = height[i] * 0.5 + 0.15;
            }
        }
    }

    // Big forests (DESIGN.md 0.2: building thins them, and they grow back),
    // spires in small clusters.
    let mut obstacle = vec![Obstacle::None; n];
    let woods = fractal(&mut rng, s, s, sf / 7.0);
    let tree_cut = quantile(&woods, 1.0 - spec.trees * 0.45);
    for i in 0..n {
        if spec.trees > 0.02 && woods[i] > tree_cut && ground[i] == Ground::Snow && rng.chance(0.6) {
            obstacle[i] = Obstacle::Tree;
        }
    }
    let clusters = (spec.spires * sf * sf / 500.0) as usize;
    for _ in 0..clusters {
        let c = v2(rng.range(0.0, sf), rng.range(0.0, sf));
        for _ in 0..3 + rng.below(6) {
            let p = c + v2(rng.gauss(), rng.gauss()) * 1.6;
            let (x, y) = p.tile();
            if x >= 0 && y >= 0 && x < s && y < s {
                obstacle[g.idx(x, y)] = Obstacle::Pinnacle;
            }
        }
    }
    for f in &spec.features {
        let c = v2(f.x, f.y) * sf;
        let r = 4.0 + f.size * sf * 0.25;
        for (x, y) in g.tiles() {
            let i = g.idx(x, y);
            let d = Grid2::centre(x, y).dist(c) / r;
            if d >= 1.0 {
                continue;
            }
            match f.kind {
                FeatureKind::Forest if ground[i] == Ground::Snow && rng.chance(0.42 * (1.0 - d * d)) => obstacle[i] = Obstacle::Tree,
                FeatureKind::Spires if rng.chance(0.18 * (1.0 - d)) => obstacle[i] = Obstacle::Pinnacle,
                FeatureKind::Clearing | FeatureKind::Crystal => obstacle[i] = Obstacle::None,
                _ => {}
            }
        }
    }

    // Igloos: a flat, clear, snowy yard around each.
    let centres = place_igloos(spec, &mut rng);
    let mut igloos = Vec::new();
    for c in &centres {
        let level = height[g.idx(c.x as i32, c.y as i32)].clamp(0.2, 0.75);
        for (x, y) in g.tiles() {
            let d = Grid2::centre(x, y).dist(*c);
            let i = g.idx(x, y);
            if d < 9.0 {
                obstacle[i] = Obstacle::None;
                let k = ((9.0 - d) / 4.0).min(1.0);
                height[i] = height[i] + (level - height[i]) * k;
            }
            if d < 6.0 {
                ground[i] = Ground::Snow;
            }
        }
        let tl = ((c.x - 2.0).round() as i32).clamp(1, s - IGLOO_SIZE - 1);
        let ty = ((c.y - 2.0).round() as i32).clamp(1, s - IGLOO_SIZE - 1);
        igloos.push((tl as u16, ty as u16));
    }
    let centres: Vec<Vec2> = igloos.iter().map(|&(x, y)| v2(x as f32 + 2.0, y as f32 + 2.0)).collect();

    let spacing = centres
        .iter()
        .enumerate()
        .flat_map(|(i, a)| centres.iter().skip(i + 1).map(move |b| a.dist(*b)))
        .fold(f32::MAX, f32::min);
    let reach = (spacing * 0.5).clamp(10.0, 32.0);

    // An archipelago: each igloo's island of snow sits in a frozen sea. The sea
    // is ice, which can be walked, so every igloo stays reachable.
    if spec.layout == Layout::Scattered && spec.ice >= 0.6 {
        let isle = reach * 0.8;
        for (x, y) in g.tiles() {
            let p = Grid2::centre(x, y);
            let i = g.idx(x, y);
            if centres.iter().any(|c| c.dist(p) <= isle) {
                continue;
            }
            ground[i] = Ground::Ice;
            if obstacle[i] == Obstacle::Tree {
                obstacle[i] = Obstacle::None;
            }
        }
    }

    // Islands and volcanoes, placed over whatever the sea and yards made.
    for f in spec.features.iter().filter(|f| matches!(f.kind, FeatureKind::Island | FeatureKind::Volcano)) {
        let c = v2(f.x, f.y) * sf;
        let r = 4.0 + f.size * sf * 0.3;
        for (x, y) in g.tiles() {
            let p = Grid2::centre(x, y);
            let d = p.dist(c);
            let i = g.idx(x, y);
            // Keep the yards snowy.
            if centres.iter().any(|q| q.dist(p) < 8.0) {
                continue;
            }
            match f.kind {
                FeatureKind::Island if d < r => ground[i] = Ground::Snow,
                FeatureKind::Island if d < r + 3.0 => {
                    ground[i] = Ground::Ice;
                    obstacle[i] = Obstacle::None;
                }
                FeatureKind::Volcano if d < r * 0.5 => {
                    ground[i] = Ground::Snow;
                    obstacle[i] = Obstacle::None;
                }
                _ => {}
            }
        }
    }

    // Open water: a bay with a wobbly shore, kept off the yards so every igloo
    // can still reach the others (checked below; if not, the water freezes over).
    let mut water = vec![false; n];
    for f in spec.features.iter().filter(|f| f.kind == FeatureKind::Water) {
        let c = v2(f.x, f.y) * sf;
        let r = 3.0 + f.size * sf * 0.22;
        for (x, y) in g.tiles() {
            let p = Grid2::centre(x, y);
            let i = g.idx(x, y);
            let wobble = (lakes[i] - 0.5) * r * 0.9;
            if p.dist(c) < r + wobble && centres.iter().all(|q| q.dist(p) > 9.0) {
                water[i] = true;
            }
        }
    }
    // Water alone must not cut an igloo off from the rest.
    let cut_off = {
        let open = |_: &Vec<Obstacle>, x: i32, y: i32| x >= 0 && y >= 0 && x < s && y < s && !water[(y * s + x) as usize];
        let seen = flood(&g, &obstacle, centres[0], &open);
        centres.iter().any(|c| !seen[g.idx(c.x as i32, c.y as i32)])
    };
    if cut_off {
        water.iter_mut().for_each(|w| *w = false);
    }
    for i in 0..n {
        if water[i] {
            ground[i] = Ground::Water;
            obstacle[i] = Obstacle::None;
            height[i] = height[i] * 0.4 - 0.1;
        }
    }

    // Open ground: farther than `reach` from every igloo, where the wild
    // fish and the powerups go.
    let open_ground: Vec<bool> = g.tiles().map(|(x, y)| {
        let p = Grid2::centre(x, y);
        centres.iter().all(|c| c.dist(p) > reach)
    }).collect();

    // Make sure everything that matters is reachable before any cliffs go
    // up, so the carve never opens a cliff back up. Cliffs that make the
    // chokepoints could still shut someone in (rare), so carve again after
    // and let the next pass re-check the lines it touched.
    // Volcano craters must stay bare: the cliffs avoid them entirely.
    let protected: Vec<(Vec2, f32)> = spec
        .features
        .iter()
        .filter(|f| f.kind == FeatureKind::Volcano)
        .map(|f| (v2(f.x, f.y) * sf, (4.0 + f.size * sf * 0.3) * 0.5))
        .collect();
    let walk = |obstacle: &Vec<Obstacle>, x: i32, y: i32| x >= 0 && y >= 0 && x < s && y < s && !obstacle[(y * s + x) as usize].blocks() && !water[(y * s + x) as usize];
    let mut reached = flood(&g, &obstacle, centres[0], &walk);
    for c in centres.iter().skip(1) {
        if !reached[g.idx(c.x as i32, c.y as i32)] {
            carve(&g, &mut obstacle, *c, &reached);
            reached = flood(&g, &obstacle, centres[0], &walk);
        }
    }
    for _ in 0..4 {
        if !build_chokepoints(&g, &centres, &protected, &mut obstacle, s) {
            break;
        }
        reached = flood(&g, &obstacle, centres[0], &walk);
        for c in centres.iter().skip(1) {
            if !reached[g.idx(c.x as i32, c.y as i32)] {
                carve(&g, &mut obstacle, *c, &reached);
                reached = flood(&g, &obstacle, centres[0], &walk);
            }
        }
    }

    // Fish: the same ring of holes around every igloo, a few more out on the ice.
    let mut resources: Vec<ResourceSpawn> = Vec::new();
    let taken = |x: i32, y: i32, resources: &Vec<ResourceSpawn>| resources.iter().any(|r| (r.x as i32 - x).abs() < 2 && (r.y as i32 - y).abs() < 2);
    let a0 = rng.range(0.0, std::f32::consts::TAU);
    for c in &centres {
        for k in 0..5 {
            let a = a0 + k as f32 * std::f32::consts::TAU / 5.0;
            for dist in [7.0, 6.0, 8.0, 5.5, 9.0] {
                let (x, y) = (*c + Vec2::from_angle(a) * dist).tile();
                if walk(&obstacle, x, y) && reached[g.idx(x, y)] && !taken(x, y, &resources) {
                    resources.push(ResourceSpawn { x: x as u16, y: y as u16, kind: ResourceKind::Fish, amount: 7 });
                    break;
                }
            }
        }
    }
    let wild_fish = (sf * sf / 1500.0) as usize;
    for _ in 0..wild_fish * 20 {
        if resources.len() >= centres.len() * 5 + wild_fish {
            break;
        }
        let (x, y) = (rng.below(s as u32) as i32, rng.below(s as u32) as i32);
        let i = g.idx(x, y);
        if open_ground[i] && reached[i] && walk(&obstacle, x, y) && !taken(x, y, &resources) {
            resources.push(ResourceSpawn { x: x as u16, y: y as u16, kind: ResourceKind::Fish, amount: 7 });
        }
    }

    // Crystal: placed features, or else a fair spread through the contested
    // ground between the igloos, worth fighting over.
    let mut crystal_at: Vec<Vec2> = spec.features.iter().filter(|f| f.kind == FeatureKind::Crystal).map(|f| v2(f.x, f.y) * sf).collect();
    if crystal_at.is_empty() {
        crystal_at.push(v2(sf, sf) * 0.5);
        let mid = v2(sf, sf) * 0.5;
        for c in &centres {
            crystal_at.push(mid + (*c - mid) * 0.45);
        }
    }
    for at in crystal_at {
        // Nearest reachable open tile to where it was asked for.
        let spot = g
            .tiles()
            .filter(|&(x, y)| reached[g.idx(x, y)] && walk(&obstacle, x, y) && !taken(x, y, &resources))
            .filter(|&(x, y)| centres.iter().all(|c| c.dist(Grid2::centre(x, y)) > 8.0))
            .min_by(|a, b| Grid2::centre(a.0, a.1).dist2(at).partial_cmp(&Grid2::centre(b.0, b.1).dist2(at)).unwrap());
        if let Some((x, y)) = spot {
            resources.push(ResourceSpawn { x: x as u16, y: y as u16, kind: ResourceKind::Crystal, amount: 25 });
        }
    }

    // Powerups out in no man's land.
    let mut powerups = Vec::new();
    for _ in 0..400 {
        if powerups.len() >= centres.len().min(8) {
            break;
        }
        let (x, y) = (rng.below(s as u32) as i32, rng.below(s as u32) as i32);
        let i = g.idx(x, y);
        if open_ground[i] && reached[i] && walk(&obstacle, x, y) && !taken(x, y, &resources) {
            powerups.push(((powerups.len() % 2) as u8, x as u16, y as u16));
        }
    }

    let goal = "Take over all enemy igloos.";
    let description = if spec.description.is_empty() { goal.into() } else { format!("{} {goal}", spec.description.trim()) };

    let lo = height.iter().cloned().fold(f32::MAX, f32::min);
    let hi = height.iter().cloned().fold(f32::MIN, f32::max);
    let span = (hi - lo).max(0.6);
    let height_raw = height.iter().map(|h| (((h - lo) / span) * 255.0).clamp(0.0, 255.0) as u8).collect();

    let mut map = Map {
        name: spec.name.clone(),
        description,
        w: s,
        h: s,
        height_raw,
        ground,
        obstacle,
        resources,
        igloos,
        powerups,
        rules: MapRules {
            workers: spec.workers,
            snowballers: spec.snowballers,
            worker_warmth: 10,
            snowballer_warmth: 12,
        },
        height: Vec::new(),
    };
    add_ponds(&mut map);
    balance_ice(&mut map, spec.seed);
    balance_keeping_passes(&mut map, spec.seed);
    // Opening a sealed start fells trees, and planting trees can seal one:
    // go round until both hold.
    for _ in 0..3 {
        connect_starts(&mut map);
        balance_keeping_passes(&mut map, spec.seed);
    }
    connect_starts(&mut map);
    add_home_crystal(&mut map);
    // Round-trip through the file format so derived data (smoothed heights) is built.
    map = Map::parse(&map.encode()).expect("generated map encodes");
    map
}

/// A crystal deposit behind every igloo that hasn't one in reach, so nobody
/// can be kept from making elites (DESIGN.md 0.2): about `HOME_CRYSTAL` out,
/// on the side away from the middle of the map, where its igloo's people
/// can walk to it.
pub fn add_home_crystal(map: &mut Map) {
    let (w, h) = (map.w, map.h);
    let mid = v2(w as f32, h as f32) * 0.5;
    let open = game_walk(map);
    let mut add: Vec<ResourceSpawn> = Vec::new();
    for k in 0..map.igloos.len() {
        let c = map.igloo_center(k);
        // Steps on foot from the door to every tile.
        let door = door_tile(map, &open, k);
        let mut steps = vec![u32::MAX; (w * h) as usize];
        let mut q = std::collections::VecDeque::new();
        if map.in_bounds(door.0, door.1) && open[map.idx(door.0, door.1)] {
            steps[map.idx(door.0, door.1)] = 0;
            q.push_back(door);
        }
        while let Some((x, y)) = q.pop_front() {
            let d = steps[map.idx(x, y)];
            for (nx, ny) in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                if map.in_bounds(nx, ny) && open[map.idx(nx, ny)] && steps[map.idx(nx, ny)] == u32::MAX {
                    steps[map.idx(nx, ny)] = d + 1;
                    q.push_back((nx, ny));
                }
            }
        }
        // A deposit stands beside open ground: count the walk to a neighbour.
        let walk_to = |x: i32, y: i32| {
            [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().filter(|(dx, dy)| map.in_bounds(x + dx, y + dy)).map(|(dx, dy)| steps[map.idx(x + dx, y + dy)]).min().unwrap_or(u32::MAX)
        };
        let far = (HOME_CRYSTAL + 4.0) as u32;
        if map.resources.iter().chain(&add).any(|r| r.kind == ResourceKind::Crystal && walk_to(r.x as i32, r.y as i32) <= far) {
            continue;
        }
        let out = if c.dist(mid) > 1.0 { (c - mid).norm() } else { v2(1.0, 0.0) };
        let want = c + out * HOME_CRYSTAL;
        let taken = |x: i32, y: i32| map.resources.iter().chain(&add).any(|r: &ResourceSpawn| (r.x as i32 - x).abs() <= 2 && (r.y as i32 - y).abs() <= 2);
        let others = |p: Vec2| (0..map.igloos.len()).all(|j| map.igloo_center(j).dist(p) > 8.0);
        // Out in the open (a deposit blocks its tile), never in a gap between
        // trees that it would close; a cramped start makes do with less room
        // and a longer walk.
        let tiers = [(2, HOME_CRYSTAL - 4.0, HOME_CRYSTAL + 2.0), (1, HOME_CRYSTAL - 6.0, HOME_CRYSTAL + 4.0), (1, 4.0, HOME_CRYSTAL + 8.0)];
        let mut best: Option<(f32, (i32, i32))> = None;
        for (room, lo, hi) in tiers {
            let near = lo as u32..=hi as u32;
            for y in 2..h - 2 {
                for x in 2..w - 2 {
                    if !(-room..=room).all(|dy| (-room..=room).all(|dx| open[map.idx(x + dx, y + dy)])) {
                        continue;
                    }
                    if !near.contains(&steps[map.idx(x, y)]) || taken(x, y) || !others(v2(x as f32 + 0.5, y as f32 + 0.5)) {
                        continue;
                    }
                    let d = v2(x as f32 + 0.5, y as f32 + 0.5).dist(want);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, (x, y)));
                    }
                }
            }
            if best.is_some() {
                break;
            }
        }
        if let Some((_, (x, y))) = best {
            add.push(ResourceSpawn { x: x as u16, y: y as u16, kind: ResourceKind::Crystal, amount: 25 });
        }
    }
    map.resources.extend(add);
}

/// Which tiles a penguin can stand on once the game starts: no tree, rock
/// or water, and not the ponds the fish holes become.
fn game_walk(map: &Map) -> Vec<bool> {
    let mut m = map.clone();
    m.holes_to_ponds();
    (0..(m.w * m.h) as usize).map(|i| !m.obstacle[i].blocks() && m.ground[i] != Ground::Water).collect()
}

/// Where igloo `k`'s door will be, as the game picks it: south when it can.
fn door_tile(map: &Map, open: &[bool], k: usize) -> (i32, i32) {
    let (ix, iy) = (map.igloos[k].0 as i32, map.igloos[k].1 as i32);
    let mid = IGLOO_SIZE / 2;
    let ok = |x: i32, y: i32| map.in_bounds(x, y) && open[map.idx(x, y)];
    [(ix + mid, iy + IGLOO_SIZE), (ix + mid - 1, iy + IGLOO_SIZE), (ix + IGLOO_SIZE, iy + mid), (ix - 1, iy + mid), (ix + mid, iy - 1)]
        .into_iter()
        .find(|&(x, y)| ok(x, y))
        .unwrap_or((ix + mid, iy + IGLOO_SIZE))
}

/// Make sure every igloo can walk to every other, as the game will see the
/// map: forests grown thick, or balanced, can seal a start in between
/// cliffs and water. For each igloo cut off, clear the trees and rocks on
/// the way (over land, through as few of them as can be) to the others.
pub fn connect_starts(map: &mut Map) {
    let (w, h) = (map.w, map.h);
    let idx = |x: i32, y: i32| (y * w + x) as usize;
    for _ in 0..map.igloos.len() {
        let open = game_walk(map);
        let door = |k: usize| door_tile(map, &open, k);
        // Reached from igloo 0.
        let mut reached = vec![false; (w * h) as usize];
        let mut stack = vec![door(0)];
        while let Some((x, y)) = stack.pop() {
            if x < 0 || y < 0 || x >= w || y >= h || reached[idx(x, y)] || !open[idx(x, y)] {
                continue;
            }
            reached[idx(x, y)] = true;
            stack.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
        }
        let Some(k) = (1..map.igloos.len()).find(|&k| {
            let (x, y) = door(k);
            !reached[idx(x, y)]
        }) else {
            return;
        };
        // Cheapest way over land from igloo k to anywhere reached: a tree or
        // rock costs one, open ground nothing (0-1 search).
        let water = |x: i32, y: i32| map.ground_at(x, y) == Ground::Water;
        let mut cost = vec![u32::MAX; (w * h) as usize];
        let mut from = vec![usize::MAX; (w * h) as usize];
        let mut dq = std::collections::VecDeque::new();
        let s = door(k);
        cost[idx(s.0, s.1)] = 0;
        dq.push_back(s);
        let mut goal = None;
        while let Some((x, y)) = dq.pop_front() {
            if reached[idx(x, y)] {
                goal = Some((x, y));
                break;
            }
            for (nx, ny) in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                if nx < 0 || ny < 0 || nx >= w || ny >= h || water(nx, ny) {
                    continue;
                }
                let step = (!open[idx(nx, ny)]) as u32;
                let c = cost[idx(x, y)] + step;
                if c < cost[idx(nx, ny)] {
                    cost[idx(nx, ny)] = c;
                    from[idx(nx, ny)] = idx(x, y);
                    if step == 0 {
                        dq.push_front((nx, ny));
                    } else {
                        dq.push_back((nx, ny));
                    }
                }
            }
        }
        let Some((gx, gy)) = goal else { return };
        // Clear the way, a tile either side, keeping resources and water.
        let mut at = idx(gx, gy);
        while at != usize::MAX {
            let (x, y) = ((at as i32) % w, (at as i32) / w);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (xx, yy) = (x + dx, y + dy);
                    if map.in_bounds(xx, yy) {
                        let i = idx(xx, yy);
                        map.obstacle[i] = Obstacle::None;
                    }
                }
            }
            at = from[at];
        }
    }
}

/// Ponds are this big, and sit this far from the igloo they serve.
const POND_R: f32 = 2.6;

/// Give every igloo room for two fishing outposts (see
/// `balance::outpost_sites`): places for a Store with water beside it, a
/// walk from home. An igloo short of them gets a small pond out behind it
/// (away from the middle of the map), clear of fish holes, powerups and
/// other igloos, and the map is looked at again, a few times.
pub fn add_ponds(map: &mut Map) {
    let (w, h) = (map.w, map.h);
    let centre = v2(w as f32 / 2.0, h as f32 / 2.0);
    for _ in 0..3 {
        let setups = (0..map.igloos.len()).map(|i| crate::game::PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
        let g = crate::game::Game::new(map.clone(), setups, 1);
        let mut added = false;
        for i in 0..map.igloos.len() {
            let c = map.igloo_center(i);
            let have = crate::balance::outpost_sites(&g, g.igloos[i].door);
            if have.len() >= 2 {
                continue;
            }
            let outward = (c - centre).norm();
            // Candidate spots round the igloo, preferring "behind" it.
            let mut best: Option<(f32, Vec2)> = None;
            for k in 0..32 {
                let a = k as f32 / 32.0 * std::f32::consts::TAU;
                for d in [10.0f32, 12.0, 14.0, 16.0, 18.0] {
                    let p = c + Vec2::from_angle(a) * d;
                    let (x, y) = p.tile();
                    if x < 6 || y < 6 || x >= w - 6 || y >= h - 6 {
                        continue;
                    }
                    let clear = (0..map.igloos.len()).all(|j| j == i || map.igloo_center(j).dist(p) > 14.0)
                        && map.resources.iter().all(|r| v2(r.x as f32 + 0.5, r.y as f32 + 0.5).dist(p) > POND_R + 2.5)
                        && map.powerups.iter().all(|&(_, px, py)| v2(px as f32 + 0.5, py as f32 + 0.5).dist(p) > POND_R + 2.5)
                        && have.iter().all(|&o| o.dist(p) >= crate::balance::OUTPOST_SPACING + 1.0);
                    if !clear {
                        continue;
                    }
                    let land = map.ground_at(x, y) == Ground::Snow;
                    let score = (p - c).norm().dot(outward) * 2.0 + if land { 1.0 } else { 0.0 } - d * 0.02;
                    if best.is_none_or(|(b, _)| score > b) {
                        best = Some((score, p));
                    }
                }
            }
            let Some((_, p)) = best else { continue };
            for y in 0..h {
                for x in 0..w {
                    let d = v2(x as f32 + 0.5, y as f32 + 0.5).dist(p);
                    let j = map.idx(x, y);
                    if d <= POND_R {
                        map.ground[j] = Ground::Water;
                        map.obstacle[j] = Obstacle::None;
                    } else if d <= POND_R + 1.5 && map.obstacle[j] == Obstacle::Pinnacle {
                        // Leave a shore to stand on.
                        map.obstacle[j] = Obstacle::None;
                    }
                }
            }
            added = true;
        }
        if !added {
            break;
        }
    }
}

/// The share of the land within reach of igloo `i` that is ice (no snowballs
/// there, but fast to cross).
pub fn ice_near(map: &Map, i: usize) -> f32 {
    let c = map.igloo_center(i);
    let r = WOOD_REACH.ceil() as i32;
    let (cx, cy) = c.tile();
    let (mut land, mut ice) = (0.0f32, 0.0f32);
    for y in cy - r..=cy + r {
        for x in cx - r..=cx + r {
            if map.in_bounds(x, y) && v2(x as f32 + 0.5, y as f32 + 0.5).dist(c) <= WOOD_REACH && map.ground_at(x, y) != Ground::Water {
                land += 1.0;
                if map.ground_at(x, y) == Ground::Ice {
                    ice += 1.0;
                }
            }
        }
    }
    if land > 0.0 { ice / land } else { 0.0 }
}

/// The most ice any start may have in reach: snowballs are scooped from snow.
pub const MAX_START_ICE: f32 = 0.3;
/// How far from the typical start's ice share a start may be.
pub const ICE_BAND: f32 = 0.07;

/// Even out the ice around the starting igloos, without making the land any
/// more alike than that: a start with much more ice than the typical one has
/// some of it turned to snow in patches, a start with much less gets a few
/// patches of ice out beyond its yard. Nobody ends up over `MAX_START_ICE`.
pub fn balance_ice(map: &mut Map, seed: u64) {
    let n = map.igloos.len();
    if n < 2 {
        return;
    }
    let mut rng = Rng::new(seed ^ 0x1CE0_1CE0);
    let mut shares: Vec<f32> = (0..n).map(|i| ice_near(map, i)).collect();
    let mut sorted = shares.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = sorted[n / 2].min(MAX_START_ICE);
    let (lo, hi) = ((median - ICE_BAND).max(0.0), (median + ICE_BAND).min(MAX_START_ICE));
    for i in 0..n {
        let c = map.igloo_center(i);
        let patch = |map: &mut Map, at: Vec2, from: Ground, to: Ground, r: f32| {
            for y in (at.y as i32 - 3)..=(at.y as i32 + 3) {
                for x in (at.x as i32 - 3)..=(at.x as i32 + 3) {
                    if map.in_bounds(x, y) && map.ground_at(x, y) == from && v2(x as f32 + 0.5, y as f32 + 0.5).dist(at) <= r {
                        // Not on the igloo's yard, nor a fish hole or a powerup.
                        let q = v2(x as f32 + 0.5, y as f32 + 0.5);
                        if q.dist(c) > 6.0 && map.resources.iter().all(|r| v2(r.x as f32 + 0.5, r.y as f32 + 0.5).dist(q) > 1.5) && map.powerups.iter().all(|&(_, px, py)| v2(px as f32 + 0.5, py as f32 + 0.5).dist(q) > 1.5) {
                            let k = map.idx(x, y);
                            map.ground[k] = to;
                        }
                    }
                }
            }
        };
        let mut tries = 0;
        while shares[i] > hi && tries < 300 {
            tries += 1;
            let at = c + Vec2::from_angle(rng.range(0.0, std::f32::consts::TAU)) * rng.range(6.0, WOOD_REACH);
            patch(map, at, Ground::Ice, Ground::Snow, rng.range(1.5, 3.0));
            shares[i] = ice_near(map, i);
        }
        while shares[i] < lo && tries < 300 {
            tries += 1;
            let at = c + Vec2::from_angle(rng.range(0.0, std::f32::consts::TAU)) * rng.range(9.0, WOOD_REACH + 3.0);
            patch(map, at, Ground::Snow, Ground::Ice, rng.range(1.5, 3.0));
            shares[i] = ice_near(map, i);
        }
    }
}

/// How far from its igloo a tribe is expected to go for wood.
pub const WOOD_REACH: f32 = 16.0;
/// Every start gets at least this many trees within `WOOD_REACH`.
pub const MIN_TREES: usize = 80;
/// Every start has a crystal deposit about this far from its igloo.
pub const HOME_CRYSTAL: f32 = 12.0;

/// Trees within `reach` of a point.
pub fn trees_near(map: &Map, c: Vec2, reach: f32) -> usize {
    let r = reach.ceil() as i32;
    let (cx, cy) = c.tile();
    let mut n = 0;
    for y in cy - r..=cy + r {
        for x in cx - r..=cx + r {
            if map.in_bounds(x, y) && map.obstacle_at(x, y) == Obstacle::Tree && v2(x as f32 + 0.5, y as f32 + 0.5).dist(c) <= reach {
                n += 1;
            }
        }
    }
    n
}

/// Even out the wood around the starting igloos. A start with fewer trees
/// than the minimum gets a few small groves planted 6 to 13 tiles out; one
/// with far more than the typical start is thinned. The band is wide on
/// purpose: a forest map stays a forest map, but nobody starts in a desert
/// while a rival sits in the woods. Planting keeps clear of igloos, fish and
/// powerups.
pub fn balance_trees(map: &mut Map, seed: u64) {
    balance_with(map, seed, true);
}

/// Narrow passes between the two starts farthest apart.
fn main_passes(map: &Map) -> usize {
    let starts: Vec<Vec2> = (0..map.igloos.len()).map(|i| map.igloo_center(i)).collect();
    let mut best = (0, 1, 0.0_f32);
    for i in 0..starts.len() {
        for j in i + 1..starts.len() {
            let d = starts[i].dist(starts[j]);
            if d > best.2 {
                best = (i, j, d);
            }
        }
    }
    narrow_passes(map, starts[best.0], starts[best.1], 4).len()
}

/// Balance the wood without losing the narrow passes the layout was built
/// around: try thinning and planting, then planting alone, and keep the
/// first that leaves at least as many passes (up to the two a map needs).
fn balance_keeping_passes(map: &mut Map, seed: u64) {
    let before = main_passes(map).min(2);
    let saved = map.obstacle.clone();
    for thin in [true, false] {
        balance_with(map, seed, thin);
        if main_passes(map) >= before {
            return;
        }
        map.obstacle = saved.clone();
    }
}

fn balance_with(map: &mut Map, seed: u64, thin: bool) {
    let n = map.igloos.len();
    if n < 2 {
        return;
    }
    let mut rng = Rng::new(seed ^ 0xB0A2_7E55);
    let centres: Vec<Vec2> = (0..n).map(|i| map.igloo_center(i)).collect();
    let mut counts: Vec<usize> = centres.iter().map(|&c| trees_near(map, c, WOOD_REACH)).collect();
    let mut sorted = counts.clone();
    sorted.sort();
    let median = sorted[n / 2];
    // A few over the minimum: starting a game clears some trees round the igloos.
    let low = (MIN_TREES + 5).max(median * 3 / 4);
    let high = (MIN_TREES + 12).max(median * 115 / 100);
    let free = |map: &Map, x: i32, y: i32| {
        let c = v2(x as f32 + 0.5, y as f32 + 0.5);
        map.in_bounds(x, y)
            && map.obstacle_at(x, y) == Obstacle::None
            && map.ground_at(x, y) != Ground::Water
            // Banks stay clear: they are where workers stand to fish.
            && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().all(|&(dx, dy)| !map.in_bounds(x + dx, y + dy) || map.ground_at(x + dx, y + dy) != Ground::Water)
            && centres.iter().all(|&ic| ic.dist(c) > 5.5)
            && map.resources.iter().all(|r| v2(r.x as f32 + 0.5, r.y as f32 + 0.5).dist(c) > 2.5)
            && map.powerups.iter().all(|&(_, px, py)| v2(px as f32 + 0.5, py as f32 + 0.5).dist(c) > 2.5)
    };
    for i in 0..n {
        let c = centres[i];
        // Plant groves.
        let mut tries = 0;
        while counts[i] < low && tries < 800 {
            tries += 1;
            let a = rng.range(0.0, std::f32::consts::TAU);
            let d = rng.range(6.0, 15.0);
            let grove = c + Vec2::from_angle(a) * d;
            for _ in 0..rng.below(6) + 5 {
                let at = grove + Vec2::from_angle(rng.range(0.0, std::f32::consts::TAU)) * rng.range(0.0, 2.5);
                let (x, y) = at.tile();
                if counts[i] < low && free(map, x, y) {
                    let k = map.idx(x, y);
                    map.obstacle[k] = Obstacle::Tree;
                    // The tree counts for every start within reach of it.
                    for (j, &o) in centres.iter().enumerate() {
                        if o.dist(v2(x as f32 + 0.5, y as f32 + 0.5)) <= WOOD_REACH {
                            counts[j] += 1;
                        }
                    }
                }
            }
        }
        // Thin the forest around an overstocked start, outer trees first.
        if thin && counts[i] > high {
            let mut trees: Vec<(f32, i32, i32)> = Vec::new();
            for y in 0..map.h {
                for x in 0..map.w {
                    let d = v2(x as f32 + 0.5, y as f32 + 0.5).dist(c);
                    if map.obstacle_at(x, y) == Obstacle::Tree && d <= WOOD_REACH && d > 6.0 {
                        trees.push((d, x, y));
                    }
                }
            }
            // A scatter, not a ring: shuffle, then cut.
            for k in (1..trees.len()).rev() {
                trees.swap(k, rng.below(k as u32 + 1) as usize);
            }
            for (_, x, y) in trees {
                if counts[i] <= high {
                    break;
                }
                // Never take a tree from a start that would drop under its floor.
                let at = v2(x as f32 + 0.5, y as f32 + 0.5);
                if centres.iter().enumerate().any(|(j, &o)| o.dist(at) <= WOOD_REACH && counts[j] <= low) {
                    continue;
                }
                let k = map.idx(x, y);
                map.obstacle[k] = Obstacle::None;
                for (j, &o) in centres.iter().enumerate() {
                    if o.dist(at) <= WOOD_REACH {
                        counts[j] -= 1;
                    }
                }
            }
        }
    }
}

fn flood(g: &Grid2, obstacle: &Vec<Obstacle>, from: Vec2, walk: &dyn Fn(&Vec<Obstacle>, i32, i32) -> bool) -> Vec<bool> {
    let mut seen = vec![false; (g.w * g.h) as usize];
    let (sx, sy) = from.tile();
    let mut stack = vec![(sx, sy)];
    while let Some((x, y)) = stack.pop() {
        if !walk(obstacle, x, y) || seen[g.idx(x, y)] {
            continue;
        }
        seen[g.idx(x, y)] = true;
        stack.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
    }
    seen
}

/// Clear a 3-wide path from `from` to the nearest tile already reached.
fn carve(g: &Grid2, obstacle: &mut [Obstacle], from: Vec2, reached: &[bool]) {
    let Some((tx, ty)) = g
        .tiles()
        .filter(|&(x, y)| reached[g.idx(x, y)])
        .min_by(|a, b| Grid2::centre(a.0, a.1).dist2(from).partial_cmp(&Grid2::centre(b.0, b.1).dist2(from)).unwrap())
    else {
        return;
    };
    let to = Grid2::centre(tx, ty);
    let steps = (from.dist(to) * 2.0).ceil() as i32 + 1;
    for k in 0..=steps {
        let p = from.lerp(to, k as f32 / steps as f32);
        let (x, y) = p.tile();
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (xx, yy) = (x + dx, y + dy);
                if xx >= 0 && yy >= 0 && xx < g.w && yy < g.h {
                    obstacle[g.idx(xx, yy)] = Obstacle::None;
                }
            }
        }
    }
}

/// Raise ice cliffs of spires across the straight line between the two
/// starts farthest apart, until it crosses two passages at most four
/// tiles wide (DESIGN.md §0: frozen penguins clog narrow passes
/// shut, so maps are built to have them). Returns whether any cliffs
/// went up, so the reachability carving can take another turn after.
fn build_chokepoints(g: &Grid2, centres: &[Vec2], protected: &[(Vec2, f32)], obstacle: &mut [Obstacle], s: i32) -> bool {
    // The two farthest-apart starts: the main front line.
    let mut pair = (0, 1);
    let mut far = 0.0_f32;
    for i in 0..centres.len() {
        for j in i + 1..centres.len() {
            let d = centres[i].dist(centres[j]);
            if d > far {
                far = d;
                pair = (i, j);
            }
        }
    }
    let (a, b) = (centres[pair.0], centres[pair.1]);
    let runs = narrow_runs(&blocks_on(obstacle, s, s), a, b, 4);
    if far < 30.0 || runs.len() >= 2 {
        return false;
    }
    let (ex, ey) = across_step(b - a);
    let len = (far * 0.2).clamp(5.0, 11.0) as i32;
    // Keep each gap ten tiles clear of the igloo yards.
    let near = (10.0 / far).min(0.5);
    let (lo, hi) = (near, 1.0 - near);
    if hi - lo < 0.3 {
        return false;
    }
    let sample = |k: i32| a + (b - a) * (k as f32 / far);
    // No wall tile may sit in an igloo's yard or a protected place.
    let ok_tile = |wx: i32, wy: i32| -> bool {
        let p = Grid2::centre(wx, wy);
        !(centres.iter().any(|c| c.dist(p) < 6.0) || protected.iter().any(|(c, r)| p.dist(*c) < *r))
    };
    // How far out from the line a wall can run on one side of a doorway:
    // checked from the wall tiles the measurement itself walks (a couple
    // of either side of the doorway, where the wall has to pinch the line
    // to width three), stopping short of any yard or protected place.
    let side_len = |t: f32, side: i32| -> i32 {
        let k0 = ((t * far).round() as i32).clamp(0, far as i32);
        let mut r = 1;
        for rr in 2..=len {
            let mut ok = true;
            for kk in (k0 - 2)..=(k0 + 2) {
                let (tx, ty) = sample(kk).tile();
                let (wx, wy) = (tx + ex * side * rr, ty + ey * side * rr);
                if wx < 0 || wy < 0 || wx >= s || wy >= s || !ok_tile(wx, wy) {
                    ok = false;
                    break;
                }
            }
            if !ok { break; }
            r = rr;
        }
        r
    };
    // Sweep along the line for gap spots clear of the yards: the first
    // clear one, and one well past it. A spot works when spires can span
    // at least two tiles out to each side, so the line is pinched to a
    // walking width of three there.
    let mut gaps: Vec<f32> = Vec::new();
    for k in 0..=24 {
        let t = lo + (hi - lo) * (k as f32 / 24.0);
        if centres.iter().any(|c| c.dist(sample((t * far) as i32)) < 9.0) {
            continue;
        }
        if side_len(t, 1) < 2 || side_len(t, -1) < 2 {
            continue;
        }
        if gaps.is_empty() || t - gaps[gaps.len() - 1] > 0.3 {
            gaps.push(t);
            if gaps.len() == 2 { break; }
        }
    }
    let mut placed = false;
    for t in gaps {
        placed = true;
        let k0 = ((t * far).round() as i32).clamp(0, far as i32);
        // A solid block of spires across the line, one to two tiles thick
        // at every step of the measurement's own walk, with the doorway
        // row left open.
        for side in [-1, 1] {
            let l = side_len(t, side);
            for r in 2..=l {
                for kk in (k0 - 2)..=(k0 + 2) {
                    let (tx, ty) = sample(kk).tile();
                    let (wx, wy) = (tx + ex * side * r, ty + ey * side * r);
                    if wx < 0 || wy < 0 || wx >= s || wy >= s || !ok_tile(wx, wy) {
                        continue;
                    }
                    obstacle[g.idx(wx, wy)] = Obstacle::Pinnacle;
                }
            }
        }
        // Clear the doorway, so the gap is walkable whatever grew there.
        for kk in (k0 - 1)..=(k0 + 1) {
            let p = sample(kk);
            for dy in -2..=2 {
                for dx in -2..=2 {
                    let (x, y) = (p.x as i32 + dx, p.y as i32 + dy);
                    if x < 0 || y < 0 || x >= s || y >= s { continue; }
                    if Grid2::centre(x, y).dist2(p) <= 1.5 * 1.5 {
                        obstacle[g.idx(x, y)] = Obstacle::None;
                    }
                }
            }
        }
    }
    placed
}

/// The walk test used while a map is still being built: inside the map,
/// and not blocked by a tree or a spire.
fn blocks_on(obstacle: &[Obstacle], w: i32, h: i32) -> impl Fn(i32, i32) -> bool + '_ {
    move |x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < h && !obstacle[(y * w + x) as usize].blocks()
}

/// The axis across the line, rounded to a whole-tile step: walking
/// across steps tiles along it, so widths and walls line up exactly.
fn across_step(d: Vec2) -> (i32, i32) {
    let n = v2(-d.y, d.x);
    if n.x.abs() > n.y.abs() {
        (n.x.signum() as i32, 0)
    } else {
        (0, n.y.signum() as i32)
    }
}

/// How far sideways you can get from each tile of the straight line
/// between two points before something blocks you: the walking width
/// across the line, one number per tile of it, 0 where you can't stand.
fn cross_widths_over(walk: &dyn Fn(i32, i32) -> bool, a: Vec2, b: Vec2) -> Vec<i32> {
    let d = b - a;
    let dist = d.len();
    if dist < 2.0 {
        return Vec::new();
    }
    let (ex, ey) = across_step(d);
    let width_at = |p: Vec2| -> i32 {
        let here = p.tile();
        if !walk(here.0, here.1) {
            return 0;
        }
        let mut w = 1;
        for &side in &[1, -1] {
            for r in 1..=4 {
                if !walk(here.0 + ex * side * r, here.1 + ey * side * r) {
                    break;
                }
                w += 1;
            }
        }
        w
    };
    (0..=dist as i32).map(|k| width_at(a + d * (k as f32 / dist))).collect()
}

/// Passes at most `wide` tiles wide on the straight line from `a` to `b`:
/// separate stretches of the line where you can't get far sideways, only
/// counting ones with somewhere to stand (a solid wall is no pass).
fn narrow_runs(walk: &dyn Fn(i32, i32) -> bool, a: Vec2, b: Vec2, wide: i32) -> Vec<i32> {
    let mut runs: Vec<i32> = Vec::new();
    let mut run = 0;
    for w in cross_widths_over(walk, a, b) {
        if w <= wide {
            run = run.max(w);
        } else if run > 0 {
            if run >= 2 { runs.push(run); }
            run = 0;
        }
    }
    if run >= 2 { runs.push(run); }
    runs
}

/// Passes at most `wide` wide between two points: separate stretches of
/// the straight line between them where you can't get more than `wide`
/// tiles sideways, but can still get through. Frozen penguins clog these
/// shut (DESIGN.md §0), so maps are built to have them between starts.
pub fn narrow_passes(map: &Map, a: Vec2, b: Vec2, wide: i32) -> Vec<i32> {
    let walk = |x: i32, y: i32| map.in_bounds(x, y) && !map.obstacle[map.idx(x, y)].blocks();
    narrow_runs(&walk, a, b, wide)
}

/// Problems that would make a map unplayable, if any.
pub fn check(map: &Map) -> Result<(), String> {
    if map.igloos.len() < 2 {
        return Err("needs at least two igloos".into());
    }
    let walk = |o: &Vec<Obstacle>, x: i32, y: i32| map.in_bounds(x, y) && !o[map.idx(x, y)].blocks();
    let g = Grid2 { w: map.w, h: map.h };
    let reached = flood(&g, &map.obstacle, map.igloo_center(0), &walk);
    // As the game will see it, with water and the fish-hole ponds: every
    // igloo's door reaches igloo 0's.
    let open = game_walk(map);
    let at_door = |k: usize| door_tile(map, &open, k);
    let mut on_foot = vec![false; open.len()];
    let mut stack = vec![at_door(0)];
    while let Some((x, y)) = stack.pop() {
        if !map.in_bounds(x, y) || on_foot[map.idx(x, y)] || !open[map.idx(x, y)] {
            continue;
        }
        on_foot[map.idx(x, y)] = true;
        stack.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
    }
    for i in 0..map.igloos.len() {
        let (x, y) = map.igloo_center(i).tile();
        if !reached[map.idx(x, y)] {
            return Err(format!("igloo {i} can't be reached"));
        }
        let (dx, dy) = at_door(i);
        if !on_foot[map.idx(dx, dy)] {
            return Err(format!("igloo {i} is cut off by water"));
        }
        let fish = map
            .resources
            .iter()
            .filter(|r| r.kind == ResourceKind::Fish && map.igloo_center(i).dist(v2(r.x as f32, r.y as f32)) < 12.0)
            .count();
        if fish < 3 {
            return Err(format!("igloo {i} has only {fish} fish holes nearby"));
        }
    }
    for r in &map.resources {
        if !reached[map.idx(r.x as i32, r.y as i32)] {
            return Err("a resource can't be reached".into());
        }
    }
    Ok(())
}

/// The rule tests' main stage: a small, flat, open map for three tribes,
/// built by hand so no seed can put a hill, a thicket or a pond where a test
/// wants room. It still has everything the rules touch: a few clumps of
/// trees within a short walk of each igloo, ice, pinnacles, a crystal and water to fish from. Shipped as
/// `assets/maps/fixtures/trio.sbmap` (see the `make_maps` example).
pub fn fixture_arena() -> Map {
    let s = 100;
    let n = (s * s) as usize;
    let mut map = Map {
        name: "Trio".into(),
        description: "Test fixture: three tribes on a small flat map. Take over all enemy igloos.".into(),
        w: s,
        h: s,
        height_raw: vec![128; n],
        ground: vec![Ground::Snow; n],
        obstacle: vec![Obstacle::None; n],
        resources: Vec::new(),
        igloos: vec![(18, 22), (76, 26), (46, 76)],
        powerups: vec![(0, 50, 30), (1, 30, 55), (0, 70, 60), (1, 50, 55)],
        rules: MapRules { workers: 12, snowballers: 16, worker_warmth: 10, snowballer_warmth: 12 },
        height: Vec::new(),
    };
    let at = |x: i32, y: i32| (y * s + x) as usize;
    // A frozen strip between the lower igloos, and a patch up the right.
    for y in 58..66 {
        for x in 68..90 {
            map.ground[at(x, y)] = Ground::Ice;
        }
    }
    for y in 6..16 {
        for x in 60..70 {
            map.ground[at(x, y)] = Ground::Ice;
        }
    }
    // Pinnacles in a few small groups off the lines between the igloos.
    for &(cx, cy) in &[(52, 12), (30, 46), (68, 48), (86, 70), (12, 74)] {
        for &(dx, dy) in &[(0, 0), (1, 0), (0, 2), (-2, 1), (2, 2)] {
            map.obstacle[at(cx + dx, cy + dy)] = Obstacle::Pinnacle;
        }
    }
    // Trees in clumps, 12 to 20 tiles from the igloos.
    for &(cx, cy) in &[(36, 24), (20, 44), (62, 34), (90, 44), (30, 70), (62, 84), (46, 56), (10, 10)] {
        for dy in -3i32..=3 {
            for dx in -3i32..=3 {
                if dx * dx + dy * dy <= 8 && (dx + dy * 3).rem_euclid(3) != 0 {
                    map.obstacle[at(cx + dx, cy + dy)] = Obstacle::Tree;
                }
            }
        }
    }
    // A pond in the middle, and a crystal on its far side.
    for y in 66..72 {
        for x in 46..53 {
            let (dx, dy) = (x as f32 - 49.5, y as f32 - 69.0);
            if dx * dx / 12.0 + dy * dy / 6.0 < 1.0 {
                map.ground[at(x, y)] = Ground::Water;
            }
        }
    }
    map.resources.push(ResourceSpawn { x: 50, y: 40, kind: ResourceKind::Crystal, amount: 5 });
    // Fish holes round every igloo.
    for &(ix, iy) in &map.igloos.clone() {
        for k in 0..5 {
            let a = k as f32 / 5.0 * std::f32::consts::TAU + 0.4;
            let (x, y) = (ix as f32 + 1.0 + a.cos() * 7.0, iy as f32 + 1.0 + a.sin() * 7.0);
            map.resources.push(ResourceSpawn { x: x as u16, y: y as u16, kind: ResourceKind::Fish, amount: 6 });
        }
    }
    add_ponds(&mut map);
    connect_starts(&mut map);
    Map::parse(&map.encode()).expect("fixture encodes")
}
