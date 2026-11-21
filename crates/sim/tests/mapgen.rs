//! Generated maps: always playable, deterministic, and specs survive JSON.

use sim::json::Json;
use sim::map::{Ground, Map, Obstacle, IGLOO_SIZE};
use sim::mapgen::*;
use sim::path::Grid;
use sim::rng::Rng;

/// A spec with every knob set at random, as a careless model might.
fn random_spec(rng: &mut Rng, seed: u64) -> MapSpec {
    let kinds = ["hill", "crater", "ridge", "lake", "forest", "spires", "clearing", "crystal", "island", "volcano"];
    let layouts = ["ring", "corners", "sides", "scattered"];
    let feats: Vec<String> = (0..rng.below(10))
        .map(|_| format!("{{\"kind\":\"{}\",\"x\":{},\"y\":{},\"size\":{}}}", kinds[rng.below(kinds.len() as u32) as usize], rng.f32(), rng.f32(), rng.f32()))
        .collect();
    let j = format!(
        "{{\"size\":{},\"igloos\":{},\"layout\":\"{}\",\"hills\":{},\"ice\":{},\"trees\":{},\"spires\":{},\"features\":[{}]}}",
        40 + rng.below(140),
        1 + rng.below(12),
        layouts[rng.below(4) as usize],
        rng.f32(),
        rng.f32(),
        rng.f32(),
        rng.f32(),
        feats.join(","),
    );
    MapSpec::from_json(&Json::parse(&j).unwrap(), seed)
}

#[test]
fn random_specs_make_playable_maps() {
    // Whatever the spec says, every igloo and resource can be reached and each igloo has fish.
    let mut rng = Rng::new(3);
    for seed in 0..60 {
        let spec = random_spec(&mut rng, seed);
        let map = generate(&spec);
        assert_eq!(map.igloos.len(), spec.igloos, "{spec:?}");
        if let Err(e) = check(&map) {
            panic!("seed {seed}: {e}\n{spec:?}");
        }
    }
}

#[test]
fn same_seed_same_map() {
    // Generation is deterministic, so a spec and seed are enough to share a map.
    let spec = MapSpec::from_words("icy forest for 5", 11);
    assert_eq!(generate(&spec).encode(), generate(&spec).encode());
}

#[test]
fn spec_round_trips_through_json() {
    // What the game writes out reads back the same.
    let spec = MapSpec::from_words("volcano crater crystal race with spires", 4);
    let back = MapSpec::from_json(&Json::parse(&spec.to_json()).unwrap(), 4);
    assert_eq!(back.igloos, spec.igloos);
    assert_eq!(back.layout, spec.layout);
    assert_eq!(back.features.len(), spec.features.len());
    assert!((back.trees - spec.trees).abs() < 0.01);
}

#[test]
fn model_reply_wrapped_in_prose_still_parses() {
    // Models often add chatter or code fences around the JSON.
    let reply = "Sure! Here's your map:\n```json\n{\"name\": \"Glacier Gap\", \"igloos\": 3, \"ice\": 0.9}\n```\nEnjoy!";
    let spec = MapSpec::from_json(&Json::find_object(reply).unwrap(), 1);
    assert_eq!(spec.name, "Glacier Gap");
    assert_eq!(spec.igloos, 3);
    assert!(spec.ice > 0.85);
}

#[test]
fn every_map_has_crystal_to_fight_over() {
    // Crystal pays for Ice Palaces, so every map has some.
    for (words, seed) in [("crystal mines for 6", 2), ("forest lakes 4", 9), ("duel", 3)] {
        let map = generate(&MapSpec::from_words(words, seed));
        assert!(map.resources.iter().any(|r| r.kind == sim::map::ResourceKind::Crystal), "{words}");
    }
}

#[test]
fn generated_maps_play() {
    // A generated map runs a real game without trouble.
    let map = generate(&MapSpec::from_words("forest lakes 4", 9));
    let setups = (0..4)
        .map(|i| sim::game::PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: Some(sim::ai::AiLevel::Normal) })
        .collect();
    // A smoke test: four computers play a minute on a generated map.
    let mut g = sim::game::Game::new_with(map, setups, 1, sim::game::CoreRules::default().map_start());
    for _ in 0..60 * sim::TICK_RATE {
        g.tick();
    }
    assert!(g.penguins.len() > 4 * 18);
}

