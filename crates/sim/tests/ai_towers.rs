//! The computer builds watchtowers by its doors and mans them.

use sim::ai::AiLevel;
use sim::game::*;
use sim::map::Map;

#[test]
fn the_hard_ai_builds_and_mans_towers() {
    let path = format!("{}/../../assets/maps/pinewood-hollow.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("AI {i}"), color: i as u8, team: i as u8, ai: Some(AiLevel::Hard) }).collect();
    let mut g = Game::new_with(map, setups, 11, sim::game::CoreRules::default().map_start());
    g.weather.calm = true;
    let mut best = (0, 0);
    for _ in 0..14 * 60 * sim::TICK_RATE {
        g.tick();
        g.events.clear();
        let towers: Vec<u16> = g.buildings.iter().filter(|b| b.alive && b.done() && b.kind == BuildKind::Tower).map(|b| b.id).collect();
        let manned = towers.iter().map(|&b| g.garrison(b).count()).max().unwrap_or(0);
        best = (best.0.max(towers.len()), best.1.max(manned));
        if best.0 > 0 && best.1 >= 1 {
            break;
        }
    }
    assert!(best.0 > 0, "no tower was ever finished");
    assert!(best.1 >= 1, "towers stood empty (best garrison {})", best.1);
}

#[test]
fn the_ai_drafts_snowballers_trains_specialists_and_crews_stores() {
    let path = format!("{}/../../assets/maps/aurora-basin.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("AI {i}"), color: i as u8, team: i as u8, ai: Some(AiLevel::Hard) }).collect();
    let mut g = Game::new_with(map, setups, 5, sim::game::CoreRules::default().map_start());
    g.weather.calm = true;
    let mut most = 0;
    let mut trained = 0;
    let mut crewed = 0;
    for _ in 0..10 * 60 * sim::TICK_RATE {
        g.tick();
        trained += g.events.iter().filter(|e| matches!(e, Event::Trained { penguin, .. } if g.penguins[*penguin as usize].owner == 0)).count();
        g.events.clear();
        let army = g.penguins.iter().filter(|p| p.owner == 0 && p.kind.fights() && p.kind != Kind::Chieftain).count();
        most = most.max(army);
        let crew = g.penguins.iter().filter(|p| p.owner == 0 && matches!(p.fishery, Some(Fishery::House(_)))).count();
        crewed = crewed.max(crew);
    }
    assert!(most >= 8, "the army never grew past {most}");
    assert!(trained >= 1, "no camp trained a specialist");
    assert!(crewed >= FISH_SLOTS[0] as usize, "no Sushi House was built and crewed (best {crewed})");
}
