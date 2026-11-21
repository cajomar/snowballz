//! Saunas: the way penguins warm up. A worker has to stay beside a sauna
//! and tend its fire, which burns the tribe's wood; while it is lit, up to
//! six penguins at a time bathe for a few seconds and come out fully warm.
//! Unattended, the fire goes out.
//!
//! With armies (DESIGN.md 0.2) no wood is stored: every finished sauna and
//! campfire has a stoker's slot, which calls in an idle worker. The stoker
//! fetches logs from the trees nearby and keeps the fire fed, so a campfire
//! burns for as long as it has one.

use super::*;

/// Penguins bathing in one sauna at a time.
pub const SAUNA_CAPACITY: usize = 6;
/// A bath takes this long and warms the bather all the way.
pub const BATH_TICKS: u16 = secs(8.0);
/// One piece of wood keeps the fire going this long.
pub const FUEL_PER_WOOD: u16 = secs(20.0);
/// The tender tops the fire up when it is down to this much.
const REFUEL_BELOW: u16 = secs(8.0);
/// With armies a stoker keeps this much fuel in the fire...
pub const STOKE_FULL: u16 = secs(60.0);
/// ...and a sauna stays lit while its stoker is off fetching a log, for this long.
const STOKER_AWAY: u32 = secs(40.0) as u32;
/// A tender stands this close to the sauna.
pub const TEND_REACH: f32 = 1.4;
/// A bather steps up to this close before going in.
pub const BATH_REACH: f32 = 1.5;
/// A campfire burns this long.
pub const CAMPFIRE_TICKS: u16 = secs(60.0);
/// A penguin warms by a campfire only this close to its edge...
pub const CAMPFIRE_REACH: f32 = 1.0;
/// ...one warmth at a time, this often.
pub const CAMPFIRE_WARM_TICKS: u32 = secs(3.0) as u32;
/// Cold penguins look this far for a campfire when there is no sauna.
pub const SEEK_FIRE: f32 = 25.0;
/// Frozen-through penguins look this far for a lit sauna...
pub const SEEK_COLD: f32 = 40.0;
/// ...and merely hurt ones, idle, this far.
pub const SEEK_HURT: f32 = 14.0;

impl Game {
    /// Penguins bathing in building `b` right now.
    pub fn bathers(&self, b: usize) -> usize {
        self.penguins.iter().filter(|p| p.act == Act::Bathe && p.bath_at == Some(b as u16)).count()
    }

    /// The nearest friendly lit sauna with room, within `range` of penguin `i`.
    fn sauna_for(&self, i: usize, range: f32) -> Option<usize> {
        let p = &self.penguins[i];
        let team = self.team(p.owner);
        self.buildings
            .iter()
            .filter(|b| b.alive && b.done() && b.kind == BuildKind::Sauna && b.lit && self.team(b.owner) == team)
            .filter(|b| b.dist_to(p.pos) <= range && self.bathers(b.id as usize) < SAUNA_CAPACITY)
            .min_by(|a, b| a.dist_to(p.pos).partial_cmp(&b.dist_to(p.pos)).unwrap())
            .map(|b| b.id as usize)
    }

    /// Head for a lit sauna and bathe there. False if there is none to use.
    pub(super) fn seek_bath(&mut self, i: usize, range: f32) -> bool {
        let Some(b) = self.sauna_for(i, range) else { return false };
        let pos = self.penguins[i].pos;
        if self.buildings[b].dist_to(pos) <= BATH_REACH {
            let c = self.buildings[b].center();
            let p = &mut self.penguins[i];
            p.act = Act::Bathe;
            p.bath_at = Some(b as u16);
            p.bath_t = 0;
            p.goal = None;
            p.path.clear();
            p.facing = (c - p.pos).angle();
            return true;
        }
        match self.build_spot(b as u16, pos) {
            Some(spot) => {
                self.travel(i, spot, 0.3);
                true
            }
            None => false,
        }
    }