#[test]
fn shipped_maps_match_their_specs() {
    // Every map in assets/maps is exactly what its spec in assets/mapspecs builds,
    // so nobody edits one and forgets the other (rebuild with the make_maps example).
    let root = format!("{}/../..", env!("CARGO_MANIFEST_DIR"));
    let mut specs: Vec<_> = std::fs::read_dir(format!("{root}/assets/mapspecs")).unwrap().flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    assert!(specs.len() >= 8);
    specs.extend(std::fs::read_dir(format!("{root}/assets/mapspecs/fixtures")).unwrap().flatten().map(|e| e.path()));
    for path in specs {
        let j = Json::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let seed = j.get("seed").and_then(Json::as_f64).unwrap_or(1.0) as u64;
        let built = generate(&MapSpec::from_json(&j, seed)).encode();
        let sub = if path.parent().unwrap().ends_with("fixtures") { "fixtures/" } else { "" };
        let shipped = std::fs::read(format!("{root}/assets/maps/{sub}{}.sbmap", path.file_stem().unwrap().to_string_lossy())).unwrap();
        assert!(built == shipped, "{} is out of date: run cargo run -p sim --release --example make_maps", path.display());
    }
}

#[test]
fn the_test_arena_matches_its_file() {
    let root = format!("{}/../..", env!("CARGO_MANIFEST_DIR"));
    let shipped = std::fs::read(format!("{root}/assets/maps/fixtures/trio.sbmap")).unwrap();
    assert!(sim::mapgen::fixture_arena().encode() == shipped, "trio.sbmap is out of date: run cargo run -p sim --release --example make_maps");
}

/// A map with one feature in the middle and nothing else to muddy it.
fn one_feature(kind: FeatureKind, size: f32, ice: f32) -> Map {
    let spec = MapSpec {
        hills: 0.0,
        ice,
        trees: 0.0,
        spires: 0.0,
        igloos: 2,
        features: vec![Feature { kind, x: 0.5, y: 0.5, size }],
        ..MapSpec::default()
    };
    generate(&spec)
}

/// Ground at a point in tile coordinates.
fn ground_at(map: &Map, x: f32, y: f32) -> Ground {
    map.ground[map.idx(x.floor() as i32, y.floor() as i32)]
}

#[test]
fn island_is_snow_ringed_by_ice() {
    // The landmass is snow, and a ring of ice runs all the way round it.
    let map = one_feature(FeatureKind::Island, 0.5, 0.0);
    let c = 60.0;
    let r = 4.0 + 0.5 * 120.0 * 0.3;
    assert_eq!(ground_at(&map, c, c), Ground::Snow);
    for k in 0..36 {
        let a = k as f32 * std::f32::consts::TAU / 36.0;
        let (x, y) = (c + (r + 1.5) * a.cos(), c + (r + 1.5) * a.sin());
        assert_eq!(ground_at(&map, x, y), Ground::Ice, "no ice at angle {a}");
    }
}

#[test]
fn volcano_has_raised_rim_and_bare_warm_middle() {
    // The rim stands above both the crater floor and the land outside, and the middle is snow with nothing growing on it.
    let map = one_feature(FeatureKind::Volcano, 0.4, 0.8);
    let c = 60.0;
    let r = 4.0 + 0.4 * 120.0 * 0.3;
    let mean = |lo: f32, hi: f32| {
        let (mut sum, mut n) = (0.0, 0);
        for y in 0..map.h {
            for x in 0..map.w {
                let d = ((x as f32 + 0.5 - c).powi(2) + (y as f32 + 0.5 - c).powi(2)).sqrt();
                if d >= lo && d < hi {
                    sum += map.height_raw[map.idx(x, y)] as f32;
                    n += 1;
                }
            }
        }
        sum / n as f32
    };
    let rim = mean(0.8 * r, r);
    assert!(rim > mean(0.0, 0.3 * r) + 60.0, "rim {rim} doesn't stand above the crater floor");
    assert!(rim > mean(1.6 * r, 2.0 * r) + 60.0, "rim {rim} doesn't stand above the land outside");
    for y in 0..map.h {
        for x in 0..map.w {
            let d = ((x as f32 + 0.5 - c).powi(2) + (y as f32 + 0.5 - c).powi(2)).sqrt();
            if d < 0.5 * r {
                let i = map.idx(x, y);
                assert_eq!(map.ground[i], Ground::Snow, "ice in the warm middle at {x},{y}");
                assert_eq!(map.obstacle[i], Obstacle::None, "something growing in the warm middle at {x},{y}");
            }
        }
    }
}

