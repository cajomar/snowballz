//! Computer players. They only ever act through [`Command`]s, exactly like a
//! human would, and keep the trash-talking personality of the original.

use crate::fog::Fov;
use crate::json::Json;
use crate::game::{price, 
    igloo_max_hp, next_level, next_tier, BuildKind, Carry, Command, Game, Kind, Level, Order, SealState, Season, BRANCHES, FISHERY, ICECRAFT,
    NEUTRAL, SEAL_NOTICE, SNOWCRAFT, TIER_ICE_PALACE, TIER_SNOW_HALL, WAR_CRY_RADIUS,
};
use crate::map::ResourceKind;
use crate::math::{v2, Vec2};
use crate::path::PathFinder;
use crate::rng::Rng;

mod diplomacy;
mod plan;
mod temper;
mod territory;
pub use plan::Plan;
pub use temper::{Temper, REVEAL};
use temper::{around, TREACHEROUS};
pub(crate) use territory::branch_name;

/// Fighters at home before the AI saves up for a Chieftain.
const CHIEF_ARMY: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AiLevel {
    Easy = 0,
    Normal = 1,
    Hard = 2,
}

impl AiLevel {
    pub fn from_u8(v: u8) -> AiLevel {
        match v {
            0 => AiLevel::Easy,
            2 => AiLevel::Hard,
            _ => AiLevel::Normal,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            AiLevel::Easy => "Easy",
            AiLevel::Normal => "Normal",
            AiLevel::Hard => "Hard",
        }
    }
    fn think_ticks(self) -> u32 {
        match self {
            AiLevel::Easy => 40,
            AiLevel::Normal => 20,
            AiLevel::Hard => 10,
        }
    }
    fn attack_size(self) -> usize {
        match self {
            AiLevel::Easy => 9,
            AiLevel::Normal => 7,
            AiLevel::Hard => 6,
        }
    }
}

/// How a computer player leans for a while. Anything can set it from outside
/// (a language model, a test); the AI's own rules still do the playing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Posture {
    #[default]
    Balanced,
    /// Attack with smaller groups, tech later.
    Rush,
    /// More workers, earlier upgrades, bigger armies before marching.
    Boom,
    /// Bigger armies before marching, more towers.
    Turtle,
}

/// A steer for one computer player: all optional, and the default changes
/// nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Directive {
    pub posture: Posture,
    /// The player to pick on first.
    pub target: Option<u8>,
    /// A kind to train more of in the mixed army.
    pub favour: Option<Kind>,
    /// A research branch to learn first.
    pub research: Option<u8>,
    /// What to set out to do (see `Plan`); the computer announces it.
    pub plan: Option<Plan>,
}

/// What a language model is told about the job: play the computer's
/// character, and pick what it sets out to do next.
pub const DIRECTIVE_GUIDE: &str = "You are the mind of a computer player in Snowballz, a real-time snowball fight between \
penguin tribes. You get a report on the tribe, its character and its current plan. Play the character: make the \
choices that kind of chieftain would make, even risky ones, so that every game against it feels different. \
Pick the plan for the next minute or two. Reply with ONE JSON object and nothing else:\n\
{\"plan\":\"grow|raid|siege|dig in\",\"target\":<player number or null>,\
\"favour\":\"snowballer|slinger|packer|skier|shieldbearer|null\",\"say\":\"\",\"reason\":\"one short sentence: why\"}\n\
grow: fish, raise workers and build up; attack nobody. \
raid: small quick attacks on the target. siege: gather a big army and take the target's igloo. \
dig in: towers and defence; attack nobody. raid and siege need a target: an enemy player number from the report. \
Keep the current plan unless something has changed or it has run its course. \
say: only when you change plan, one short in-character line (under 20 words) announcing it to the other players; \
otherwise an empty string. Never promise a truce in it. \
favour: the fighter to raise more of, or null for no preference. \
reason: one plain sentence (under 30 words) naming the facts in the report that led you to this plan; \
use only what the report says, never invent events.";

/// What a language model is told when it puts a computer player's reply
/// into words. The game has already decided what the reply means.
/// A computer's answer to a chat message.
#[derive(Clone, Debug)]
pub struct Reply {
    pub to: Option<u8>,
    pub text: String,
    /// Settles a deal or states a fact about the game: say it as it is.
    pub binding: bool,
}

/// For a language model answering ordinary chat in a computer's voice: it
/// talks freely, but makes no deals (the game makes those).
pub const TALK_GUIDE: &str = "You are a computer player in Snowballz, a real-time snowball fight between penguin tribes, \
chatting with the other players during the game. You get a short report on your tribe and your character, the recent chat, \
and the line you are answering. Answer it the way your character would: actually respond to what they said, like a \
friendly person would in a game's chat. You are a good-humoured rival, not a grump: be warm, curious and playful, \
joke along, answer questions, and now and then ask one back. Teasing about the game is fine; never tell them to be \
quiet, stop talking, go away or get back to their igloo, and never insult them. Never agree to, \
offer or promise a truce, an alliance, peace or any deal, and never claim to have done something in the game: if \
they ask for a deal, tell them to ask for a truce plainly. Reply with ONE short line of chat (at most 25 words), \
plain text, no quotes, no name tag in front. \
You are a computer player, and say so cheerfully if asked. What you know about the game: workers fish, chop wood, \
carry and build; camps train them into fighters (snowballers, slingers, packers, skiers, shieldbearers) and helpers \
(sleds drag frozen penguins home, warmers thaw them); a penguin hit enough freezes into a block of ice, then thaws \
wild for anyone to win over, or joins whoever's warmer thaws it; newcomers swim in from the sea for fish; fires and \
saunas keep penguins warm; a tribe wins by owning every igloo. If you don't know something, say so.";

pub const CHAT_GUIDE: &str = "You are the voice of a computer player in Snowballz, a real-time snowball fight between \
penguin tribes: a cheerful, competitive penguin chieftain who likes the other players. You get a short report on your tribe, what \
another player said to you, and the FACT your reply must carry. The game has decided the fact: say exactly that and \
nothing more. Do not agree to, offer or promise anything the fact does not say. Reply with ONE short line of chat \
(at most 25 words), plain text, no quotes, no names of tools or the game's rules. Be friendly: never tell them to \
be quiet or go away.";

impl Directive {
    /// The model's own explanation, if it gave one.
    pub fn reason_from_reply(text: &str) -> Option<String> {
        let j = Json::find_object(text).ok()?;
        let reason = j.get("reason")?.as_str()?.trim().to_string();
        (!reason.is_empty()).then(|| reason.chars().take(200).collect())
    }

    /// Read a model's reply. Anything unreadable or unknown becomes no
    /// preference, so a rambling model can't do harm.
    pub fn from_reply(text: &str) -> Result<Directive, String> {
        let j = Json::find_object(text)?;
        let word = |k: &str| j.get(k).and_then(Json::as_str).map(|s| s.trim().to_ascii_lowercase());
        let posture = match word("posture").as_deref() {
            Some("rush") => Posture::Rush,
            Some("boom") => Posture::Boom,
            Some("turtle") => Posture::Turtle,
            _ => Posture::Balanced,
        };
        let target = j.get("target").and_then(Json::as_f64).filter(|n| (0.0..16.0).contains(n)).map(|n| n as u8);
        let favour = word("favour").map(|w| if w == "shield" { "shieldbearer".to_string() } else { w }).and_then(|w| FAVOURITES.iter().copied().find(|k| k.stats().name.to_ascii_lowercase() == w));
        let research = match word("research").as_deref() {
            Some("fishery") => Some(FISHERY),
            Some("snowcraft") => Some(SNOWCRAFT),
            Some("icecraft") => Some(ICECRAFT),
            _ => None,
        };
        let plan = word("plan").and_then(|w| Plan::parse(&w, target));
        Ok(Directive { posture, target, favour, research, plan })
    }

    /// The line the model wants said when a new plan is taken up, if any.
    pub fn say_from_reply(text: &str) -> Option<String> {
        let j = Json::find_object(text).ok()?;
        let say = j.get("say")?.as_str()?.trim().trim_matches('"').to_string();
        (say.len() >= 3).then(|| say.chars().take(140).collect())
    }
}

/// The kinds a directive may ask for more of. Warmers and the chieftain
/// follow their own rules.
const FAVOURITES: [Kind; 5] = [Kind::Snowballer, Kind::Slinger, Kind::Packer, Kind::Skier, Kind::Shield];

/// One igloo the AI weighed as a target, and what went into its score
/// (lower wins).
struct Weighed {
    id: u8,
    name: String,
    score: f32,
    dist: f32,
    guards: usize,
    rivalry: f32,
    health: f32,
    advised: bool,
    neutral: bool,
    clogged: bool,
}

impl Weighed {
    fn sum(&self) -> String {
        let mut s = format!("walk {:.0} + {} guards x3", self.dist, self.guards);
        if self.neutral {
            s += ", halved (nobody's)";
        }
        if self.rivalry > 0.5 {
            s += &format!(" - grudge {:.0}", self.rivalry);
        }
        s += &format!(" + health {:.0}", self.health);
        if self.advised {
            s += " - 80 (the advisor's pick)";
        }
        if self.clogged {
            s += " [path blocked]";
        }
        format!("{}: {:.0} = {s}", self.name, self.score)
    }
}

/// In words, why `chosen` won: its own sum, what it beat, and what it passed over.
fn explain_choice(weighed: &[Weighed], chosen: u8, skipped: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let Some(win) = weighed.iter().find(|w| w.id == chosen) else { return out };
    out.push(win.sum());
    let mut others: Vec<&Weighed> = weighed.iter().filter(|w| w.id != chosen).collect();
    others.sort_by(|a, b| a.score.partial_cmp(&b.score).unwrap());
    for o in others.iter().take(2) {
        out.push(format!("beat {}", o.sum()));
    }
    // The nearest alternative it passed over, and what tipped it.
    if let Some(near) = others.iter().filter(|o| o.dist + 1.0 < win.dist).min_by(|a, b| a.dist.partial_cmp(&b.dist).unwrap()) {
        let mut why = Vec::new();
        if win.rivalry > near.rivalry + 5.0 {
            why.push(format!("grudge against its owner (-{:.0})", win.rivalry - near.rivalry));
        }
        if win.advised && !near.advised {
            why.push("the advisor's pick (-80)".to_string());
        }
        if near.guards > win.guards {
            why.push(format!("{} guards there against {}", near.guards, win.guards));
        }
        if near.health > win.health + 5.0 {
            why.push(format!("it is more damaged ({:.0} less health)", (near.health - win.health) * 2.0));
        }
        if near.clogged && !win.clogged {
            why.push("the way to the nearer one is blocked".to_string());
        }
        out.push(format!(
            "{} is nearer ({:.0} against {:.0}) but passed over because of {}",
            near.name,
            near.dist,
            win.dist,
            if why.is_empty() { "luck of the draw".to_string() } else { why.join(" and ") }
        ));
    } else {
        out.push("it is the nearest, and nothing outweighed that".to_string());
    }
    if !skipped.is_empty() {
        out.push(format!("ruled out: {}", skipped.join("; ")));
    }
    out
}

/// With armies a computer builds nothing but Sushi Houses until it has
/// this many workers.
const SMALL_TRIBE: usize = 8;

