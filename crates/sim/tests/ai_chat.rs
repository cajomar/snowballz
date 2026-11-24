//! Who answers chat, and which answers are binding. Deals and questions
//! about the game are the game's to settle; anything else is small talk,
//! which a language model may answer in its own words.

use sim::ai::AiLevel;
use sim::game::*;
use sim::map::Map;

/// One person (player 0) and three computers named Tux, Flipper and Waddles.
fn game(seed: u64) -> Game {
    let path = format!("{}/../../assets/maps/glacier-crossroads.sbmap", env!("CARGO_MANIFEST_DIR"));
    let map = Map::load_file(std::path::Path::new(&path)).unwrap();
    let names = ["You", "Tux", "Flipper", "Waddles"];
    let setups = (0..4)
        .map(|i| PlayerSetup { name: names[i].to_string(), color: i as u8, team: i as u8, ai: (i > 0).then_some(AiLevel::Hard) })
        .collect();
    Game::new_with(map, setups, seed, CoreRules::default())
}

#[test]
fn a_computer_spoken_to_always_answers_and_small_talk_is_not_binding() {
    for seed in 0..20 {
        let mut g = game(seed);
        let replies = g.ai_replies_full(0, Some(2), "what a lovely day for it");
        assert_eq!(replies.len(), 1, "seed {seed}: {replies:?}");
        let (who, r) = &replies[0];
        assert_eq!((*who, r.to), (2, Some(0)));
        assert!(!r.binding, "small talk came back binding: {r:?}");
    }
}

#[test]
fn a_truce_request_is_binding() {
    let mut g = game(1);
    let replies = g.ai_replies_full(0, Some(1), "truce please");
    assert!(replies.iter().any(|(p, r)| *p == 1 && r.binding), "{replies:?}");
}

#[test]
fn a_public_remark_gets_one_voice_at_most() {
    for seed in 0..20 {
        let mut g = game(seed);
        let chatty = g.ai_replies_full(0, None, "this map is huge").into_iter().filter(|(_, r)| !r.binding).count();
        assert!(chatty <= 1, "seed {seed}: {chatty} computers answered at once");
    }
}

#[test]
fn naming_a_computer_in_public_gets_that_one() {
    for seed in 0..20 {
        let mut g = game(seed);
        let replies = g.ai_replies_full(0, None, "hey Waddles, nice igloo");
        let who: Vec<u8> = replies.iter().map(|(p, _)| *p).collect();
        assert_eq!(who, vec![3], "seed {seed}");
    }
}

/// Friendly chat gets a friendly answer, even before a language model is
/// ready: no telling people to hush or go away.
#[test]
fn small_talk_never_tells_you_to_be_quiet() {
    let rude = ["babbl", "shut", "quiet", "less talk", "stop talking", "go away", "featherbrain", "n00b", "die"];
    for seed in 0..40 {
        let mut g = game(seed);
        for said in ["hi!", "how are you?", "nice igloo", "what's your favourite fish?", "are you a bot?", "lol"] {
            for (_, r) in g.ai_replies_full(0, Some(1), said) {
                let t = r.text.to_lowercase();
                assert!(!rude.iter().any(|w| t.contains(w)), "seed {seed}, {said:?} got {:?}", r.text);
            }
        }
    }
}

/// Asked whether it is a bot, a computer says so, in words no model may
/// change; "both" is not a question about bots.
#[test]
fn a_computer_says_plainly_that_it_is_one() {
    for ask in ["are you a bot?", "Tux, is that a real person?", "you an AI?"] {
        let mut g = game(1);
        let replies = g.ai_replies_full(0, Some(1), ask);
        let (_, r) = replies.first().unwrap_or_else(|| panic!("no answer to {ask:?}"));
        assert!(r.binding && r.text.contains("computer player"), "{ask:?} -> {r:?}");
    }
    let mut g = game(1);
    assert!(g.ai_replies_full(0, Some(1), "we both love fish").iter().all(|(_, r)| !r.text.contains("computer player")));
}
