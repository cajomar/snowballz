//! Who may place buildings, and what counts as a standing one.

use sim::game::*;
use sim::map::Map;
use sim::math::v2;

fn map(name: &str) -> Map {
    let path = format!("{}/../../assets/maps/{name}.sbmap", env!("CARGO_MANIFEST_DIR"));
    Map::load_file(std::path::Path::new(&path)).unwrap()
}

fn game(name: &str, players: u8, core: CoreRules) -> Game {
    let setups = (0..players).map(|i| PlayerSetup { name: format!("P{i}"), color: i, team: i, ai: None }).collect();
    Game::new_with(map(name), setups, 7, core)
}

fn worker(g: &Game, owner: u8) -> u16 {
    g.penguins.iter().find(|p| p.owner == owner && p.kind == Kind::Worker && p.inside.is_none()).unwrap().id
}

fn sites(g: &Game, owner: u8) -> usize {
    g.buildings.iter().filter(|b| b.alive && b.owner == owner).count()
}

fn spot(g: &Game, player: u8, kind: BuildKind, dx: f32) -> (i32, i32) {
    let door = g.igloos.iter().find(|i| i.owner == player).unwrap().door;
    g.find_site(player, kind, door + v2(dx, 0.0), 14).expect("a place to build")
}

#[test]
fn nobody_builds_without_a_worker() {
    let mut g = game("fixtures/trio", 2, CoreRules::default());
    // (A tribe with workers sends its own crew to a site placed with nobody
    // picked: see tests/jobs.rs.) This one has none.
    let hands: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Worker).map(|p| p.id).collect();
    for &h in &hands {
        g.penguins[h as usize].kind = Kind::Snowballer;
    }
    let t = spot(&g, 0, BuildKind::Palisade, 6.0);
    g.command(0, Command::Build { units: vec![], kind: BuildKind::Palisade, tiles: vec![t] });
    assert_eq!(sites(&g, 0), 0, "no units, no site");

    let fighter = g.penguins.iter().find(|p| p.owner == 0 && p.kind == Kind::Snowballer).unwrap().id;
    g.command(0, Command::Build { units: vec![fighter], kind: BuildKind::Palisade, tiles: vec![t] });
    assert_eq!(sites(&g, 0), 0, "a snowballer can't build");

    // Someone else's worker doesn't count either.
    let theirs = worker(&g, 1);
    g.command(0, Command::Build { units: vec![theirs], kind: BuildKind::Palisade, tiles: vec![t] });
    assert_eq!(sites(&g, 0), 0);

    let w = hands[0];
    g.penguins[w as usize].kind = Kind::Worker;
    g.command(0, Command::Build { units: vec![w], kind: BuildKind::Palisade, tiles: vec![t] });
    assert_eq!(sites(&g, 0), 1, "a worker can");
}

#[test]
fn a_tribe_has_only_so_many_open_sites() {
    let mut g = game("fixtures/trio", 2, CoreRules::default());
    let w = worker(&g, 0);
    let door = g.igloos.iter().find(|i| i.owner == 0).unwrap().door;
    // Ask for far more walls than allowed, a few at a time.
    for round in 0..40 {
        let tiles: Vec<(i32, i32)> =
            (0..8).filter_map(|k| g.find_site(0, BuildKind::Palisade, door + v2(3.0 + round as f32 * 0.7 + k as f32, 0.0), 14)).collect();
        g.command(0, Command::Build { units: vec![w], kind: BuildKind::Palisade, tiles });
    }
    let open = g.buildings.iter().filter(|b| b.alive && b.owner == 0 && !b.done()).count();
    assert!(open > 0 && open <= MAX_OPEN_SITES, "{open} open sites");
}

#[test]
fn empty_sites_do_not_keep_a_tribe_in_the_game() {
    let mut g = game("fixtures/trio", 2, CoreRules::default());
    let w = worker(&g, 1);
    let t = spot(&g, 1, BuildKind::Palisade, 6.0);
    g.command(1, Command::Build { units: vec![w], kind: BuildKind::Palisade, tiles: vec![t] });
    assert_eq!(sites(&g, 1), 1);
    for ig in g.igloos.iter_mut().filter(|i| i.owner == 1) {
        ig.owner = NEUTRAL;
    }
    for _ in 0..20 {
        g.tick(); // victory is checked every 10 ticks
    }
    assert!(g.players[1].out, "a building site is not a standing building");
    assert_eq!(g.winner, Some(0));
}

#[test]
fn unclaimed_igloos_start_empty() {
    // A map with more igloos than players: the spare ones are sites to
    // found, not stocked with free penguins.
    let g = game("pinewood-hollow", 2, CoreRules::default());
    let spare: Vec<u8> = g.igloos.iter().filter(|i| i.owner == NEUTRAL).map(|i| i.id).collect();
    assert!(!spare.is_empty(), "this map has spare igloos");
    for ig in &spare {
        assert!(g.penguins.iter().all(|p| p.home != *ig), "igloo {ig} has residents");
    }
}

#[test]
fn far_away_move_targets_stay_on_the_map() {
    let mut g = game("fixtures/trio", 2, CoreRules::default());
    let w = worker(&g, 0);
    for target in [v2(1.0e9, -1.0e9), v2(-5.0, 3.0e38), v2(f32::MAX, f32::MAX)] {
        g.command(0, Command::Move { units: vec![w], target, run: false, queue: false });
        g.command(0, Command::Move { units: vec![w], target, run: true, queue: true });
    }
    for _ in 0..40 {
        g.tick();
    }
    let p = g.penguins[w as usize].pos;
    assert!(p.x >= 0.0 && p.y >= 0.0 && p.x <= g.map.w as f32 && p.y <= g.map.h as f32);
}