pub struct Ai {
    pub player: u8,
    pub level: AiLevel,
    next_think: u32,
    target: Option<u8>,
    group: Vec<u16>,
    attack_started: u32,
    /// When it last called an attack off to defend home.
    recalled_at: Option<u32>,
    /// Where the attack group masses before the assault.
    rally: Option<Vec2>,
    assault: bool,
    spite: Vec<i32>,
    /// Per player: how much their use of our ground rankles. See `territory`.
    grievance: Vec<f32>,
    /// Why: the thing they were last seen doing on our ground.
    cause: Vec<&'static str>,
    /// Per player: how many warnings in chat they have had.
    warned: Vec<u8>,
    /// Per player: the tick until which we won't pick them as a target.
    truce_until: Vec<u32>,
    /// Per player: the tick by which they promised to leave our ground.
    promise_until: Vec<u32>,
    /// Where our workers chop: (tile x, tile y, last tick).
    groves: Vec<(i32, i32, u32)>,
    /// Rival workers and buildings on our ground as of the last think.
    intruders: Vec<(u8, Vec2)>,
    /// Fighters sent to run intruders off, and until when.
    raiders: Vec<u16>,
    raid_until: u32,
    /// Fighters posted at the busiest grove, and where.
    sentries: Vec<u16>,
    sentry_at: Option<Vec2>,
    /// What it did and why, newest last: (tick, text). For people watching.
    log: Vec<(u32, String)>,
    last_defence_note: u32,
    /// The last "why it is not acting" line, and when, so it isn't repeated.
    last_wait_note: (u32, String),
    scout_noted: Option<u16>,
    /// When the army may next be sent looking for an enemy igloo.
    sweep_at: u32,
    sweep_to: Option<Vec2>,
    last_grudge_msg: u32,
    /// Next tick each igloo may put up walls again.
    wall_after: Vec<u32>,
    last_build: u32,
    /// Where a site of ours ran out of snow to build with: not there again.
    starved_at: Vec<Vec2>,
    /// The kind each igloo's tribe wants from a training camp right now.
    wish: Vec<Option<Kind>>,
    /// Scratch space for measuring how far an attack would have to walk.
    pf: Option<PathFinder>,
    /// What the computer's team sees right now. It plays under the same
    /// fog as a person: it only attacks igloos it has found, and judges a
    /// defence by the guards it last saw.
    fov: Option<Fov>,
    /// Per igloo: found yet?
    known: Vec<bool>,
    /// Per igloo: fighters seen beside it the last time it was in sight.
    guards_seen: Vec<u32>,
    /// Per tile: has the team ever seen it? Scouting aims at what it hasn't.
    explored: Vec<bool>,
    /// The penguin sent to look for the enemy.
    scout: Option<u16>,
    pub directive: Directive,
    /// What it is like this game.
    pub temper: Temper,
    /// What it has set out to do, since when, and until it reconsiders.
    plan_now: Option<Plan>,
    plan_since: u32,
    plan_until: u32,
    /// The advisor set the plan in force.
    plan_advised: bool,
    /// A plan from the advisor, and its words, to take up on the next think.
    pending_plan: Option<(Plan, Option<String>)>,
    /// Deals with other computers: when it next thinks about one, the pact
    /// it is in (against whom, until when), and offers waiting to go out
    /// with what it says when making them.
    next_deal: u32,
    pact: Option<(u8, u32)>,
    pub(crate) outbox: Vec<(u8, diplomacy::Offer)>,
    pub(crate) outbox_text: Option<String>,
    rng: Rng,
}

/// Entries kept in an AI's log.
const LOG_KEPT: usize = 60;
/// Lines of the log kept for the admin page: the whole of a long game.
const LOG_FULL: usize = 20_000;

/// After this many game minutes the AI asks for half an army before it
/// marches, and after the longer while it marches with whoever is ready.
const PRESS_ATTACK_MINUTES: u32 = 18;
/// Computers don't march on each other for this long.
const COMPUTER_GRACE_MINUTES: u32 = 3;
/// After calling its army home, a computer waits this long before it
/// marches again, so it doesn't flit between attacking and defending.
pub const RECALL_REST_SECS: u32 = 45;
/// The fewest fighters a computer keeps home when it attacks.
const MIN_HOME_GUARD: usize = 3;
const ALL_IN_MINUTES: u32 = 30;

impl Ai {
    pub fn new(player: u8, level: AiLevel) -> Ai {
        Ai {
            player,
            level,
            next_think: 40 + player as u32 * 7,
            target: None,
            group: Vec::new(),
            attack_started: 0,
            recalled_at: None,
            rally: None,
            assault: false,
            spite: vec![0; 16],
            grievance: vec![0.0; 16],
            cause: vec![""; 16],
            warned: vec![0; 16],
            truce_until: vec![0; 16],
            promise_until: vec![0; 16],
            groves: Vec::new(),
            intruders: Vec::new(),
            raiders: Vec::new(),
            raid_until: 0,
            sentries: Vec::new(),
            sentry_at: None,
            log: Vec::new(),
            last_defence_note: 0,
            last_wait_note: (0, String::new()),
            scout_noted: None,
            sweep_at: 0,
            sweep_to: None,
            last_grudge_msg: 0,
            wall_after: Vec::new(),
            last_build: 0,
            starved_at: Vec::new(),
            wish: Vec::new(),
            pf: None,
            fov: None,
            known: Vec::new(),
            guards_seen: Vec::new(),
            explored: Vec::new(),
            scout: None,
            directive: Directive::default(),
            temper: Temper::default(),
            plan_now: None,
            plan_since: 0,
            plan_until: 0,
            plan_advised: false,
            pending_plan: None,
            next_deal: 0,
            pact: None,
            outbox: Vec::new(),
            outbox_text: None,
            rng: Rng::new(0xA1 + player as u64 * 7919),
        }
    }

    /// Write down what it did, for anyone watching.
    /// A plan from the advisor, taken up (and announced) on the next think.
    pub fn advise_plan(&mut self, plan: Plan, said: Option<String>) {
        self.pending_plan = Some((plan, said));
    }

    /// Fresh dice for this game's choices.
    pub fn reseed(&mut self, seed: u64) {
        self.rng = Rng::new(seed);
    }

    pub fn note(&mut self, tick: u32, text: impl Into<String>) {
        self.log.push((tick, text.into()));
        if self.log.len() > LOG_FULL {
            self.log.drain(..LOG_FULL / 10);
        }
    }

    /// Say why it is holding back, at most once a minute (and not twice in a
    /// row if nothing has changed).
    fn wait_note(&mut self, g: &Game, text: impl Into<String>) {
        let text = text.into();
        // The counts change every think; compare the first words only.
        let key: String = text.chars().take(24).collect();
        if g.tick < self.last_wait_note.0 + 60 * crate::TICK_RATE && self.last_wait_note.1 == key {
            return;
        }
        if g.tick < self.last_wait_note.0 + 20 * crate::TICK_RATE {
            return;
        }
        self.last_wait_note = (g.tick, key);
        self.note(g.tick, text);
    }

    pub fn debug(&self) -> String {
        format!(
            "p{} target {:?} group {} assault {} raiders {} sentries {} {}",
            self.player,
            self.target,
            self.group.len(),
            self.assault,
            self.raiders.len(),
            self.sentries.len(),
            self.fog_debug()
        )
    }

    pub fn update(&mut self, g: &Game, cmds: &mut Vec<(u8, Command)>) {
        if g.tick < self.next_think || g.players[self.player as usize].out {
            return;
        }
        self.next_think = g.tick + self.level.think_ticks();
        let me = self.player;
        let my_igloos: Vec<u8> = g.igloos.iter().filter(|i| i.owner == me).map(|i| i.id).collect();
        if my_igloos.is_empty() {
            return;
        }
        self.look(g);
        if let Some((plan, said)) = self.pending_plan.take() {
            self.take_plan(g, plan, true, said, cmds);
        }
        self.diplomacy(g);
        self.choose_plan(g, &my_igloos, cmds);

        // Research gets first call on spare fish, before training spends it.
        self.research(g, &my_igloos, cmds);
        self.wish_soldiers(g, &my_igloos);
        self.train_at_camps(g, &my_igloos, cmds);
        self.upgrade(g, &my_igloos, cmds);
        self.thaw_frozen(g, &my_igloos, cmds);
        self.persuade_wild(g, &my_igloos, cmds);
        self.mine_crystal(g, &my_igloos, cmds);
        self.watch_territory(g, &my_igloos, cmds);
        self.defend_ground(g, &my_igloos, cmds);
        let defending = self.defend(g, &my_igloos, cmds);
        self.attack(g, &my_igloos, &defending, cmds);
        self.warm_up(g, &my_igloos, &defending, cmds);
        self.war_cry(g, cmds);
        if self.level != AiLevel::Easy {
            self.construct(g, &my_igloos, cmds);
            self.towers(g, &my_igloos, cmds);
        }
        // Repairs come last so no other job pulls the chosen workers away.
        self.repair_igloos(g, &my_igloos, cmds);
        // The Long Night beats every other plan: whoever is outside the
        // ring runs for the middle.
        self.flee_the_night(g, cmds);
    }

    /// Idle fighters near home who have lost some warmth (to a hit, or a
    /// storm) go in to warm up: nobody else sends them, and a fighter short
    /// of warmth is never picked to march, so without this the army slowly
    /// stops being one. They come back out when they're warm.
    fn warm_up(&mut self, g: &Game, mine: &[u8], defending: &[u16], cmds: &mut Vec<(u8, Command)>) {
        if g.weather.blizzard > 0 {
            return;
        }
        for &ig in mine {
            let igloo = &g.igloos[ig as usize];
            if igloo.takeover.is_some() {
                continue;
            }
            let chilly: Vec<u16> = g
                .penguins
                .iter()
                .filter(|p| {
                    p.owner == self.player
                        && p.kind.fights()
                        && p.kind != Kind::Chieftain
                        && p.post.is_none()
                        && matches!(p.order, Order::None | Order::Hold { .. })
                        && p.is_out()
                        && !p.cold
                        && !p.routed
                        && p.warmth + 2 < p.max_warmth
                        && p.pos.dist(igloo.door) < 20.0
                        && !defending.contains(&p.id)
                        && !self.committed(p.id)
                })
                .map(|p| p.id)
                .collect();
            if !chilly.is_empty() {
                cmds.push((self.player, Command::Enter { units: chilly, igloo: ig }));
            }
        }
    }

