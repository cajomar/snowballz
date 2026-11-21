//! Catapult Towers (the `armies` rule): a Watchtower built up. Three
//! workers crew the catapult on its roof from the foot of the tower, out in
//! the open, and it lobs a big snowball at an enemy building or igloo far
//! off every few seconds: the nearest, or the one its owner aims it at.
//! Freeze its whole crew and anyone's workers can walk up and take it over.

use super::*;

/// How far a catapult reaches, from the middle of its tower.
pub const CATAPULT_RANGE: f32 = 14.0;
/// Workers it takes to crew one.
pub const CATAPULT_CREW: u8 = 3;
/// Snow steps and logs to build a Watchtower up into one.
pub const CATAPULT_UPGRADE: (u8, u8) = (4, 6);
/// Seconds between shots with a full crew (slower with fewer).
pub const CATAPULT_RELOAD: u16 = secs(6.0);
/// What one big snowball does to a building, and to an igloo.
pub const CATAPULT_DAMAGE: u8 = 6;
pub const CATAPULT_IGLOO_DAMAGE: u16 = 10;
/// Crew stand this close to the tower's foot to work it.
const CREW_REACH: f32 = 1.0;
/// How far a big snowball lands from where it was aimed, roughly.
const CATAPULT_SPREAD: f32 = 0.45;

impl Game {
    /// Penguins of the catapult's own tribe crewing it (on the way or there).
    pub fn crew(&self, building: u16) -> impl Iterator<Item = &Penguin> {
        let owner = self.buildings[building as usize].owner;
        self.penguins.iter().filter(move |p| p.owner == owner && p.order == Order::Tend { building } && p.act != Act::Frozen)
    }

    /// Some of its own tribe at the foot of the tower: until they are all
    /// frozen (or gone), nobody else can take it over.
    pub fn manned(&self, building: u16) -> bool {
        let b = &self.buildings[building as usize];
        self.crew(building).any(|p| p.is_out() && b.dist_to(p.pos) <= CREW_REACH + 0.3)
    }

    /// Crew at the foot of the tower, warm enough to work it.
    fn crew_at_post(&self, building: u16) -> Vec<u16> {
        let b = &self.buildings[building as usize];
        self.crew(building).filter(|p| !p.cold && p.is_out() && b.dist_to(p.pos) <= CREW_REACH + 0.3).map(|p| p.id).collect()
    }

    /// A crewman's job: walk to the foot of the tower and work the catapult.
    /// At an enemy's catapult nobody crews, it takes the tower over.
    pub(super) fn crew_job(&mut self, i: usize, building: u16) {
        let (owner, pos) = (self.penguins[i].owner, self.penguins[i].pos);
        let b = &self.buildings[building as usize];
        if !(b.alive && b.done() && b.kind == BuildKind::Catapult) {
            self.penguins[i].order = Order::None;
            return;
        }
        let theirs = b.owner != owner;
        if theirs && self.manned(building) {
            // Still crewed: go home.
            self.penguins[i].order = Order::None;
            return;
        }
        if b.dist_to(pos) > CREW_REACH {
            match self.build_spot(building, pos) {
                Some(spot) => {
                    self.travel(i, spot, 0.3);
                }
                None => self.penguins[i].order = Order::None,
            }
            return;
        }
        if theirs {
            let from = b.owner;
            let bd = &mut self.buildings[building as usize];
            bd.owner = owner;
            bd.rally = None;
            bd.reload = CATAPULT_RELOAD;
            self.events.push(Event::TakenOver { building, from, by: owner });
        }
        let c = self.buildings[building as usize].center();
        let p = &mut self.penguins[i];
        p.goal = None;
        p.path.clear();
        p.facing = (c - p.pos).angle();
        if p.act == Act::Walk {
            p.act = Act::Idle;
        }
    }

    /// Once a second: every catapult short of crew calls in the nearest idle
    /// workers of its tribe, or newcomers from the sea.
    pub(super) fn fill_crews(&mut self) {
        for b in 0..self.buildings.len() {
            let bd = &self.buildings[b];
            if !bd.alive || !bd.done() || bd.kind != BuildKind::Catapult || bd.owner == NEUTRAL {
                continue;
            }
            let (owner, at, want) = (bd.owner, bd.center(), bd.want.min(bd.slots) as usize);
            for _ in self.crew(b as u16).count()..want {
                let pick = self
                    .penguins
                    .iter()
                    .filter(|p| p.owner == owner && p.kind == Kind::Worker && p.order == Order::None && p.fishery.is_none() && !p.shore_pin && p.is_out() && p.act != Act::Frozen && !p.cold && !p.routed && p.carrying.is_none())
                    .min_by(|a, c| a.pos.dist2(at).partial_cmp(&c.pos.dist2(at)).unwrap())
                    .map(|p| p.id);
                let pick = pick.or_else(|| if self.may_call_sea(owner) { self.call_from_sea(owner, at, 0) } else { None });
                let Some(id) = pick else { break };
                self.give_order(id, Order::Tend { building: b as u16 });
            }
        }
    }

