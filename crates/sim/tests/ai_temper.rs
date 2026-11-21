//! Every game each computer player is somebody new: rolled traits that
//! change how it plays, a title that hints at the strongest, and a reveal at
//! the end. A game that rolls nothing plays exactly as the plain rules do.

use sim::ai::{AiLevel, Plan, Temper};
use sim::game::*;
use sim::map::Map;

fn game(players: usize, seed: u64) -> Game {
    let path = format!("{}/../../assets/maps/pinewood-hollow.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..players)
        .map(|i| PlayerSetup { name: format!("Tux{i} Bot"), color: i as u8, team: i as u8, ai: Some(AiLevel::Hard) })
        .collect();
    let mut g = Game::new_with(map, setups, seed, CoreRules::default().map_start());
    g.weather.calm = true;
    g
}

fn run(g: &mut Game, seconds: u32) {
    for _ in 0..seconds * sim::TICK_RATE {
        g.tick();
        g.events.clear();
    }
}

fn log(g: &Game, player: u8) -> String {
    g.ai_notes().into_iter().find(|(p, _)| *p == player).unwrap().1.join("\n")
}

/// Game time, in seconds, of the first log line containing `what`.
fn first(g: &Game, player: u8, what: &str) -> Option<u32> {
    let line = log(g, player).lines().find(|l| l.contains(what))?.to_string();
    let clock = line.split_whitespace().next()?;
    let (m, s) = clock.split_once(':')?;
    Some(m.parse::<u32>().ok()? * 60 + s.parse::<u32>().ok()?)
}

/// A rolled character, which also picks its own plans.
fn with(f: impl Fn(&mut Temper)) -> Temper {
    let mut t = Temper { rolled: true, ..Temper::default() };
    f(&mut t);
    t
}

/// Just the traits, without plans: to see what one trait does.
fn traits(f: impl Fn(&mut Temper)) -> Temper {
    Temper { rolled: false, ..with(f) }
}

#[test]
fn without_a_roll_they_play_plain_and_keep_their_names() {
    let g = game(3, 1);
    assert!(g.players.iter().all(|p| p.name.ends_with(" Bot")));
    assert!(!log(&g, 0).contains("Temperament"));
}

#[test]
fn each_game_rolls_new_characters_and_titles_them() {
    let rolled = |seed: u64| {
        let mut g = game(4, 1);
        g.roll_ai_tempers(seed);
        g.players.iter().map(|p| p.name.clone()).collect::<Vec<_>>()
    };
    let a = rolled(11);
    assert_eq!(a, rolled(11), "the same seed rolls the same characters");
    assert!((12..40).any(|s| rolled(s) != a), "other games roll other characters");
    for name in &a {
        assert!(name.contains(" the ") && !name.contains("Bot"), "{name} has no title");
    }
}

#[test]
fn the_title_names_the_trait_that_stands_out() {
    assert_eq!(with(|t| t.bold = 0.95).title(), "the Reckless");
    assert_eq!(with(|t| t.honour = 0.05).title(), "the Treacherous");
    assert_eq!(with(|t| t.spite = 0.9).title(), "the Grudge-Holder");
    assert_eq!(with(|t| t.favourite = Some(Kind::Slinger)).title(), "the Slinger");
    assert_eq!(Temper::default().title(), "the Steady");
    let d = with(|t| {
        t.bold = 0.95;
        t.honour = 0.05;
    })
    .describe();
    assert!(d.contains("small groups") && d.contains("breaks truces"), "{d}");
}

#[test]
fn a_bold_computer_marches_sooner_than_a_patient_one() {
    let first_attack = |bold: f32| {
        let mut g = game(2, 7);
        g.set_ai_temper(0, traits(|t| t.bold = bold));
        run(&mut g, 20 * 60);
        first(&g, 0, "ATTACKS").unwrap_or(u32::MAX)
    };
    let (bold, patient) = (first_attack(1.0), first_attack(0.0));
    assert!(bold < u32::MAX, "the bold computer never attacked");
    assert!(bold < patient, "bold marched at {bold}s, patient at {patient}s");
}

#[test]
fn a_treacherous_computer_breaks_a_truce_and_an_honourable_one_keeps_it() {
    let run_truce = |honour: f32| {
        let mut g = game(2, 3);
        // Bold, so it marches with what is free once the towers are manned.
        g.set_ai_temper(0, traits(|t| {
            t.honour = honour;
            t.bold = 1.0;
        }));
        // Player 1 is a person with no fighters, who stays at home: its
        // igloo is there for the taking.
        g.players[1].ai = None;
        for p in g.penguins.iter_mut().filter(|p| p.owner == 1 && p.kind == Kind::Snowballer) {
            p.kind = Kind::Worker;
        }
        // ...and player 0 knows where it is: a fighter of its stands at the door.
        let door = g.igloos.iter().find(|i| i.owner == 1).unwrap().door;
        let spy = g.penguins.iter().find(|p| p.owner == 0 && p.kind == Kind::Snowballer && p.inside.is_none()).unwrap().id;
        g.penguins[spy as usize].pos = door + sim::math::v2(3.0, 0.0);
        for _ in 0..24 {
            // Player 1 keeps asking for peace, so a truce is always on, and
            // player 0's army stays warm enough to march.
            g.deliver_chat_to_ai(1, Some(0), "truce please");
            for p in g.penguins.iter_mut().filter(|p| p.owner == 0 && p.kind.fights()) {
                p.warmth = p.max_warmth;
            }
            run(&mut g, 30);
            if log(&g, 0).contains("Breaks its truce") {
                break;
            }
        }
        log(&g, 0)
    };
    let treacherous = run_truce(0.0);
    assert!(treacherous.contains("Breaks its truce"), "a treacherous computer kept every truce:\n{treacherous}");
    let honourable = run_truce(1.0);
    assert!(!honourable.contains("Breaks its truce"), "an honourable computer broke a truce:\n{honourable}");
}

#[test]
fn a_greedy_computer_keeps_more_workers() {
    let workers = |greed: f32| {
        let mut g = game(2, 5);
        g.set_ai_temper(0, traits(|t| t.greed = greed));
        run(&mut g, 6 * 60);
        g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Worker).count()
    };
    let (greedy, spartan) = (workers(1.0), workers(0.0));
    assert!(greedy > spartan, "greedy {greedy} workers, spartan {spartan}");
}

