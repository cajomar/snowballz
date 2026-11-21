//! Does the advice help? Plays seeded computer-vs-computer games in which
//! half the seats take directives from one advisor and the other half from
//! another (by default none), swapping the seats between games so that
//! neither side keeps the better starts, and counts who comes out on top.
//!
//!     snowballz-eval [--games 20] [--map glacier-crossroads] [--players 4]
//!                    [--minutes 25] [--level hard] [--seed 1]
//!                    [--advisor MODEL|auto|balanced|rush|boom|turtle|random]
//!                    [--against none|...same...] [--guide FILE] [--tempers] [--quiet]
//!
//! `--guide FILE` tells the model advisors the job in the file's words
//! instead of the game's own, to try a new prompt before a live game uses it.
//! `--tempers` gives the computers rolled characters, as live games do, and
//! counts how often each title finishes on top.
//!
//! A model advisor is asked every 30 s of game time, as in a live game, but
//! the game waits for its answer, so a slow model is judged on its advice
//! and not on its speed.

use server::{Advice, Advisor};
use sim::ai::{AiLevel, Directive};
use sim::game::{Game, PlayerSetup};
use sim::map::Map;
use sim::rng::Rng;
use sim::TICK_RATE;
use std::collections::BTreeMap;
use std::time::Instant;

const FIRST_AFTER: u32 = 15 * TICK_RATE;
const EVERY: u32 = 30 * TICK_RATE;
const POSTURES: [&str; 4] = ["balanced", "rush", "boom", "turtle"];

/// Where one side's directives come from.
enum Source {
    None,
    Fixed(String),
    Random(Rng),
    Model(Advisor),
}

impl Source {
    fn parse(spec: &str, seed: u64, guide: Option<&String>) -> Source {
        match spec {
            "none" => Source::None,
            "random" => Source::Random(Rng::new(seed ^ 0x5eed)),
            p if POSTURES.contains(&p) => Source::Fixed(format!("{{\"posture\":\"{p}\"}}")),
            model => {
                let mut advisor = Advisor::new(model);
                if let Some(guide) = guide {
                    advisor.guide = guide.clone();
                }
                Source::Model(advisor)
            }
        }
    }

    fn advise(&mut self, report: &str) -> Option<Result<Advice, String>> {
        let plain = |d: Directive| Advice { directive: d, reason: None, say: None };
        match self {
            Source::None => None,
            Source::Fixed(reply) => Some(Directive::from_reply(reply).map(plain)),
            Source::Random(rng) => {
                let p = POSTURES[rng.below(POSTURES.len() as u32) as usize];
                Some(Directive::from_reply(&format!("{{\"posture\":\"{p}\"}}")).map(plain))
            }
            Source::Model(advisor) => Some(advisor.advise(report)),
        }
    }
}

struct Config {
    games: u32,
    map: String,
    players: usize,
    minutes: u32,
    level: AiLevel,
    seed: u64,
    advisor: String,
    against: String,
    guide: Option<String>,
    tempers: bool,
    quiet: bool,
}

/// How one side did in one game.
#[derive(Default, Clone)]
struct Side {
    /// The best-placed player of the game was on this side.
    won: u32,
    igloos: u32,
    penguins: u32,
    survivors: u32,
    seats: u32,
    asked: u32,
    failed: u32,
    secs_asking: f32,
    postures: BTreeMap<String, u32>,
}

impl Side {
    fn add(&mut self, o: &Side) {
        self.won += o.won;
        self.igloos += o.igloos;
        self.penguins += o.penguins;
        self.survivors += o.survivors;
        self.seats += o.seats;
        self.asked += o.asked;
        self.failed += o.failed;
        self.secs_asking += o.secs_asking;
        for (k, v) in &o.postures {
            *self.postures.entry(k.clone()).or_default() += v;
        }
    }
}

