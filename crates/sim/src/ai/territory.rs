//! Whose ground it is. The computer notices who works and builds on what it
//! thinks of as its own (the land around its igloos and the groves it
//! chops), keeps a grudge against them, and lets that grudge, more than
//! how weak they are, decide whom it picks on.

use super::*;

/// How far from an igloo's door the computer calls the ground its own.
const CLAIM: f32 = 20.0;
/// ...and from a grove its workers chop.
const GROVE: f32 = 14.0;
/// A grove is forgotten when nobody has chopped there for this long.
const GROVE_KEEP: u32 = 180 * crate::TICK_RATE;
const MAX_GROVES: usize = 6;
/// A grudge never grows past this.
const MAX_GRIEVANCE: f32 = 40.0;
/// Just outside the ground it calls its own, it is already watching: this
/// much further from a door, and from a grove.
const NEAR_CLAIM: f32 = 34.0;
const NEAR_GROVE: f32 = 24.0;
/// The grievance at which the computer warns (once each) in chat: first a
/// grumble that someone is getting close, then it gets sharper.
const WARN_AT: [f32; 4] = [1.5, 4.0, 10.0, 20.0];

/// A research branch in words.
pub(crate) fn branch_name(branch: u8) -> &'static str {
    match branch {
        crate::game::SNOWCRAFT => "Snowcraft",
        crate::game::ICECRAFT => "Icecraft",
        crate::game::FISHERY => "Fishery",
        _ => "?",
    }
}

impl AiLevel {
    /// The share of a grudge forgotten each game minute: the easy computer
    /// lets go quickly, the hard one holds on.
    fn forgiveness(self) -> f32 {
        match self {
            AiLevel::Easy => 0.5,
            AiLevel::Normal => 0.25,
            AiLevel::Hard => 0.08,
        }
    }
    /// Minutes at the start of a game in which this level won't march on a
    /// person's igloo: time to settle in. Easy gives the longest.
    pub fn grace_minutes(self) -> u32 {
        match self {
            AiLevel::Easy => 8,
            AiLevel::Normal => 5,
            AiLevel::Hard => 3,
        }
    }
    /// How heavily a rivalry weighs against distance and weakness when
    /// picking whom to attack.
    pub(super) fn vindictiveness(self) -> f32 {
        match self {
            AiLevel::Easy => 0.5,
            AiLevel::Normal => 1.0,
            AiLevel::Hard => 2.0,
        }
    }
}

/// Where somebody stands against the computer's ground.
#[derive(Clone, Copy)]
struct Zone {
    inside: bool,
    /// Near a grove it chops, rather than near its igloo.
    forest: bool,
}

impl Ai {
    /// How much the computer holds against `player`: blows taken and
    /// trespass suffered. In the same units as the target score.
    pub fn rivalry(&self, player: u8) -> f32 {
        let i = player as usize;
        self.spite.get(i).copied().unwrap_or(0).min(8) as f32 * 6.0 + self.grievance.get(i).copied().unwrap_or(0.0) * 4.0
    }

    /// Has this player already earned the computer's anger (trespass, or
    /// breaking a promise)? Then the grace period no longer shields them.
    pub fn provoked(&self, player: u8) -> bool {
        self.grievance.get(player as usize).is_some_and(|&x| x >= WARN_AT[2])
    }

    /// Does the opening grace period shield this player: a person, in the
    /// first minutes of a game, who hasn't angered the computer?
    pub fn grace_holds(&self, g: &Game, owner: u8) -> bool {
        owner != NEUTRAL
            && g.players.get(owner as usize).is_some_and(|p| p.ai.is_none())
            && g.tick < self.level.grace_minutes() * 60 * crate::TICK_RATE
            && !self.provoked(owner)
    }

    /// Computers leave each other alone for the first few minutes too, so
    /// every tribe has something to defend itself with before the wars start.
    pub(super) fn computer_grace_holds(&self, g: &Game, owner: u8) -> bool {
        owner != NEUTRAL
            && owner != self.player
            && g.players.get(owner as usize).is_some_and(|p| p.ai.is_some())
            && g.tick < COMPUTER_GRACE_MINUTES * 60 * crate::TICK_RATE
            && !self.provoked(owner)
    }