#[test]
fn the_end_of_the_game_reveals_what_each_was_like() {
    let mut g = game(2, 2);
    g.set_ai_temper(0, with(|t| t.chatter = 0.95));
    g.set_ai_temper(1, with(|t| t.bold = 0.95));
    assert!(log(&g, 0).contains("Temperament: talks a lot"));
    for i in 0..g.igloos.len() {
        if g.igloos[i].owner == 1 {
            g.igloos[i].owner = 0;
        }
    }
    run(&mut g, 5);
    assert!(g.winner.is_some());
    assert!(log(&g, 0).contains("What I was like this game: talks a lot"), "{}", log(&g, 0));
}

#[test]
fn a_computer_with_a_character_makes_plans_of_its_own() {
    let mut g = game(3, 4);
    g.roll_ai_tempers(4);
    run(&mut g, 8 * 60);
    for p in 0..3 {
        assert!(log(&g, p).contains("New plan, its own"), "no plans:\n{}", log(&g, p));
    }
}

#[test]
fn digging_in_stops_attacks_and_a_siege_goes_after_its_target() {
    let attacks = |plan: Plan| {
        let mut g = game(2, 7);
        g.set_ai_temper(0, traits(|t| t.bold = 1.0));
        // It knows where player 1 lives: a fighter of its stands at the
        // door, which nobody guards.
        for p in g.penguins.iter_mut().filter(|p| p.owner == 1 && p.kind == Kind::Snowballer) {
            p.kind = Kind::Worker;
        }
        let door = g.igloos.iter().find(|i| i.owner == 1).unwrap().door;
        let spy = g.penguins.iter().find(|p| p.owner == 0 && p.kind == Kind::Snowballer && p.inside.is_none()).unwrap().id;
        g.penguins[spy as usize].pos = door + sim::math::v2(3.0, 0.0);
        // (A siege wants a big army over and above the door's guards: the
        // larder is kept full, so this is about the plan, not the fishing.)
        for _ in 0..24 {
            // The advisor keeps saying the same.
            assert!(g.set_ai_plan(0, plan, None));
            for ig in g.igloos.iter_mut().filter(|i| i.owner == 0) {
                ig.fish = ig.fish.max(60);
            }
            run(&mut g, 30);
            if log(&g, 0).contains("ATTACKS") {
                break;
            }
        }
        log(&g, 0)
    };
    let dug_in = attacks(Plan::DigIn);
    assert!(dug_in.contains("New plan, the advisor's: dig in"), "{dug_in}");
    assert!(!dug_in.contains("ATTACKS"), "attacked while dug in:\n{dug_in}");
    let siege = attacks(Plan::Siege(1));
    assert!(siege.contains("ATTACKS Tux1"), "a siege that never came:\n{siege}");
}

#[test]
fn a_new_plan_is_announced_to_the_person_it_is_aimed_at() {
    let mut g = game(2, 1);
    g.players[1].ai = None;
    assert!(g.set_ai_plan(0, Plan::Siege(1), Some("Your igloo is mine, Tux1!".into())));
    assert!(!g.set_ai_plan(0, Plan::Siege(0), None), "a siege on itself");
    run(&mut g, 3);
    let log = log(&g, 0);
    assert!(log.contains("Says to Tux1 Bot: \"Your igloo is mine, Tux1!\""), "{log}");
}

#[test]
fn each_character_words_its_plans_in_its_own_voice() {
    let said = |t: Temper| {
        let mut g = game(2, 1);
        g.players[1].ai = None;
        g.set_ai_temper(0, t);
        run(&mut g, 1);
        assert!(g.set_ai_plan(0, Plan::Siege(1), None));
        run(&mut g, 3);
        let log = log(&g, 0);
        // Stock lines are only in the log now; nothing is chatted.
        assert!(!log.contains("Says to"), "{log}");
        log.lines().find(|l| l.contains("Thinks to")).unwrap_or_else(|| panic!("thought nothing:\n{log}")).to_string()
    };
    // Chatty enough that both always announce a new plan.
    let loud = said(with(|t| t.chatter = 1.0));
    let reckless = said(with(|t| {
        t.bold = 1.0;
        t.chatter = 0.9;
    }));
    assert!(loud.contains("WHOLE tribe"), "{loud}");
    assert!(reckless.contains("flattening"), "{reckless}");
}
