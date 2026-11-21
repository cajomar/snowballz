//! The computer holds it against whoever works or builds on its ground, and
//! that, more than weakness, decides whom it picks on.

use sim::ai::AiLevel;
use sim::game::*;
use sim::map::Map;

fn map(name: &str) -> Map {
    let path = format!("{}/../../assets/maps/{name}.sbmap", env!("CARGO_MANIFEST_DIR"));
    Map::load_file(std::path::Path::new(&path)).unwrap()
}

/// Player 0 is a computer of `level`; the others are people who do nothing
/// unless the test makes them.
fn game(name: &str, players: usize, level: AiLevel, seed: u64) -> Game {
    game_with(name, players, level, seed, CoreRules::default().map_start())
}

fn game_with(name: &str, players: usize, level: AiLevel, seed: u64, core: CoreRules) -> Game {
    let setups = (0..players)
        .map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: (i == 0).then_some(level) })
        .collect();
    let mut g = Game::new_with(map(name), setups, seed, core);
    g.weather.calm = true;
    g
}

fn door(g: &Game, player: u8) -> sim::math::Vec2 {
    g.igloos.iter().find(|i| i.owner == player).unwrap().door
}

/// Keep `n` of the player's workers chopping a tree beside the computer's door.
fn trespass(g: &mut Game, player: u8, n: usize) {
    let Some(tree) = g.tree_near(door(g, 0), 26) else { panic!("no tree near the computer's igloo") };
    let at = sim::math::v2(tree.0 as f32 + 0.5, tree.1 as f32 + 1.5);
    let ids: Vec<usize> = g.penguins.iter().filter(|p| p.owner == player && p.kind == Kind::Worker).map(|p| p.id as usize).take(n).collect();
    for id in ids {
        if g.penguins[id].act == Act::Frozen {
            continue;
        }
        g.penguins[id].pos = at;
        g.penguins[id].order = Order::Chop { at: tree };
    }
}

fn run(g: &mut Game, seconds: u32, intruders: Option<(u8, usize)>) {
    for _ in 0..seconds * sim::TICK_RATE {
        if let Some((player, n)) = intruders {
            trespass(g, player, n);
        }
        g.tick();
        g.events.clear();
    }
}

#[test]
fn working_on_its_ground_earns_a_grudge() {
    let mut g = game("pinewood-hollow", 2, AiLevel::Normal, 5);
    run(&mut g, 60, None);
    assert_eq!(g.ai_rivalry(0, 1), 0.0, "a grudge with no cause");
    run(&mut g, 60, Some((1, 3)));
    assert!(g.ai_rivalry(0, 1) > 10.0, "rivalry {} after a minute of trespass", g.ai_rivalry(0, 1));
}

#[test]
fn a_hard_computer_holds_a_grudge_longer_than_an_easy_one() {
    let left = |level| {
        let mut g = game("pinewood-hollow", 2, level, 5);
        run(&mut g, 90, Some((1, 3)));
        let peak = g.ai_rivalry(0, 1);
        run(&mut g, 4 * 60, None);
        g.ai_rivalry(0, 1) / peak
    };
    let (easy, hard) = (left(AiLevel::Easy), left(AiLevel::Hard));
    assert!(hard > easy * 1.5, "after four quiet minutes the hard one keeps {hard:.2} of it, the easy one {easy:.2}");
}

/// The igloo the computer first marches on once it has had ten minutes to
/// scout (a blizzard holds its army back meanwhile) and, if `intruders`,
/// two minutes of trespass.
fn first_target(g: &mut Game, intruders: Option<(u8, usize)>) -> Option<u8> {
    first_target_after(g, intruders, |_| {})
}