    /// Is there a truce with this player?
    pub(super) fn at_truce(&self, g: &Game, player: u8) -> bool {
        self.truce_until.get(player as usize).is_some_and(|&t| t > g.tick)
    }

    /// Where `at` lies against the computer's ground: on it ("land" near a
    /// door, "forest" near a grove it chops) or just outside it, watched.
    fn zone(&self, g: &Game, mine: &[u8], at: Vec2) -> Option<Zone> {
        let doors = || mine.iter().map(|&ig| g.igloos[ig as usize].door.dist(at));
        let groves = || self.groves.iter().map(|&(x, y, _)| v2(x as f32 + 0.5, y as f32 + 0.5).dist(at));
        if doors().any(|d| d < CLAIM) {
            Some(Zone { inside: true, forest: false })
        } else if groves().any(|d| d < GROVE) {
            Some(Zone { inside: true, forest: true })
        } else if doors().any(|d| d < NEAR_CLAIM) {
            Some(Zone { inside: false, forest: false })
        } else if groves().any(|d| d < NEAR_GROVE) {
            Some(Zone { inside: false, forest: true })
        } else {
            None
        }
    }

    /// Note who is working or building on the computer's ground, and hold it
    /// against them. Only what it can see counts.
    pub(super) fn watch_territory(&mut self, g: &Game, mine: &[u8], cmds: &mut Vec<(u8, Command)>) {
        let me = self.player;
        // Groves: wherever our own workers chop.
        for p in g.penguins.iter().filter(|p| p.owner == me) {
            if let Order::Chop { at } = p.order {
                match self.groves.iter_mut().find(|(x, y, _)| (x - at.0).abs() + (y - at.1).abs() < 6) {
                    Some(grove) => *grove = (at.0, at.1, g.tick),
                    None => self.groves.push((at.0, at.1, g.tick)),
                }
            }
        }
        self.groves.retain(|&(_, _, t)| g.tick < t + GROVE_KEEP);
        if self.groves.len() > MAX_GROVES {
            self.groves.sort_by_key(|&(_, _, t)| std::cmp::Reverse(t));
            self.groves.truncate(MAX_GROVES);
        }

        let forgive = self.level.forgiveness() * around(1.0 - self.temper.spite, 1.0, 3.0) * self.level.think_ticks() as f32 / (60 * crate::TICK_RATE) as f32;
        for gr in &mut self.grievance {
            *gr -= *gr * forgive;
        }

        // What each rival is doing on our ground right now: the worst of it.
        let mut worst: Vec<(f32, &'static str)> = vec![(0.0, ""); 16];
        let mut total = vec![0.0f32; 16];
        let mut note = |owner: u8, weight: f32, what: &'static str| {
            let i = owner as usize;
            if i < 16 {
                total[i] += weight;
                if weight > worst[i].0 {
                    worst[i] = (weight, what);
                }
            }
        };
        self.intruders.clear();
        let rival = |owner: u8| owner != me && owner != NEUTRAL && g.hostile(me, owner) && !g.players[owner as usize].out;
        for p in g.penguins.iter().filter(|p| rival(p.owner) && p.is_out()) {
            let what = match p.order {
                Order::Chop { .. } => "forest",
                Order::Gather { resource } if g.resources.get(resource as usize).is_some_and(|r| r.kind == ResourceKind::Crystal) => "crystals",
                _ => continue,
            };
            if !self.sees(p.pos) {
                continue;
            }
            match self.zone(g, mine, p.pos) {
                Some(z) if z.inside => {
                    note(p.owner, 0.3, what);
                    self.intruders.push((p.owner, p.pos));
                }
                // Not yet on its ground, but close enough to be watched.
                Some(_) => note(p.owner, 0.06, what),
                None => {}
            }
        }
        for b in g.buildings.iter().filter(|b| b.alive && rival(b.owner)) {
            if !self.sees(b.center()) {
                continue;
            }
            let Some(z) = self.zone(g, mine, b.center()) else { continue };
            let what = if z.forest { "forest" } else { "buildings" };
            if z.inside {
                note(b.owner, if b.done() { 1.0 } else { 0.6 }, what);
                self.intruders.push((b.owner, b.center()));
            } else {
                note(b.owner, if b.done() { 0.25 } else { 0.15 }, what);
            }
        }

        for p in 0..16u8 {
            let i = p as usize;
            let seen = total[i] > 0.0;
            // A promise to leave buys a minute's grace, then it is tested.
            if self.promise_until[i] != 0 {
                if g.tick < self.promise_until[i] {
                    continue;
                }
                self.promise_until[i] = 0;
                if seen {
                    self.grievance[i] += 8.0;
                    self.spite[i] += 2;
                    self.truce_until[i] = 0;
                    self.note(g.tick, format!("{} broke a promise to leave: grudge up, truce off", g.players[i].name));
                    self.speak_to(g, p, "You promised to leave, and you lied to me. I won't forget it!".into(), cmds);
                }
            }
            if seen {
                self.grievance[i] = (self.grievance[i] + total[i]).min(MAX_GRIEVANCE);
                self.cause[i] = worst[i].1;
                self.warn(g, p, cmds);
            } else if self.grievance[i] < 2.0 {
                self.warned[i] = 0;
            }
            // Trespass makes a truce worthless.
            if self.grievance[i] >= WARN_AT[3] && self.truce_until[i] > g.tick {
                self.truce_until[i] = 0;
                self.note(g.tick, format!("Truce with {} withdrawn: too much trespass", g.players[i].name));
                self.speak_to(g, p, "So much for our truce. Get off my land!".into(), cmds);
            }
        }
    }

