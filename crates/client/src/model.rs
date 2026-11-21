//! Vertex-animated models baked by tools/export_models.py (or built in
//! meshgen.rs), uploaded as one vertex buffer per animation frame.

use crate::gpu::{bytes, Gpu};

/// Vertex color alpha flags.
pub const FLAG_TEAM: u8 = 1;
pub const FLAG_EMISSIVE: u8 = 2;
pub const FLAG_GLOSSY: u8 = 4;
/// Bits 3..5 of the alpha: what the surface is made of, which the shader
/// paints its texture from (grain, weave, mottling...). 0 is plain paint.
pub const MAT_SHIFT: u8 = 3;
pub const MAT_MASK: u8 = 7 << MAT_SHIFT;
pub const MAT_SNOW: u8 = 1 << MAT_SHIFT;
pub const MAT_WOOD: u8 = 2 << MAT_SHIFT;
pub const MAT_STONE: u8 = 3 << MAT_SHIFT;
pub const MAT_CLOTH: u8 = 4 << MAT_SHIFT;
pub const MAT_METAL: u8 = 5 << MAT_SHIFT;
pub const MAT_ICE: u8 = 6 << MAT_SHIFT;

/// The material a plain colour most likely is, for models that don't say:
/// near-white is packed snow, brown is wood, grey is stone, team colour is
/// cloth, glossy pale blue is ice.
pub fn guess_material(c: [u8; 4]) -> u8 {
    let flags = c[3];
    if flags & MAT_MASK != 0 || flags & FLAG_EMISSIVE != 0 {
        return 0;
    }
    if flags & FLAG_TEAM != 0 {
        return MAT_CLOTH;
    }
    let [r, g, b] = [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0];
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let sat = if max > 0.0 { (max - min) / max } else { 0.0 };
    if flags & FLAG_GLOSSY != 0 {
        // Ice is a clear blue; grey-blue gloss is fish scales or wet stone.
        return if b >= r && max > 0.45 && sat > 0.25 { MAT_ICE } else { 0 };
    }
    if max > 0.72 && sat < 0.3 && b >= r {
        MAT_SNOW
    } else if r > g && g >= b && sat > 0.2 && max < 0.8 {
        MAT_WOOD
    } else if sat < 0.22 && max < 0.72 {
        if max < 0.3 { MAT_METAL } else { MAT_STONE }
    } else {
        0
    }
}

#[derive(Clone, Default)]
pub struct MeshData {
    /// rgb + flags
    pub colors: Vec<[u8; 4]>,
    pub indices: Vec<u16>,
    /// Per frame: position + normal per vertex.
    pub frames: Vec<Vec<[f32; 6]>>,
}

pub fn parse_sbm(b: &[u8]) -> Result<Vec<(String, MeshData)>, String> {
    let mut r = sim::wire::Reader::new(b);
    let err = |e: sim::wire::WireError| e.to_string();
    if r.raw(4).map_err(err)? != b"SBM1" {
        return Err("not an SBM model".into());
    }
    let n = r.u32().map_err(err)?;
    let mut out = Vec::new();
    for _ in 0..n {
        let len = r.u8().map_err(err)? as usize;
        let name = String::from_utf8_lossy(r.raw(len).map_err(err)?).to_string();
        let nv = r.u32().map_err(err)? as usize;
        let ni = r.u32().map_err(err)? as usize;
        let nf = r.u32().map_err(err)? as usize;
        let colors = r.raw(nv * 4).map_err(err)?.chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
        let indices = r.raw(ni * 2).map_err(err)?.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let mut frames = Vec::with_capacity(nf);
        for _ in 0..nf {
            let raw = r.raw(nv * 16).map_err(err)?;
            frames.push(
                raw.chunks(16)
                    .map(|c| {
                        let f = |i: usize| f32::from_le_bytes(c[i..i + 4].try_into().unwrap());
                        let n = |i: usize| c[i] as i8 as f32 / 127.0;
                        [f(0), f(4), f(8), n(12), n(13), n(14)]
                    })
                    .collect(),
            );
        }
        out.push((name, MeshData { colors, indices, frames }));
    }
    Ok(out)
}

pub struct GpuMesh {
    pub name: String,
    pub colors: wgpu::Buffer,
    pub index: wgpu::Buffer,
    pub n_indices: u32,
    pub n_verts: usize,
    pub frames: Vec<wgpu::Buffer>,
}

impl GpuMesh {
    pub fn upload(ctx: &mut Gpu, name: &str, m: &MeshData) -> GpuMesh {
        let colors = ctx.buffer(name, wgpu::BufferUsages::VERTEX, bytes(&m.colors));
        let index = ctx.buffer(name, wgpu::BufferUsages::INDEX, bytes(&m.indices));
        let frames = m.frames.iter().map(|f| ctx.buffer(name, wgpu::BufferUsages::VERTEX, bytes(f))).collect();
        GpuMesh { name: name.into(), colors, index, n_indices: m.indices.len() as u32, n_verts: m.colors.len(), frames }
    }
}

/// A set of named animations (or a single static mesh).
pub struct Model {
    pub anims: Vec<GpuMesh>,
}

impl Model {
    pub fn load(ctx: &mut Gpu, path: &std::path::Path) -> Result<Model, String> {
        let b = crate::assets::read(path)?;
        let anims = parse_sbm(&b)?.iter().map(|(n, m)| GpuMesh::upload(ctx, n, m)).collect();
        Ok(Model { anims })
    }