/// As `first_target`, with a change made to the game every second once it may march.
fn first_target_after(g: &mut Game, intruders: Option<(u8, usize)>, mut change: impl FnMut(&mut Game)) -> Option<u8> {
    for sec in 0..10 * 60 {
        g.weather.blizzard = u16::MAX;
        run(g, 1, intruders.filter(|_| sec >= 8 * 60));
    }
    g.weather.blizzard = 0;
    // Choosing is the point here, not scouting: it knows where everyone lives.
    for _ in 0..15 * 60 {
        g.ai_knows_all_for_test(0);
        change(g);
        run(g, 1, None);
        let d = &g.ai_debug()[0];
        let t = d.split("target ").nth(1)?.split(' ').next()?;
        if let Some(id) = t.strip_prefix("Some(").and_then(|r| r.strip_suffix(')')) {
            return id.parse().ok();
        }
    }
    None
}

#[test]
fn a_rival_is_attacked_before_a_weaker_neighbour() {
    // Three people-tribes around the computer: the trespasser is the one
    // farthest away, so without the grudge it picks somebody else.
    let base = game("glacier-crossroads", 4, AiLevel::Hard, 5);
    let mine = door(&base, 0);
    let mut others: Vec<u8> = (1..4).collect();
    others.sort_by(|&a, &b| door(&base, a).dist(mine).partial_cmp(&door(&base, b).dist(mine)).unwrap());
    let (near, far) = (others[0], others[2]);
    let owner = |g: &Game, ig: u8| g.igloos[ig as usize].owner;

    let mut calm = game("glacier-crossroads", 4, AiLevel::Hard, 5);
    let calm_target = first_target(&mut calm, None).map(|t| owner(&calm, t));
    let mut feud = game("glacier-crossroads", 4, AiLevel::Hard, 5);
    let feud_target = first_target(&mut feud, Some((far, 4))).map(|t| owner(&feud, t));

    println!("near {near} far {far}: calm picks {calm_target:?}, feud picks {feud_target:?}");
    assert_ne!(calm_target, Some(far), "without a grudge it already picked the far tribe {far} (near {near})");
    assert_eq!(feud_target, Some(far), "with a grudge it picked {feud_target:?}, not tribe {far} (calm picked {calm_target:?})");
}

/// The number after `word` in the computer's debug line.
fn count(g: &Game, word: &str) -> usize {
    g.ai_debug()[0].split(&format!("{word} ")).nth(1).unwrap().split(' ').next().unwrap().parse().unwrap()
}

/// Whether the computer ever had `word` (raiders, sentries) out in the next `seconds`.
fn ever(g: &mut Game, seconds: u32, word: &str, intruders: Option<(u8, usize)>) -> bool {
    for _ in 0..seconds {
        run(g, 1, intruders);
        if count(g, word) > 0 {
            return true;
        }
    }
    false
}

#[test]
fn trespassers_are_run_off() {
    let mut g = game("pinewood-hollow", 2, AiLevel::Hard, 5);
    // (Early, before it has an army out after the trespassers' igloo.)
    run(&mut g, 2 * 60, None);
    // (Followed by id: a frozen worker belongs to nobody.)
    let workers: Vec<usize> = g.penguins.iter().filter(|p| p.owner == 1 && p.kind == Kind::Worker).map(|p| p.id as usize).collect();
    // Raiders, or the guard posted at the grove they are chopping.
    let sent = (0..120).any(|_| {
        run(&mut g, 1, Some((1, 3)));
        count(&g, "raiders") + count(&g, "sentries") > 0
    });
    assert!(sent, "nobody was sent against the trespassers:\n{}", g.ai_notes()[0].1.join("\n"));
    run(&mut g, 60, Some((1, 3)));
    let hurt = workers.iter().map(|&i| &g.penguins[i]).filter(|p| p.warmth < p.max_warmth || p.act == Act::Frozen).count();
    assert!(hurt >= 1, "three workers chopped on the computer's land for three minutes untouched");
}