    /// One tick of a bath: when it is done, the bather is warm again.
    pub(super) fn bathe(&mut self, i: usize) {
        let ok = self.penguins[i].bath_at.and_then(|b| self.buildings.get(b as usize)).is_some_and(|b| b.alive && b.lit);
        let p = &mut self.penguins[i];
        if !ok {
            p.act = Act::Idle;
            p.bath_at = None;
            return;
        }
        p.bath_t += 1;
        if p.bath_t >= BATH_TICKS {
            p.warmth = p.max_warmth;
            p.chill = 0;
            p.frost = 0;
            p.cold = false;
            p.act = Act::Idle;
            p.bath_at = None;
            p.goal = None;
            p.path.clear();
        }
    }

    /// Send workers to tend a finished sauna.
    pub(super) fn order_tend(&mut self, player: u8, units: Vec<u16>, building: u16) {
        let Some(b) = self.buildings.get(building as usize) else { return };
        let fire = b.kind == BuildKind::Sauna || (b.kind == BuildKind::Campfire);
        // A catapult: one's own, or an enemy's nobody crews, to take it over.
        let catapult = b.kind == BuildKind::Catapult && (b.owner == player || (self.hostile(player, b.owner) && !self.manned(building)));
        if !(b.alive && b.done() && (b.owner == player && fire || catapult)) {
            return;
        }
        for u in units {
            if self.controllable(player, u) && self.penguins[u as usize].kind == Kind::Worker {
                self.give_order(u, Order::Tend { building });
            }
        }
    }

    /// Worker job: stand by the sauna and keep its fire fed.
    pub(super) fn tend_job(&mut self, i: usize, building: u16) {
        let (owner, pos) = (self.penguins[i].owner, self.penguins[i].pos);
        let b = &self.buildings[building as usize];
        if !(b.alive && b.done() && b.owner == owner) {
            self.penguins[i].order = Order::None;
            return;
        }
        if self.stoke(i, building) {
            return;
        }
        let b = &self.buildings[building as usize];
        if b.dist_to(pos) > TEND_REACH {
            match self.build_spot(building, pos) {
                Some(spot) => {
                    self.travel(i, spot, 0.3);
                }
                None => self.penguins[i].order = Order::None,
            }
            return;
        }
        let c = b.center();
        let p = &mut self.penguins[i];
        p.goal = None;
        p.path.clear();
        p.facing = (c - p.pos).angle();
        if p.act == Act::Walk {
            p.act = Act::Idle;
        }
        let tick = self.tick;
        self.buildings[building as usize].tended = tick;
        if self.buildings[building as usize].fuel <= REFUEL_BELOW && self.spend_wood(owner, 1) {
            self.buildings[building as usize].fuel += FUEL_PER_WOOD;
        }
    }

    /// With armies, a stoker's errands: carry a log in, or go and fetch one
    /// while the fire is short. True while it is at that.
    fn stoke(&mut self, i: usize, building: u16) -> bool {
        let tick = self.tick;
        let b = &mut self.buildings[building as usize];
        // On the job, near or not: the fire counts as tended.
        b.tended = tick;
        let (center, fuel) = (b.center(), b.fuel);
        let pos = self.penguins[i].pos;
        if self.penguins[i].carry == Carry::Wood {
            let Some(spot) = self.build_spot(building, pos) else { return false };
            if self.travel(i, spot, 0.3) {
                let b = &mut self.buildings[building as usize];
                b.fuel = (b.fuel + FUEL_PER_WOOD).min(STOKE_FULL);
                let p = &mut self.penguins[i];
                p.carry = Carry::None;
                p.facing = (center - p.pos).angle();
            }
            return true;
        }
        if self.penguins[i].carry != Carry::None {
            self.penguins[i].carry = Carry::None;
        }
        if fuel + FUEL_PER_WOOD > STOKE_FULL {
            return false;
        }
        let tree = self.penguins[i].chop_tree.filter(|&t| self.terrain.wood[t as usize] > 0).or_else(|| self.tree_walkable_from(center, WOOD_FETCH_RADIUS).map(|(x, y)| self.map.idx(x, y) as u32));
        match tree {
            Some(t) => {
                self.chop_tree_job(i, t);
                true
            }
            None => false,
        }
    }

