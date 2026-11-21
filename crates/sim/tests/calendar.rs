//! The year's clock: each season lasts four game minutes.

use sim::game::*;
use sim::map::Map;

fn game() -> Game {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    Game::new(map, setups, 9)
}

#[test]
fn each_season_lastes_four_game_minutes() {
    // Four game minutes in ticks: 240 s times the ticks-per-second constant.
    let four = 4 * 60 * sim::TICK_RATE;
    assert_eq!(SEASON_TICKS, four as u32, "seasons must last four game minutes");
    let mut g = game();
    let mut at = Vec::new();
    for _ in 1..=4 * four + sim::TICK_RATE {
        for p in &mut g.penguins {
            p.snowballs = 0;
        }
        g.tick();
        let ev = std::mem::take(&mut g.events);
        if ev.iter().any(|e| matches!(e, Event::SeasonChanged { .. })) {
            at.push(g.tick);
        }
    }
    assert_eq!(at, vec![four, 2 * four, 3 * four, 4 * four]);
}
