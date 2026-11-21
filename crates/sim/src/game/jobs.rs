//! Crews for building sites: a site calls workers sized to the building,
//! a few each second, instead of everyone who was picked.

use super::*;

/// At most this many workers change jobs per tribe each second.
pub const JOB_MOVES_PER_SECOND: usize = 3;

impl BuildKind {
    /// How many workers a site of this kind calls.
    pub fn crew(self) -> usize {
        match (self.size(), self.snow()) {
            (1, s) if s <= 3 => 1,
            (1, _) => 2,
            (_, s) if s >= 8 => 4,
            _ => 3,
        }
    }
}

impl Game {
    /// Sites of the player's still going up that are short of crew, and by how many.
    fn short_sites(&self, player: u8) -> Vec<(u16, usize)> {
        let mut crews = vec![0usize; self.buildings.len()];
        for p in self.penguins.iter().filter(|p| p.owner == player && p.kind == Kind::Worker) {
            if let Order::Build { building } = p.order {
                crews[building as usize] += 1;
            }
        }
        self.buildings
            .iter()
            .filter(|b| b.alive && b.owner == player && b.is_site() && !b.starved)
            .filter_map(|b| b.kind.crew().checked_sub(crews[b.id as usize]).filter(|&n| n > 0).map(|n| (b.id, n)))
            .collect()
    }

    /// Once a second per tribe: call workers to sites short of crew.
    pub(super) fn update_jobs(&mut self) {
        for player in 0..self.players.len() as u8 {
            if (self.tick + player as u32 * 3).is_multiple_of(crate::TICK_RATE) {
                self.crew_sites(player);
            }
        }
    }

    /// Sites short of crew call in idle workers, a few each second. A site nobody is building takes one
    /// fisher if nobody is idle, so it gets going without the fishing
    /// stopping. Once the site is up they are idle again and the fishing
    /// slots call them back.
    fn crew_sites(&mut self, player: u8) {
        for _ in 0..JOB_MOVES_PER_SECOND {
            // The site shortest of crew first.
            let sites = self.short_sites(player).into_iter().map(|(b, n)| (Order::Build { building: b }, self.buildings[b as usize].center(), n));
            let Some((order, at, _)) = sites.max_by_key(|s| s.2) else { return };
            let unmanned = !self.penguins.iter().any(|p| p.order == order);
            let pick = self
                .penguins
                .iter()
                .filter(|p| p.owner == player && p.kind == Kind::Worker && p.is_out() && p.act != Act::Frozen && !p.routed && !p.cold)
                .filter(|p| p.order == Order::None && !p.shore_pin && p.carry == Carry::None && (p.fishery.is_none() || unmanned))
                // The idle first, then fishers, nearest the site.
                .min_by(|a, b| {
                    let key = |p: &Penguin| p.fishery.is_some() as u8 as f32 * 1000.0 + p.pos.dist(at);
                    key(a).partial_cmp(&key(b)).unwrap()
                })
                .map(|p| p.id);
            let Some(id) = pick else { return };
            self.give_order(id, order);
            let p = &mut self.penguins[id as usize];
            p.crew = true;
            p.back_to = None;
        }
    }

    /// A builder whose site is done: help at the nearest
    /// site still short of crew, or go back to the job it was pulled off.
    pub(super) fn crew_next(&mut self, i: usize) -> Order {
        let (owner, pos) = (self.penguins[i].owner, self.penguins[i].pos);
        let sites = self.short_sites(owner).into_iter().map(|(b, _)| (Order::Build { building: b }, self.buildings[b as usize].dist_to(pos)));
        match sites.min_by(|a, b| a.1.partial_cmp(&b.1).unwrap()) {
            Some((order, _)) => order,
            None => self.penguins[i].back_to.take().unwrap_or(Order::None),
        }
    }
}
