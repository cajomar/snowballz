//! Every shipped map gives each tribe a fair start: wood to chop, banks to
//! fish and crystal alike.

use sim::balance::starts;
use sim::game::*;
use sim::map::Map;
use sim::mapgen::{check, MIN_TREES};

fn shipped() -> Vec<(String, Map)> {
    let dir = format!("{}/../../assets/maps", env!("CARGO_MANIFEST_DIR"));
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "sbmap"))
        .map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), Map::load_file(&p).unwrap()))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(out.len() >= 10, "found only {} maps", out.len());
    out
}

fn game(map: Map) -> Game {
    let n = map.igloos.len();
    let setups = (0..n).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    Game::new(map, setups, 1)
}

fn spread(v: impl Iterator<Item = f32>) -> (f32, f32) {
    v.fold((f32::INFINITY, 0.0f32), |(lo, hi), x| (lo.min(x), hi.max(x)))
}

#[test]
fn every_start_has_wood_and_banks_in_reach() {
    for (name, map) in shipped() {
        check(&map).unwrap_or_else(|e| panic!("{name}: {e}"));
        let g = game(map);
        for (i, s) in starts(&g).iter().enumerate() {
            assert!(s.trees >= MIN_TREES, "{name}: start {i} has only {} trees in reach", s.trees);
            assert!(s.banks >= 30, "{name}: start {i} has only {} banks", s.banks);
            assert!(s.crystal.is_finite(), "{name}: start {i} cannot walk to any crystal");
        }
    }
}

#[test]
fn every_start_has_room_for_two_fishing_outposts_and_snow_to_throw_from() {
    for (name, map) in shipped() {
        let g = game(map);
        for (i, s) in starts(&g).iter().enumerate() {
            assert!(s.outposts >= 2, "{name}: start {i} has room for only {} fishing outposts", s.outposts);
            assert!(s.ice <= sim::mapgen::MAX_START_ICE + 0.03, "{name}: start {i} is {:.0}% ice: little to scoop snowballs from", s.ice * 100.0);
        }
    }
}

#[test]
fn starts_are_even_in_wood_and_crystal() {
    for (name, map) in shipped() {
        let g = game(map);
        let s = starts(&g);
        let (lo, hi) = spread(s.iter().map(|x| x.trees as f32));
        assert!(hi <= lo * 2.0, "{name}: wood runs from {lo} to {hi} trees");
        let (lo, hi) = spread(s.iter().map(|x| x.crystal));
        assert!(hi <= lo * 3.2, "{name}: the nearest crystal is {lo} to {hi} tiles away");
    }
}

#[test]
fn ice_is_even_between_starts() {
    for (name, map) in shipped() {
        let g = game(map);
        let (lo, hi) = spread(starts(&g).iter().map(|s| s.ice));
        assert!(hi - lo <= 0.2, "{name}: ice runs from {:.0}% to {:.0}% between starts", lo * 100.0, hi * 100.0);
    }
}