#[test]
fn the_easy_computer_only_complains() {
    let mut g = game("pinewood-hollow", 2, AiLevel::Easy, 5);
    run(&mut g, 6 * 60, None);
    assert!(!ever(&mut g, 150, "sentries", Some((1, 3))) && !ever(&mut g, 1, "raiders", Some((1, 3))));
    assert!(g.ai_rivalry(0, 1) > 5.0, "it should still mind the trespass");
}



fn say(g: &mut Game, from: u8, text: &str) -> Vec<String> {
    g.ai_replies(from, Some(0), text).into_iter().map(|(_, _, t)| t).collect()
}

#[test]
fn it_explains_what_it_holds_against_you() {
    let mut g = game("pinewood-hollow", 2, AiLevel::Normal, 5);
    run(&mut g, 60, None);
    assert!(say(&mut g, 1, "why are you so angry?")[0].contains("nothing against you"));
    run(&mut g, 90, Some((1, 3)));
    let reply = say(&mut g, 1, "why are you attacking me")[0].clone();
    assert!(reply.contains("my forest"), "{reply}");
}

#[test]
fn a_truce_is_for_the_forgiving_and_holds_the_attack() {
    let mut g = game("pinewood-hollow", 2, AiLevel::Normal, 5);
    run(&mut g, 60, None);
    let ok = say(&mut g, 1, "truce?")[0].clone();
    assert!(ok.contains("truce for 5 minutes"), "{ok}");
    assert!(say(&mut g, 1, "peace please")[0].contains("already have a truce"));

    // A deep grudge is not forgiven so easily.
    let mut g = game("pinewood-hollow", 2, AiLevel::Hard, 5);
    run(&mut g, 120, Some((1, 4)));
    let no = say(&mut g, 1, "can we have a truce")[0].clone();
    assert!(no.contains("No."), "{no}");
}

#[test]
fn a_truce_means_no_attack_on_that_tribe() {
    // As in the feud test, but the hated tribe has been offered a truce.
    let base = game("glacier-crossroads", 4, AiLevel::Hard, 5);
    let mine = door(&base, 0);
    let far = (1..4u8).max_by(|&a, &b| door(&base, a).dist(mine).partial_cmp(&door(&base, b).dist(mine)).unwrap()).unwrap();
    let mut g = game("glacier-crossroads", 4, AiLevel::Hard, 5);
    // (A truce is only granted to someone not too hated, so ask before the trespass.)
    assert!(say(&mut g, far, "truce")[0].contains("truce for 3 minutes"));
    run(&mut g, 1, None);
    // The truce lasts three minutes; the computer holds its fire at least that long.
    for _ in 0..170 {
        run(&mut g, 1, None);
        let target = g.ai_debug()[0].split("target ").nth(1).unwrap().split(' ').next().unwrap().to_string();
        if let Some(id) = target.strip_prefix("Some(").and_then(|r| r.strip_suffix(')')).and_then(|t| t.parse::<usize>().ok()) {
            assert_ne!(g.igloos[id].owner, far, "attacked a tribe it had a truce with");
        }
    }
}

#[test]
fn a_broken_promise_is_not_forgiven() {
    // (An easy computer, so nobody chases the workers off before the lie is told.)
    let mut g = game("pinewood-hollow", 2, AiLevel::Easy, 5);
    run(&mut g, 60, Some((1, 3)));
    assert!(say(&mut g, 1, "ok i will leave your forest")[0].contains("a minute"));
    // The workers stay, though. A promise buys a minute without new anger...
    run(&mut g, 55, Some((1, 3)));
    let in_grace = g.ai_rivalry(0, 1);
    // ...and then the lie is found out.
    run(&mut g, 10, Some((1, 3)));
    let after = g.ai_rivalry(0, 1);
    assert!(after > in_grace + 20.0, "rivalry went {in_grace:.0} to {after:.0}");
}

fn notes(g: &Game) -> Vec<String> {
    g.ai_notes().into_iter().find(|(p, _)| *p == 0).unwrap().1
}