    pub fn single(ctx: &mut Gpu, name: &str, m: &MeshData) -> Model {
        Model { anims: vec![GpuMesh::upload(ctx, name, m)] }
    }

    /// `dir/<file>.sbm` if an artist has exported one (tools/export_models.py
    /// prop), else the procedural mesh `m`: art can replace meshgen models
    /// one at a time without touching code.
    pub fn baked_or(ctx: &mut Gpu, dir: &std::path::Path, file: &str, m: &MeshData) -> Model {
        let path = dir.join(format!("{file}.sbm"));
        if crate::assets::exists(&path) {
            match Model::load(ctx, &path) {
                Ok(model) => return model,
                Err(e) => eprintln!("using the built-in {file}: {e}"),
            }
        }
        Model::single(ctx, file, m)
    }

    pub fn anim_index(&self, name: &str) -> usize {
        self.find_anim(name).unwrap_or(0)
    }

    pub fn find_anim(&self, name: &str) -> Option<usize> {
        self.anims.iter().position(|a| a.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_colours_get_the_material_they_look_like() {
        assert_eq!(guess_material([235, 242, 250, 0]), MAT_SNOW);
        assert_eq!(guess_material([120, 80, 45, 0]), MAT_WOOD);
        assert_eq!(guess_material([120, 124, 130, 0]), MAT_STONE);
        assert_eq!(guess_material([255, 255, 255, FLAG_TEAM]), MAT_CLOTH);
        assert_eq!(guess_material([170, 215, 245, FLAG_GLOSSY]), MAT_ICE);
        assert_eq!(guess_material([150, 170, 190, FLAG_GLOSSY]), 0);
        // Fire glows as it is, and a material already given is kept.
        assert_eq!(guess_material([255, 150, 40, FLAG_EMISSIVE]), 0);
        assert_eq!(guess_material([235, 242, 250, MAT_STONE]), 0);
    }

    fn load(name: &str) -> Vec<(String, MeshData)> {
        let path = format!("{}/../../assets/models/{name}.sbm", env!("CARGO_MANIFEST_DIR"));
        parse_sbm(&std::fs::read(path).unwrap()).unwrap()
    }

    /// Thousands of penguins are drawn at once, so each pose stays light, and
    /// every animation the game asks for (`scene::Anim`) is in the file.
    #[test]
    fn penguin_models_are_light_and_have_every_animation() {
        let worker = load("worker");
        let names: Vec<&str> = worker.iter().map(|(n, _)| n.as_str()).collect();
        for want in ["stand", "walk", "carry", "fish", "unload", "hammer", "chop", "sit", "cheer", "flinch"] {
            assert!(names.contains(&want), "worker has no '{want}' animation: {names:?}");
        }
        let warrior = load("warrior");
        let names: Vec<&str> = warrior.iter().map(|(n, _)| n.as_str()).collect();
        for want in ["stand", "walk", "throw", "scoop", "cheer", "flinch"] {
            assert!(names.contains(&want), "warrior has no '{want}' animation: {names:?}");
        }
        for (name, m) in worker.iter().chain(warrior.iter()) {
            assert!(m.indices.len() / 3 <= 3000, "{name} has {} triangles", m.indices.len() / 3);
            assert!(m.frames.len() >= 2, "{name} has no frames to play");
        }
    }

    /// Each kind's own penguin (tools/penguins.py) has the clips the game
    /// plays for it, and stays light: there can be hundreds on screen.
    #[test]
    fn unit_models_have_their_clips() {
        use sim::game::Kind;
        for k in Kind::ALL {
            let file = crate::render3d::unit_file(k);
            let model = load(file);
            let names: Vec<&str> = model.iter().map(|(n, _)| n.as_str()).collect();
            let mut want = vec!["stand", "walk", "cheer", "flinch", "knocked"];
            match k {
                Kind::Worker => want.extend(["carry", "fish", "unload", "hammer", "chop", "sit"]),
                Kind::Warmer => want.extend(["warm", "scoop"]),
                Kind::Sledder => {}
                _ => want.extend(["throw", "scoop"]),
            }
            for w in want {
                assert!(names.contains(&w), "{file} has no '{w}' clip: {names:?}");
            }
            for (name, m) in &model {
                assert!(m.indices.len() / 3 <= 3500, "{file} {name} has {} triangles", m.indices.len() / 3);
                assert!(!m.frames.is_empty(), "{file} {name} has no frames");
            }
        }
    }

    /// Exported props stand in for meshgen models: each must parse, index
    /// only its own vertices, and stay light enough to draw by the hundred.
    #[test]
    fn exported_props_are_sound_and_light() {
        let dir = format!("{}/../../assets/models", env!("CARGO_MANIFEST_DIR"));
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let file = path.file_stem().unwrap().to_string_lossy().to_string();
            if file == "worker" || file == "warrior" || path.extension().is_none_or(|e| e != "sbm") {
                continue;
            }
            let anims = parse_sbm(&std::fs::read(&path).unwrap()).unwrap();
            assert!(!anims.is_empty(), "{file} is empty");
            for (_, m) in &anims {
                assert!(m.indices.iter().all(|&i| (i as usize) < m.colors.len()), "{file} indexes past its vertices");
                assert!(m.indices.len() / 3 <= 6000, "{file} has {} triangles", m.indices.len() / 3);
                assert!(!m.frames.is_empty(), "{file} has no geometry");
            }
        }
    }
}