    /// Speak to a human who keeps trespassing, a little more sharply each time.
    fn warn(&mut self, g: &Game, who: u8, cmds: &mut Vec<(u8, Command)>) {
        let i = who as usize;
        let level = WARN_AT.iter().filter(|&&w| self.grievance[i] >= w).count() as u8;
        if level <= self.warned[i] || g.tick < self.last_grudge_msg + crate::TICK_RATE * 30 {
            return;
        }
        self.warned[i] = level;
        let name = g.players[i].name.clone();
        self.note(
            g.tick,
            format!(
                "{name} is {} ({}): grudge {:.0}, {}",
                if level == 1 { "getting close to my ground" } else { "on my ground" },
                self.cause[i],
                self.grievance[i],
                if level == 1 { "a friendly grumble first".to_string() } else { format!("warning {} of 3", level - 1) }
            ),
        );
        let what = match self.cause[i] {
            "crystals" => "crystal",
            "buildings" => "land",
            _ => "forest",
        };
        let text = match (level, what) {
            (1, "forest") => format!("Hmm, {name}. Those are my trees you're building near. I'm watching you."),
            (1, "crystal") => format!("{name}, those crystals are spoken for. Look elsewhere."),
            (1, _) => format!("{name}, that's rather close to my igloo, isn't it? I'm keeping an eye on you."),
            (2, _) => format!("{name}, that is MY {what}. Take your business elsewhere."),
            (3, _) => format!("Last warning, {name}: clear out of my {what} or I will clear you out."),
            _ => format!("That's it, {name}. You asked for this!"),
        };
        self.speak_to(g, who, text, cmds);
    }

    /// Say something to a player, if it is a person.
    pub(super) fn speak_to(&mut self, g: &Game, who: u8, text: String, cmds: &mut Vec<(u8, Command)>) {
        if g.players.get(who as usize).is_some_and(|p| p.ai.is_none()) {
            self.last_grudge_msg = g.tick;
            self.say(g, Some(who), text, cmds);
        }
    }

    /// Already given a job by the territory rules or the attack group?
    pub(super) fn committed(&self, id: u16) -> bool {
        self.group.contains(&id) || self.sentries.contains(&id) || self.raiders.contains(&id)
    }