    /// Penguins stoking fire `b`.
    pub fn stokers(&self, b: u16) -> usize {
        self.penguins.iter().filter(|p| p.order == Order::Tend { building: b } && p.act != Act::Frozen).count()
    }

    /// Once a second, with armies: every fire without a stoker calls in the
    /// nearest idle worker of its tribe, or a fisher if nobody is idle.
    pub(super) fn fill_stokers(&mut self) {
        for b in 0..self.buildings.len() {
            let bd = &self.buildings[b];
            if !bd.alive || !bd.done() || bd.owner == NEUTRAL || !matches!(bd.kind, BuildKind::Sauna | BuildKind::Campfire) || self.stokers(b as u16) > 0 {
                continue;
            }
            let (owner, at) = (bd.owner, bd.center());
            let pick = self
                .penguins
                .iter()
                .filter(|p| p.owner == owner && p.kind == Kind::Worker && p.order == Order::None && !p.shore_pin && p.is_out() && p.act != Act::Frozen && !p.cold && !p.routed)
                .min_by(|a, c| {
                    let key = |p: &Penguin| p.fishery.is_some() as u8 as f32 * 1000.0 + p.pos.dist(at);
                    key(a).partial_cmp(&key(c)).unwrap()
                })
                .map(|p| p.id);
            if let Some(id) = pick {
                self.give_order(id, Order::Tend { building: b as u16 });
            }
        }
    }

    /// Head for a lit campfire and stand beside it. False if there is none.
    pub(super) fn seek_fire(&mut self, i: usize) -> bool {
        let p = &self.penguins[i];
        let team = self.team(p.owner);
        let Some(b) = self
            .buildings
            .iter()
            .filter(|b| b.alive && b.done() && b.kind == BuildKind::Campfire && b.lit && self.team(b.owner) == team)
            .filter(|b| b.dist_to(p.pos) <= SEEK_FIRE)
            .min_by(|a, b| a.dist_to(p.pos).partial_cmp(&b.dist_to(p.pos)).unwrap())
            .map(|b| b.id)
        else {
            return false;
        };
        if self.buildings[b as usize].dist_to(p.pos) > CAMPFIRE_REACH {
            if let Some(spot) = self.build_spot(b, p.pos) {
                self.travel(i, spot, 0.3);
            }
        } else {
            let p = &mut self.penguins[i];
            p.goal = None;
            p.path.clear();
            if p.act == Act::Walk {
                p.act = Act::Idle;
            }
        }
        true
    }

    /// Sauna fires burn down while someone tends them, and go out when nobody
    /// does. Campfires burn down by themselves, warming those right beside them.
    pub(super) fn update_saunas(&mut self) {
        let tick = self.tick;
        let warm = tick.is_multiple_of(CAMPFIRE_WARM_TICKS);
        let away = STOKER_AWAY;
        let mut out = Vec::new();
        for b in 0..self.buildings.len() {
            let bd = &mut self.buildings[b];
            if !bd.alive || !bd.done() {
                continue;
            }
            match bd.kind {
                BuildKind::Sauna => {
                    let tended = tick.saturating_sub(bd.tended) <= away && bd.tended != 0;
                    bd.lit = tended && bd.fuel > 0;
                    if bd.lit {
                        bd.fuel -= 1;
                    }
                }
                BuildKind::Campfire => {
                    bd.fuel = bd.fuel.saturating_sub(1);
                    bd.lit = bd.fuel > 0;
                    if !bd.lit {
                        out.push(b);
                    } else if warm {
                        let (edge, owner) = (bd.clone(), bd.owner);
                        let team = self.team(owner);
                        for p in &mut self.penguins {
                            if p.inside.is_some() || p.act == Act::Frozen || p.warmth >= p.max_warmth || p.owner == NEUTRAL {
                                continue;
                            }
                            if self.players[p.owner as usize].team == team && edge.dist_to(p.pos) <= CAMPFIRE_REACH {
                                p.warmth += 1;
                                if p.warmth >= p.max_warmth {
                                    p.cold = false;
                                    p.chill = 0;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        for b in out {
            self.destroy(b);
        }
    }
}
