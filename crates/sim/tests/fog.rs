//! Fog of war: what a team can see.

use sim::fog::{visible, IGLOO_SIGHT, PENGUIN_SIGHT};
use sim::map::Map;
use sim::math::v2;

fn map() -> Map {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    Map::load_file(std::path::Path::new(&path)).unwrap()
}

#[test]
fn a_penguin_sees_around_itself_and_no_farther() {
    let m = map();
    let at = v2(30.5, 30.5);
    let seen = visible(&m, &[(at, 5.0)]);
    assert!(seen[m.idx(30, 30)]);
    assert!(seen[m.idx(34, 30)], "4 tiles away");
    assert!(!seen[m.idx(36, 30)], "6 tiles away");
    assert!(!seen[m.idx(34, 34)], "the corner of the square is out of the circle");
}

#[test]
fn a_tower_sees_far() {
    let m = map();
    let at = v2(30.5, 30.5);
    let seen = visible(&m, &[(at, sim::fog::TOWER_SIGHT)]);
    assert!(seen[m.idx(45, 30)], "15 tiles away");
    assert!(!seen[m.idx(48, 30)], "18 tiles away");
}

#[test]
fn nobody_sees_nothing_and_eyes_add_up() {
    let m = map();
    assert!(visible(&m, &[]).iter().all(|&s| !s));
    let a = visible(&m, &[(v2(10.5, 10.5), 3.0)]);
    let b = visible(&m, &[(v2(40.5, 40.5), 3.0)]);
    let both = visible(&m, &[(v2(10.5, 10.5), 3.0), (v2(40.5, 40.5), 3.0)]);
    for i in 0..both.len() {
        assert_eq!(both[i], a[i] || b[i]);
    }
    // Off the edge of the map is fine.
    let edge = visible(&m, &[(v2(0.2, 0.2), 4.0)]);
    assert!(edge[0]);
}

#[test]
fn igloos_see_further_than_penguins() {
    let m = map();
    // An eye on a tile center: a tile 11 to the right has its center
    // exactly 11.0 away, which is both IGLOO_SIGHT - 1 and PENGUIN_SIGHT + 2.
    let at = v2(30.5, 30.5);
    let far = m.idx(41, 30);

    let igloo = visible(&m, &[(at, IGLOO_SIGHT)]);
    assert!(igloo[far], "an igloo sees IGLOO_SIGHT - 1 away");

    let penguin = visible(&m, &[(at, PENGUIN_SIGHT)]);
    assert!(!penguin[far], "a lone penguin does not see PENGUIN_SIGHT + 2 away");

    // No eyes near this tile: the same eye is 32 away from it.
    assert!(!igloo[m.idx(5, 50)], "no eyes near this tile");
}