#[test]
fn archipelago_igloos_all_reachable() {
    // Each igloo gets its own island in a frozen sea. The sea can be walked, so every igloo still has a way in.
    for seed in 0..12 {
        let mut spec = MapSpec::from_words("frozen archipelago with a central volcanic island", seed);
        assert_eq!(spec.layout, Layout::Scattered);
        assert!(spec.features.iter().any(|f| f.kind == FeatureKind::Island));
        assert!(spec.features.iter().any(|f| f.kind == FeatureKind::Volcano));
        spec.igloos = 8;
        let map = generate(&spec);
        assert_eq!(map.igloos.len(), 8);
        if let Err(e) = check(&map) {
            panic!("seed {seed}: {e}");
        }
        let ice = map.ground.iter().filter(|&&g| g == Ground::Ice).count();
        assert!(ice * 2 > map.ground.len(), "seed {seed}: the sea isn't mostly ice");
    }
}

#[test]
fn new_features_read_from_json_with_aliases() {
    // Models say "isle" or "volcanic" as often as the exact names; both must work and survive a round trip.
    let j = Json::parse(r#"{"features":[{"kind":"isle","x":0.5,"y":0.5},{"kind":"Volcanic","x":0.3,"y":0.6,"size":0.2}]}"#).unwrap();
    let spec = MapSpec::from_json(&j, 1);
    assert_eq!(spec.features.iter().map(|f| f.kind).collect::<Vec<_>>(), [FeatureKind::Island, FeatureKind::Volcano]);
    let back = MapSpec::from_json(&Json::parse(&spec.to_json()).unwrap(), 1);
    assert_eq!(back.features, spec.features);
}

#[test]
fn every_igloo_walks_to_every_other() {
    // No matter the spec, every igloo must be able to walk to every other.
    // We build the same walk grid the game does in Game::new: everything
    // except obstacles, igloo floors and resource tiles is walkable, then
    // flood fill from each igloo's door the way PathFinder moves (diagonals
    // allowed, no corner cutting). Every other door must be in the component.
    let sizes = [MIN_SIZE, 96, 120, 150, MAX_SIZE];
    let mut rng = Rng::new(401);
    let kinds = ["hill", "crater", "ridge", "lake", "forest", "spires", "clearing", "crystal", "island", "volcano"];
    for seed in 0..20 {
        for size in sizes {
            let layouts = ["ring", "corners", "sides", "scattered"];
            let feats: Vec<String> = (0..rng.below(9))
                .map(|_| {
                    format!(
                        "{{\"kind\":\"{}\",\"x\":{:.2},\"y\":{:.2},\"size\":{:.2}}}",
                        kinds[rng.below(kinds.len() as u32) as usize],
                        rng.f32(),
                        rng.f32(),
                        rng.f32()
                    )
                })
                .collect();
            let j = format!(
                "{{\"size\":{size},\"igloos\":{},\"layout\":\"{}\",\"hills\":{:.2},\"ice\":{:.2},\"trees\":{:.2},\"spires\":{:.2},\"features\":[{}]}}",
                2 + rng.below(9),
                layouts[rng.below(4) as usize],
                rng.f32(),
                rng.f32(),
                rng.f32(),
                rng.f32(),
                feats.join(",")
            );
            let spec = MapSpec::from_json(&Json::parse(&j).unwrap(), 1000 * seed + size as u64);
            let spec_json = spec.to_json();
            let map = generate(&spec);
            assert_eq!(map.igloos.len(), spec.igloos, "{spec_json}");

            let n = (map.w * map.h) as usize;
            let mut walk = vec![true; n];
            for y in 0..map.h {
                for x in 0..map.w {
                    let i = map.idx(x, y);
                    walk[i] = !map.obstacle[i].blocks();
                }
            }
            for &(ix, iy) in &map.igloos {
                for y in iy as i32..iy as i32 + IGLOO_SIZE {
                    for x in ix as i32..ix as i32 + IGLOO_SIZE {
                        if map.in_bounds(x, y) {
                            walk[map.idx(x, y)] = false;
                        }
                    }
                }
            }
            for r in &map.resources {
                if map.in_bounds(r.x as i32, r.y as i32) {
                    walk[map.idx(r.x as i32, r.y as i32)] = false;
                }
            }
            let grid = Grid { w: map.w, h: map.h, walk, cost: vec![1.0; n] };

            // The game puts a door on the first walkable tile from this list.
            let door = |i: usize| {
                let (ix, iy) = (map.igloos[i].0 as i32, map.igloos[i].1 as i32);
                let mid = IGLOO_SIZE / 2;
                let candidates = [
                    (ix + mid, iy + IGLOO_SIZE),
                    (ix + mid - 1, iy + IGLOO_SIZE),
                    (ix + IGLOO_SIZE, iy + mid),
                    (ix - 1, iy + mid),
                    (ix + mid, iy - 1),
                ];
                candidates
                    .iter()
                    .copied()
                    .find(|&(x, y)| grid.walkable(x, y))
                    .or_else(|| grid.nearest_walkable((ix + mid, iy + IGLOO_SIZE), 6))
                    .expect("every igloo has a walkable door tile nearby")
            };
            let doors: Vec<(i32, i32)> = (0..map.igloos.len()).map(door).collect();

            for (i, &(sx, sy)) in doors.iter().enumerate() {
                let mut seen = vec![false; n];
                let mut stack = vec![(sx, sy)];
                while let Some((x, y)) = stack.pop() {
                    if !grid.walkable(x, y) || seen[map.idx(x, y)] {
                        continue;
                    }
                    seen[map.idx(x, y)] = true;
                    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                        // no corner cutting, like PathFinder
                        if dx != 0 && dy != 0 && !(grid.walkable(x + dx, y) && grid.walkable(x, y + dy)) {
                            continue;
                        }
                        stack.push((x + dx, y + dy));
                    }
                }
                for (j2, &(tx, ty)) in doors.iter().enumerate() {
                    if j2 != i {
                        assert!(seen[map.idx(tx, ty)], "seed {} size {}: igloo {i} can't walk to igloo {j2}\n{spec_json}", spec.seed, size);
                    }
                }
            }
        }
    }
}

