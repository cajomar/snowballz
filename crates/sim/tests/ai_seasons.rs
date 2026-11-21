//! The AI's reactions to seasons and blizzards.

use sim::ai::AiLevel;
use sim::game::*;
use sim::map::Map;
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};

fn game() -> Game {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2)
        .map(|i| PlayerSetup {
            name: format!("P{i}"),
            color: i as u8,
            team: i as u8,
            ai: Some(AiLevel::Hard),
        })
        .collect();
    Game::new(map, setups, 7)
}

/// True while some igloo is being taken over.
fn taken_over(g: &Game) -> bool {
    g.igloos.iter().any(|i| i.takeover.is_some())
}

#[test]
fn a_blizzard_keeps_the_ai_from_starting_attacks() {
    let mut g = game();
    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    // Hold the storm for the whole fight.
    g.weather.blizzard = BLIZZARD_TICKS;
    let mut starts = 0;
    for _ in 0..60 * sim::TICK_RATE {
        g.weather.blizzard = BLIZZARD_TICKS;
        g.tick();
        let ev = std::mem::take(&mut g.events);
        for e in &ev {
            if matches!(e, Event::TakeoverStart { .. }) {
                starts += 1;
            }
        }
        assert!(!taken_over(&g), "a takeover is under way in the blizzard");
        view.apply(&enc.encode(&g, &ev)).unwrap();
    }
    assert!(g.weather.blizzard > 0, "the blizzard never blew");
    assert_eq!(starts, 0, "a takeover started during the blizzard");
}