fn main() {
    let cfg = parse_args();
    let path = server::maps_dir(&server::find_assets()).join(format!("{}.sbmap", cfg.map));
    let map = Map::load_file(&path).unwrap_or_else(|e| fail(&format!("{}: {e}", path.display())));
    if map.igloos.len() < cfg.players || cfg.players < 2 {
        fail(&format!("{} has room for {} players", cfg.map, map.igloos.len()));
    }
    let mut sources = [Source::parse(&cfg.advisor, cfg.seed, cfg.guide.as_ref()), Source::parse(&cfg.against, cfg.seed + 1, cfg.guide.as_ref())];
    let names = [cfg.advisor.as_str(), cfg.against.as_str()];
    println!(
        "{} games of {} on {}, {} minutes at most, {:?} computers: \"{}\" (side A) against \"{}\" (side B)",
        cfg.games, cfg.players, cfg.map, cfg.minutes, cfg.level, names[0], names[1]
    );
    let mut totals = [Side::default(), Side::default()];
    let mut lengths = Vec::new();
    // Per title: seats played, and games finished on top.
    let mut titles: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    let began = Instant::now();
    for i in 0..cfg.games {
        let seed = cfg.seed + (i / 2) as u64;
        // Each seed is played twice with the sides swapped, so a lucky
        // start counts for both.
        let flip = i % 2 == 1;
        let side_of = |seat: usize| (seat % 2 == 1) ^ flip;
        let (sides, secs, winner) = play(&cfg, map.clone(), seed, &side_of, &mut sources, &mut titles);
        lengths.push(secs);
        for s in 0..2 {
            totals[s].add(&sides[s]);
        }
        if !cfg.quiet {
            let ab = |seat: usize| if side_of(seat) { 'B' } else { 'A' };
            let seats: String = (0..cfg.players).map(ab).collect();
            println!(
                "game {:>3}  seed {seed:<4} seats {seats}  {:>5.1} min  top: player {winner} ({})  igloos A {} B {}",
                i + 1,
                secs / 60.0,
                ab(winner),
                sides[0].igloos,
                sides[1].igloos
            );
        }
    }
    let n = cfg.games as f32;
    println!();
    println!("{:<28}{:>14}{:>14}", "", format!("A: {}", names[0]), format!("B: {}", names[1]));
    let row = |label: &str, f: &dyn Fn(&Side) -> String| println!("{label:<28}{:>14}{:>14}", f(&totals[0]), f(&totals[1]));
    row("top player's side", &|s| format!("{:.0}%", 100.0 * s.won as f32 / n));
    row("igloos held at the end", &|s| format!("{:.2}", s.igloos as f32 / s.seats.max(1) as f32));
    row("penguins at the end", &|s| format!("{:.1}", s.penguins as f32 / s.seats.max(1) as f32));
    row("still in the game", &|s| format!("{:.0}%", 100.0 * s.survivors as f32 / s.seats.max(1) as f32));
    row("advice asked (failed)", &|s| format!("{} ({})", s.asked, s.failed));
    row("seconds per answer", &|s| if s.asked > 0 { format!("{:.2}", s.secs_asking / s.asked as f32) } else { "-".into() });
    for s in 0..2 {
        if !totals[s].postures.is_empty() {
            let said: Vec<String> = totals[s].postures.iter().map(|(k, v)| format!("{k} {v}")).collect();
            println!("side {} was told: {}", if s == 0 { 'A' } else { 'B' }, said.join(", "));
        }
    }
    if !titles.is_empty() {
        let fair = 100.0 / cfg.players as f32;
        println!("\nBy character (a fair share of top finishes is {fair:.0}%):");
        let mut rows: Vec<_> = titles.iter().collect();
        rows.sort_by_key(|(_, (seats, tops))| std::cmp::Reverse(tops * 1000 / seats.max(&1)));
        for (title, (seats, tops)) in rows {
            println!("  {title:<20} {:>4.0}% on top of {seats} games", 100.0 * *tops as f32 / *seats as f32);
        }
    }
    // Two standard errors of a fair coin's share: a gap smaller than this is noise.
    println!(
        "A's share of wins is {:.0}%, give or take {:.0} points (50% means no difference); games average {:.1} min; took {:.0}s",
        100.0 * totals[0].won as f32 / n,
        100.0 / n.sqrt(),
        lengths.iter().sum::<f32>() / n / 60.0,
        began.elapsed().as_secs_f32()
    );
}

