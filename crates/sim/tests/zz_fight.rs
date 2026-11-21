use sim::ai::AiLevel;
use sim::game::*;
use sim::map::Map;
#[test]
#[ignore]
fn fight() {
    let path = format!("{}/../../assets/maps/glacier-crossroads.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..4).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: Some(AiLevel::Hard) }).collect();
    let mut g = Game::new_with(map, setups, 4, CoreRules::default());
    g.roll_ai_tempers(4);
    let mut n = 0;
    for _ in 0..12 * 60 * 20 {
        g.tick();
        for e in std::mem::take(&mut g.events) {
            if let Event::Frozen { penguin, .. } = e {
                let p = &g.penguins[penguin as usize];
                if n < 12 { println!("{}s frozen P{} at {:.0},{:.0}", g.tick / 20, p.owner, p.pos.x, p.pos.y); }
                n += 1;
            }
        }
    }
    println!("total frozen {n}");
}