    /// Fighters free for a job, nearest to `spot` first.
    fn spare_fighters(&self, g: &Game, spot: Vec2, within: f32) -> Vec<u16> {
        let mut free: Vec<&crate::game::Penguin> = g
            .penguins
            .iter()
            .filter(|p| {
                p.owner == self.player
                    && p.kind.fights()
                    && p.kind != Kind::Chieftain
                    && p.post.is_none()
                    && p.is_out()
                    && !p.cold
                    && !p.routed
                    && p.act != crate::game::Act::Frozen
                    && !matches!(p.order, Order::Garrison { .. } | Order::Enter { .. })
                    && !self.committed(p.id)
                    && p.pos.dist(spot) < within
            })
            .collect();
        free.sort_by(|a, b| a.pos.dist2(spot).partial_cmp(&b.pos.dist2(spot)).unwrap());
        free.iter().map(|p| p.id).collect()
    }

    /// Fight for the ground: run trespassers off, and keep a guard at the
    /// grove being contested. The easy computer only complains.
    pub(super) fn defend_ground(&mut self, g: &Game, mine: &[u8], cmds: &mut Vec<(u8, Command)>) {
        if self.level == AiLevel::Easy {
            return;
        }
        let me = self.player;
        let fit = |p: &crate::game::Penguin| p.owner == me && p.kind.fights() && !p.cold && !p.routed && p.is_out();
        self.raiders.retain(|&id| fit(&g.penguins[id as usize]));
        self.sentries.retain(|&id| fit(&g.penguins[id as usize]));
        let (raid_at, want_raiders, want_guards) = match self.level {
            AiLevel::Hard => (6.0, 5, 4),
            _ => (10.0, 3, 2),
        };

        // Run the trespassers off: the nearest ones of the most hated.
        let home = g.igloos[mine[0] as usize].door;
        if g.tick >= self.raid_until {
            self.raiders.clear();
        }
        let hated = self
            .intruders
            .iter()
            .filter(|(owner, _)| self.grievance[*owner as usize] >= raid_at && !self.at_truce(g, *owner))
            .min_by(|a, b| a.1.dist2(home).partial_cmp(&b.1.dist2(home)).unwrap());
        if let Some(&(owner, spot)) = hated {
            if self.raiders.is_empty() {
                let free = self.spare_fighters(g, spot, 50.0);
                // Send what can be spared: a small tribe's raid is small.
                if free.len() >= 2 {
                    self.raiders = free[..want_raiders.min(free.len())].to_vec();
                    self.raid_until = g.tick + 40 * crate::TICK_RATE;
                    self.note(g.tick, format!("Sends {} fighters to run {} off its ground", self.raiders.len(), g.players[owner as usize].name));
                }
            }
            if !self.raiders.is_empty() {
                cmds.push((me, Command::Move { units: self.raiders.clone(), target: spot, run: false, queue: false }));
            }
        }

        // A guard at the grove, while anyone is contesting the ground.
        let contested = self.grievance.iter().any(|&x| x >= 4.0);
        let grove = self.groves.iter().max_by_key(|&&(_, _, t)| t).copied();
        let Some((gx, gy, _)) = grove.filter(|_| contested) else {
            if !self.sentries.is_empty() {
                cmds.push((me, Command::Stop { units: std::mem::take(&mut self.sentries) }));
            }
            self.sentry_at = None;
            return;
        };
        let Some(tile) = g.grid.nearest_walkable((gx, gy), 4) else { return };
        let post = v2(tile.0 as f32 + 0.5, tile.1 as f32 + 0.5);
        if self.sentry_at.is_none_or(|at| at.dist(post) > 8.0) {
            // The busiest grove moved: the old guards are free again.
            if !self.sentries.is_empty() {
                cmds.push((me, Command::Stop { units: std::mem::take(&mut self.sentries) }));
            }
            self.sentry_at = Some(post);
            self.note(g.tick, format!("Guards the grove at ({}, {}) while the ground is contested", tile.0, tile.1));
        }
        if self.sentries.len() < want_guards {
            let free = self.spare_fighters(g, post, 50.0);
            let more: Vec<u16> = free.into_iter().take(want_guards - self.sentries.len()).collect();
            if !more.is_empty() {
                cmds.push((me, Command::Move { units: more.clone(), target: post, run: false, queue: false }));
                self.sentries.extend(more);
            }
        }
        // Once at the post, stand there and throw at whoever comes.
        let arrived: Vec<u16> = self
            .sentries
            .iter()
            .copied()
            .filter(|&id| {
                let p = &g.penguins[id as usize];
                p.order == Order::None && p.pos.dist(post) < 6.0
            })
            .collect();
        if !arrived.is_empty() {
            cmds.push((me, Command::Hold { units: arrived }));
        }
    }

