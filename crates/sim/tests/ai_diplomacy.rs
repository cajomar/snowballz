//! Computers deal with each other: a truce with one rival while fighting
//! another, and a pact against whoever runs away with the game. Offers and
//! answers are in their logs (no stock lines are chatted), and a truce binds both sides.

use sim::ai::{AiLevel, Plan, Temper};
use sim::game::*;
use sim::map::Map;

fn game(players: usize, seed: u64) -> Game {
    let path = format!("{}/../../assets/maps/glacier-crossroads.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..players)
        .map(|i| PlayerSetup { name: format!("Tux{i}"), color: i as u8, team: i as u8, ai: Some(AiLevel::Hard) })
        .collect();
    let mut g = Game::new_with(map, setups, seed, CoreRules::default().map_start());
    g.weather.calm = true;
    g
}

/// What the computers had to say to everyone in `seconds`, as "Name: line".
/// Offers and answers are stock lines, so they are in the logs, not the chat.
fn run(g: &mut Game, seconds: u32) -> Vec<String> {
    let since = g.tick;
    for _ in 0..seconds * sim::TICK_RATE {
        g.tick();
        for e in std::mem::take(&mut g.events) {
            assert!(!matches!(e, Event::Chat { to: None, .. }), "a stock line was chatted: {e:?}");
        }
    }
    let mut said: Vec<(u32, String)> = Vec::new();
    for (p, _, log) in g.ai_admin(since) {
        let name = &g.players[p as usize].name;
        said.extend(log.into_iter().filter_map(|(t, l)| l.strip_prefix("Thinks: ").map(|s| (t, format!("{name}: {}", s.trim_matches('"'))))));
    }
    said.sort_by_key(|(t, _)| *t);
    said.into_iter().map(|(_, l)| l).collect()
}

fn log(g: &Game, player: u8) -> String {
    g.ai_notes().into_iter().find(|(p, _)| *p == player).unwrap().1.join("\n")
}

fn character(f: impl Fn(&mut Temper)) -> Temper {
    let mut t = Temper { rolled: true, ..Temper::default() };
    f(&mut t);
    t
}

#[test]
fn a_computer_at_war_offers_another_a_truce_and_both_keep_it() {
    let mut g = game(3, 3);
    for p in 0..3 {
        // Honourable, so it makes offers, and keen on the deal.
        g.set_ai_temper(p, character(|t| t.honour = 1.0));
    }
    // (Deals start at three minutes; one may be struck the moment they do.)
    let mut said = run(&mut g, 3 * 60 + 1);
    let agreed = |g: &Game| log(g, 0).contains("Agrees a") || log(g, 1).contains("Agrees a");
    // Tux0 and Tux1 are both out to get Tux2, so each is somebody the other
    // could make peace with (left to itself Tux1 may pick on Tux0 instead).
    // Offers are a roll of the dice each minute or two: give it time.
    for _ in 0..30 {
        g.set_ai_plan(0, Plan::Siege(2), None);
        g.set_ai_plan(1, Plan::Siege(2), None);
        said.extend(run(&mut g, 30));
        if agreed(&g) {
            break;
        }
    }
    // Either may be the one to ask; the other says yes in public.
    let offer = said.iter().any(|l| (l.starts_with("Tux0") && l.contains("Tux1") || l.starts_with("Tux1") && l.contains("Tux0")) && l.contains("a truce?"));
    assert!(offer, "no truce offered in public between Tux0 and Tux1:\n{}", said.join("\n"));
    assert!(agreed(&g) && said.iter().any(|l| (l.starts_with("Tux0") || l.starts_with("Tux1")) && l.contains("Deal")), "{}\n{}", log(&g, 0), log(&g, 1));
    // Both sides hold back: neither will pick the other as a target now.
    let briefing = |p: u8| g.ai_briefings().into_iter().find(|(q, _)| *q == p).unwrap().1;
    assert!(briefing(0).contains("Truces: player 1"), "{}", briefing(0));
    assert!(briefing(1).contains("Truces: player 0"), "{}", briefing(1));
}

#[test]
fn the_others_gang_up_on_a_runaway_leader() {
    // (A game where nobody is crushed before the call: in some, one of the
    // callers is down to a worker by then and has no say.)
    let mut g = game(4, 4);
    for p in 0..3 {
        // Spiteful and honourable: the ones to stand up and call for it.
        g.set_ai_temper(p, character(|t| {
            t.spite = 1.0;
            t.honour = 1.0;
        }));
    }
    // Somebody to answer the call: keen on a fight.
    g.set_ai_temper(1, character(|t| {
        t.spite = 1.0;
        t.bold = 1.0;
    }));
    g.set_ai_temper(3, character(|t| t.greed = 1.0));
    // Nobody bothers anybody for the first minutes.
    for a in 0..4 {
        for b in 0..4 {
            g.deliver_chat_to_ai(a, Some(b), "truce please");
        }
    }
    run(&mut g, 7 * 60);
    // Tux3 has a horde, and keeps it: it is running away with the game.
    // (Topped up every half minute, as hordes get thinned in the wars.)
    let home = g.igloos.iter().find(|i| i.owner == 3).unwrap().id;
    let mut said = Vec::new();
    for _ in 0..8 {
        let rivals = (0..3).map(|p| g.population(p)).max().unwrap();
        while g.population(3) < rivals * 2 + 60 {
            g.spawn_for_test(home, Kind::Worker);
        }
        said.extend(run(&mut g, 30));
    }
    let call = said.iter().find(|l| l.contains("getting too big"));
    assert!(call.is_some_and(|l| l.contains("Tux3")), "nobody called for a pact against Tux3:\n{}", said.join("\n"));
    let caller = (0..3).find(|&p| log(&g, p).contains("Calls for a pact against Tux3")).expect("the call isn't in a log");
    let plan = log(&g, caller);
    assert!(plan.contains("siege Tux3") || plan.contains("raid Tux3"), "{plan}");
    // Everyone else who was asked gave an answer, one way or the other.
    for p in (0..3).filter(|&p| p != caller) {
        assert!(log(&g, p).contains("pact against Tux3"), "Tux{p} never answered:\n{}", log(&g, p));
    }
}