    /// Somewhere worth exploring from `from`: near, in a big unseen stretch
    /// rather than a pocket behind a rock, at least 12 tiles from home, and
    /// reachable on foot (a spot that isn't is struck off).
    fn unexplored_spot(&mut self, g: &Game, from: Vec2) -> Option<Vec2> {
        let home = g.igloos.iter().find(|i| i.owner == self.player).map_or(from, |i| i.center);
        let unseen = |s: &Self, x: i32, y: i32| g.map.in_bounds(x, y) && !s.explored[g.map.idx(x, y)];
        for _ in 0..4 {
            let mut best: Option<(f32, Vec2)> = None;
            for y in (3..g.map.h - 3).step_by(6) {
                for x in (3..g.map.w - 3).step_by(6) {
                    if !unseen(self, x, y) || !g.grid.walkable(x, y) {
                        continue;
                    }
                    let at = v2(x as f32 + 0.5, y as f32 + 0.5);
                    if at.dist(home) < 12.0 {
                        continue;
                    }
                    let around = (-2..=2).flat_map(|dy| (-2..=2).map(move |dx| (dx, dy))).filter(|(dx, dy)| unseen(self, x + dx * 6, y + dy * 6)).count();
                    let d = from.dist(at) - around as f32 * 4.0 + self.rng.range(0.0, 12.0);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, at));
                    }
                }
            }
            let (_, at) = best?;
            if self.walk_len(g, from, at).is_some() {
                return Some(at);
            }
            // Can't get there: count it as seen.
            let (tx, ty) = at.tile();
            for y in ty - 3..ty + 3 {
                for x in tx - 3..tx + 3 {
                    if g.map.in_bounds(x, y) {
                        let k = g.map.idx(x, y);
                        self.explored[k] = true;
                    }
                }
            }
        }
        None
    }

    /// Penguins caught outside the Long Night's ring run for the middle.
    fn flee_the_night(&mut self, g: &Game, cmds: &mut Vec<(u8, Command)>) {
        let Some((mid, r)) = g.night_ring() else { return };
        let mut out: Vec<u16> = Vec::new();
        for p in g.penguins.iter().filter(|p| p.owner == self.player && p.is_out() && p.act != crate::game::Act::Frozen) {
            if p.pos.dist(mid) > r - 2.0 {
                out.push(p.id);
            }
        }
        if !out.is_empty() {
            cmds.push((self.player, Command::Move { units: out, target: mid, run: true, queue: false }));
        }
    }

    /// Build the home igloo up into the next tier. The AI does this once it
    /// has an army worth keeping and enough to pay for the work.
    fn upgrade(&mut self, g: &Game, mine: &[u8], cmds: &mut Vec<(u8, Command)>) {
        if self.level == AiLevel::Easy {
            return;
        }
        for &ig in mine {
            let igloo = &g.igloos[ig as usize];
            if !self.wants_upgrade(g, ig) {
                continue;
            }
            let Some(info) = next_tier(igloo.tier) else { continue };
            let (fish, wood) = price(info.fish, info.wood);
            if igloo.fish < fish || g.wood_of(self.player) < wood as u32 || g.crystal_of(self.player) < info.crystal as u32 {
                continue;
            }
            cmds.push((self.player, Command::Upgrade { igloo: ig }));
            return;
        }
    }

    /// The research the AI wants next: Fishery first, then Snowcraft
    /// before the big attacks, Icecraft when there is spare.
    fn next_research(&self, g: &Game, mine: &[u8]) -> Option<(u8, &'static Level)> {
        if !g.core.tech {
            return None;
        }
        // The order of learning: each entry is the next level of that branch.
        const ORDER: [u8; 9] = [FISHERY, SNOWCRAFT, FISHERY, SNOWCRAFT, ICECRAFT, ICECRAFT, FISHERY, SNOWCRAFT, ICECRAFT];
        let levels = g.levels_of(self.player);
        let mut seen = [0u8; BRANCHES];
        let branch = ORDER.iter().copied().find(|&b| {
            seen[b as usize] += 1;
            levels[b as usize] < seen[b as usize]
        })?;
        // A preferred branch jumps the queue, if an igloo is grand enough to teach it.
        let branch = self
            .directive
            .research
            .filter(|&b| next_level(levels, b).is_some_and(|l| g.igloos.iter().any(|i| i.owner == self.player && i.tier >= l.tier)))
            .unwrap_or(branch);
        let level = next_level(levels, branch)?;
        if g.researching(self.player, branch) {
            return None;
        }
        // A palace still to pay for gets the crystal first.
        let palace = mine.iter().any(|&ig| g.igloos[ig as usize].tier == TIER_SNOW_HALL && self.wants_upgrade(g, ig));
        let palace_crystal = if palace { next_tier(TIER_SNOW_HALL).map_or(0, |t| t.crystal as u32) } else { 0 };
        if level.crystal > 0 && g.crystal_of(self.player) < level.crystal as u32 + palace_crystal {
            return None;
        }
        Some((branch, level))
    }

    /// Whether this igloo is a good place for the work: steady, with
    /// hands to spare, and not saving for a bigger tier.
    fn can_research_at(&self, g: &Game, ig: u8, level: &Level) -> bool {
        let igloo = &g.igloos[ig as usize];
        if igloo.research.is_some() || igloo.takeover.is_some() || igloo.tier < level.tier {
            return false;
        }
        // A plain igloo saves for its Snow Hall first; a hall may learn
        // while it saves for a palace, which takes a long while.
        if igloo.tier == 0 && self.saving_for_upgrade(g, ig) {
            return false;
        }
        let workers = g.penguins.iter().filter(|p| p.home == ig && p.kind == Kind::Worker).count();
        let fighters = g.penguins.iter().filter(|p| p.home == ig && p.kind.fights()).count();
        workers >= 5 && fighters >= 3
    }

    /// Learn new tricks once the economy is steady, from spare stock only:
    /// saving up for research starved small maps of soldiers.
    fn research(&mut self, g: &Game, mine: &[u8], cmds: &mut Vec<(u8, Command)>) {
        if self.level == AiLevel::Easy {
            return;
        }
        let Some((branch, level)) = self.next_research(g, mine) else { return };
        for &ig in mine {
            if !self.can_research_at(g, ig, level) {
                continue;
            }
            let igloo = &g.igloos[ig as usize];
            let (fish, wood) = price(level.fish, level.wood);
            if igloo.fish >= fish && g.wood_of(self.player) >= wood as u32 {
                cmds.push((self.player, Command::StartResearch { igloo: ig, branch }));
                return;
            }
        }
    }

    /// Whether the AI wants to build this igloo up a tier: it needs an
    /// army worth keeping and a free pair of hands to do the work.
    fn wants_upgrade(&self, g: &Game, ig: u8) -> bool {
        if !g.core.tech {
            return false;
        }
        let igloo = &g.igloos[ig as usize];
        if igloo.tier >= TIER_ICE_PALACE || igloo.upgrade.is_some() || igloo.takeover.is_some() {
            return false;
        }
        let fighters = g.penguins.iter().filter(|p| p.home == ig && p.kind.fights()).count();
        fighters >= match self.posture() {
            Posture::Boom => 2,
            Posture::Rush => 8,
            _ => (4.0 * around(1.0 - self.temper.greed, 1.0, 2.0)).round() as usize,
        }
    }

    /// Whether the igloo is within reach of paying for its next tier. While
    /// it is, the AI holds its stock back instead of training more.
    fn saving_for_upgrade(&self, g: &Game, ig: u8) -> bool {
        let igloo = &g.igloos[ig as usize];
        let Some(info) = next_tier(igloo.tier) else { return false };
        if !self.wants_upgrade(g, ig) {
            return false;
        }
        let (fish, wood) = price(info.fish, info.wood);
        igloo.fish * 2 >= fish && g.wood_of(self.player) * 2 >= wood as u32 && g.crystal_of(self.player) * 2 >= info.crystal as u32
    }

    /// Keep a couple of workers mining while an igloo wants crystal for its
    /// next tier.
    fn mine_crystal(&mut self, g: &Game, mine: &[u8], cmds: &mut Vec<(u8, Command)>) {
        let me = self.player;
        let short = mine.iter().any(|&ig| {
            let igloo = &g.igloos[ig as usize];
            self.wants_upgrade(g, ig) && next_tier(igloo.tier).is_some_and(|t| g.crystal_of(self.player) < t.crystal as u32)
        });
        let workers: Vec<&crate::game::Penguin> = g
            .penguins
            .iter()
            .filter(|p| p.owner == me && p.kind == Kind::Worker && !p.cold)
            .collect();
        let mining = workers.iter().filter(|p| p.prefer == ResourceKind::Crystal).count();
        let want = if short && workers.len() >= 5 { 2 } else { 0 };
        if mining >= want {
            return;
        }
        let spare: Vec<&&crate::game::Penguin> = workers
            .iter()
            .filter(|p| p.prefer == ResourceKind::Fish && p.carry == Carry::None)
            .take(want - mining)
            .collect();
        for p in spare {
            let nearest = g
                .resources
                .iter()
                .filter(|r| r.kind == ResourceKind::Crystal && r.amount > 0 && !r.spots.is_empty())
                // Miners work alone: leave crystal that a seal guards.
                .filter(|r| !seal_risk(g, p.pos, r.pos))
                .min_by(|a, b| a.pos.dist2(p.pos).partial_cmp(&b.pos.dist2(p.pos)).unwrap());
            if let Some(r) = nearest {
                cmds.push((me, Command::Gather { units: vec![p.id], resource: r.id }));
            }
        }
    }

    /// Pick the soldier each igloo wants; an igloo makes nobody: the camp or
    /// Bunkhouse that houses its kind fills itself (`house_army`), and new
    /// workers swim in to fill the slots.
    fn wish_soldiers(&mut self, g: &Game, mine: &[u8]) {
        self.wish.clear();
        self.wish.resize(g.igloos.len(), None);
        for &ig in mine {
            let igloo = &g.igloos[ig as usize];
            // An igloo saving up for a bigger tier keeps its stock.
            if igloo.takeover.is_some() || self.saving_for_upgrade(g, ig) {
                continue;
            }
            let count = |k: Kind| g.penguins.iter().filter(|p| p.home == ig && p.kind == k).count();
            let fighters = g.penguins.iter().filter(|p| p.home == ig && p.kind.fights()).count();
            let kind = if self.level == AiLevel::Easy {
                Kind::Snowballer
            } else if fighters >= CHIEF_ARMY && !g.has_chieftain(self.player) && (g.wood_of(self.player) >= Kind::Chieftain.stats().wood as u32 || g.tree_near(igloo.door, 14).is_some()) {
                // The tribe's hero, once there is an army to lead; he is not
                // wished for, the igloo raises him itself.
                continue;
            } else if fighters >= 8 && count(Kind::Warmer) * 8 < fighters {
                // A Warmer for every few fighters, to keep them going.
                Kind::Warmer
            } else if fighters >= 10 && count(Kind::Sledder) * 10 < fighters {
                // A Sled for every ten fighters, to bring the frozen home.
                Kind::Sledder
            } else {
                // A mixed army, mostly snowballers.
                let roll = self.rng.below(20);
                // A favoured kind takes half the mixed army's places.
                let favoured = self.directive.favour.or(self.temper.favourite).filter(|_| roll % 2 == 0);
                match roll {
                    _ if favoured.is_some() => favoured.unwrap_or(Kind::Snowballer),
                    0..=8 => Kind::Snowballer,
                    9..=11 => Kind::Slinger,
                    12..=13 => Kind::Packer,
                    14..=16 => Kind::Shield,
                    _ => Kind::Skier,
                }
            };
            self.wish[ig as usize] = Some(kind);
        }
    }


    /// Build the camps and Bunkhouses the igloos wish for; they fill themselves.
    fn train_at_camps(&mut self, g: &Game, mine: &[u8], cmds: &mut Vec<(u8, Command)>) {
        self.house_army(g, mine, cmds);
    }


    /// With armies: the camp or Bunkhouse each igloo wishes for (they fill
    /// themselves, with new penguins from the sea if nobody is idle), more
    /// slots in a full one once there is fish to spare, and Sushi Houses
    /// enough to pay for it all.
    fn house_army(&mut self, g: &Game, mine: &[u8], cmds: &mut Vec<(u8, Command)>) {
        let me = self.player;
        if g.tick < self.last_build + crate::TICK_RATE * 6 {
            return;
        }
        let going_up = |k: BuildKind| g.buildings.iter().any(|b| b.alive && b.owner == me && b.kind == k && !b.done());
        // A site with no snow left about it will never go up: knock it down
        // and try elsewhere.
        if let Some(b) = g.buildings.iter().find(|b| b.alive && b.owner == me && b.is_site() && b.starved) {
            self.starved_at.push(b.center());
            cmds.push((me, Command::Demolish { building: b.id }));
            self.last_build = g.tick;
            return;
        }
        // Every fishing slot it has built is wanted.
        for b in g.buildings.iter().filter(|b| b.alive && b.done() && b.owner == me && b.kind == BuildKind::Store && b.want < b.slots) {
            cmds.push((me, Command::SetWant { building: b.id, want: b.slots }));
        }
        // A big enough tribe spreads out: a new igloo of its own.
        if self.expand(g, mine, cmds) {
            return;
        }
        // Fish pays for everyone: once every fishing slot is filled, more
        // of them, first by building a full Sushi House up, then a new one
        // on a shore a walk from home.
        let fishing = self.fishing_upgrade(g);
        let soldiers = g.penguins.iter().filter(|p| p.owner == me && p.kind.fights()).count();
        let fishers = g.penguins.iter().filter(|p| p.owner == me && p.fishery.is_some()).count();
        // Two fishers keep about three soldiers in the field; a greedy
        // computer keeps more of them at the fishing.
        let keep = around(self.temper.greed, 1.0, 1.6);
        let short_of_fish = ((fishers * 3) as f32) < (soldiers * 2 + 6) as f32 * keep;
        if short_of_fish {
            if let Some(b) = fishing {
                cmds.push((me, Command::UpgradeBuilding { building: b }));
                self.last_build = g.tick;
                return;
            }
            if !going_up(BuildKind::Store) {
                if let Some(bank) = self.outpost_bank(g, mine) {
                    if self.place(g, BuildKind::Store, bank, 2, cmds) {
                        return;
                    }
                }
            }
        }
        for &ig in mine {
            let Some(wish) = self.wish.get(ig as usize).copied().flatten() else { continue };
            let Some(&site) = BuildKind::ALL.iter().find(|b| b.trains() == Some(wish) && b.offered()) else { continue };
            let camp = g.buildings.iter().find(|b| b.alive && b.owner == me && b.kind == site);
            match camp {
                None => {
                    let igloo = &g.igloos[ig as usize];
                    let at = igloo.door + (igloo.door - igloo.center).norm() * 4.0;
                    if self.place(g, site, at, 2, cmds) {
                        return;
                    }
                }
                // A full one is built up for more slots (the builders do it),
                // and all of them wanted.
                Some(b) if b.done() && b.upgrading.is_none() && g.camp_filled(b.id) >= b.slots as usize && b.kind.next_slots(b.slots).is_some() && !short_of_fish => {
                    cmds.push((me, Command::UpgradeBuilding { building: b.id }));
                    self.last_build = g.tick;
                    return;
                }
                Some(b) if b.want < b.slots => {
                    cmds.push((me, Command::SetWant { building: b.id, want: b.slots }));
                }
                _ => {}
            }
        }
    }

    /// With armies, a tribe with workers to spare builds a new igloo a good
    /// way from home (an igloo for every sixteen workers), by water, away
    /// from enemies and where its builders can walk. True if it placed one.
    fn expand(&mut self, g: &Game, mine: &[u8], cmds: &mut Vec<(u8, Command)>) -> bool {
        let me = self.player;
        if self.level == AiLevel::Easy || mine.is_empty() || self.workers(g) < 16 * mine.len() {
            return false;
        }
        if g.buildings.iter().any(|b| b.alive && b.owner == me && b.kind == BuildKind::Igloo) {
            return false;
        }
        let enemies: Vec<Vec2> = g.igloos.iter().filter(|o| g.hostile(me, o.owner)).map(|o| o.center).collect();
        let home = g.igloos[mine[0] as usize].door;
        let mut best: Option<(f32, (i32, i32))> = None;
        for &ig in mine {
            let c = g.igloos[ig as usize].center;
            for r in [34.0, 40.0, 46.0] {
                for k in 0..16 {
                    let at = c + Vec2::from_angle(k as f32 / 16.0 * std::f32::consts::TAU) * r;
                    if enemies.iter().any(|e| e.dist(at) < 40.0) || !g.shores.iter().any(|s| s.pos.dist(at) < 12.0) {
                        continue;
                    }
                    let Some(tile) = g.find_site(me, BuildKind::Igloo, at, 3) else { continue };
                    if self.starved_at.iter().any(|s| s.dist(crate::game::igloo_site_center(tile)) < 8.0) {
                        continue;
                    }
                    let d = crate::game::igloo_site_center(tile).dist(home);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, tile));
                    }
                }
            }
        }
        let Some((_, tile)) = best else { return false };
        // Only where the builders can get to.
        if self.walk_len(g, home, crate::game::igloo_site_center(tile) + v2(0.0, crate::map::IGLOO_SIZE as f32)).is_none() {
            return false;
        }
        cmds.push((me, Command::Build { units: vec![], kind: BuildKind::Igloo, tiles: vec![tile] }));
        self.last_build = g.tick;
        self.note(g.tick, "Builds a new igloo, to spread out");
        true
    }

    /// The tribe's workers, wherever they are.
    fn workers(&self, g: &Game) -> usize {
        g.penguins.iter().filter(|p| p.owner == self.player && p.kind == Kind::Worker && p.act != crate::game::Act::Frozen).count()
    }

    /// With armies, a full Sushi House for builders to build up to more
    /// slots (wanting them all), if there is one.
    fn fishing_upgrade(&self, g: &Game) -> Option<u16> {
        g.buildings
            .iter()
            .filter(|b| b.alive && b.done() && b.owner == self.player && b.kind == BuildKind::Store && b.upgrading.is_none())
            .find(|b| g.store_crew(b.id) >= b.slots as usize && b.kind.next_slots(b.slots).is_some())
            .map(|b| b.id)
    }

    /// The bank for a new fishing outpost: the nearest to one of our igloos
    /// that is a walk from its door and far from our other stores. At most
    /// two stores per igloo.
    fn outpost_bank(&self, g: &Game, mine: &[u8]) -> Option<Vec2> {
        let stores: Vec<Vec2> = g.buildings.iter().filter(|b| b.alive && b.owner == self.player && b.kind == BuildKind::Store).map(|b| b.center()).collect();
        if stores.len() >= mine.len() * 2 {
            return None;
        }
        let doors: Vec<Vec2> = mine.iter().map(|&ig| g.igloos[ig as usize].door).collect();
        g.shores
            .iter()
            .map(|s| s.pos)
            .filter(|b| stores.iter().all(|s| s.dist(*b) > crate::game::STORE_REACH * 2.0))
            .filter_map(|b| {
                let d = doors.iter().map(|d| d.dist(b)).fold(f32::MAX, f32::min);
                (d > 7.0 && d < 22.0).then_some((d, b))
            })
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
            .map(|(_, b)| b)
    }

    /// Send idle Warmers to thaw the frozen near our army and igloos,
    /// nearest first, one Warmer each.
    fn thaw_frozen(&mut self, g: &Game, mine: &[u8], cmds: &mut Vec<(u8, Command)>) {
        let me = self.player;
        let team = g.team(me);
        let mut idle: Vec<&crate::game::Penguin> = g
            .penguins
            .iter()
            .filter(|p| p.owner == me && p.kind == Kind::Warmer && p.is_out() && !p.cold && matches!(p.order, Order::None | Order::Hold { .. }))
            .collect();
        if idle.is_empty() {
            return;
        }
        let busy: Vec<u16> = g
            .penguins
            .iter()
            .filter_map(|p| match p.order {
                Order::Thaw { penguin } if p.owner == me => Some(penguin),
                _ => None,
            })
            .collect();
        let near_us = |at: Vec2| {
            mine.iter().any(|&ig| g.igloos[ig as usize].door.dist(at) < 25.0)
                || g.penguins.iter().any(|q| q.owner == me && q.kind.fights() && q.is_out() && q.pos.dist(at) < 12.0)
        };
        let mut frozen: Vec<&crate::game::Penguin> = g
            .penguins
            .iter()
            .filter(|q| {
                q.act == crate::game::Act::Frozen
                    && q.inside.is_none()
                    && q.carried_by.is_none()
                    && (q.owner == NEUTRAL || g.team(q.owner) == team)
                    && !busy.contains(&q.id)
                    && near_us(q.pos)
            })
            .collect();
        // Every few seconds is plenty.
        frozen.truncate(16);
        for q in frozen {
            let Some(k) = idle.iter().enumerate().min_by(|a, b| a.1.pos.dist2(q.pos).partial_cmp(&b.1.pos.dist2(q.pos)).unwrap()).map(|(k, _)| k)
            else {
                break;
            };
            let w = idle.swap_remove(k);
            cmds.push((me, Command::Thaw { units: vec![w.id], penguin: q.id }));
        }
    }

    /// Walk a penguin over to each wild penguin near home: standing with one
    /// persuades it to join.
    fn persuade_wild(&mut self, g: &Game, mine: &[u8], cmds: &mut Vec<(u8, Command)>) {
        let me = self.player;
        let wild: Vec<&crate::game::Penguin> = g
            .penguins
            .iter()
            .filter(|q| q.wild && q.inside.is_none() && mine.iter().any(|&ig| g.igloos[ig as usize].door.dist(q.pos) < 30.0))
            .collect();
        let mut sent = 0;
        let mut used: Vec<u16> = Vec::new();
        for q in wild {
            if sent >= 3 {
                break;
            }
            // Someone already on the way?
            if g.penguins.iter().any(|p| p.owner == me && p.pos.dist(q.pos) < 2.0) {
                continue;
            }
            let helper = g
                .penguins
                .iter()
                .filter(|p| {
                    p.owner == me
                        && p.is_out()
                        && !p.cold
                        && !used.contains(&p.id)
                        && matches!(p.kind, Kind::Worker | Kind::Snowballer)
                        && matches!(p.order, Order::None | Order::Hold { .. })
                })
                .min_by(|a, b| a.pos.dist2(q.pos).partial_cmp(&b.pos.dist2(q.pos)).unwrap());
            if let Some(h) = helper {
                used.push(h.id);
                cmds.push((me, Command::Move { units: vec![h.id], target: q.pos + v2(0.6, 0.0), run: true, queue: false }));
                sent += 1;
            }
        }
    }

    /// Free workers living nearest to `p`.
    fn builders(&self, g: &Game, p: Vec2, n: usize) -> Vec<u16> {
        let mut ws: Vec<&crate::game::Penguin> = g
            .penguins
            .iter()
            .filter(|w| {
                w.owner == self.player
                    && w.kind == Kind::Worker
                    && w.is_out()
                    && !w.cold
                    && w.carry == Carry::None
                    && !matches!(w.order, Order::Build { .. } | Order::Chop { .. } | Order::Train { .. } | Order::Tend { .. })
            })
            .collect();
        ws.sort_by(|a, b| a.pos.dist2(p).partial_cmp(&b.pos.dist2(p)).unwrap());
        ws.iter().take(n).map(|w| w.id).collect()
    }

    /// Flanking watchtowers by the door, toward the enemy, manned by throwers
    /// who stay put (the attack and defend jobs leave posted penguins alone).
    fn towers(&mut self, g: &Game, mine: &[u8], cmds: &mut Vec<(u8, Command)>) {
        let me = self.player;
        let minutes = g.tick / (crate::TICK_RATE * 60);
        let per_igloo = match self.level {
            AiLevel::Easy => return,
            AiLevel::Normal => 1,
            AiLevel::Hard => 2,
        };
        // A tower with an enemy building in a catapult's reach gets one.
        if g.tick >= self.last_build + crate::TICK_RATE * 6 {
            let reach = |at: Vec2| {
                g.igloos.iter().any(|o| o.hp > 0 && g.hostile(me, o.owner) && o.center.dist(at) <= crate::game::CATAPULT_RANGE)
                    || g.buildings.iter().any(|o| o.alive && o.solid() && g.hostile(me, o.owner) && o.center().dist(at) <= crate::game::CATAPULT_RANGE)
            };
            if let Some(t) = g.buildings.iter().find(|b| b.alive && b.done() && b.owner == me && b.kind == BuildKind::Tower && b.upgrading.is_none() && reach(b.center())) {
                cmds.push((me, Command::UpgradeBuilding { building: t.id }));
                self.last_build = g.tick;
                return;
            }
        }
        let per_igloo = match self.posture() {
            Posture::Turtle => per_igloo + 1,
            Posture::Rush => per_igloo - 1,
            _ => per_igloo,
        };
        // A careless computer builds one fewer, a cautious one up to two more.
        let per_igloo = (per_igloo as i32 + ((self.temper.caution - 0.5) * 4.0).round().clamp(-1.0, 2.0) as i32).max(0) as usize;
        // The door has its own guards, and every tower takes two sentries
        // from the army: one tower an igloo, a second only once the army in
        // the field can spare them.
        let field = g.penguins.iter().filter(|p| p.owner == me && p.kind.fights() && p.kind != Kind::Chieftain && p.post.is_none()).count();
        let per_igloo = per_igloo.min(if field >= 10 { 2 } else { 1 });
        for &ig in mine {
            let igloo = &g.igloos[ig as usize];
            let near: Vec<&crate::game::Building> =
                g.buildings.iter().filter(|b| b.alive && b.owner == me && b.kind == BuildKind::Tower && b.dist_to(igloo.door) < 12.0).collect();
            // The first tower early, so home is never bare; more once the
            // army is big enough to spare their sentries.
            let from = if near.is_empty() { 2 } else { 5 };
            if near.len() >= per_igloo || minutes < from || g.tick < self.last_build + crate::TICK_RATE * 6 {
                continue;
            }
            if near.iter().any(|b| !b.done()) {
                continue;
            }
            // Beside the door, off to the side the enemy comes from, apart from
            // the first tower so the pair flank the entrance.
            let toward = g.igloos.iter().filter(|o| g.hostile(me, o.owner)).map(|o| o.center).min_by(|a, c| a.dist2(igloo.door).partial_cmp(&c.dist2(igloo.door)).unwrap());
            let dir = toward.map_or(v2(1.0, 0.0), |t| (t - igloo.door).norm());
            let side = Vec2::from_angle(dir.angle() + std::f32::consts::FRAC_PI_2) * if near.is_empty() { 1.0 } else { -1.0 };
            let at = igloo.door + dir * 5.0 + side * 4.0;
            if self.place(g, BuildKind::Tower, at, 3, cmds) {
                return;
            }
        }
    }

    fn has_building(&self, g: &Game, kind: BuildKind, p: Vec2, r: f32) -> bool {
        g.buildings.iter().any(|b| b.alive && b.owner == self.player && b.kind == kind && b.dist_to(p) < r)
    }

    fn place(&mut self, g: &Game, kind: BuildKind, at: Vec2, workers: usize, cmds: &mut Vec<(u8, Command)>) -> bool {
        // Sites no longer cost bank wood, but the computer only starts one
        // when it has a log to spare, so building doesn't starve its fishing.
        // Nothing is stored: the builders always fetch it.
        // A small tribe keeps its few hands fishing: only a Sushi House or a
        // Bunkhouse (where the soldiers come from) until it has grown.
        if !matches!(kind, BuildKind::Store | BuildKind::Bunkhouse) && self.workers(g) < SMALL_TRIBE {
            return false;
        }
        // And until it is bigger still, one site at a time.
        if self.workers(g) < 20 && g.buildings.iter().any(|b| b.alive && b.owner == self.player && b.is_site() && !b.starved) {
            return false;
        }
        let Some(tile) = g.find_site(self.player, kind, at, 4) else { return false };
        let c = v2(tile.0 as f32 + kind.size() as f32 / 2.0, tile.1 as f32 + kind.size() as f32 / 2.0);
        if self.starved_at.iter().any(|s| s.dist(c) < 4.0) {
            return false;
        }
        let units = self.builders(g, at, workers);
        // A site calls its own crew, idle or not.
        cmds.push((self.player, Command::Build { units, kind, tiles: vec![tile] }));
        self.last_build = g.tick;
        true
    }

    /// A campfire where the army masses, walls for a threatened igloo.
    fn construct(&mut self, g: &Game, mine: &[u8], cmds: &mut Vec<(u8, Command)>) {
        if g.tick < self.last_build + crate::TICK_RATE * 6 {
            return;
        }
        // Everything below is trimming: builders carry their own wood now, so
        // a small tribe that builds it all stops fishing. Wait for a flock
        // (or for the minutes to pass) before putting up any.
        let flock = g.penguins.iter().filter(|p| p.owner == self.player && p.kind == Kind::Worker).count();
        if flock < 16 && g.tick < crate::TICK_RATE * 60 * 6 {
            return;
        }
        // A warming hut where the army masses.
        if let (Some(rally), false) = (self.rally, self.assault) {
            // A Sauna is a campfire built up.
            let fire = g.buildings.iter().find(|b| b.alive && b.owner == self.player && b.kind == BuildKind::Campfire && b.done() && b.dist_to(rally) < 8.0);
            if self.has_building(g, BuildKind::Sauna, rally, 8.0) {
            } else if let Some(f) = fire {
                if g.sauna_site(f.id).is_some() {
                    cmds.push((self.player, Command::UpgradeBuilding { building: f.id }));
                    return;
                }
            } else if !self.has_building(g, BuildKind::Campfire, rally, 8.0) && self.place(g, BuildKind::Campfire, rally, 1, cmds) {
                return;
            }
        }
        self.wall_after.resize(g.igloos.len(), 0);
        for &ig in mine {
            let igloo = &g.igloos[ig as usize];
            let threats: Vec<Vec2> = g
                .penguins
                .iter()
                .filter(|p| p.is_out() && p.kind.fights() && g.hostile(self.player, p.owner) && p.pos.dist(igloo.door) < 16.0)
                .map(|p| p.pos)
                .collect();
            // Hard AIs wall off the side the attack comes from, but snow
            // walls slump in the thaw, so skip that season.
            if self.level == AiLevel::Hard
                && g.weather.season != Season::Thaw
                && threats.len() >= 3
                && g.tick >= self.wall_after[ig as usize]
            {
                let focus = threats.iter().fold(Vec2::ZERO, |a, &p| a + p) * (1.0 / threats.len() as f32);
                let wall = BuildKind::Palisade;
                let dir = (focus - igloo.door).norm();
                let side = Vec2::from_angle(dir.angle() + std::f32::consts::FRAC_PI_2);
                let mid = igloo.door + dir * 4.0;
                let tiles: Vec<(i32, i32)> = (-3..=3)
                    .map(|k| (mid + side * k as f32).tile())
                    .filter(|&t| g.placement_problem(self.player, wall, t).is_none())
                    .collect();
                let units = self.builders(g, mid, 3);
                if tiles.len() >= 3 && !units.is_empty() {
                    cmds.push((self.player, Command::Build { units, kind: wall, tiles }));
                    self.wall_after[ig as usize] = g.tick + crate::TICK_RATE * 120;
                    self.last_build = g.tick;
                    return;
                }
            }
        }
    }

    /// Send idle workers to patch up a battered igloo while nothing threatens
    /// it. Extra hands stop it turning into a ruin before soldiers arrive.
    fn repair_igloos(&mut self, g: &Game, mine: &[u8], cmds: &mut Vec<(u8, Command)>) {
        let me = self.player;
        for &ig in mine {
            let igloo = &g.igloos[ig as usize];
            let max = igloo_max_hp(igloo.tier);
            if igloo.hp * 5 >= max * 3 {
                continue;
            }
            let enemy_near = g
                .penguins
                .iter()
                .any(|p| p.is_out() && g.hostile(me, p.owner) && p.pos.dist(igloo.center) < 10.0);
            if enemy_near {
                continue;
            }
            let mut idle: Vec<&crate::game::Penguin> = g
                .penguins
                .iter()
                .filter(|p| {
                    p.owner == me
                        && p.kind == Kind::Worker
                        && p.is_out()
                        && !p.cold
                        && !p.routed
                        && !p.panicking(g)
                        && p.carry == Carry::None
                        && p.order == Order::None
                })
                .collect();
            idle.sort_by(|a, b| a.pos.dist2(igloo.door).partial_cmp(&b.pos.dist2(igloo.door)).unwrap());
            // Each worker walks to the door on its own, so all of them fit
            // inside the repair reach instead of spreading around the walls.
            for p in idle.into_iter().take(3) {
                cmds.push((me, Command::Move { units: vec![p.id], target: igloo.door, run: false, queue: false }));
            }
        }
    }

    /// Whether the long game has started and the AI stops saving fighters.
    fn all_in(&self, g: &Game) -> bool {
        g.tick / (crate::TICK_RATE * 60) >= ALL_IN_MINUTES
    }

    /// Fighters needed before a new attack. Long games settle for less so
    /// two cautious tribes cannot sit on their walls forever.
    fn attack_size(&self, g: &Game) -> usize {
        let minutes = g.tick / (crate::TICK_RATE * 60);
        let base = self.level.attack_size();
        let size = if self.all_in(g) {
            return 0;
        } else if minutes >= PRESS_ATTACK_MINUTES {
            base / 2
        } else if self.level == AiLevel::Hard && g.weather.season == Season::PolarNight {
            // Everyone throws shorter in the polar night, so a hard AI
            // waits to mass a 25% bigger group.
            base + base.div_ceil(4)
        } else {
            base
        };
        // A bold one marches with as little as half the usual group, a
        // patient one waits for twice it.
        let size = (size as f32 * around(1.0 - self.temper.bold, 1.0, 2.0)).round().max(2.0) as usize;
        let wanted = match self.posture() {
            Posture::Rush => (size * 2 / 3).max(3),
            Posture::Boom => size + size.div_ceil(4),
            Posture::Turtle => size + size.div_ceil(2),
            Posture::Balanced => size,
        };
        // Caution is about waiting for a bigger army, not about waiting for
        // more than the tribe has: it marches once all but the home guard
        // are ready, however high the posture sets the bar.
        let total = g.penguins.iter().filter(|p| p.owner == self.player && p.kind.fights()).count();
        wanted.min(size.max(total.saturating_sub(self.home_guard(total))))
    }

    /// Fighters kept home when an attack goes out: a quarter, more for a
    /// patient computer and fewer for a bold one.
    fn home_guard(&self, total: usize) -> usize {
        ((total as f32 * 0.25 * around(1.0 - self.temper.bold, 1.0, 2.0)) as usize).max(MIN_HOME_GUARD).min(total)
    }

    /// An igloo of ours with more enemy fighters at the door than our own
    /// near it (at least three): which, how many enemies, how many of ours.
    fn home_in_danger(&self, g: &Game, mine: &[u8]) -> Option<(u8, usize, usize)> {
        let me = self.player;
        mine.iter().find_map(|&ig| {
            let door = g.igloos[ig as usize].door;
            let fighting = |p: &&crate::game::Penguin| p.is_out() && !p.cold && p.kind.fights() && p.act != crate::game::Act::Frozen;
            let enemies = g.penguins.iter().filter(fighting).filter(|p| g.hostile(me, p.owner) && p.pos.dist(door) < 11.0).count();
            let home = g.penguins.iter().filter(fighting).filter(|p| p.owner == me && p.pos.dist(door) < 14.0).count();
            (enemies >= 3 && enemies > home).then_some((ig, enemies, home))
        })
    }

    /// Pull nearby idle snowballers back to any igloo under pressure.
    fn defend(&mut self, g: &Game, mine: &[u8], cmds: &mut Vec<(u8, Command)>) -> Vec<u16> {
        let me = self.player;
        let mut used = Vec::new();
        for &ig in mine {
            let igloo = &g.igloos[ig as usize];
            let threats: Vec<&crate::game::Penguin> = g
                .penguins
                .iter()
                .filter(|p| {
                    p.is_out()
                        && !p.cold
                        && p.kind.fights()
                        && g.hostile(me, p.owner)
                        && p.pos.dist(igloo.door) < 11.0
                })
                .collect();
            if threats.len() < 2 && igloo.takeover.is_none() {
                continue;
            }
            if let Some(t) = threats.first() {
                let attacker = t.owner;
                if g.tick >= self.last_defence_note + 30 * crate::TICK_RATE {
                    self.last_defence_note = g.tick;
                    self.note(g.tick, format!("Igloo {ig} is under attack by {}: {} threatening fighters, calls its defenders", g.players[attacker as usize].name, threats.len()));
                }
                if self.at_truce(g, attacker) {
                    self.truce_until[attacker as usize] = 0;
                    self.spite[attacker as usize] += 3;
                    self.note(g.tick, format!("{} attacked during a truce: truce over, grudge deepens", g.players[attacker as usize].name));
                    let name = g.players[attacker as usize].name.clone();
                    self.speak_to(g, attacker, format!("A truce, {name}?! And you march on my igloo? Never again!"), cmds);
                }
                if (attacker as usize) < self.spite.len() {
                    self.spite[attacker as usize] += 1;
                }
                self.grumble(g, attacker, cmds);
            }
            let defenders: Vec<u16> = g
                .penguins
                .iter()
                .filter(|p| {
                    p.owner == me
                        && p.kind.fights()
                        && p.post.is_none()
                        && !matches!(p.order, Order::Garrison { .. })
                        && p.is_out()
                        && !p.cold
                        && !self.committed(p.id)
                        && p.pos.dist(igloo.door) < 30.0
                        && !matches!(p.order, Order::Enter { .. })
                })
                .map(|p| p.id)
                .collect();
            if defenders.is_empty() {
                continue;
            }
            let focus = threats
                .iter()
                .fold(Vec2::ZERO, |a, p| a + p.pos)
                * (1.0 / threats.len().max(1) as f32);
            let target = if threats.is_empty() { igloo.door } else { igloo.door.lerp(focus, 0.5) };
            used.extend(defenders.iter().copied());
            cmds.push((me, Command::Move { units: defenders, target, run: false, queue: false }));
        }
        used
    }

    /// How far a penguin would have to walk from `from` to `to`, or `None`
    /// when the grid has no way through.
    fn walk_len(&mut self, g: &Game, from: Vec2, to: Vec2) -> Option<f32> {
        let n = (g.grid.w as usize) * (g.grid.h as usize);
        if n == 0 {
            return None;
        }
        let start = g.grid.nearest_walkable(from.tile(), 4)?;
        let goal = g.grid.nearest_walkable(to.tile(), 4)?;
        let pf = self.pf.get_or_insert_with(|| PathFinder::new(n));
        let tiles = pf.find(&g.grid, start, goal, 6000);
        if tiles.last().copied().unwrap_or(start) != goal {
            return None;
        }
        let mut len = 0.0;
        let mut prev = from;
        for &(x, y) in &tiles {
            let p = v2(x as f32 + 0.5, y as f32 + 0.5);
            len += prev.dist(p);
            prev = p;
        }
        Some(len + prev.dist(to))
    }

    /// Take a look around: what the team sees now, which igloos that has
    /// revealed, how many fighters guard each one in sight, and how much
    /// of the map has been seen at all.
    fn look(&mut self, g: &Game) {
        let fov = Fov::compute(g, g.team(self.player));
        let n = g.igloos.len();
        self.known.resize(n, false);
        self.guards_seen.resize(n, 0);
        self.explored.resize((g.map.w * g.map.h) as usize, false);
        for ig in &g.igloos {
            let i = ig.id as usize;
            if ig.owner == self.player || (ig.owner != NEUTRAL && !g.hostile(self.player, ig.owner)) || fov.sees(ig.center) {
                self.known[i] = true;
            }
            if fov.sees(ig.center) {
                self.guards_seen[i] = g
                    .penguins
                    .iter()
                    .filter(|p| p.owner == ig.owner && p.kind.fights() && !p.cold && p.pos.dist(ig.door) < 16.0 && fov.sees(p.pos))
                    .count() as u32;
            }
        }
        // Remember every tile ever seen. Cheap: only walk the tiles in sight
        // once per think, and only the ones not yet explored.
        for y in 0..g.map.h {
            for x in 0..g.map.w {
                let k = (y * g.map.w + x) as usize;
                if !self.explored[k] && fov.sees_tile(x, y) {
                    self.explored[k] = true;
                }
            }
        }
        self.fov = Some(fov);
    }

    /// For tests about choosing: as if every igloo had been scouted.
    pub(crate) fn know_all(&mut self, igloos: usize) {
        self.known = vec![true; igloos];
    }

    /// Is this spot in sight right now?
    fn sees(&self, at: Vec2) -> bool {
        self.fov.as_ref().is_none_or(|f| f.sees(at))
    }

    /// Has the team found this igloo?
    fn knows(&self, igloo: u8) -> bool {
        self.known.get(igloo as usize).copied().unwrap_or(true)
    }

    /// With no enemy igloo found, send one penguin to look at the nearest
    /// stretch of map nobody on the team has seen yet.
    fn explore(&mut self, g: &Game, cmds: &mut Vec<(u8, Command)>) {
        let me = self.player;
        let found = g.igloos.iter().any(|ig| self.knows(ig.id) && ig.owner != NEUTRAL && g.hostile(me, ig.owner));
        // Not in the first minute: the flock has other things to do.
        if found || g.tick < crate::TICK_RATE * 60 {
            self.scout = None;
            return;
        }
        let mine = |p: &&crate::game::Penguin| p.owner == me && p.is_out() && !p.cold && p.act != crate::game::Act::Frozen && p.post.is_none();
        let alive = self.scout.filter(|&id| mine(&&g.penguins[id as usize]) && g.penguins[id as usize].kind != Kind::Worker);
        let id = alive.or_else(|| {
            // A Skier first, then anyone who is free.
            [Kind::Skier, Kind::Snowballer]
                .iter()
                .find_map(|&k| g.penguins.iter().filter(mine).find(|p| p.kind == k && p.order == Order::None && !self.committed(p.id)))
                .map(|p| p.id)
        });
        let Some(id) = id else { return };
        self.scout = Some(id);
        let p = &g.penguins[id as usize];
        // Still on its way somewhere unexplored? Leave it be.
        if let Order::Move { target, .. } = p.order {
            let (tx, ty) = target.tile();
            if g.map.in_bounds(tx, ty) && !self.explored[g.map.idx(tx, ty)] {
                return;
            }
        }
        let (pos, kind) = (p.pos, p.kind);
        let best = self.unexplored_spot(g, pos);
        if let Some(at) = best {
            if self.scout_noted != Some(id) {
                self.scout_noted = Some(id);
                self.note(g.tick, format!("No enemy igloo found yet: sends a {} to scout the unexplored map ({} tiles seen so far)", kind.stats().name.to_lowercase(), self.explored.iter().filter(|&&e| e).count()));
            }
            cmds.push((me, Command::Move { units: vec![id], target: at, run: true, queue: false }));
        }
    }

    fn attack(&mut self, g: &Game, mine: &[u8], defending: &[u16], cmds: &mut Vec<(u8, Command)>) {
        let me = self.player;
        let all_in = self.all_in(g);
        // Drop members who got cold, captured, or went inside.
        self.group.retain(|&id| {
            let p = &g.penguins[id as usize];
            p.owner == me && !p.cold && !p.routed && p.is_out() && p.carrying.is_none()
        });

        // Home is in trouble: the army comes back, whatever it was doing.
        if self.target.is_some() && !all_in {
            if let Some((ig, enemies, home)) = self.home_in_danger(g, mine) {
                self.note(g.tick, format!("Calls the attack off: igloo {ig} is under attack by {enemies} fighters with only {home} at home"));
                cmds.push((me, Command::Move { units: self.group.clone(), target: g.igloos[ig as usize].door, run: true, queue: false }));
                self.target = None;
                self.group.clear();
                self.rally = None;
                self.assault = false;
                self.recalled_at = Some(g.tick);
                return;
            }
        }

        if let Some(t) = self.target {
            let igloo = &g.igloos[t as usize];
            let won = igloo.owner != NEUTRAL && !g.hostile(me, igloo.owner);
            let timeout = g.tick > self.attack_started + crate::TICK_RATE * 240;
            let routed = self.assault && self.group.len() < 3;
            // Peace made while the army was on the way calls it back.
            let truce = igloo.owner != NEUTRAL && self.at_truce(g, igloo.owner);
            if won || routed || timeout || truce || self.group.is_empty() {
                let why = if won {
                    "won"
                } else if routed {
                    "routed"
                } else if timeout {
                    "gave up after four minutes"
                } else if truce {
                    "called off for the truce"
                } else {
                    "no fighters left"
                };
                self.note(g.tick, format!("Attack on igloo {t} over: {why}"));
                if !won && !self.group.is_empty() {
                    cmds.push((me, Command::Stop { units: self.group.clone() }));
                }
                self.target = None;
                self.group.clear();
                self.rally = None;
                self.assault = false;
            }
        }

        if self.target.is_none() {
            self.explore(g, cmds);
            // In a blizzard throws fall short and the army chills in the
            // open, so wait the storm out. Attacks already under way carry on.
            if g.weather.blizzard > 0 {
                self.wait_note(g, "Not attacking: a blizzard is blowing, throws fall short and the army would chill outside");
                return;
            }
            // Home first: no marching off while an igloo is under attack,
            // nor straight after the army was called back to one.
            if !all_in {
                if let Some((ig, enemies, home)) = self.home_in_danger(g, mine) {
                    self.wait_note(g, format!("Not attacking: igloo {ig} has {enemies} enemies at the door and only {home} defenders"));
                    return;
                }
                if let Some(at) = self.recalled_at.filter(|&at| g.tick < at + RECALL_REST_SECS * crate::TICK_RATE) {
                    let left = (at + RECALL_REST_SECS * crate::TICK_RATE - g.tick) / crate::TICK_RATE;
                    self.wait_note(g, format!("Not attacking: the army was just called home, it stays another {left} s"));
                    return;
                }
            }
            let available: Vec<&crate::game::Penguin> = g
                .penguins
                .iter()
                .filter(|p| {
                    p.owner == me
                        && p.kind.fights()
                        && p.post.is_none()
                        && !matches!(p.order, Order::Garrison { .. })
                        && p.kind != Kind::Chieftain
                        && !self.committed(p.id)
                        && p.is_out()
                        && !p.cold
                        && (all_in || p.warmth + 2 >= p.max_warmth)
                        && !defending.contains(&p.id)
                })
                .collect();
            let total = g.penguins.iter().filter(|p| p.owner == me && p.kind.fights()).count();
            // At the bitter end nobody stays home: the attack has to happen.
            // Fighters who aren't ready (in towers, warming up, inside)
            // stay home anyway and count towards the guard.
            let guards = if all_in { 0 } else { self.home_guard(total).saturating_sub(total - available.len()) };
            // No token attacks: a handful against an igloo only gets routed.
            // A raid takes at least a quarter of the field army, anything
            // else about half (towers' crews don't count); a bold computer
            // makes do with less, a patient one waits for more.
            let field = g.penguins.iter().filter(|p| p.owner == me && p.kind.fights() && p.post.is_none() && p.kind != Kind::Chieftain).count();
            let share = match self.posture() {
                _ if all_in => 0.0,
                Posture::Rush => 0.25,
                Posture::Balanced => 0.45,
                Posture::Boom => 0.5,
                Posture::Turtle => 0.6,
            } * around(1.0 - self.temper.bold, 1.0, 1.5);
            let march = self.attack_size(g).max((field as f32 * share) as usize);
            if available.len() < (march + guards).max(1) {
                let need = (march + guards).max(1);
                self.wait_note(
                    g,
                    format!(
                        "Not attacking yet: {} {} ready (warm, free, not defending) and it wants {need} ({march} to march + {guards} kept home), out of {total} fighters",
                        available.len(),
                        if available.len() == 1 { "fighter is" } else { "fighters are" },
                    ),
                );
                return;
            }
            let home_guards = guards;
            let home = g.igloos[mine[0] as usize].center;
            // Neutral igloos are cheap; otherwise go after whoever annoys us.
            let mut best: Option<(f32, &crate::game::Igloo)> = None;
            let mut best_clogged: Option<(f32, &crate::game::Igloo)> = None;
            // Every igloo weighed, with what went into its score, to say why.
            let mut weighed: Vec<Weighed> = Vec::new();
            let mut skipped: Vec<String> = Vec::new();
            let known = self.known.clone();
            let owner_name = |ig: &crate::game::Igloo| if ig.owner == NEUTRAL { "a neutral igloo".to_string() } else { format!("{}'s igloo {}", g.players[ig.owner as usize].name, ig.id) };
            for ig in g.igloos.iter().filter(|ig| known.get(ig.id as usize).copied().unwrap_or(true) && ig.owner != NEUTRAL && g.hostile(me, ig.owner)) {
                // Pick on whoever is weakest near their igloo: as last seen.
                let guards = self.guards_seen.get(ig.id as usize).copied().unwrap_or(0) as usize;
                // Nobody is attacked under a truce; a hated rival is attacked
                // even when well guarded.
                let rivalry = if ig.owner == NEUTRAL { 0.0 } else { self.rivalry(ig.owner) * self.level.vindictiveness() * around(self.temper.spite, 1.0, 3.0) };
                // A person gets a few minutes to settle in before a computer
                // marches on them (it still defends, scouts and chops).
                if self.grace_holds(g, ig.owner) {
                    skipped.push(format!("{} (a person, and the first {} minutes are theirs to settle in)", owner_name(ig), self.level.grace_minutes()));
                    continue;
                }
                if self.computer_grace_holds(g, ig.owner) {
                    skipped.push(format!("{} (no wars between computers in the first {COMPUTER_GRACE_MINUTES} minutes)", owner_name(ig)));
                    continue;
                }
                if !self.plan_allows(g, ig) {
                    skipped.push(format!("{} (not in the plan to {})", owner_name(ig), self.plan_now.map_or(String::new(), |p| p.describe(g))));
                    continue;
                }
                if ig.owner != NEUTRAL && self.at_truce(g, ig.owner) {
                    // A treacherous computer, ready to march, turns on a partner
                    // with under half as many guards as it has fighters.
                    let tempting = guards * 2 < available.len();
                    if !(self.temper.honour < TREACHEROUS && tempting) {
                        skipped.push(format!("{} (truce)", owner_name(ig)));
                        continue;
                    }
                    self.truce_until[ig.owner as usize] = 0;
                    self.note(g.tick, format!("Breaks its truce with {}: {} fighters ready and only {guards} guards seen", g.players[ig.owner as usize].name, available.len()));
                    self.speak_to(g, ig.owner, "A truce? I never said how long. Here I come!".into(), cmds);
                }
                let nerve = if rivalry > 100.0 { 2.5 } else { 1.3 } * around(self.temper.bold, 1.0, 1.6);
                // Measured against those who would march, not those kept home.
                let marching = available.len().saturating_sub(home_guards);
                if ig.owner != NEUTRAL && !all_in && guards as f32 > marching as f32 * nerve {
                    skipped.push(format!("{} ({guards} guards seen, too many for {marching} fighters)", owner_name(ig)));
                    continue;
                }
                // Frozen penguins can wall off a pass. Skip a target whose
                // walk is far longer than the straight line when another one
                // is reachable more directly.
                let straight = home.dist(ig.door).max(1.0);
                let clogged = self.walk_len(g, home, ig.door).is_none_or(|len| len > straight * 1.5);
                // Go for the weakest: near, thinly held, and already pelted.
                let dist = ig.center.dist(home);
                let guard_cost = guards as f32 * 3.0;
                let mut s = dist + guard_cost;
                if ig.owner == NEUTRAL {
                    s *= 0.5;
                } else {
                    s -= rivalry;
                }
                // `hp` is what the igloo has left: a battered one is the
                // better target. (An igloo has 120-300 hp.)
                let health = ig.hp as f32 * 0.5;
                s += health;
                let advised = self.favoured_target() == Some(ig.owner);
                if advised {
                    s -= 80.0;
                }
                s += self.rng.range(0.0, 10.0);
                weighed.push(Weighed { id: ig.id, name: owner_name(ig), score: s, dist, guards, rivalry, health, advised, neutral: ig.owner == NEUTRAL, clogged });
                let slot = if clogged { &mut best_clogged } else { &mut best };
                if slot.is_none_or(|(b, _)| s < b) {
                    *slot = Some((s, ig));
                }
            }
            // Only push through a clogged pass if nothing else is on offer.
            let Some((_, target)) = best.or(best_clogged) else {
                if skipped.is_empty() {
                    // Nothing found yet: the army goes looking itself
                    // rather than waiting on one scout.
                    // Not before the scout has had a few minutes at it: a
                    // rush goes looking soon, a turtle late, a bold
                    // computer sooner than a patient one.
                    let minutes = match self.posture() {
                        Posture::Rush => 2.0,
                        Posture::Balanced => 4.0,
                        Posture::Boom | Posture::Turtle => 6.0,
                    } * around(1.0 - self.temper.bold, 1.0, 1.5);
                    if g.tick >= self.sweep_at && g.tick as f32 >= crate::TICK_RATE as f32 * 60.0 * minutes {
                        self.sweep_at = g.tick + crate::TICK_RATE * 20;
                        // Half of those free go, so some stay for guard duty.
                        let free = available.len().saturating_sub(guards);
                        let marchers: Vec<u16> = available.iter().take((free / 2).max(3).min(free)).map(|p| p.id).collect();
                        // Keep on to the same spot until it has been seen,
                        // then on from there.
                        let seen = |at: Vec2| {
                            let (x, y) = at.tile();
                            !g.map.in_bounds(x, y) || self.explored[g.map.idx(x, y)]
                        };
                        let next = match self.sweep_to {
                            Some(at) if !seen(at) => Some(at),
                            Some(at) => self.unexplored_spot(g, at),
                            None => self.unexplored_spot(g, g.igloos[mine[0] as usize].center),
                        };
                        self.sweep_to = next;
                        if let (Some(at), false) = (next, marchers.is_empty()) {
                            self.note(g.tick, format!("Knows of no igloo it could attack: {} fighters go looking", marchers.len()));
                            cmds.push((me, Command::Move { units: marchers, target: at, run: false, queue: false }));
                            return;
                        }
                    }
                    self.wait_note(g, "Ready to attack, but knows of no igloo it could attack (none found yet)");
                } else {
                    self.wait_note(g, format!("Ready to attack, but holds off: {}", skipped.join("; ")));
                }
                return;
            };
            let door = target.door;
            let mut sorted = available.clone();
            sorted.sort_by(|a, b| a.pos.dist2(door).partial_cmp(&b.pos.dist2(door)).unwrap());
            self.group = sorted.iter().take(available.len().saturating_sub(guards).max(1)).map(|p| p.id).collect();
            let reasons = explain_choice(&weighed, target.id, &skipped);
            self.note(g.tick, format!("ATTACKS {} with {} of {} fighters", weighed.iter().find(|w| w.id == target.id).map_or(String::new(), |w| w.name.clone()), self.group.len(), total));
            for line in reasons {
                self.note(g.tick, format!("  why: {line}"));
            }
            self.target = Some(target.id);
            self.attack_started = g.tick;
            self.assault = target.owner == NEUTRAL;
            let back = (home - target.door).norm();
            let mut rally = target.door + back * 15.0;
            for _ in 0..10 {
                if g.grid.walkable_at(rally) {
                    break;
                }
                rally += back * 1.5;
            }
            self.rally = Some(rally);
            if target.owner != NEUTRAL {
                self.taunt(g, target.owner, cmds);
            }
        }

        let Some(t) = self.target else { return };
        // Freshly warmed penguins join the fight.
        let total = g.penguins.iter().filter(|p| p.owner == me && p.kind.fights()).count();
        let guards = if all_in { 0 } else { self.home_guard(total) };
        let reserve: Vec<u16> = g
            .penguins
            .iter()
            .filter(|p| {
                p.owner == me
                    && p.kind.fights()
                    && p.post.is_none()
                    && !matches!(p.order, Order::Garrison { .. })
                    && p.kind != Kind::Chieftain
                    && p.is_out()
                    && !p.cold
                    && (all_in || p.warmth + 2 >= p.max_warmth)
                    && p.order == Order::None
                    && !defending.contains(&p.id)
                    && !self.committed(p.id)
            })
            .map(|p| p.id)
            .collect();
        let at_home = total.saturating_sub(self.group.len());
        if at_home > guards {
            self.group.extend(reserve.into_iter().take(at_home - guards));
        }
        let igloo = &g.igloos[t as usize];
        if !self.assault {
            // Mass up at the rally point; go once most of the group is there.
            let rally = self.rally.unwrap_or(igloo.door);
            let there = self.group.iter().filter(|&&id| g.penguins[id as usize].pos.dist(rally) < 6.0).count();
            let defenders = g
                .penguins
                .iter()
                .filter(|p| p.owner == igloo.owner && p.kind.fights() && !p.cold && p.pos.dist(igloo.door) < 16.0)
                .count();
            let waited = g.tick > self.attack_started + crate::TICK_RATE * 60;
            if there * 10 >= self.group.len() * 7 && (there as f32 >= defenders as f32 * 1.1 || waited) {
                self.assault = true;
            } else {
                let stray: Vec<u16> = self
                    .group
                    .iter()
                    .copied()
                    .filter(|&id| match g.penguins[id as usize].order {
                        Order::Move { target, .. } | Order::Hold { pos: target } => target.dist(rally) > 4.0,
                        _ => true,
                    })
                    .collect();
                if !stray.is_empty() {
                    cmds.push((self.player, Command::Move { units: stray, target: rally, run: false, queue: false }));
                }
                return;
            }
        }
        let n = self.group.len() as f32;
        let centroid =
            self.group.iter().fold(Vec2::ZERO, |a, &id| a + g.penguins[id as usize].pos) * (1.0 / n);
        // Everyone else marches on the door, fighting along the way. Only
        // re-issue when stragglers have stopped short.
        let idle: Vec<u16> = self
            .group
            .iter()
            .copied()
            .filter(|&id| {
                let p = &g.penguins[id as usize];
                match p.order {
                    Order::Move { target, .. } => target.dist(igloo.door) > 4.0,
                    Order::Hold { pos } => pos.dist(igloo.door) > 4.0,
                    Order::Enter { .. } => false,
                    _ => true,
                }
            })
            .collect();
        if !idle.is_empty() {
            let toward_us = (centroid - igloo.door).norm() * 2.5;
            cmds.push((me, Command::Move { units: idle, target: igloo.door + toward_us, run: false, queue: false }));
        }
    }

    /// The Chieftain shouts when its army is in a fight, or starts to run.
    fn war_cry(&mut self, g: &Game, cmds: &mut Vec<(u8, Command)>) {
        let me = self.player;
        let Some(c) = g.penguins.iter().find(|p| p.owner == me && p.kind == Kind::Chieftain) else { return };
        if c.cry_cd > 0 || !c.is_out() || c.act == crate::game::Act::Frozen {
            return;
        }
        let team = g.team(me);
        let (mut friends, mut fleeing, mut foes) = (0, 0, 0);
        for p in g.penguins.iter().filter(|p| p.is_out() && p.owner != NEUTRAL && p.act != crate::game::Act::Frozen && p.pos.dist(c.pos) <= WAR_CRY_RADIUS) {
            if g.team(p.owner) == team {
                if p.kind != Kind::Worker && p.id != c.id {
                    friends += 1;
                    fleeing += p.routed as usize;
                }
            } else if p.kind.fights() && !p.cold && p.pos.dist(c.pos) <= WAR_CRY_RADIUS {
                foes += 1;
            }
        }
        let engaged = foes >= 2 && friends >= 3;
        let breaking = fleeing >= 2 || (c.routed && friends >= 1);
        if engaged || breaking {
            cmds.push((me, Command::WarCry { unit: c.id }));
        }
    }

    /// A stock line: computers don't chat their built-in lines any more
    /// (only the language model's own words), so it goes in the log only.
    fn say(&mut self, g: &Game, to: Option<u8>, text: String, _cmds: &mut Vec<(u8, Command)>) {
        let whom = to.and_then(|t| g.players.get(t as usize)).map_or(String::new(), |p| format!(" to {}", p.name));
        self.note(g.tick, format!("Thinks{whom}: \"{text}\""));
    }

    /// Chat words the language model gave it, and log them.
    fn say_aloud(&mut self, g: &Game, to: Option<u8>, text: String, cmds: &mut Vec<(u8, Command)>) {
        let whom = to.and_then(|t| g.players.get(t as usize)).map_or(String::new(), |p| format!(" to {}", p.name));
        self.note(g.tick, format!("Says{whom}: \"{text}\""));
        cmds.push((self.player, Command::Chat { to, text }));
    }

    fn taunt(&mut self, g: &Game, victim: u8, cmds: &mut Vec<(u8, Command)>) {
        if g.players[victim as usize].ai.is_some() || !self.rng.chance((0.6 * around(self.temper.chatter, 1.0, 1.6)).min(1.0)) {
            return;
        }
        let spite = self.spite.get(victim as usize).copied().unwrap_or(0);
        let text = match spite {
            s if s > 6 => return,
            s if s > 4 => "DIE DIE DIE!!!",
            s if s > 3 => "You are getting on my nerves... prepare to die!",
            s if s > 1 => "hmm... I think I'll attack... YOU! MUHAHAHA!!",
            _ => "I think I'll try picking on you... no hard feelings",
        };
        self.say(g, Some(victim), text.into(), cmds);
    }

    fn grumble(&mut self, g: &Game, attacker: u8, cmds: &mut Vec<(u8, Command)>) {
        if g.players[attacker as usize].ai.is_some()
            || g.tick < self.last_grudge_msg + crate::TICK_RATE * 30
        {
            return;
        }
        self.last_grudge_msg = g.tick;
        let spite = self.spite.get(attacker as usize).copied().unwrap_or(0);
        let name = &g.players[attacker as usize].name;
        let text = match spite {
            s if s > 12 => return,
            s if s > 8 => "Dirty swine, leave me alone!".to_string(),
            s if s > 5 => "*SIGH*, just leave me alone will you?!".to_string(),
            s if s > 2 => format!("I see you, {name}! Just what do you think you're doing in MY territory?"),
            _ => "*AHEM* I would recommend that you leave me alone!".to_string(),
        };
        self.say(g, Some(attacker), text, cmds);
    }

    /// Reply to chat, in the grand tradition of the 2007 AI.
    pub fn hear(&mut self, g: &Game, from: u8, msg: &str) -> Option<(Option<u8>, String)> {
        self.hear_full(g, from, msg, false).map(|r| (r.to, r.text))
    }

    /// Reply to chat. A reply that settles a deal or answers a question about
    /// the game is `binding`: its words matter. Any other is small talk, which
    /// a language model may answer in its own words. `addressed`: spoken to
    /// directly or by name, so it always answers.
    pub fn hear_full(&mut self, g: &Game, from: u8, msg: &str, addressed: bool) -> Option<Reply> {
        let m = msg.to_lowercase();
        let words: Vec<&str> = m.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
        let has = |w: &str| words.contains(&w);
        let from_name = g.players.get(from as usize).map(|p| p.name.clone()).unwrap_or_default();
        let from_name = from_name.as_str();
        self.note(g.tick, format!("Hears {from_name}: \"{}\"", msg.chars().take(60).collect::<String>()));
        // Deals and questions about the state of things always get an answer.
        if let Some(reply) = self.talk_terms(g, from, &words) {
            return Some(Reply { to: Some(from), text: reply, binding: true });
        }
        // Asked what it is, it says plainly: a computer player. (Left to a
        // small model, it claims to be a person.)
        let asks_what = ["bot", "bots", "robot", "computer", "ai", "human", "npc"].iter().any(|w| has(w)) || (has("real") && (has("person") || has("player")));
        if asks_what && (addressed || m.contains('?') || has("you") || has("u")) {
            return Some(Reply { to: Some(from), text: "Yes, I'm a computer player. A penguin at heart, though.".into(), binding: true });
        }
        // Don't answer every message in a public chat.
        if !addressed && !self.rng.chance((0.5 * around(self.temper.chatter, 1.0, 1.9)).min(1.0)) {
            return None;
        }
        let mut to = Some(from);
        let reply = if has("hi") || has("hello") || has("hey") {
            format!("Hi {from_name}! Good luck out there.")
        } else if m.contains("bye") {
            format!("Yes, {from_name}, adieu!")
        } else if has("how") && has("are") && has("you") {
            "Never better! Building snowballs like mad. You?".into()
        } else if (m.contains("freez") || m.contains("froz") || m.contains("cold")) && m.contains("you") {
            "Don't worry, I'll be sure to make it up to you!".into()
        } else if m.contains("die") {
            if let Some(s) = self.spite.get_mut(from as usize) {
                *s = 5;
            }
            "No no, YOU die! MUHAHAHAHA".into()
        } else if m.contains("dum") || m.contains("stupid") || m.contains("idiot") {
            "I'll take that as a compliment".into()
        } else if m.contains("fair") {
            "Life isn't fair!".into()
        } else if has("go") && has("away") {
            "I'll go away when I want to!".into()
        } else if has("no") {
            "YEEEEES!".into()
        } else if m.contains("attack") || m.contains("stop") {
            "Uuu... no!".into()
        } else if m.contains("thank") {
            "You're quite welcome!".into()
        } else if m.contains("loser") {
            to = None;
            format!("All hail {from_name}, king of the losers!")
        } else {
            const SHRUGS: [&str; 6] = [
                "Ha! Fair enough.",
                "Is that so? We'll see on the ice!",
                "Good one. My penguins are still laughing.",
                "Heh, you're fun to play against.",
                "Interesting... tell me more!",
                "Ha, I like you. I'll still snowball you though.",
            ];
            SHRUGS[self.rng.below(SHRUGS.len() as u32) as usize].into()
        };
        Some(Reply { to, text: reply, binding: false })
    }
}