/// One game: how each side did, how long it ran (seconds of game time) and
/// the best-placed player.
fn play(cfg: &Config, map: Map, seed: u64, side_of: &dyn Fn(usize) -> bool, sources: &mut [Source; 2], titles: &mut BTreeMap<String, (u32, u32)>) -> ([Side; 2], f32, usize) {
    let setups = (0..cfg.players)
        .map(|i| PlayerSetup { name: format!("AI {i}"), color: i as u8, team: i as u8, ai: Some(cfg.level) })
        .collect();
    let mut g = Game::new(map, setups, seed);
    if cfg.tempers {
        g.roll_ai_tempers(seed);
    }
    let mut sides = [Side::default(), Side::default()];
    let limit = cfg.minutes * 60 * TICK_RATE;
    let mut next_round = FIRST_AFTER;
    while g.tick < limit && g.players.iter().filter(|p| !p.out).count() > 1 {
        if g.tick >= next_round {
            next_round += EVERY;
            for (player, report) in g.ai_briefings() {
                let s = side_of(player as usize) as usize;
                let t = Instant::now();
                let Some(answer) = sources[s].advise(&report) else { continue };
                sides[s].asked += 1;
                sides[s].secs_asking += t.elapsed().as_secs_f32();
                match answer {
                    Ok(Advice { directive: d, say, .. }) => {
                        let said = d.plan.map_or_else(|| format!("{:?}", d.posture), |p| format!("{:?}", p).split('(').next().unwrap_or("").to_string());
                        *sides[s].postures.entry(said.to_lowercase()).or_default() += 1;
                        g.set_ai_directive(player, d);
                        if let (Some(plan), Some(say)) = (d.plan, say) {
                            g.set_ai_plan(player, plan, Some(say));
                        }
                    }
                    Err(_) => sides[s].failed += 1,
                }
            }
        }
        g.tick();
        g.events.clear();
    }
    let igloos = |p: usize| g.igloos.iter().filter(|i| i.owner as usize == p).count() as u32;
    let penguins = |p: usize| g.penguins.iter().filter(|q| q.owner as usize == p).count() as u32;
    let winner = (0..cfg.players).max_by_key(|&p| (igloos(p), penguins(p))).unwrap_or(0);
    for p in 0..cfg.players {
        let s = &mut sides[side_of(p) as usize];
        s.seats += 1;
        s.igloos += igloos(p);
        s.penguins += penguins(p);
        s.survivors += !g.players[p].out as u32;
    }
    sides[side_of(winner) as usize].won += 1;
    if cfg.tempers {
        for p in 0..cfg.players {
            let title = g.players[p].name.split_once(" the ").map_or("?", |(_, t)| t).to_string();
            let row = titles.entry(title).or_default();
            row.0 += 1;
            row.1 += (p == winner) as u32;
        }
    }
    (sides, g.tick as f32 / TICK_RATE as f32, winner)
}

fn parse_args() -> Config {
    let mut cfg = Config {
        games: 20,
        map: "glacier-crossroads".into(),
        players: 4,
        minutes: 25,
        level: AiLevel::Hard,
        seed: 1,
        advisor: "auto".into(),
        against: "none".into(),
        guide: None,
        tempers: false,
        quiet: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| fail(&format!("{a} needs a value")));
        let number = |v: String| v.parse().unwrap_or_else(|_| fail(&format!("not a number: {v}")));
        match a.as_str() {
            "--games" => cfg.games = number(value()) as u32,
            "--map" => cfg.map = value().to_lowercase().replace(' ', "-"),
            "--players" => cfg.players = number(value()) as usize,
            "--minutes" => cfg.minutes = number(value()) as u32,
            "--seed" => cfg.seed = number(value()),
            "--level" => {
                cfg.level = match value().as_str() {
                    "easy" => AiLevel::Easy,
                    "normal" => AiLevel::Normal,
                    "hard" => AiLevel::Hard,
                    other => fail(&format!("no level {other:?} (easy, normal, hard)")),
                }
            }
            "--advisor" => cfg.advisor = value(),
            "--against" => cfg.against = value(),
            "--guide" => {
                let path = value();
                cfg.guide = Some(std::fs::read_to_string(&path).unwrap_or_else(|e| fail(&format!("{path}: {e}"))));
            }
            "--tempers" => cfg.tempers = true,
            "--quiet" => cfg.quiet = true,
            "-h" | "--help" => {
                println!("{}", include_str!("snowballz-eval.rs").lines().take_while(|l| l.starts_with("//!")).map(|l| l.trim_start_matches("//!").trim_start_matches(' ')).collect::<Vec<_>>().join("\n"));
                std::process::exit(0);
            }
            other => fail(&format!("unknown argument: {other}")),
        }
    }
    cfg.games = cfg.games.max(2) / 2 * 2;
    cfg
}

fn fail(why: &str) -> ! {
    eprintln!("{why}");
    std::process::exit(2)
}