#[test]
fn the_computer_explains_itself_to_anyone_watching() {
    let mut g = game("pinewood-hollow", 2, AiLevel::Normal, 5);
    run(&mut g, 60, None);
    let calm = notes(&g);
    assert!(calm[0].contains("P0") && calm[0].contains("Normal level"), "{calm:?}");
    assert!(calm.iter().any(|l| l.contains("Holds no grudges")), "{calm:?}");

    run(&mut g, 90, Some((1, 3)));
    say(&mut g, 1, "truce?");
    let angry = notes(&g);
    assert!(angry.iter().any(|l| l.starts_with("Grudges: P1")), "{angry:?}");
    assert!(angry.iter().any(|l| l.contains("P1 is on my ground (forest)")), "the warning is not in the log: {angry:?}");
    assert!(angry.iter().any(|l| l.contains("Hears P1: \"truce?\"")), "{angry:?}");
    assert!(angry.iter().any(|l| l.contains("Refuses P1 a truce")), "{angry:?}");
    // Times read as minutes and seconds, oldest first.
    assert!(angry.iter().any(|l| l.starts_with("2:") || l.starts_with("1:") || l.starts_with("3:")), "{angry:?}");
}

#[test]
fn the_advisor_and_the_wording_are_in_the_log_too() {
    use sim::ai::{Directive, Posture};
    let mut g = game("pinewood-hollow", 2, AiLevel::Normal, 5);
    g.set_ai_directive(0, Directive { posture: Posture::Rush, ..Default::default() });
    g.ai_say(0, Some(1), "Hello there".into());
    let n = notes(&g);
    assert!(n[0].contains("Advice: Rush"), "{n:?}");
    assert!(n.iter().any(|l| l.contains("Advisor says Rush")), "{n:?}");
    assert!(n.iter().any(|l| l.contains("Says to P1: \"Hello there\"")), "{n:?}");
}

#[test]
fn it_says_why_it_attacked_that_one() {
    // The feud setup: the computer marches on the far tribe it has a grudge
    // against, past a nearer one. The log must say so.
    let base = game("glacier-crossroads", 4, AiLevel::Hard, 5);
    let mine = door(&base, 0);
    let far = (1..4u8).max_by(|&a, &b| door(&base, a).dist(mine).partial_cmp(&door(&base, b).dist(mine)).unwrap()).unwrap();
    let mut g = game("glacier-crossroads", 4, AiLevel::Hard, 5);
    first_target(&mut g, Some((far, 4)));
    let log = notes(&g).join("\n");
    assert!(log.contains(&format!("ATTACKS P{far}'s igloo")), "{log}");
    assert!(log.contains("why:") && log.contains("grudge"), "no reasons given:\n{log}");
    assert!(log.contains("is nearer") && log.contains("passed over because of grudge against its owner"), "it does not say what tipped it:\n{log}");
}

#[test]
fn it_says_why_it_is_not_attacking() {
    let mut g = game("pinewood-hollow", 2, AiLevel::Normal, 5);
    run(&mut g, 5 * 60, None);
    let log = notes(&g).join("\n");
    assert!(log.contains("Not attacking yet:") && (log.contains("fighters are ready") || log.contains("fighter is ready")), "{log}");
}

