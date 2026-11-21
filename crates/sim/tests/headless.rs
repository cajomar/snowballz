//! Runs whole AI-vs-AI games without graphics as a smoke test of the rules.

use sim::ai::AiLevel;
use sim::game::{Act, Event, Game, PlayerSetup};
use sim::map::Map;
use sim::proto::{SnapshotEncoder, WorldView, GameInfo};

fn run(map_name: &str, players: usize, minutes: u32, verbose: bool) -> Game {
    run_with(map_name, players, minutes, verbose, sim::game::CoreRules::default())
}

fn run_with(map_name: &str, players: usize, minutes: u32, verbose: bool, core: sim::game::CoreRules) -> Game {
    let path = format!("{}/../../assets/maps/{map_name}.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..players)
        .map(|i| PlayerSetup {
            name: format!("AI {i}"),
            color: i as u8,
            team: i as u8,
            ai: Some(if i % 2 == 0 { AiLevel::Normal } else { AiLevel::Hard }),
        })
        .collect();
    // SB_SEED=n tries other games.
    let seed = std::env::var("SB_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(42);
    let mut g = Game::new_with(map, setups, seed, core);
    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    let (mut thrown, mut direct, mut splash, mut frozen, mut bytes) = (0, 0, 0, 0, 0usize);
    let (mut tk_start, mut tk_fail) = (0, 0);
    let (mut routs, mut jailed, mut turned) = (0, 0, 0);
    let mut war_cries = 0;
    let mut collapsed = 0;
    let (mut surfaced, mut snatched, mut driven) = (0, 0, 0);
    let start = std::time::Instant::now();
    let ticks = minutes * 60 * sim::TICK_RATE;
    let (mut tick_total, mut tick_worst, mut tick_count) = (0u128, 0u128, 0u32);
    let mut peak_penguins = g.penguins.len();
    for _ in 0..ticks {
        let t = std::time::Instant::now();
        g.tick();
        let us = t.elapsed().as_micros();
        tick_total += us;
        tick_worst = tick_worst.max(us);
        tick_count += 1;
        peak_penguins = peak_penguins.max(g.penguins.len());
        let events = std::mem::take(&mut g.events);
        for e in &events {
            match e {
                Event::Thrown(_) => thrown += 1,
                Event::Landed { hit: sim::game::HitKind::Direct, .. } => direct += 1,
                Event::Landed { hit: sim::game::HitKind::Splash, .. } => splash += 1,
                Event::Frozen { .. } => frozen += 1,
                Event::TakeoverStart { .. } => tk_start += 1,
                Event::TakeoverFailed { .. } => tk_fail += 1,
                Event::Routed { .. } => routs += 1,
                Event::Imprisoned { .. } => jailed += 1,
                Event::Converted { .. } => turned += 1,
                Event::WarCry { .. } => war_cries += 1,
                Event::Collapsed { .. } => collapsed += 1,
                Event::SealSurfaced { .. } => surfaced += 1,
                Event::SealSnatched { seal, penguin } => {
                    snatched += 1;
                    if verbose {
                        println!("[{}s] a seal snatched penguin {penguin} (seal {seal})", g.tick / 20);
                    }
                }
                Event::SealDriven { .. } => driven += 1,
                Event::Chat { from, text, .. } if verbose => println!("[{}] {}: {text}", g.tick / 20, g.players[*from as usize].name),
                Event::Captured { igloo, from, to } if verbose => println!("[{}s] igloo {igloo} captured {from} -> {to}", g.tick / 20),
                Event::Victory { team } if verbose => println!("[{}s] team {team} wins", g.tick / 20),
                Event::PlayerOut { player } if verbose => println!("[{}s] player {player} is out", g.tick / 20),
                _ => {}
            }
        }
        if verbose && g.tick % (60 * 20) == 0 {
            let mut line = format!("[{}s]", g.tick / 20);
            for pl in &g.players {
                let mine = |f: &dyn Fn(&sim::game::Penguin) -> bool| g.penguins.iter().filter(|p| p.owner == pl.id && p.kind == sim::game::Kind::Snowballer && f(p)).count();
                line += &format!(" | p{}: sb {} warm-out {} inside {} cold {}", pl.id, mine(&|_| true), mine(&|p| p.is_out() && !p.cold), mine(&|p| !p.is_out()), mine(&|p| p.cold));
            }
            println!("{line}\n      {:?}", g.ai_debug());
            for p in g.penguins.iter().filter(|p| p.cold && p.is_out()).take(12) {
                println!("        cold #{} owner {} act {:?} t {} warmth {} dist-home {:.1} pos {:?}", p.id, p.owner, p.act, p.act_t, p.warmth, p.pos.dist(g.igloos[p.home as usize].door), p.pos);
            }
        }
        let snap = enc.encode(&g, &events);
        bytes += snap.len();
        view.apply(&snap).unwrap();
        if g.winner.is_some() {
            break;
        }
    }
    let el = start.elapsed();
    let secs = g.tick as f32 / 20.0;
    let walking = g.penguins.iter().filter(|p| p.act == Act::Walk).count();
    let cold = g.penguins.iter().filter(|p| p.cold).count();
    println!(
        "{map_name}: {:.0}s game in {:.2?} ({:.3} ms/tick), {} penguins ({walking} walking, {cold} cold), thrown {thrown}, direct {direct}, splash {splash}, frozen {frozen}, takeovers {tk_start} (failed {tk_fail}), igloos collapsed {collapsed}, avg snapshot {:.0} B/s, winner {:?}",
        secs, el, el.as_secs_f64() * 1000.0 / g.tick as f64, g.penguins.len(), bytes as f32 / secs, g.winner
    );
    println!(
        "  ticks: {:.1} us average, {} us worst, {} penguins at most alive at once",
        tick_total as f64 / tick_count.max(1) as f64,
        tick_worst,
        peak_penguins
    );
    for ig in &g.igloos {
        println!("  igloo {} owner {} tier {} fish {} wood {} crystal {} pop {}", ig.id, ig.owner, ig.tier, ig.fish, ig.wood, ig.crystal,
            g.penguins.iter().filter(|p| p.home == ig.id).count());
    }
    println!("  research (snowcraft, icecraft, fishery): {:?}", g.players.iter().map(|p| p.research).collect::<Vec<_>>());
    let bare = g.terrain.snow.iter().zip(&g.map.ground).filter(|(&s, _)| s == 0).count();
    let stumps = g.map.obstacle.iter().filter(|&&o| o == sim::map::Obstacle::Stump).count();
    println!("  terrain: {} tiles changed, {bare} bare, {stumps} trees felled", g.terrain.touched.len());
    let mut kinds = std::collections::BTreeMap::new();
    for b in g.buildings.iter() {
        let e = kinds.entry(b.kind.name()).or_insert((0, 0, 0));
        e.0 += 1;
        e.1 += b.done() as u32;
        e.2 += (!b.alive) as u32;
    }
    println!("  buildings (placed, finished, lost): {kinds:?}");
    println!("  courage: {routs} routs; prisoners: {jailed} taken, {turned} turned");
    println!("  seals: {surfaced} surfacings, {snatched} snatches, {driven} driven off");
    let mut army = std::collections::BTreeMap::new();
    for p in &g.penguins {
        *army.entry(p.kind.stats().name).or_insert(0) += 1;
    }
    println!("  units: {army:?}");
    println!("  heroes: {war_cries} war cries");
    // Where the map's penguins ended up: per tribe, wild, or frozen.
    let frozen_now = g.penguins.iter().filter(|p| p.act == Act::Frozen).count();
    let wild_now = g.penguins.iter().filter(|p| p.wild).count();
    let tribe: Vec<usize> = g
        .players
        .iter()
        .map(|pl| g.penguins.iter().filter(|p| p.owner == pl.id).count())
        .collect();
    println!("  penguins at end: owned {tribe:?}, wild {wild_now}, frozen {frozen_now}");
    // A game that only stays in sync proves little: the AIs must really have
    // fought (a stalemate where nobody does anything fails here).
    assert!(thrown > 100, "the AIs hardly threw a snowball ({thrown})");
    assert!(frozen > 0, "nobody was ever frozen");
    let server: Vec<_> = g.buildings.iter().map(|b| (b.kind, b.owner, b.tile, b.hp, b.built, b.alive)).collect();
    let client: Vec<_> = view.buildings.iter().map(|b| (b.kind, b.owner, b.tile, b.hp, b.built, b.alive)).collect();
    assert_eq!(server, client);
    assert_eq!(view.research, g.players.iter().map(|p| p.research).collect::<Vec<_>>());
    // The client's copy of the changing ground stays in step.
    assert_eq!(view.snow, g.terrain.snow);
    assert_eq!(view.info.map.ground, g.map.ground);
    assert_eq!(view.info.map.obstacle, g.map.obstacle);
    assert_eq!(view.penguins.len(), g.penguins.len());
    // The client tracks every seal looking or diving.
    assert_eq!(view.seals.len(), g.seals.len());
    for (a, b) in view.seals.iter().zip(&g.seals) {
        assert_eq!(a.state, b.state);
        assert_eq!(a.hp, b.hp);
        assert!(a.pos.dist(b.pos) < 0.05);
    }
    for (a, b) in view.penguins.iter().zip(&g.penguins) {
        assert!(a.pos.dist(b.pos) < 0.05);
    }
    g
}

#[test]
fn aurora_basin_ai_battle() {
    run("aurora-basin", 3, 12, true);
}

#[test]
fn pinewood_hollow_two_players() {
    run("pinewood-hollow", 2, 25, true);
}

#[test]
fn crystal_rush() {
    run("crystal-caldera", 4, 25, true);
}

#[test]
fn pinewood_hollow_default_rules() {
    run_with("pinewood-hollow", 2, 15, false, sim::game::CoreRules::default());
}
