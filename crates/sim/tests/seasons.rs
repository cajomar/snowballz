//! Seasons and blizzards.

use sim::game::*;
use sim::map::Map;
use sim::math::v2;
use sim::proto::{GameInfo, SnapshotEncoder, WorldView};

fn game() -> Game {
    let path = format!("{}/../../assets/maps/fixtures/trio.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: None }).collect();
    Game::new_with(map, setups, 9, sim::game::CoreRules::default().map_start())
}

fn calm(g: &mut Game) {
    for p in &mut g.penguins {
        p.snowballs = 0;
    }
}

#[test]
fn the_year_turns_and_clients_see_it() {
    let mut g = game();
    let mut enc = SnapshotEncoder::new();
    let mut view = WorldView::new(GameInfo::from_game(&g, 0));
    assert_eq!(g.weather.season, Season::DeepWinter);
    let mut seen = Vec::new();
    for _ in 0..SEASON_TICKS * 4 + 5 {
        calm(&mut g);
        g.tick();
        let ev = std::mem::take(&mut g.events);
        for e in &ev {
            if let Event::SeasonChanged { season } = e {
                seen.push(*season);
            }
        }
        view.apply(&enc.encode(&g, &ev)).unwrap();
    }
    assert_eq!(seen, vec![Season::Thaw, Season::Midsummer, Season::PolarNight, Season::DeepWinter]);
    assert_eq!(view.weather.season, g.weather.season);
    assert_eq!(view.weather.season_t, g.weather.season_t);
}

#[test]
fn the_thaw_slumps_snow_buildings() {
    let mut g = game();
    let ig = g.igloos.iter().position(|i| i.owner == 0).unwrap();
    let door = g.igloos[ig].door;
    let t = g.find_site(0, BuildKind::Palisade, door + v2(4.0, 0.0), 6).unwrap();
    g.igloos[ig].wood = 20;
    let workers: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Worker).map(|p| p.id).collect();
    g.command(0, Command::Build { units: workers, kind: BuildKind::Palisade, tiles: vec![t] });
    for _ in 0..30 * sim::TICK_RATE {
        calm(&mut g);
        g.tick();
    }
    let wall = g.buildings[0].clone();
    assert!(wall.done());
    // Send everyone home so nobody repairs it, then let it thaw.
    for p in &mut g.penguins {
        p.pos = g.igloos[p.home as usize].door;
    }
    let all: Vec<u16> = g.penguins.iter().filter(|p| p.owner == 0).map(|p| p.id).collect();
    g.command(0, Command::Stop { units: all });
    g.weather.season = Season::Thaw;
    g.weather.season_t = 0;
    for _ in 0..THAW_MELT_TICKS * 3 {
        calm(&mut g);
        g.tick();
    }
    assert!(g.buildings[0].hp < wall.hp, "hp {} -> {}", wall.hp, g.buildings[0].hp);
}

#[test]
fn blizzards_shorten_throws_and_chill_but_never_send_penguins_home() {
    let mut g = game();
    let sb = g.penguins.iter().find(|p| p.owner == 0 && p.kind == Kind::Snowballer).unwrap().id;
    let enemy = g.penguins.iter().find(|p| p.owner == 1 && p.kind == Kind::Snowballer).unwrap().id;
    // Out in the open, well away from home.
    // Flat ground away from home, so height doesn't change the range.
    let door = g.igloos[g.penguins[sb as usize].home as usize].door;
    let at = (0..g.map.h)
        .flat_map(|y| (0..g.map.w).map(move |x| v2(x as f32 + 0.5, y as f32 + 0.5)))
        .filter(|&p| p.dist(door) > 10.0 && g.grid.walkable_at(p) && g.grid.walkable_at(p + v2(5.2, 0.0)))
        .find(|&p| (g.map.height_at(p) - g.map.height_at(p + v2(5.2, 0.0))).abs() < 0.02)
        .unwrap();
    let place = |g: &mut Game| {
        g.penguins[sb as usize].pos = at;
        g.penguins[enemy as usize].pos = at + v2(5.2, 0.0);
    };
    // In clear weather they're in range of each other (6 tiles)...
    let mut thrown = 0;
    for _ in 0..3 * sim::TICK_RATE {
        place(&mut g);
        g.penguins[sb as usize].snowballs = MAX_SNOWBALLS;
        g.penguins[enemy as usize].snowballs = 0;
        g.tick();
        thrown += g.events.drain(..).filter(|e| matches!(e, Event::Thrown(_))).count();
    }
    assert!(thrown > 0, "threw in clear weather");
    // ...but not in a blizzard.
    g.weather.blizzard = BLIZZARD_TICKS;
    // Let any throw already in its wind-up finish first.
    for _ in 0..sim::TICK_RATE {
        place(&mut g);
        g.tick();
        g.events.clear();
    }
    let mut thrown = 0;
    for _ in 0..3 * sim::TICK_RATE {
        place(&mut g);
        g.penguins[sb as usize].snowballs = MAX_SNOWBALLS;
        g.penguins[enemy as usize].snowballs = 0;
        g.tick();
        thrown += g.events.drain(..).filter(|e| matches!(e, Event::Thrown(_))).count();
    }
    assert_eq!(thrown, 0, "threw 5.2 tiles in a blizzard");
    // A long blizzard chills, down to a floor that doesn't send it home.
    let start = g.penguins[sb as usize].warmth;
    for _ in 0..40 * sim::TICK_RATE {
        g.weather.blizzard = BLIZZARD_TICKS;
        place(&mut g);
        calm(&mut g);
        g.tick();
    }
    let p = &g.penguins[sb as usize];
    assert!(p.warmth < start, "chilled: {start} -> {}", p.warmth);
    assert_eq!(p.warmth, BLIZZARD_CHILL_FLOOR);
    assert!(!p.cold && p.inside.is_none());
}

#[test]
fn midsummer_fish_bite_faster_at_the_bank() {
    let caught = |season: Season| {
        let mut g = game();
        g.weather.season = season;
        let w = g.penguins.iter().find(|p| p.owner == 0 && p.kind == Kind::Worker).unwrap().id;
        let bank = g.shores.iter().min_by(|a, b| a.pos.dist2(g.penguins[w as usize].pos).partial_cmp(&b.pos.dist2(g.penguins[w as usize].pos)).unwrap()).unwrap().pos;
        g.penguins[w as usize].pos = bank;
        // Sent to fish by hand, so it fishes whatever the rules on slots;
        // count each fish it lands.
        let water = [(0, 1), (1, 0), (-1, 0), (0, -1)].iter().map(|&(dx, dy)| (bank.tile().0 + dx, bank.tile().1 + dy)).find(|&(x, y)| g.map.ground_at(x, y) == sim::map::Ground::Water).unwrap();
        g.command(0, Command::Fish { units: vec![w], tile: water });
        let mut landed = 0;
        for _ in 0..120 * sim::TICK_RATE {
            g.weather.season_t = 0;
            calm(&mut g);
            g.penguins[w as usize].warmth = 255;
            g.penguins[w as usize].cold = false;
            let had = g.penguins[w as usize].carry == Carry::Fish;
            g.tick();
            landed += (!had && g.penguins[w as usize].carry == Carry::Fish) as u32;
        }
        landed
    };
    let (summer, winter) = (caught(Season::Midsummer), caught(Season::DeepWinter));
    assert!(summer > winter, "summer {summer} winter {winter}");
}
