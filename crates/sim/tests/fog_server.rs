//! The server sends each player only what their team can see.

use sim::fog::Fov;
use sim::game::*;
use sim::map::Map;
use sim::math::Vec2;
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};

fn game() -> Game {
    let path = format!("{}/../../assets/maps/pinewood-hollow.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    let mut g = Game::new_with(map, setups, 4, sim::game::CoreRules::default().map_start());
    g.weather.calm = true;
    g
}

fn step(g: &mut Game, enc: &mut SnapshotEncoder, view: &mut WorldView, fog: bool) {
    g.tick();
    let ev = std::mem::take(&mut g.events);
    let fov = fog.then(|| Fov::compute(g, g.team(0)));
    view.apply(&enc.encode_for(g, &ev, fov.as_ref())).unwrap();
}

fn far_enemy(g: &Game) -> u16 {
    g.penguins.iter().filter(|p| p.owner == 1).max_by(|a, b| a.pos.dist2(g.igloos[0].center).partial_cmp(&b.pos.dist2(g.igloos[0].center)).unwrap()).unwrap().id
}

#[test]
fn enemies_out_of_sight_are_not_in_the_snapshot() {
    let mut g = game();
    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    step(&mut g, &mut enc, &mut view, true);
    let foe = far_enemy(&g);
    let v = &view.penguins[foe as usize];
    assert_eq!(v.owner, NEUTRAL, "the viewer is told nothing of who it is");
    assert!(v.inside && v.pos == Vec2::ZERO, "nor where");
    // Our own are all there.
    for p in g.penguins.iter().filter(|p| p.owner == 0) {
        assert_eq!(view.penguins[p.id as usize].owner, 0);
        assert!(view.penguins[p.id as usize].pos.dist(p.pos) < 0.1);
    }
    // Without a field of view (a spectator), everyone is.
    let mut enc2 = SnapshotEncoder::new();
    let mut spec = WorldView::new(GameInfo::from_game(&g, 0));
    step(&mut g, &mut enc2, &mut spec, false);
    assert_eq!(spec.penguins[foe as usize].owner, 1);
}

#[test]
fn walking_into_view_shows_them_and_enemy_buildings_are_remembered() {
    let mut g = game();
    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    step(&mut g, &mut enc, &mut view, true);

    // An enemy builds far away: we know nothing of it.
    let ig1 = g.igloos.iter().find(|i| i.owner == 1).unwrap().clone();
    g.igloos.iter_mut().filter(|i| i.owner == 1).for_each(|i| i.wood = 30);
    let t = g.find_site(1, BuildKind::Campfire, ig1.door, 6).expect("a site");
    let w = g.penguins.iter().find(|p| p.owner == 1 && p.kind == Kind::Worker).unwrap().id;
    g.command(1, Command::Build { units: vec![w], kind: BuildKind::Campfire, tiles: vec![t] });
    for _ in 0..4 {
        step(&mut g, &mut enc, &mut view, true);
    }
    assert!(view.buildings.iter().all(|b| !b.alive || b.owner == 0), "an unseen enemy site is not in our picture");

    // One of ours walks there: now it is.
    let scout = g.penguins.iter().find(|p| p.owner == 0).unwrap().id;
    g.penguins[scout as usize].pos = ig1.door;
    step(&mut g, &mut enc, &mut view, true);
    assert!(view.buildings.iter().any(|b| b.alive && b.owner == 1), "seen once we are there");
    assert_eq!(view.penguins[w as usize].owner, 1, "and so is its builder");

    // We leave: the building is remembered, the people are gone from view.
    g.penguins[scout as usize].pos = g.igloos[0].center;
    for _ in 0..2 {
        step(&mut g, &mut enc, &mut view, true);
    }
    assert!(view.buildings.iter().any(|b| b.alive && b.owner == 1), "remembered");
    assert_eq!(view.penguins[w as usize].owner, NEUTRAL, "but nobody is seen");
}

#[test]
fn events_out_of_sight_are_dropped_and_a_snowball_far_away_is_not_sent() {
    let mut g = game();
    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    step(&mut g, &mut enc, &mut view, true);
    let a = g.penguins.iter().find(|p| p.owner == 1 && p.kind == Kind::Snowballer).unwrap().id;
    let foe_pos = g.penguins[a as usize].pos;
    let fov = Fov::compute(&g, 0);
    assert!(!fov.sees(foe_pos), "the enemy is far off");
    let ev = vec![Event::Frozen { penguin: a }, Event::Landed { id: 1, pos: foe_pos, hit: HitKind::Direct }, Event::Blizzard { on: true }];
    let bytes = enc.encode_for(&g, &ev, Some(&fov));
    let events = view.apply(&bytes).unwrap();
    assert_eq!(events.len(), 1, "only the blizzard is news: {events:?}");
}
