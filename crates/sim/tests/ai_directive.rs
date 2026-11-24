//! A computer player can be steered from outside (by a language model, say)
//! while its own rules still do the playing.

use sim::ai::{AiLevel, Directive, Posture};
use sim::game::*;
use sim::map::Map;

fn game(players: usize, seed: u64) -> Game {
    game_with(players, seed, CoreRules::default().map_start())
}

fn game_with(players: usize, seed: u64, core: CoreRules) -> Game {
    let path = format!("{}/../../assets/maps/pinewood-hollow.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let setups = (0..players)
        .map(|i| PlayerSetup { name: format!("AI {i}"), color: i as u8, team: i as u8, ai: Some(AiLevel::Hard) })
        .collect();
    let mut g = Game::new_with(map, setups, seed, core);
    g.weather.calm = true;
    g
}

fn run(g: &mut Game, minutes: u32) {
    for _ in 0..minutes * 60 * sim::TICK_RATE {
        g.tick();
        g.events.clear();
    }
}

#[test]
fn a_reply_is_read_even_in_prose() {
    let d = Directive::from_reply("Sure! ```json\n{\"posture\":\"Rush\",\"target\":1,\"favour\":\"Slinger\",\"research\":\"icecraft\"}\n```").unwrap();
    assert_eq!(d.posture, Posture::Rush);
    assert_eq!(d.target, Some(1));
    assert_eq!(d.favour, Some(Kind::Slinger));
    assert_eq!(d.research, Some(sim::game::ICECRAFT));
}

#[test]
fn nonsense_in_a_reply_means_no_preference() {
    assert!(Directive::from_reply("I would attack, honestly").is_err());
    let d = Directive::from_reply(r#"{"posture":"yolo","target":99,"favour":"chieftain","research":"magic"}"#).unwrap();
    assert_eq!(d, Directive::default());
    let d = Directive::from_reply(r#"{"posture":"turtle","target":null,"favour":null,"research":null}"#).unwrap();
    assert_eq!(d, Directive { posture: Posture::Turtle, ..Default::default() });
}

#[test]
fn only_a_living_enemy_can_be_the_target() {
    let mut g = game(3, 3);
    let aim = |g: &mut Game, p: u8, t: Option<u8>| g.set_ai_directive(p, Directive { target: t, ..Default::default() });
    assert!(aim(&mut g, 0, Some(1)));
    assert!(aim(&mut g, 0, Some(0)), "aiming at yourself is dropped, not an error");
    assert!(!g.set_ai_directive(7, Directive::default()), "player 7 is no computer");
    g.players[2].out = true;
    g.set_ai_directive(0, Directive { target: Some(2), ..Default::default() });
    assert!(g.ai_briefings().iter().all(|(p, _)| *p != 2), "a beaten player gets no briefing");
}

#[test]
fn the_briefing_tells_what_the_tribe_knows() {
    let mut g = game(2, 4);
    run(&mut g, 1);
    let (player, text) = g.ai_briefings().remove(0);
    assert_eq!(player, 0);
    for want in ["player 0", "Workers", "Player 1 (AI 1): enemy"] {
        assert!(text.contains(want), "no {want:?} in:\n{text}");
    }
    // The model reads plain words, not Rust's debug text, and nothing about
    // tiers or research while those rules are off.
    assert!(!text.contains("Some(") && !text.contains("None") && !text.contains('{'), "debug text in:\n{text}");
    assert!(!text.contains("research") && !text.contains("tiers"), "tech in the briefing with tech off:\n{text}");
}

/// The second an AI is first ready to attack someone (it may still hold off
/// in the first minutes, when computers leave each other be), or 1800 if
/// it never is.
fn first_attack(posture: Posture) -> u32 {
    let mut g = game(2, 42);
    g.set_ai_directive(0, Directive { posture, ..Default::default() });
    // (It knows where the enemy is: this is about when it is ready, not
    // how soon its scouts happen to find them.)
    g.ai_knows_all_for_test(0);
    for _ in 0..30 * 60 {
        for _ in 0..sim::TICK_RATE {
            g.tick();
            g.events.clear();
        }
        let ready = g.ai_notes()[0].1.iter().any(|l| l.contains("Ready to attack") || l.contains("ATTACKS"));
        if ready || !g.ai_debug()[0].contains("target None") {
            return g.tick / sim::TICK_RATE;
        }
    }
    30 * 60
}

#[test]
fn a_rush_attacks_before_a_turtle_does() {
    let (rush, turtle) = (first_attack(Posture::Rush), first_attack(Posture::Turtle));
    assert!(rush < turtle, "rush at {rush} s, turtle at {turtle} s");
}

#[test]
fn a_boom_raises_more_workers() {
    // Over a few games: one game alone is mostly about who got raided.
    let workers = |posture| {
        [9, 1, 2]
            .into_iter()
            .map(|seed| {
                let mut g = game(2, seed);
                g.set_ai_directive(0, Directive { posture, ..Default::default() });
                run(&mut g, 8);
                g.penguins.iter().filter(|p| p.owner == 0 && p.kind == Kind::Worker).count()
            })
            .sum::<usize>()
    };
    let (plain, boom) = (workers(Posture::Balanced), workers(Posture::Boom));
    assert!(boom > plain, "{boom} workers in a boom, {plain} otherwise");
}




#[test]
fn the_models_reason_is_kept_apart_from_its_advice() {
    let reply = r#"{"posture":"rush","target":1,"favour":null,"research":null,"reason":"  Tux has no guards and 3 igloos undefended. "}"#;
    assert_eq!(Directive::reason_from_reply(reply).as_deref(), Some("Tux has no guards and 3 igloos undefended."));
    assert_eq!(Directive::from_reply(reply).unwrap().posture, Posture::Rush);
    assert_eq!(Directive::reason_from_reply(r#"{"posture":"rush"}"#), None);
    assert_eq!(Directive::reason_from_reply(r#"{"reason":""}"#), None);
}

#[test]
fn a_cautious_posture_never_asks_for_more_fighters_than_the_tribe_has() {
    // A turtle that wants 13 fighters ready while it owns 11 would sit on its
    // hands for 18 minutes. It must march once all but the home guard are ready.
    let mut g = game(2, 42);
    g.set_ai_directive(0, Directive { posture: Posture::Turtle, ..Default::default() });
    let log_after = |g: &mut Game| {
        run(g, 6);
        g.ai_notes().into_iter().find(|(p, _)| *p == 0).unwrap().1.join("\n")
    };
    let log = log_after(&mut g);
    // Find the "wants N ... out of M fighters" line and check N-with-guards fits in M.
    let line = log.lines().rev().find(|l| l.contains("Not attacking yet")).unwrap_or_else(|| panic!("no waiting note in:\n{log}"));
    let num = |after: &str| -> usize { line.split(after).nth(1).unwrap().trim_start().split(|c: char| !c.is_ascii_digit()).next().unwrap().parse().unwrap() };
    let (wants, total) = (num("it wants "), num("out of "));
    assert!(wants <= total, "wants {wants} fighters ready but has only {total}: it can never attack\n{line}");
}

#[test]
fn the_advisor_is_told_to_play_the_character_and_pick_a_plan() {
    for needle in ["Play the character", "grow|raid|siege|dig in", "raid and siege need a target", "Never promise a truce"] {
        assert!(sim::ai::DIRECTIVE_GUIDE.contains(needle), "the guide lost: {needle}");
    }
}

#[test]
fn a_plan_is_read_from_a_reply_and_needs_a_target_to_attack() {
    let d = Directive::from_reply(r#"{"plan":"siege","target":1,"say":"Your igloo is mine, penguin!"}"#).unwrap();
    assert_eq!(d.plan, Some(sim::ai::Plan::Siege(1)));
    assert_eq!(Directive::say_from_reply(r#"{"plan":"siege","target":1,"say":"Your igloo is mine, penguin!"}"#).as_deref(), Some("Your igloo is mine, penguin!"));
    assert_eq!(Directive::from_reply(r#"{"plan":"raid","target":null}"#).unwrap().plan, None, "a raid on nobody");
    assert_eq!(Directive::from_reply(r#"{"plan":"Dig In"}"#).unwrap().plan, Some(sim::ai::Plan::DigIn));
    assert_eq!(Directive::say_from_reply(r#"{"plan":"grow","say":""}"#), None);
}

/// Every option the model is offered does something: the fighters it may
/// favour parse, and "expand" (empty igloos can't be walked into) means grow.
#[test]
fn the_models_options_all_mean_something() {
    for (word, kind) in [("snowballer", Kind::Snowballer), ("slinger", Kind::Slinger), ("packer", Kind::Packer), ("skier", Kind::Skier), ("shieldbearer", Kind::Shield), ("shield", Kind::Shield)] {
        let d = Directive::from_reply(&format!(r#"{{"plan":"grow","favour":"{word}"}}"#)).unwrap();
        assert_eq!(d.favour, Some(kind), "{word}");
    }
    assert_eq!(Directive::from_reply(r#"{"plan":"expand"}"#).unwrap().plan, Some(sim::ai::Plan::Grow));
    for gone in ["sledder", "expand", "research"] {
        assert!(!sim::ai::DIRECTIVE_GUIDE.contains(gone), "the guide still offers {gone}");
    }
}

#[test]
fn the_advisors_advice_is_logged_in_plain_words() {
    let mut g = game(2, 42);
    g.set_ai_directive(0, Directive { posture: Posture::Rush, target: Some(1), favour: Some(Kind::Slinger), research: Some(sim::game::SNOWCRAFT), plan: None });
    let log = g.ai_notes().into_iter().find(|(p, _)| *p == 0).unwrap().1.join("\n");
    assert!(log.contains("Advisor says Rush; pick on AI 1; more slingers; learn Snowcraft"), "{log}");
    assert!(!log.contains("Some(") && !log.contains("None"), "Rust debug text in the log:\n{log}");
    g.set_ai_directive(0, Directive::default());
    let log = g.ai_notes().into_iter().find(|(p, _)| *p == 0).unwrap().1.join("\n");
    assert!(log.contains("pick on nobody; more of whatever; learn none in particular"), "{log}");
}

#[test]
fn it_says_when_it_sends_a_scout() {
    let mut g = game(2, 42);
    run(&mut g, 3);
    let log = g.ai_notes().into_iter().find(|(p, _)| *p == 0).unwrap().1.join("\n");
    assert!(log.contains("No enemy igloo found yet: sends a snowballer to scout"), "{log}");
}
