//! Build the shipped maps from their specs:
//! `cargo run -p sim --release --example make_maps` turns every
//! `assets/mapspecs/*.json` into `assets/maps/<same name>.sbmap`, and
//! `assets/mapspecs/fixtures/*.json` into `assets/maps/fixtures/`.

use sim::json::Json;
use sim::mapgen::{check, generate, MapSpec};

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // `fixtures/` holds small maps the rule tests play on; they are not offered in the lobby.
    let mut specs: Vec<_> = Vec::new();
    for dir in ["assets/mapspecs", "assets/mapspecs/fixtures"] {
        specs.extend(std::fs::read_dir(root.join(dir)).unwrap().flatten().map(|e| e.path()));
    }
    specs.sort();
    // The rule tests' hand-built stage has no spec.
    let arena = sim::mapgen::fixture_arena();
    sim::mapgen::check(&arena).expect("the test arena is sound");
    std::fs::write(root.join("assets/maps/fixtures/trio.sbmap"), arena.encode()).unwrap();
    for path in specs.into_iter().filter(|p| p.extension().map_or(false, |e| e == "json")) {
        let text = std::fs::read_to_string(&path).unwrap();
        let j = Json::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let seed = j.get("seed").and_then(Json::as_f64).unwrap_or(1.0) as u64;
        let map = generate(&MapSpec::from_json(&j, seed));
        if let Err(e) = check(&map) {
            panic!("{}: {e}", path.display());
        }
        let sub = if path.parent().unwrap().ends_with("fixtures") { "assets/maps/fixtures" } else { "assets/maps" };
        let out = root.join(sub).join(path.file_stem().unwrap()).with_extension("sbmap");
        std::fs::write(&out, map.encode()).unwrap();
        println!("{} -> {} ({}x{}, {} igloos)", path.file_name().unwrap().to_string_lossy(), out.file_name().unwrap().to_string_lossy(), map.w, map.h, map.igloos.len());
    }
}