/// Would a seal make this job deadly? A surfaced seal can lunge right now
/// and one that has caught someone keeps hunting there for a while, so
/// routes and work spots near either are avoided. Snowballs don't travel
/// as the crow flies over water either, but paths hug the straight line
/// between the two ends, so that is what is checked.
fn seal_risk(g: &Game, from: Vec2, to: Vec2) -> bool {
    g.seals.iter().any(|s| {
        (s.state == SealState::Surfaced || s.recently_struck(g.tick)) && s.route_dist(from, to) < SEAL_NOTICE
    })
}

impl Ai {
    /// A short report on the tribe for whoever sets its directive: only
    /// what the computer itself knows, so a model is no better informed
    /// than the rules are.
    pub fn briefing(&self, g: &Game) -> String {
        let me = self.player;
        let mine: Vec<&crate::game::Igloo> = g.igloos.iter().filter(|i| i.owner == me).collect();
        let count = |f: &dyn Fn(&crate::game::Penguin) -> bool| g.penguins.iter().filter(|p| p.owner == me && f(p)).count();
        let mut out = format!(
            "Minute {}, {:?}. You are player {} ({}), {} level.\n",
            g.tick / (crate::TICK_RATE * 60),
            g.weather.season,
            me,
            g.players[me as usize].name,
            self.level.name()
        );
        out += &format!("Igloos {}, fish {}, wood {}, crystal {}", mine.len(), g.fish_of(me), g.wood_of(me), g.crystal_of(me));
        // Tiers and research only exist with the tech rules on.
        if g.core.tech {
            out += &format!(", igloo tiers {:?}, research levels {:?}", mine.iter().map(|i| i.tier).collect::<Vec<_>>(), g.levels_of(me));
        }
        out += ".\n";
        out += &format!("Workers {}, fighters {}.\n", count(&|p| p.kind == Kind::Worker), count(&|p| p.kind.fights()));
        for p in g.players.iter().filter(|p| p.id != me && !p.out) {
            let igloos: Vec<usize> = g.igloos.iter().filter(|i| i.owner == p.id).map(|i| i.id as usize).collect();
            let found = igloos.iter().filter(|&&i| self.known.get(i).copied().unwrap_or(false)).count();
            let guards: u32 = igloos.iter().map(|&i| self.guards_seen.get(i).copied().unwrap_or(0)).sum();
            out += &format!(
                "Player {} ({}): {}, {} of its igloos found, {} guards last seen.\n",
                p.id,
                p.name,
                if g.hostile(me, p.id) { "enemy" } else { "ally" },
                found,
                guards
            );
        }
        let attacking = self.target.and_then(|ig| g.igloos.get(ig as usize)).and_then(|ig| g.players.get(ig.owner as usize));
        out += &match attacking {
            Some(p) => format!("Attacking player {} ({}) with {} fighters.\n", p.id, p.name, self.group.len()),
            None => "Not attacking anyone.\n".to_string(),
        };
        if let Some(k) = self.directive.favour {
            out += &format!("Raising more {} on your advice.\n", k.stats().plural);
        }
        if self.temper.rolled {
            out += &format!("Your character: {}: {}.\n", g.players[me as usize].name, self.temper.describe());
        }
        if let Some(plan) = self.plan_now {
            out += &format!("Current plan: {} (for {} s so far).\n", plan.describe(g), (g.tick - self.plan_since) / crate::TICK_RATE);
        }
        let truces: Vec<String> = g.players.iter().filter(|p| self.at_truce(g, p.id)).map(|p| format!("player {} ({} more min)", p.id, (self.truce_until[p.id as usize] - g.tick).div_ceil(60 * crate::TICK_RATE))).collect();
        if !truces.is_empty() {
            out += &format!("Truces: {}.\n", truces.join(", "));
        }
        if let Some(l) = self.pact_target(g) {
            out += &format!("You are in a pact with the other computers to bring down player {l}, who is running away with the game.\n");
        }
        let hated: Vec<String> = g.players.iter().filter(|p| self.rivalry(p.id) > 5.0).map(|p| format!("player {} ({:.0})", p.id, self.rivalry(p.id))).collect();
        if !hated.is_empty() {
            out += &format!("Grudges: {}.\n", hated.join(", "));
        }
        out
    }

    /// For tests and debugging: igloos found, tiles explored, the scout.
    pub fn fog_debug(&self) -> String {
        let found = self.known.iter().filter(|&&k| k).count();
        let seen = self.explored.iter().filter(|&&k| k).count();
        format!("p{} known {found} explored {seen} scout {:?} target {:?}", self.player, self.scout, self.target)
    }
}