#[test]
fn a_battered_igloo_is_the_better_target() {
    // `hp` is what an igloo has left, so the weaker the igloo the better the target.
    let build = || game("glacier-crossroads", 3, AiLevel::Hard, 3);
    let nearer = |hurt: bool| {
        let mut g = build();
        // Nobody at home to count as guards: only distance and damage choose.
        let armed: Vec<u16> = g.penguins.iter().filter(|p| p.owner != 0 && p.kind.fights()).map(|p| p.id).collect();
        for id in armed {
            g.retrain_for_test(id, Kind::Worker);
        }
        let mine = door(&g, 0);
        let mut by_dist: Vec<u8> = (1..3).collect();
        by_dist.sort_by(|&a, &b| door(&g, a).dist(mine).partial_cmp(&door(&g, b).dist(mine)).unwrap());
        // Hurt the farther tribe's igloo almost to nothing (igloos mend, so
        // it is done again every second).
        let far = by_dist[1];
        let target = first_target_after(&mut g, None, |g| {
            if hurt {
                g.igloos.iter_mut().filter(|i| i.owner == far).for_each(|ig| ig.hp = 5);
            }
        })
        .map(|t| g.igloos[t as usize].owner);
        (target, by_dist)
    };
    let (calm, order) = nearer(false);
    let (hurt, _) = nearer(true);
    assert_eq!(calm, Some(order[0]), "without damage the nearer tribe is hit");
    assert_eq!(hurt, Some(order[1]), "a nearly wrecked igloo is not passed over for a nearer, healthy one");
}




/// Every line the computer (player 0) had in mind to say in `seconds`, with
/// the game ticking on. Stock lines go in its log, not the chat: nothing
/// may be chatted.
fn heard_from_the_computer(g: &mut Game, seconds: u32, mut each_second: impl FnMut(&mut Game)) -> Vec<String> {
    let since = g.tick;
    for _ in 0..seconds {
        each_second(g);
        for _ in 0..sim::TICK_RATE {
            g.tick();
            for e in g.events.drain(..) {
                assert!(!matches!(e, Event::Chat { from: 0, .. }), "a stock line was chatted: {e:?}");
            }
        }
    }
    let (_, _, log) = g.ai_admin(since).into_iter().find(|(p, _, _)| *p == 0).unwrap();
    log.into_iter().filter_map(|(_, l)| l.starts_with("Thinks").then(|| l.split_once(": ").map_or(l.clone(), |(_, t)| t.trim_matches('"').to_string()))).collect()
}

fn attacked(g: &Game, who: u8) -> bool {
    notes(g).iter().any(|l| l.contains(&format!("ATTACKS P{who}'s igloo")))
}

/// aurora-basin with the first computer (Normal: five minutes' grace for
/// people) and two others, who are computers or people.
fn basin(others_are_computers: bool) -> Game {
    let setups = (0..3)
        .map(|i| PlayerSetup { name: format!("P{i}"), color: i as u8, team: i as u8, ai: (i == 0 || others_are_computers).then_some(AiLevel::Normal) })
        .collect();
    let mut g = Game::new_with(map("aurora-basin"), setups, 5, CoreRules::default().map_start());
    g.weather.calm = true;
    g
}

/// Seconds until the first computer first marches on P1 or P2, if within `limit`.
fn first_attack(g: &mut Game, limit: u32) -> Option<u32> {
    (1..=limit).find(|_| {
        run(g, 1, None);
        attacked(g, 1) || attacked(g, 2)
    })
}

#[test]
fn a_person_gets_a_few_minutes_before_a_computer_marches_on_them() {
    // Computers leave each other alone for three minutes...
    let mut control = basin(true);
    let war = first_attack(&mut control, 15 * 60).unwrap_or_else(|| panic!("computers never fought:\n{}", notes(&control).join("\n")));
    assert!(war >= 3 * 60, "computers fought at {war} s, inside their three minutes");
    // ...and a person gets five on Normal, then is fair game.
    let mut g = basin(false);
    let war = first_attack(&mut g, 15 * 60).unwrap_or_else(|| panic!("it never attacked, even after the grace period:\n{}", notes(&g).join("\n")));
    assert!(war >= AiLevel::Normal.grace_minutes() * 60, "a computer marched on a person at {war} s, inside the grace period");
}

