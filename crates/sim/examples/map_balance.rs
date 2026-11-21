//! How even is each shipped map? Prints, for every starting igloo, what is
//! within reach and how far the nearest rival is on foot.
//! `cargo run -p sim --release --example map_balance`

use sim::balance::starts;
use sim::game::*;
use sim::map::Map;

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps");
    let mut files: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "sbmap")).collect();
    files.sort();
    for path in files {
        let map = Map::load_file(&path).unwrap();
        let n = map.igloos.len().min(8);
        let setups = (0..n).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
        let g = Game::new(map, setups, 1);
        println!("\n{} ({}x{}, {} starts)", g.map.name, g.map.w, g.map.h, n);
        println!("  start  trees banks outposts crystal  ice%  to-rival");
        for (i, s) in starts(&g).iter().enumerate() {
            println!("  {i:>5}  {:>5} {:>5} {:>8} {:>7.0} {:>5.0} {:>9.0}", s.trees, s.banks, s.outposts, s.crystal, s.ice * 100.0, s.to_rival);
        }
    }
}