    /// An enemy building or igloo at `at` that a catapult of `owner` may hit.
    fn catapult_target_at(&self, owner: u8, at: Vec2) -> bool {
        let (x, y) = at.tile();
        if let Some(ig) = self.igloo_on(x, y) {
            let ig = &self.igloos[ig as usize];
            return self.hostile(owner, ig.owner) && ig.hp > 0;
        }
        self.building_at(x, y).is_some_and(|b| b.solid() && self.hostile(owner, b.owner))
    }

    /// Where catapult `b` lobs next: where its owner aimed it while there is
    /// still something to hit there, or else the nearest enemy building or
    /// igloo in reach.
    fn catapult_aim(&self, b: usize) -> Option<Vec2> {
        let bd = &self.buildings[b];
        let (owner, c) = (bd.owner, bd.center());
        if let Some(at) = bd.rally.filter(|&at| at.dist(c) <= CATAPULT_RANGE && self.catapult_target_at(owner, at)) {
            return Some(at);
        }
        let igloos = self.igloos.iter().filter(|ig| ig.owner != NEUTRAL && ig.hp > 0 && self.hostile(owner, ig.owner)).map(|ig| ig.center);
        let buildings = self.buildings.iter().filter(|o| o.alive && o.solid() && self.hostile(owner, o.owner)).map(|o| o.center());
        igloos.chain(buildings).filter(|at| at.dist(c) <= CATAPULT_RANGE).min_by(|a, d| a.dist2(c).partial_cmp(&d.dist2(c)).unwrap())
    }

    /// Owner's aim for catapult `building`: a spot on an enemy building or
    /// igloo in reach, or anywhere else to let it pick by itself again.
    pub(super) fn aim_catapult(&mut self, player: u8, building: u16, at: Vec2) {
        let Some(b) = self.buildings.get(building as usize) else { return };
        if !b.alive || b.owner != player || b.kind != BuildKind::Catapult || !at.x.is_finite() || !at.y.is_finite() {
            return;
        }
        let ok = at.dist(b.center()) <= CATAPULT_RANGE && self.catapult_target_at(player, at);
        self.buildings[building as usize].rally = ok.then_some(at);
    }

    /// Every tick: crewed catapults count down and lob.
    pub(super) fn update_catapults(&mut self) {
        for b in 0..self.buildings.len() {
            let bd = &mut self.buildings[b];
            if !bd.alive || !bd.done() || bd.kind != BuildKind::Catapult || bd.owner == NEUTRAL {
                continue;
            }
            if bd.reload > 0 {
                bd.reload -= 1;
                continue;
            }
            let crew = self.crew_at_post(b as u16);
            let Some(&loader) = crew.first() else { continue };
            let Some(aim) = self.catapult_aim(b) else { continue };
            let bd = &self.buildings[b];
            let from = bd.center();
            let from_z = self.map.height_at(from) + TOWER_DECK + 0.8;
            let to = aim + v2(self.rng.gauss(), self.rng.gauss()) * CATAPULT_SPREAD;
            let to = v2(to.x.clamp(0.0, self.map.w as f32 - 0.01), to.y.clamp(0.0, self.map.h as f32 - 0.01));
            let flight_s = 0.9 + from.dist(to) * 0.08;
            let sb = Snowball {
                id: self.next_snowball,
                from,
                from_z,
                to,
                to_z: self.map.height_at(to) + if self.building_at(to.tile().0, to.tile().1).is_some() { 0.6 } else { 0.0 },
                start_tick: self.tick,
                flight: ((flight_s / DT).round() as u16).max(2),
                thrower: loader,
                team: self.team(bd.owner),
                kind: Kind::Worker,
                big: true,
            };
            self.next_snowball += 1;
            self.events.push(Event::Thrown(sb.clone()));
            self.snowballs.push(sb);
            // Fewer hands, slower loading.
            self.buildings[b].reload = CATAPULT_RELOAD * CATAPULT_CREW as u16 / crew.len().min(CATAPULT_CREW as usize) as u16;
        }
    }

    /// A big snowball came down: it wrecks the enemy building or igloo it
    /// lands on. True if it hit one.
    pub(super) fn catapult_landed(&mut self, s: &Snowball) -> bool {
        let (tx, ty) = s.to.tile();
        if let Some(ig) = self.igloo_on(tx, ty) {
            let owner = self.igloos[ig as usize].owner;
            if owner != NEUTRAL && self.team(owner) != s.team && self.igloos[ig as usize].hp > 0 {
                let igloo = &mut self.igloos[ig as usize];
                igloo.hp = igloo.hp.saturating_sub(CATAPULT_IGLOO_DAMAGE);
                if igloo.hp == 0 {
                    self.collapse(ig);
                }
            }
            return true;
        }
        if let Some(b) = self.building_at(tx, ty).filter(|b| b.solid()) {
            let (b, friendly) = (b.id as usize, self.team(b.owner) == s.team);
            if !friendly {
                self.damage_building(b, CATAPULT_DAMAGE);
            }
            return true;
        }
        false
    }
}