#[test]
fn starts_have_narrow_passes_between_them() {
    // Frozen penguins clog narrow passes shut (DESIGN.md §0), so the
    // straight line between the two starts farthest apart (the main
    // front) must cross two passes at most four tiles wide: separate
    // stretches where you can't get more than that sideways, each with
    // somewhere to walk through.
    let mut rng = Rng::new(707);
    for seed in 0..20 {
        let spec = random_spec(&mut rng, seed);
        let map = generate(&spec);
        // The two starts farthest apart.
        let starts: Vec<sim::math::Vec2> = (0..map.igloos.len()).map(|i| map.igloo_center(i)).collect();
        let (mut best, mut far) = ((0, 1), 0.0_f32);
        for i in 0..starts.len() {
            for j in i + 1..starts.len() {
                let d = starts[i].dist(starts[j]);
                if d > far {
                    far = d;
                    best = (i, j);
                }
            }
        }
        let (a, b) = (starts[best.0], starts[best.1]);
        let apart = a.dist(b);
        let passes = narrow_passes(&map, a, b, 4);
        assert!(passes.len() >= 2, "seed {seed}: only {passes:?} narrow passes between the main starts {apart:.0} apart\n{}", spec.to_json());
        assert!(passes.iter().all(|&w| (2..=4).contains(&w)));
        assert!(apart >= 30.0, "seed {seed}: main starts only {apart:.0} apart");
    }
}