#[test]
fn the_grace_ends_with_the_minutes_and_with_trespass() {
    let mut g = game("pinewood-hollow", 2, AiLevel::Normal, 5);
    assert!(g.ai_grace_holds(0, 1), "a person is shielded at the start");
    assert!(!g.ai_grace_holds(0, 0), "a computer's own tribe is not a person to shield");
    // Camping in its forest forfeits it.
    run(&mut g, 90, Some((1, 4)));
    assert!(!g.ai_grace_holds(0, 1), "a trespasser is still shielded (grudge {})", g.ai_rivalry(0, 1));
    // So does time.
    let mut calm = game("pinewood-hollow", 2, AiLevel::Normal, 5);
    run(&mut calm, 5 * 60 + 5, None);
    assert!(!calm.ai_grace_holds(0, 1), "the grace outlived its five minutes");
}

#[test]
fn the_grace_period_is_longer_on_easier_levels() {
    assert!(AiLevel::Easy.grace_minutes() > AiLevel::Normal.grace_minutes() && AiLevel::Normal.grace_minutes() > AiLevel::Hard.grace_minutes());
}

#[test]
fn it_grumbles_when_someone_builds_close_to_its_ground_before_they_trespass() {
    let mut g = game("pinewood-hollow", 2, AiLevel::Normal, 5);
    run(&mut g, 30, None);
    let door = door(&g, 0);
    // A spot just outside the ground it claims (20 tiles), inside the strip it watches.
    let site = (0..24)
        .flat_map(|k| (21..30).map(move |d| (k, d as f32)))
        .filter_map(|(k, d)| {
            let a = k as f32 * std::f32::consts::TAU / 24.0;
            g.find_site(1, BuildKind::Palisade, door + sim::math::v2(a.cos() * d, a.sin() * d), 1)
        })
        .find(|&(x, y)| (21.0..32.0).contains(&sim::math::v2(x as f32 + 0.5, y as f32 + 0.5).dist(door)))
        .expect("a buildable spot just outside its ground");
    let at = sim::math::v2(site.0 as f32 + 0.5, site.1 as f32 + 0.5);
    let w = g.penguins.iter().find(|p| p.owner == 1 && p.kind == Kind::Worker && p.inside.is_none()).unwrap().id;
    g.command(1, Command::Build { units: vec![w], kind: BuildKind::Palisade, tiles: vec![site] });
    // It only notices what it can see: one of its fighters stands watch beside the site.
    // (A Warmer sees but doesn't throw: a fighter would knock the site down.)
    let lookout = g.penguins.iter().find(|p| p.owner == 0 && p.kind == Kind::Snowballer && p.inside.is_none()).unwrap().id;
    g.penguins[lookout as usize].kind = Kind::Warmer;
    let said = heard_from_the_computer(&mut g, 60, |g| {
        g.penguins[w as usize].pos = at;
        g.penguins[lookout as usize].pos = at + sim::math::v2(2.0, 0.0);
        g.penguins[lookout as usize].order = Order::None;
    });
    // The first thing it says is the gentle line, not a threat (if the site
    // is also near a grove it chops, it gets sharper as time goes on).
    let first = said.first().unwrap_or_else(|| panic!("it said nothing about the building ({:.0} tiles out):\n{}", at.dist(door), notes(&g).join("\n")));
    assert!(first.contains("keeping an eye on you") || first.contains("watching you"), "it opened with {first:?}: {said:?}");
}

#[test]
fn it_grumbles_about_its_trees_before_it_threatens() {
    let mut g = game("pinewood-hollow", 2, AiLevel::Normal, 5);
    run(&mut g, 90, None);
    let said = heard_from_the_computer(&mut g, 150, |g| trespass(g, 1, 3));
    let at = |words: &[&str]| said.iter().position(|t| words.iter().any(|w| t.contains(w)));
    let grumble = at(&["watching you", "keeping an eye", "spoken for"]);
    let threat = at(&["is MY", "Last warning", "asked for this"]);
    assert!(grumble.is_some() && threat.is_some(), "it should grumble, then threaten: {said:?}");
    assert!(grumble < threat, "the grumble must come first: {said:?}");
}