    /// Minutes of truce the computer grants, and the most it holds against
    /// someone and still agrees: the easy one is the most forgiving.
    pub(super) fn truce_terms(&self) -> (u32, f32) {
        let (minutes, limit) = match self.level {
            AiLevel::Easy => (8, 100.0),
            AiLevel::Normal => (5, 70.0),
            AiLevel::Hard => (3, 40.0),
        };
        // An honourable computer grants longer truces, to people it is angrier with.
        let h = around(self.temper.honour, 1.0, 2.0);
        (((minutes as f32) * h).round().max(1.0) as u32, limit * h)
    }

    /// Answer what a player says about the state of things: asking for peace,
    /// promising to leave, asking why it is angry. `None` for small talk.
    pub(super) fn talk_terms(&mut self, g: &Game, from: u8, words: &[&str]) -> Option<String> {
        let has = |w: &str| words.contains(&w);
        let i = from as usize;
        if i >= 16 || from == self.player || !g.hostile(self.player, from) || g.players[i].out {
            return None;
        }
        let name = g.players[i].name.clone();
        let minutes_left = |t: u32| (t - g.tick).div_ceil(60 * crate::TICK_RATE);
        let rivalry = self.rivalry(from);
        let what = match self.cause[i] {
            "crystals" => "crystals",
            "buildings" => "land",
            _ => "forest",
        };

        if has("truce") || has("peace") || has("ceasefire") || has("sorry") || (has("be") && has("friends")) {
            if self.truce_until[i] > g.tick {
                return Some(format!("We already have a truce, {name}. {} more minutes.", minutes_left(self.truce_until[i])));
            }
            let (minutes, forgiving) = self.truce_terms();
            if rivalry > forgiving {
                self.note(g.tick, format!("Refuses {name} a truce: grudge {rivalry:.0} is over its limit {forgiving:.0}"));
                return Some(format!("A truce? After what you've done, {name}? No."));
            }
            self.truce_until[i] = g.tick + minutes * 60 * crate::TICK_RATE;
            self.grievance[i] *= 0.5;
            self.note(g.tick, format!("Grants {name} a {minutes}-minute truce (grudge {rivalry:.0} is under its limit {forgiving:.0})"));
            let mut text = format!("Very well, {name}. A truce for {minutes} minutes.");
            if self.grievance[i] > 1.0 {
                text += &format!(" Stay out of my {what}.");
            }
            return Some(text);
        }
        let promises = has("i") && (has("leave") || has("leaving") || has("move") || has("moving") || has("quit") || has("go"));
        if promises && !has("away") && self.grievance[i] > 1.0 {
            self.promise_until[i] = g.tick + 60 * crate::TICK_RATE;
            self.note(g.tick, format!("{name} promises to leave its {what}: checks again in a minute"));
            return Some(format!("See that you do, {name}. You have a minute to clear out of my {what}."));
        }
        if has("why") && (has("attack") || has("attacking") || has("angry") || has("mad") || has("hate") || has("war") || has("fight")) {
            let spite = self.spite[i];
            return Some(if rivalry < 1.0 {
                format!("I have nothing against you, {name}. Yet.")
            } else if self.grievance[i] * 4.0 > spite as f32 * 6.0 {
                format!("Your people are in my {what}, {name}. Get them out and we can talk.")
            } else {
                format!("You attacked my igloo, {name}. Did you think I would forget?")
            });
        }
        None
    }

