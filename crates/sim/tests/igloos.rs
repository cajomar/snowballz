//! Igloos built anywhere (the `armies` rule, DESIGN.md 0.2 and PLAN.md C7
//! stage 5): an Igloo goes on the build card, at least 30 tiles from every
//! other, and once its builders finish it is a real igloo of the tribe with
//! its own Chieftain, guards and fishing.

use sim::ai::AiLevel;
use sim::game::*;
use sim::map::Map;
use sim::math::{v2, Vec2};
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};

fn map(name: &str) -> Map {
    let path = format!("{}/../../assets/maps/{name}.sbmap", env!("CARGO_MANIFEST_DIR"));
    Map::load_file(std::path::Path::new(&path)).unwrap()
}

fn game() -> Game {
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let mut g = Game::new_with(map("pinewood-hollow"), setups, 7, CoreRules::default().map_start());
    g.weather.calm = true;
    g
}

fn run(g: &mut Game, secs: u32) {
    for _ in 0..secs * sim::TICK_RATE {
        g.tick();
        g.events.clear();
    }
}

fn home(g: &Game) -> u8 {
    g.igloos.iter().position(|i| i.owner == 0).unwrap() as u8
}

/// The nearest place to home where an igloo may go, with trees in reach.
fn far_site(g: &Game) -> (i32, i32) {
    let c = g.igloos[home(g) as usize].center;
    let mut best: Option<(f32, (i32, i32))> = None;
    for r in [32.0, 36.0, 40.0, 44.0] {
        for k in 0..32 {
            let at = c + Vec2::from_angle(k as f32 / 32.0 * std::f32::consts::TAU) * r;
            let Some(t) = g.find_site(0, BuildKind::Igloo, at, 2) else { continue };
            if g.tree_near(igloo_site_center(t), 12).is_none() {
                continue;
            }
            let d = igloo_site_center(t).dist(c);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, t));
            }
        }
    }
    best.expect("room for an igloo").1
}

#[test]
fn an_igloo_goes_a_good_way_from_every_other() {
    let g = game();
    assert!(BuildKind::Igloo.offered());
    let door = g.igloos[home(&g) as usize].door;
    let near = g.find_site(0, BuildKind::Palisade, door + v2(8.0, 0.0), 6).unwrap();
    assert_eq!(g.placement_problem(0, BuildKind::Igloo, near), Some("too close to another igloo"));
    let far = far_site(&g);
    assert!(g.igloos.iter().all(|ig| ig.center.dist(igloo_site_center(far)) >= IGLOO_SPACING));
    assert_eq!(g.placement_problem(0, BuildKind::Igloo, far), None);
}

#[test]
fn builders_raise_a_new_igloo_that_is_the_tribes_own() {
    let mut g = game();
    let mut view = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 0)));
    let n = g.igloos.len();
    let t = far_site(&g);
    g.command(0, Command::Build { units: vec![], kind: BuildKind::Igloo, tiles: vec![t] });
    let site = g.buildings.iter().rposition(|b| b.kind == BuildKind::Igloo).expect("the site was staked") as u16;
    // A second igloo can't go beside the first one's site.
    let beside = g.find_site(0, BuildKind::Igloo, igloo_site_center(t) + v2(12.0, 0.0), 6);
    assert!(beside.is_none_or(|b| igloo_site_center(b).dist(igloo_site_center(t)) >= IGLOO_SPACING));
    let mut builders = Vec::new();
    for _ in 0..600 {
        for p in g.penguins.iter().filter(|p| p.order == (Order::Build { building: site })) {
            if !builders.contains(&p.id) {
                builders.push(p.id);
            }
        }
        g.tick();
        let ev = std::mem::take(&mut g.events);
        view.1.apply(&view.0.encode(&g, &ev)).unwrap();
        if g.igloos.len() > n {
            break;
        }
        for _ in 1..sim::TICK_RATE {
            g.tick();
            let ev = std::mem::take(&mut g.events);
            view.1.apply(&view.0.encode(&g, &ev)).unwrap();
        }
    }
    assert_eq!(g.igloos.len(), n + 1, "the igloo was never finished");
    let new = n as u8;
    let ig = &g.igloos[n];
    assert_eq!(ig.owner, 0);
    assert_eq!(ig.tile, t);
    assert!(!g.buildings[site as usize].alive, "the site is the igloo now");
    assert!(g.grid.walkable_at(ig.door), "its door can be reached");
    let (x, y) = (t.0 + 1, t.1 + 1);
    assert!(!g.grid.walkable(x, y), "and its walls can't be walked through");
    assert_eq!(g.igloo_on(x, y), Some(new));
    assert!(!builders.is_empty());
    assert!(builders.iter().any(|&b| g.penguins[b as usize].home == new), "its builders live there");

    // It raises its own Chieftain and calls in fishers (no guards).
    run(&mut g, 5);
    assert!(g.penguins.iter().any(|p| p.owner == 0 && p.kind == Kind::Chieftain && p.home == new), "no Chieftain for the new igloo");
    assert_eq!(g.igloos[n].fish_want, IGLOO_FISH_SLOTS, "its two fishing slots");

    // The client learns where it is and whose it is.
    view.1.apply(&view.0.encode(&g, &[])).unwrap();
    assert_eq!(view.1.info.igloos.len(), n + 1);
    assert_eq!(view.1.info.igloos[n].door, g.igloos[n].door);
    assert_eq!(view.1.igloos[n].owner, 0);
    // A client that joins later hears of it too.
    let mut late = (SnapshotEncoder::new(), WorldView::new(GameInfo::from_game(&g, 1)));
    late.1.apply(&late.0.encode(&g, &[])).unwrap();
    assert_eq!(late.1.info.igloos.len(), n + 1);
}

#[test]
fn a_computer_with_workers_to_spare_builds_an_igloo() {
    let setups = (0..2).map(|i| PlayerSetup { name: format!("AI {i}"), color: i as u8, team: i as u8, ai: Some(AiLevel::Hard) }).collect();
    let mut g = Game::new_with(map("pinewood-hollow"), setups, 3, CoreRules::default().map_start());
    let at_start: Vec<usize> = (0..2u8).map(|p| g.igloos.iter().filter(|i| i.owner == p).count()).collect();
    let n = g.igloos.len();
    run(&mut g, 15 * 60);
    let built = g.igloos.len() - n;
    let notes: String = g.ai_notes().iter().map(|(_, l)| l.join("\n")).collect();
    assert!(built > 0, "no computer built an igloo (they had {at_start:?}):\n{}", notes.lines().filter(|l| l.contains("igloo")).collect::<Vec<_>>().join("\n"));
}
