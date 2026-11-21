//! The ground changes as it's used: chopped trees and worn-out snow.

use sim::game::{Game, Kind, PlayerSetup, SNOW_DEPTH};
use sim::map::Map;

fn game(map_name: &str) -> Game {
    let path = format!("{}/../../assets/maps/fixtures/{map_name}.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = vec![PlayerSetup { name: "Me".into(), color: 0, team: 0, ai: None }];
    Game::new_with(map, setups, 7, sim::game::CoreRules::default().map_start())
}

#[test]
fn snowballers_wear_out_the_snow_and_it_falls_again() {
    let mut g = game("trio");
    // Every snowballer scoops where it stands.
    for p in &mut g.penguins {
        if p.kind == Kind::Snowballer {
            p.snowballs = 0;
        }
    }
    let sb: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Snowballer).map(|p| p.id).collect();
    let spot = g.penguins[sb[0] as usize].pos;
    let i = g.map.idx(spot.tile().0, spot.tile().1);
    assert_eq!(g.terrain.snow[i], SNOW_DEPTH);
    // Throw snowballs away by scooping over and over on one tile.
    // No snowfall during the thaw, so the scooping shows plainly.
    g.weather.season = sim::game::Season::Thaw;
    for _ in 0..SNOW_DEPTH {
        for _ in 0..3 * sim::TICK_RATE {
            g.penguins[sb[0] as usize].snowballs = 0;
            g.penguins[sb[0] as usize].pos = spot;
            g.tick();
        }
    }
    assert_eq!(g.terrain.snow[i], 0, "scooped down to the ice");
    assert_eq!(g.map.ground[i], sim::map::Ground::Ice);
    // Out of ammo on bare ice, it goes looking for snow.
    g.penguins[sb[0] as usize].snowballs = 0;
    for _ in 0..4 * sim::TICK_RATE {
        g.tick();
    }
    assert!(g.penguins[sb[0] as usize].snowballs > 0, "found snow to scoop");
    // Snow returns given time.
    g.weather.season = sim::game::Season::DeepWinter;
    for _ in 0..600 * sim::TICK_RATE {
        g.weather.season_t = 0;
        g.tick();
    }
    assert!(g.terrain.snow[i] > 0, "snowfall covered the ice again");
}
