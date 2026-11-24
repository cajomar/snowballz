//! What a computer player is out to do for the next minute or two. A
//! language model picks the plan when there is one; otherwise the computer
//! picks for itself, by its temperament and how things stand. Only a
//! computer with a rolled temperament, or one given a plan, follows plans:
//! a plain one plays as it always has.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Fish, raise workers and build up; attack nobody.
    Grow,
    /// Take empty igloos.
    Expand,
    /// Small, quick attacks on one player.
    Raid(u8),
    /// A big army against one player's igloo.
    Siege(u8),
    /// Towers and defence; attack nobody.
    DigIn,
}

/// The shortest a plan is followed before the computer reconsiders it,
/// and the longest.
const PLAN_MIN: u32 = 60 * crate::TICK_RATE;
const PLAN_MAX: u32 = 150 * crate::TICK_RATE;

impl Plan {
    /// Read a plan from a model's words; raid and siege need a target.
    pub fn parse(word: &str, target: Option<u8>) -> Option<Plan> {
        match word.trim().to_ascii_lowercase().replace(['_', '-'], " ").as_str() {
            // Empty igloos can't be walked into any more, so a model that
            // still says expand means build up.
            "grow" | "expand" => Some(Plan::Grow),
            "raid" => target.map(Plan::Raid),
            "siege" => target.map(Plan::Siege),
            "dig in" | "digin" | "defend" => Some(Plan::DigIn),
            _ => None,
        }
    }

    pub fn target(self) -> Option<u8> {
        match self {
            Plan::Raid(t) | Plan::Siege(t) => Some(t),
            _ => None,
        }
    }

    /// How the plan sizes armies, sets towers and raises workers.
    pub(super) fn posture(self) -> Posture {
        match self {
            Plan::Grow => Posture::Boom,
            Plan::Expand => Posture::Balanced,
            Plan::Raid(_) => Posture::Rush,
            Plan::Siege(_) | Plan::DigIn => Posture::Turtle,
        }
    }

    pub fn describe(self, g: &Game) -> String {
        let name = |p: u8| g.players.get(p as usize).map_or_else(|| format!("player {p}"), |pl| pl.name.clone());
        match self {
            Plan::Grow => "grow: build up, attack nobody".to_string(),
            Plan::Expand => "expand: take empty igloos".to_string(),
            Plan::Raid(t) => format!("raid {}: small quick attacks", name(t)),
            Plan::Siege(t) => format!("siege {}: a big army for their igloo", name(t)),
            Plan::DigIn => "dig in: towers and defence".to_string(),
        }
    }

    /// What the computer says when it takes the plan up, in plain words and
    /// in the voice of its character (its title); `{t}` is the target.
    fn announce(self, g: &Game, title: &str) -> String {
        let name = |p: u8| g.players.get(p as usize).map_or_else(String::new, |pl| pl.name.clone());
        let i = match self {
            Plan::Grow => 0,
            Plan::Expand => 1,
            Plan::Raid(_) => 2,
            Plan::Siege(_) => 3,
            Plan::DigIn => 4,
        };
        let lines = VOICES.iter().find(|(t, _)| *t == title).map_or(PLAIN, |(_, l)| l);
        lines[i].replace("{t}", &self.target().map_or_else(String::new, name))
    }
}

