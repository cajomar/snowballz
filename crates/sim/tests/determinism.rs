//! A game replayed from the same map and seed must play out exactly the same.

use sim::ai::AiLevel;
use sim::game::{CoreRules, Game, PlayerSetup};
use sim::map::Map;

fn play(core: CoreRules) -> Game {
    play_seed(core, 42)
}

fn play_seed(core: CoreRules, seed: u64) -> Game {
    let path = format!("{}/../../assets/maps/pinewood-hollow.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2)
        .map(|i| PlayerSetup {
            name: format!("AI {i}"),
            color: i as u8,
            team: i as u8,
            ai: Some(if i % 2 == 0 { AiLevel::Normal } else { AiLevel::Hard }),
        })
        .collect();
    let mut g = Game::new_with(map, setups, seed, core);
    for _ in 0..5 * 60 * sim::TICK_RATE {
        g.tick();
    }
    g
}

fn assert_same(a: &Game, b: &Game) {
    assert_eq!(a.tick, b.tick);
    let penguins = |g: &Game| g.penguins.iter().map(|p| (p.pos, p.warmth, p.owner)).collect::<Vec<_>>();
    assert_eq!(penguins(a), penguins(b));
    let igloos = |g: &Game| g.igloos.iter().map(|i| (i.owner, i.hp)).collect::<Vec<_>>();
    assert_eq!(igloos(a), igloos(b));
    let bits = |g: &Game| g.penguins.iter().map(|p| (p.kind as u8, p.act as u8, p.snowballs, p.courage, p.home)).collect::<Vec<_>>();
    assert_eq!(bits(a), bits(b));
    let buildings = |g: &Game| g.buildings.iter().map(|b| (b.kind as u8, b.owner, b.tile, b.hp, b.built, b.alive)).collect::<Vec<_>>();
    assert_eq!(buildings(a), buildings(b));
    let stores = |g: &Game| g.igloos.iter().map(|i| (i.fish, i.wood)).collect::<Vec<_>>();
    assert_eq!(stores(a), stores(b));
    assert_eq!(a.weather.season as u8, b.weather.season as u8);
}

#[test]
fn another_seed_plays_differently() {
    // Otherwise "same seed, same game" would pass by never using the seed.
    let (a, b) = (play_seed(CoreRules::default(), 42), play_seed(CoreRules::default(), 43));
    let pos = |g: &Game| g.penguins.iter().map(|p| p.pos).collect::<Vec<_>>();
    assert_ne!(pos(&a), pos(&b));
}

#[test]
fn same_seed_same_game_new_core() {
    assert_same(&play(CoreRules::default()), &play(CoreRules::default()));
}