    /// What the computer is up to and thinking, one short line each, for a
    /// person watching: where things stand, then what it did lately.
    pub fn notes(&self, g: &Game) -> Vec<String> {
        self.notes_last(g, LOG_KEPT)
    }

    /// Everything it logged, oldest first: (tick, text).
    pub fn log(&self) -> &[(u32, String)] {
        &self.log
    }

    /// The summary and the last `lines` of the log.
    pub fn notes_last(&self, g: &Game, lines: usize) -> Vec<String> {
        let me = self.player;
        let name = |p: u8| g.players.get(p as usize).map_or_else(|| format!("player {p}"), |pl| pl.name.clone());
        let clock = |tick: u32| format!("{}:{:02}", tick / (60 * crate::TICK_RATE), tick / crate::TICK_RATE % 60);
        let mut out = Vec::new();
        let d = &self.directive;
        let mut advice = format!("{:?}", d.posture);
        if let Some(t) = d.target {
            advice += &format!(", pick on {}", name(t));
        }
        if let Some(k) = d.favour {
            advice += &format!(", more {}", k.stats().plural);
        }
        if let Some(b) = d.research {
            advice += &format!(", learn {}", branch_name(b));
        }
        out.push(format!("{} - {} level. Advice: {advice}", name(me), self.level.name()));
        if self.temper.rolled {
            out.push(format!("Temperament: {}", self.temper.describe()));
        }
        if let Some(plan) = self.plan_now {
            out.push(format!("Plan: {} (reconsiders in {} s)", plan.describe(g), self.plan_until.saturating_sub(g.tick) / crate::TICK_RATE));
        }
        out.push(match self.target {
            Some(t) => format!(
                "Attacking {} with {} fighters{}",
                if g.igloos[t as usize].owner == crate::game::NEUTRAL { format!("neutral igloo {t}") } else { format!("{}'s igloo {t}", name(g.igloos[t as usize].owner)) },
                self.group.len(),
                // The march began against a neutral igloo that someone has taken since.
                if self.assault && g.igloos[t as usize].owner != crate::game::NEUTRAL { " (it was neutral when they set out)" } else { "" }
            ),
            None => "Not attacking anyone".to_string(),
        });
        let (fighters, workers) = (
            g.penguins.iter().filter(|p| p.owner == me && p.kind.fights()).count(),
            g.penguins.iter().filter(|p| p.owner == me && p.kind == Kind::Worker).count(),
        );
        out.push(format!(
            "{workers} workers, {fighters} fighters ({} raiding, {} guarding). Fish {}, wood {}, crystal {}",
            self.raiders.len(),
            self.sentries.len(),
            g.fish_of(me),
            g.wood_of(me),
            g.crystal_of(me)
        ));
        let mut held: Vec<String> = (0..16u8)
            .filter(|&p| p != me && (self.rivalry(p) > 1.0 || self.truce_until[p as usize] > g.tick))
            .map(|p| {
                let mut s = format!("{} {:.0}", name(p), self.rivalry(p));
                if !self.cause[p as usize].is_empty() && self.grievance[p as usize] > 1.0 {
                    s += &format!(" ({})", self.cause[p as usize]);
                }
                if self.truce_until[p as usize] > g.tick {
                    s += &format!(" truce {}m", (self.truce_until[p as usize] - g.tick).div_ceil(60 * crate::TICK_RATE));
                }
                s
            })
            .collect();
        held.sort();
        out.push(if held.is_empty() { "Holds no grudges".to_string() } else { format!("Grudges: {}", held.join(", ")) });
        out.push(format!("Groves it works: {}", self.groves.len()));
        for (tick, text) in &self.log[self.log.len().saturating_sub(lines)..] {
            out.push(format!("{} {text}", clock(*tick)));
        }
        out
    }
}