/// Grow, expand, raid, siege, dig in.
type Lines = [&'static str; 5];

const PLAIN: &Lines = &[
    "Busy fishing over here. Leave me be and nobody gets hurt.",
    "Those empty igloos look lonely. I'll take them.",
    "{t}, mind your fishers. I'm coming to say hello.",
    "{t}, your igloo will be mine. I'm bringing everyone.",
    "Walls up, towers manned. Come and try it.",
];

const VOICES: &[(&str, Lines)] = &[
    ("the Reckless", [
        "Fishing. Boring. It won't last.",
        "Empty igloos! Last one there is a frozen fish!",
        "{t}! Ready or not, here I come!",
        "Everybody grab a snowball, we're flattening {t}!",
        "Fine, I'll sit still. For a bit.",
    ]),
    ("the Patient", [
        "No hurry. The fish aren't going anywhere, and neither am I.",
        "One igloo at a time.",
        "{t}, a little visit. Just to see how you are.",
        "I have waited long enough, {t}. Now it's your turn to wait for me.",
        "I can wait all winter.",
    ]),
    ("the Grudge-Holder", [
        "Building up. I haven't forgotten anything.",
        "Taking what's empty. And keeping a list.",
        "{t}, I remember what you did.",
        "{t}, I said I'd be back. I keep my grudges warm.",
        "Dig in, sharpen snowballs, remember names.",
    ]),
    ("the Forgiving", [
        "Plenty of fish for everyone. Let's all just get along.",
        "Nobody's using those igloos, are they? I'll look after them.",
        "Sorry, {t}, nothing personal. Just a few snowballs.",
        "{t}, I don't hold it against you. I'm still taking your igloo.",
        "I'll be here when everyone's calmed down.",
    ]),
    ("the Honourable", [
        "I'll be fishing. Anyone who leaves me be, I'll leave be.",
        "I claim the empty igloos, fair and square.",
        "{t}, fair warning: I'm sending a raiding party.",
        "{t}, I declare a siege on your igloo. Defend it well.",
        "I'll hold my ground. Nobody can say I started it.",
    ]),
    ("the Treacherous", [
        "Just fishing. Totally harmless. You can trust me.",
        "Those igloos? Oh, I'm only borrowing them.",
        "{t}, friend! I'm sending you a present.",
        "{t}, you'll want to see what I'm bringing. Stay right there.",
        "Building towers. For defence. Honest.",
    ]),
    ("the Builder", [
        "More fishers, more fish, more penguins. Simple maths.",
        "Room to grow! Those empty igloos are mine.",
        "{t}, you're sitting on good fishing. I'll be needing that.",
        "{t}, your igloo would make a nice warehouse.",
        "Protecting the investment.",
    ]),
    ("the Spartan", [
        "A few fish. Enough.",
        "More igloos, more soldiers.",
        "{t}. Soldiers, march.",
        "{t}, every penguin I have is coming for you.",
        "Soldiers on the walls. Nobody gets in.",
    ]),
    ("the Cautious", [
        "Keeping my head down and my towers up.",
        "Carefully now... those igloos look safe enough.",
        "{t}, just a small raid. Nothing risky.",
        "{t}, I've checked twice. Now I'm coming.",
        "More towers. You can never have too many towers.",
    ]),
    ("the Careless", [
        "Fishing! Towers? What towers?",
        "Ooh, empty igloos. Mine, mine, mine.",
        "{t}! Let's go, nobody needs to stay home.",
        "Everyone at {t}'s igloo! Who's watching mine? Nobody!",
        "Fine, I'll defend. Ugh.",
    ]),
    ("the Loudmouth", [
        "Watch me build the GREATEST tribe this glacier has ever seen!",
        "Every empty igloo on this map is MINE. You heard it here first!",
        "Hey {t}! Yeah, YOU! My snowballs are coming for your fishers!",
        "{t}, start packing! I'm bringing the WHOLE tribe and they're ANGRY!",
        "Go on, attack me. I DARE you. My towers are bigger than yours!",
    ]),
    ("the Silent", [
        "...",
        "Mine.",
        "{t}.",
        "{t}. Soon.",
        "Ready.",
    ]),
];

impl Ai {
    /// The posture in force: the plan's, else the advisor's.
    pub(super) fn posture(&self) -> Posture {
        self.plan_now.map_or(self.directive.posture, Plan::posture)
    }

    /// The player to pick on first: the plan's target, else the advisor's.
    pub(super) fn favoured_target(&self) -> Option<u8> {
        self.plan_now.and_then(Plan::target).or(self.directive.target)
    }

    /// Whether the plan lets the computer march on this igloo now.
    pub(super) fn plan_allows(&self, g: &Game, ig: &crate::game::Igloo) -> bool {
        let neutral = ig.owner == NEUTRAL;
        // At the bitter end the attack has to happen, whatever the plan.
        if self.all_in(g) {
            return true;
        }
        match self.plan_now {
            None => true,
            Some(Plan::DigIn) => false,
            Some(Plan::Grow) => neutral,
            // An empty igloo, or failing any, whoever has wronged it.
            Some(Plan::Expand) => neutral || self.rivalry(ig.owner) > 30.0,
            // Its target, or an empty igloo on the way.
            Some(Plan::Raid(t)) | Some(Plan::Siege(t)) => ig.owner == t || neutral || g.players.get(t as usize).is_none_or(|p| p.out),
        }
    }

    /// Take up a plan: from the advisor (`by_advisor`), with its own words to
    /// say if it gave any, or of the computer's own choosing.
    pub(super) fn take_plan(&mut self, g: &Game, plan: Plan, by_advisor: bool, said: Option<String>, cmds: &mut Vec<(u8, Command)>) {
        let old = self.plan_now;
        // The advisor may not change its mind more than once a minute.
        if by_advisor && self.plan_advised && old.is_some_and(|o| o != plan) && g.tick < self.plan_since + PLAN_MIN {
            return;
        }
        self.plan_now = Some(plan);
        self.plan_advised = by_advisor;
        self.plan_since = g.tick;
        self.plan_until = g.tick + if by_advisor { PLAN_MAX } else { self.rng.below(PLAN_MAX - PLAN_MIN) + PLAN_MIN };
        if old == Some(plan) {
            return;
        }
        self.note(g.tick, format!("New plan, {}: {}", if by_advisor { "the advisor's" } else { "its own" }, plan.describe(g)));
        // Grown-up plans are worth announcing; how often depends on how chatty it is.
        // Only the model's own words are said aloud.
        if g.tick > 0 {
            let to = plan.target().filter(|&t| g.players.get(t as usize).is_some_and(|p| p.ai.is_none()));
            match said {
                Some(text) => self.say_aloud(g, to, text, cmds),
                None => {
                    let text = plan.announce(g, self.temper.title());
                    self.say(g, to, text, cmds);
                }
            }
        }
    }

    /// Take up a plan agreed with other computers, quietly.
    pub(super) fn pledge(&mut self, g: &Game, plan: Plan) {
        self.plan_until = g.tick + PLAN_MAX;
        self.plan_advised = false;
        if self.plan_now == Some(plan) {
            return;
        }
        self.plan_now = Some(plan);
        self.plan_since = g.tick;
        self.note(g.tick, format!("New plan, from a pact: {}", plan.describe(g)));
    }

    /// Pick a plan when the old one has run its course: by temperament,
    /// nudged by how things stand. Only a computer with a character does.
    pub(super) fn choose_plan(&mut self, g: &Game, mine: &[u8], cmds: &mut Vec<(u8, Command)>) {
        if !self.temper.rolled || g.tick < self.plan_until {
            return;
        }
        // A pact holds it to the fight against the leader.
        if let Some(leader) = self.pact_target(g) {
            let plan = if self.temper.bold > 0.6 { Plan::Raid(leader) } else { Plan::Siege(leader) };
            self.take_plan(g, plan, false, None, cmds);
            return;
        }
        let me = self.player;
        let t = self.temper;
        let minutes = g.tick / (60 * crate::TICK_RATE);
        // Who to go after: the most hated, else the weakest-looking enemy it knows.
        let enemies: Vec<u8> = g.players.iter().filter(|p| p.id != me && !p.out && g.hostile(me, p.id) && !self.at_truce(g, p.id)).map(|p| p.id).collect();
        let known_igloos = |p: u8| g.igloos.iter().filter(|i| i.owner == p && self.knows(i.id)).count();
        let guards = |p: u8| g.igloos.iter().filter(|i| i.owner == p).map(|i| self.guards_seen.get(i.id as usize).copied().unwrap_or(0)).sum::<u32>();
        let victim = enemies
            .iter()
            .copied()
            .filter(|&p| known_igloos(p) > 0 && !self.grace_holds(g, p) && !self.computer_grace_holds(g, p))
            .min_by(|&a, &b| {
                let score = |p: u8| guards(p) as f32 * 2.0 - self.rivalry(p) * around(t.spite, 1.0, 3.0);
                score(a).partial_cmp(&score(b)).unwrap()
            });
        // An empty igloo it could take by marching in.
        let neutral_known = false && g.igloos.iter().any(|i| i.owner == NEUTRAL && self.knows(i.id));
        let under_attack = mine.iter().any(|&ig| g.igloos[ig as usize].takeover.is_some())
            || g.penguins.iter().any(|p| p.owner != me && p.kind.fights() && g.hostile(me, p.owner) && mine.iter().any(|&ig| g.igloos[ig as usize].door.dist(p.pos) < 14.0));
        let fighters = g.penguins.iter().filter(|p| p.owner == me && p.kind.fights()).count() as f32;

        // Weights, one per plan, from the character and the moment.
        let early = minutes < 4;
        // A big army sitting at home wants using, and a long game wants
        // finishing: peaceful plans fade as either grows.
        let idle = if fighters > 30.0 { 30.0 / fighters } else { 1.0 } * if minutes >= PRESS_ATTACK_MINUTES { 0.3 } else { 1.0 };
        let mut w = [
            (Plan::Grow, (1.0 + 3.0 * t.greed + if early { 2.0 } else { 0.0 }) * idle),
            (Plan::Expand, if neutral_known { 1.0 + 2.0 * t.greed } else { 0.0 }),
            (Plan::DigIn, if under_attack { 0.5 + 3.0 * t.caution + 4.0 } else { (0.5 + 3.0 * t.caution) * idle }),
            (Plan::Grow, 0.0),
            (Plan::Grow, 0.0),
        ];
        if let Some(v) = victim {
            let ready = (fighters / 12.0).min(3.0);
            w[3] = (Plan::Raid(v), (0.5 + 3.0 * t.bold) * ready + if self.rivalry(v) > 20.0 { 1.0 + 2.0 * t.spite } else { 0.0 });
            w[4] = (Plan::Siege(v), (0.3 + 2.0 * (1.0 - t.bold) + 1.0 * t.spite) * ready * if minutes >= 6 { 1.0 } else { 0.3 });
        }
        let total: f32 = w.iter().map(|x| x.1.max(0.0)).sum();
        let mut roll = self.rng.range(0.0, total);
        let mut pick = Plan::Grow;
        for (plan, weight) in w {
            if roll < weight.max(0.0) {
                pick = plan;
                break;
            }
            roll -= weight.max(0.0);
        }
        self.take_plan(g, pick, false, None, cmds);
    }
}
