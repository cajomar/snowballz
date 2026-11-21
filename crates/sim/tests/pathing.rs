//! Walking keeps clear of trees: a straight line may not brush past one.

use sim::map::{Map, Obstacle};
use sim::math::v2;
use sim::path::clear_of_trees;

/// An open field with one tree at (6, 6).
fn field() -> Map {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    let mut map = Map::load_file(std::path::Path::new(&path)).unwrap();
    for o in map.obstacle.iter_mut() {
        *o = Obstacle::None;
    }
    let i = map.idx(6, 6);
    map.obstacle[i] = Obstacle::Tree;
    map
}

#[test]
fn a_straight_walk_keeps_clear_of_a_tree() {
    let m = field();
    // 0.1 and 0.25 tile from its edge: right through the branches.
    assert!(!clear_of_trees(&m, v2(2.0, 5.9), v2(10.0, 5.9)), "brushed the tree");
    assert!(!clear_of_trees(&m, v2(2.0, 5.75), v2(10.0, 5.75)), "within the boughs");
    // Half a tile off, and the next row over, are fine.
    assert!(clear_of_trees(&m, v2(2.0, 5.5), v2(10.0, 5.5)));
    assert!(clear_of_trees(&m, v2(2.0, 4.5), v2(10.0, 4.5)));
}

#[test]
fn a_one_tile_gap_between_trees_can_still_be_walked() {
    let mut m = field();
    let i = m.idx(6, 8);
    m.obstacle[i] = Obstacle::Tree;
    // The middle of the gap between (6,6) and (6,8) is the row y = 7.5.
    assert!(clear_of_trees(&m, v2(2.0, 7.5), v2(10.0, 7.5)));
}
