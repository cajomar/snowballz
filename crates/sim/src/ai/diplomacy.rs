//! Computers dealing with each other: a truce with one rival to deal with
//! another, and a pact to bring down whoever is running away with the game.
//! Offers go through `Game::run_ai`, which hands each one to the computer it
//! is for and says the offer and the answer in public chat, so people see the
//! deals being made (and, with a treacherous computer, broken). Only a
//! computer with a character makes offers; any computer answers a truce.

use super::*;

/// What one computer offers another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Offer {
    /// Leave each other alone for a few minutes.
    Truce,
    /// Join in against this player, and leave each other alone meanwhile.
    Pact(u8),
}

/// How long a pact against the leader holds.
const PACT_TICKS: u32 = 5 * 60 * crate::TICK_RATE;
/// How often a computer thinks about deals, at most.
const DEAL_EVERY: u32 = 60 * crate::TICK_RATE;

/// How far ahead a player is: igloos count for a lot, penguins for the rest.
pub(super) fn standing(g: &Game, player: u8) -> f32 {
    let igloos = g.igloos.iter().filter(|i| i.owner == player).count() as f32;
    igloos * 40.0 + g.population(player) as f32
}

/// The player running away with the game, if anyone is: half as strong
/// again as anyone else, after the first eight minutes.
pub(super) fn runaway(g: &Game) -> Option<u8> {
    if g.tick < 8 * 60 * crate::TICK_RATE {
        return None;
    }
    let mut live: Vec<(u8, f32)> = g.players.iter().filter(|p| !p.out).map(|p| (p.id, standing(g, p.id))).collect();
    if live.len() < 3 {
        return None;
    }
    live.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    (live[0].1 >= live[1].1 * 1.5).then_some(live[0].0)
}

impl Ai {
    /// Think about deals: a pact against a runaway leader, else a truce with
    /// a rival it isn't busy with. Offers wait in `outbox` for the game.
    pub(super) fn diplomacy(&mut self, g: &Game) {
        if !self.temper.rolled || g.tick < self.next_deal || g.tick < 3 * 60 * crate::TICK_RATE {
            return;
        }
        self.next_deal = g.tick + DEAL_EVERY + self.rng.below(DEAL_EVERY);
        let me = self.player;
        let t = self.temper;
        let name = |p: u8| g.players[p as usize].name.clone();
        let other_computers: Vec<u8> = g
            .players
            .iter()
            .filter(|p| p.id != me && !p.out && p.ai.is_some() && g.hostile(me, p.id) && g.igloos.iter().any(|i| i.owner == p.id))
            .map(|p| p.id)
            .collect();

        // Somebody is running away with it: rally the rest. A spiteful or an
        // honourable computer is the one to stand up and say so.
        if let Some(leader) = runaway(g).filter(|&l| l != me && g.hostile(me, l)) {
            let pact_on = self.pact.is_some_and(|(l, until)| l == leader && until > g.tick);
            let partners: Vec<u8> = other_computers.iter().copied().filter(|&p| p != leader).collect();
            if !pact_on && !partners.is_empty() && self.rng.chance(0.2 + 0.4 * t.spite + 0.3 * t.honour) {
                let text = format!("Everyone! {} is getting too big. Help me bring them down!", name(leader));
                self.note(g.tick, format!("Calls for a pact against {}, who is running away with the game", name(leader)));
                self.join_pact(g, leader);
                for p in partners {
                    self.outbox.push((p, Offer::Pact(leader)));
                }
                self.outbox_text = Some(text);
                return;
            }
        }

        // Busy with one enemy: make peace with another. A treacherous one
        // offers truces as readily as an honourable one; it has its reasons.
        let busy = self.plan_now.and_then(Plan::target).or(self.target.map(|ig| g.igloos[ig as usize].owner).filter(|&o| o != NEUTRAL));
        let Some(busy) = busy else { return };
        let candidates: Vec<u8> = other_computers
            .iter()
            .copied()
            .filter(|&p| p != busy && !self.at_truce(g, p) && self.rivalry(p) < 20.0 && self.pact.is_none_or(|(l, _)| l != p))
            .collect();
        if candidates.is_empty() || !self.rng.chance(0.3 + 0.6 * (t.honour - 0.5).abs()) {
            return;
        }
        let to = candidates[self.rng.below(candidates.len() as u32) as usize];
        self.note(g.tick, format!("Offers {} a truce, to deal with {}", name(to), name(busy)));
        self.outbox.push((to, Offer::Truce));
        self.outbox_text = Some(format!("{}, a truce? We both have bigger fish to fry.", name(to)));
    }

    /// Answer another computer's offer. On a yes, returns the minutes both
    /// leave each other alone, and what it says either way.
    pub(crate) fn consider(&mut self, g: &Game, from: u8, offer: Offer) -> (Option<u32>, String) {
        let i = from as usize;
        let name = g.players[i].name.clone();
        let (minutes, limit) = self.truce_terms();
        match offer {
            Offer::Truce => {
                if self.favoured_target() == Some(from) {
                    self.note(g.tick, format!("Turns down {name}'s truce: {name} is the one it is after"));
                    return (None, format!("No deal, {name}. You're the one I'm after."));
                }
                if self.rivalry(from) > limit {
                    self.note(g.tick, format!("Turns down {name}'s truce (grudge {:.0}, limit {limit:.0})", self.rivalry(from)));
                    return (None, format!("No deal, {name}. Not after what you did."));
                }
                self.note(g.tick, format!("Agrees a {minutes}-minute truce with {name}"));
                (Some(minutes), format!("Deal, {name}. {minutes} minutes of peace."))
            }
            Offer::Pact(leader) => {
                let lname = g.players[leader as usize].name.clone();
                let t = self.temper;
                // Not against a friend it's at peace with, unless it never
                // meant the peace anyway.
                let bound = self.at_truce(g, leader) && t.honour >= TREACHEROUS;
                let keen = self.rivalry(leader) > 20.0 || self.rng.chance(if t.rolled { 0.3 + 0.4 * t.bold + 0.2 * t.spite } else { 0.5 });
                if !t.rolled || bound || leader == self.player || !keen {
                    self.note(g.tick, format!("Stays out of {name}'s pact against {lname}"));
                    return (None, if bound { format!("I have a truce with {lname}. Count me out.") } else { "Not my fight.".to_string() });
                }
                self.note(g.tick, format!("Joins {name}'s pact against {lname}"));
                self.join_pact(g, leader);
                (Some(PACT_TICKS / (60 * crate::TICK_RATE)), format!("I'm in, {name}. {lname} goes down."))
            }
        }
    }

    /// Set out against the leader, without a word (the pact was the word).
    fn join_pact(&mut self, g: &Game, leader: u8) {
        self.pact = Some((leader, g.tick + PACT_TICKS));
        self.pledge(g, Plan::Siege(leader));
    }

    /// Leave a fellow computer alone for `minutes`.
    pub(crate) fn keep_peace(&mut self, g: &Game, with: u8, minutes: u32) {
        if let Some(t) = self.truce_until.get_mut(with as usize) {
            *t = g.tick + minutes * 60 * crate::TICK_RATE;
        }
    }

    /// The pact it is in, if it still holds: the leader it is against.
    pub(super) fn pact_target(&self, g: &Game) -> Option<u8> {
        self.pact.filter(|&(l, until)| until > g.tick && g.players.get(l as usize).is_some_and(|p| !p.out)).map(|(l, _)| l)
    }
}