#[test]
fn starts_are_fair() {
    use sim::map::ResourceKind;
    use sim::math::v2;

    // What every starting igloo has nearby on 20 random maps: fish holes and
    // trees within 12 tiles.
    // Being dealt the richest start shouldn't be worth much: at most 1.5×
    // the fish of the poorest.
    let mut failing = Vec::new();
    let mut rng = Rng::new(902);
    for seed in 0..20 {
        let spec = random_spec(&mut rng, seed);
        let map = generate(&spec);
        let centres = (0..map.igloos.len()).map(|i| map.igloo_center(i)).collect::<Vec<_>>();
        let yard: Vec<[f32; 2]> = centres
            .iter()
            .map(|&c| {
                let fish = map
                    .resources
                    .iter()
                    .filter(|r| r.kind == ResourceKind::Fish && c.dist(v2(r.x as f32, r.y as f32)) < 12.0)
                    .count() as f32;
                let mut trees = 0.0;
                for y in ((c.y - 12.0) as i32).max(0)..=((c.y + 12.0) as i32).min(map.h - 1) {
                    for x in ((c.x - 12.0) as i32).max(0)..=((c.x + 12.0) as i32).min(map.w - 1) {
                        if map.obstacle[map.idx(x, y)] == Obstacle::Tree && c.dist(v2(x as f32 + 0.5, y as f32 + 0.5)) < 12.0 {
                            trees += 1.0;
                        }
                    }
                }
                [fish, trees]
            })
            .collect();
        let fish = yard.iter().map(|y| y[0]).collect::<Vec<_>>();
        let trees = yard.iter().map(|y| y[1]).collect::<Vec<_>>();
        let poorest = fish.iter().fold(f32::MAX, |a, &b| a.min(b));
        let ratio = fish.iter().fold(f32::MIN, |a, &b| a.max(b)) / poorest.max(1.0);
        let fmt = |vals: &[f32], p: usize, w: usize| vals.iter().map(|v| format!("{:>w$.p$}", v, w = w, p = p)).collect::<Vec<_>>().join(" ");
        let bad = !(ratio <= 1.5);
        println!(
            "seed {seed:>2}: fish {} trees {} ratio {ratio:.2}{}",
            fmt(&fish, 0, 3),
            fmt(&trees, 0, 4),
            if bad { " FAIL" } else { "" }
        );
        if bad {
            println!("  {}", spec.to_json());
            failing.push(seed);
        }
    }
    assert!(
        failing.is_empty(),
        "starts are unfair on seeds {failing:?}: the richest has more than 1.5× the poorest's fish"
    );
}

/// Share of tiles that match when the whole map is turned about its middle
/// by `quarter` quarter-turns (or mirrored left to right when `quarter` is 9).
fn likeness(map: &sim::map::Map, quarter: u32) -> f32 {
    let (w, h) = (map.w, map.h);
    let mut same = 0;
    for y in 0..h {
        for x in 0..w {
            let (tx, ty) = match quarter {
                1 => (h - 1 - y, x),
                2 => (w - 1 - x, h - 1 - y),
                3 => (y, w - 1 - x),
                _ => (w - 1 - x, y),
            };
            if tx < w && ty < h && map.ground[map.idx(x, y)] == map.ground[map.idx(tx, ty)] && map.obstacle[map.idx(x, y)] == map.obstacle[map.idx(tx, ty)] {
                same += 1;
            }
        }
    }
    same as f32 / (w * h) as f32
}

#[test]
fn shipped_maps_are_balanced_but_not_mirror_images() {
    // Tiles that are plain snow match under any turn by chance; a map copied
    // round its middle would match nearly everywhere.
    let dir = format!("{}/../../assets/maps", env!("CARGO_MANIFEST_DIR"));
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "sbmap") {
            continue;
        }
        let map = sim::map::Map::load_file(&path).unwrap();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        for q in [1, 2, 3, 9] {
            let like = likeness(&map, q);
            assert!(like < 0.93, "{name} is {:.0}% the same under turn {q}: a copy, not a map", like * 100.0);
        }
    }
}

#[test]
fn every_start_gets_a_fair_share_of_ice_and_wood_without_the_land_being_copied() {
    use sim::mapgen::{ice_near, trees_near, ICE_BAND, MAX_START_ICE, WOOD_REACH};
    let mut rng = Rng::new(31);
    for seed in 0..12 {
        let mut spec = random_spec(&mut rng, seed);
        spec.igloos = 4;
        spec.size = 128;
        spec.layout = Layout::Ring;
        spec.ice = 0.7;
        let map = generate(&spec);
        let ice: Vec<f32> = (0..4).map(|i| ice_near(&map, i)).collect();
        let (lo, hi) = ice.iter().fold((1.0f32, 0.0f32), |(l, h), &x| (l.min(x), h.max(x)));
        assert!(hi <= MAX_START_ICE + 0.03, "seed {seed}: a start is {:.0}% ice", hi * 100.0);
        assert!(hi - lo <= 2.0 * ICE_BAND + 0.12, "seed {seed}: ice runs {:.0}% to {:.0}%", lo * 100.0, hi * 100.0);
        let trees: Vec<usize> = (0..4).map(|i| trees_near(&map, map.igloo_center(i), WOOD_REACH)).collect();
        assert!(trees.iter().all(|&t| t >= MIN_TREES), "seed {seed}: trees {trees:?}");
    }
}
