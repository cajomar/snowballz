//! Training camps. A worker sent to a camp walks over, trains for a while
//! and comes out as the camp's specialist. The fish (and wood) is paid
//! when the training is done, fish from its home igloo (or the richest one),
//! so a tribe short of either just has its trainees waiting. A camp needs an
//! igloo of the tier its kind asks for.
//!
//! With armies (DESIGN.md 0.1, 0.2) a camp is elite housing instead: it has
//! a few slots (1, built up to 3 then 5), and each empty one calls in an idle
//! worker. Crystal gates elites: the worker mines a crystal first, then trains
//! quickly and becomes the camp's specialist, the crystal used up. When the
//! specialist freezes, the slot calls in another. A Bunkhouse is the same for
//! Snowballers, the cheap and quick soldiers, with no crystal to fetch. Only
//! as many slots as the tribe wants (`Building::want`) are filled.

use super::*;

/// With armies, once it has its crystal: training is short.
pub const ELITE_TRAIN_TICKS: u16 = secs(3.0);
/// With armies a mined-out crystal grows back one at a time, this often.
pub const CRYSTAL_REGROW_TICKS: u32 = secs(30.0) as u32;
/// A trainee stands this close to the camp.
pub const TRAIN_REACH: f32 = 1.3;
/// Bunks a Bunkhouse can be built up to.
pub const BUNK_SLOTS: [u8; 3] = [4, 8, 12];
/// Slots a camp can be built up to, and the work each step takes
/// (building steps, logs): builders do it, like a new site.
pub const CAMP_SLOTS: [u8; 3] = [1, 3, 5];
pub const CAMP_UPGRADE: [(u8, u8); 2] = [(4, 2), (8, 4)];

/// The work to build a camp with `slots` up to more, if it can be.
pub fn camp_upgrade(slots: u8) -> Option<(u8, u8)> {
    CAMP_SLOTS.iter().position(|&s| s == slots).and_then(|k| CAMP_UPGRADE.get(k).copied())
}

impl Game {
    /// Make penguin `w` a `kind`, with that kind's gear and a clean slate.
    pub(super) fn retrain(&mut self, w: u16, kind: Kind) {
        let snowballs = self.effects_of(self.penguins[w as usize].owner).ammo(kind);
        let max_warmth = self.max_warmth_of(kind);
        let p = &mut self.penguins[w as usize];
        p.kind = kind;
        p.carry = Carry::None;
        p.gather_res = None;
        p.shore = None;
        p.order = Order::None;
        p.snowballs = snowballs;
        p.max_warmth = max_warmth;
        p.warmth = p.warmth.min(p.max_warmth);
    }

    /// Does the tribe have an igloo grand enough to train this kind?
    fn can_train(&self, player: u8, kind: Kind) -> bool {
        self.igloos.iter().any(|ig| ig.owner == player && self.tier_met(kind, ig.tier))
    }

    /// The igloo that pays `fish` for `player`: the penguin's home if it can.
    pub(super) fn fish_igloo(&self, player: u8, home: u8, fish: u16) -> Option<usize> {
        let ok = |ig: &Igloo| ig.owner == player && ig.fish >= fish;
        if self.igloos.get(home as usize).is_some_and(ok) {
            return Some(home as usize);
        }
        (0..self.igloos.len()).filter(|&k| ok(&self.igloos[k])).max_by_key(|&k| self.igloos[k].fish)
    }

    /// Worker job loop for training: walk to the camp, wait, change kind.
    pub(super) fn train_job(&mut self, i: usize, building: u16) {
        // With armies a worker brings a crystal before it can train (not
        // to be drafted into a Bunkhouse).
        let snowballer = self.buildings.get(building as usize).is_some_and(|b| b.kind.trains() == Some(Kind::Snowballer));
        let elite = self.penguins[i].kind == Kind::Worker;
        let crystal = elite && !snowballer;
        if crystal && self.penguins[i].carry != Carry::Crystal {
            if self.deliver(i) {
                return;
            }
            if !self.fetch_crystal(i) {
                self.penguins[i].order = Order::None;
            }
            return;
        }
        if !elite && self.deliver(i) {
            return;
        }
        let (owner, home, pos) = (self.penguins[i].owner, self.penguins[i].home, self.penguins[i].pos);
        let b = &self.buildings[building as usize];
        let Some(kind) = b.kind.trains().filter(|_| b.alive && b.done() && b.owner == owner) else {
            self.penguins[i].order = Order::None;
            return;
        };
        if b.dist_to(pos) > TRAIN_REACH {
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
        let ticks = self.effects_of(owner).scale(Stat::TrainTime, ELITE_TRAIN_TICKS as u32) as u16;
        let p = &mut self.penguins[i];
        p.train_t = (p.train_t + 1).min(ticks);
        if p.train_t < ticks {
            return;
        }
        // With armies the camp was paid when it called the trainee in.
        let (fish, wood) = if elite { (0, 0) } else { price(kind.stats().fish, kind.stats().wood) };
        let paid = self.can_train(owner, kind) && self.wood_of(owner) >= wood as u32;
        let Some(ig) = self.fish_igloo(owner, home, fish).filter(|_| paid) else {
            // With armies a snowballer doesn't hang about a camp the tribe
            // can't pay for: it goes back to its army, and the slot calls
            // someone in again once there is the fish.
            self.penguins[i].order = Order::None;
        
            return;
        };
        self.spend_wood(owner, wood);
        self.igloos[ig].fish -= fish;
        self.retrain(i as u16, kind);
        self.penguins[i].camp = Some(building);
    
        self.events.push(Event::Trained { building, penguin: i as u16 });
        if let Some(target) = self.buildings[building as usize].rally {
            self.give_order(i as u16, Order::Move { target, run: false, auto: false });
        }
    }

    /// Set where a camp's graduates walk to; pointing at the camp itself clears it.
    pub(super) fn set_rally(&mut self, player: u8, building: u16, at: Vec2) {
        let (w, h) = (self.map.w as f32, self.map.h as f32);
        let Some(b) = self.buildings.get_mut(building as usize) else { return };
        if !b.alive || b.owner != player || b.kind.trains().is_none() || !at.x.is_finite() || !at.y.is_finite() {
            return;
        }
        let tiles = b.kind.size() as f32;
        let inside = at.x >= b.tile.0 as f32 && at.y >= b.tile.1 as f32 && at.x < b.tile.0 as f32 + tiles && at.y < b.tile.1 as f32 + tiles;
        b.rally = (!inside).then(|| v2(at.x.clamp(0.5, w - 0.5), at.y.clamp(0.5, h - 0.5)));
    }

    /// Penguins filling camp `b`'s slots: its specialists, and whoever is on
    /// the way to train there.
    pub fn camp_filled(&self, b: u16) -> usize {
        let owner = self.buildings[b as usize].owner;
        self.penguins.iter().filter(|p| p.owner == owner && p.act != Act::Frozen && (p.camp == Some(b) || p.order == Order::Train { building: b })).count()
    }

    /// The furthest-along trainee at camp `b`, as a fraction of the training.
    pub fn camp_training(&self, b: u16) -> Option<f32> {
        let ticks = self.effects_of(self.buildings[b as usize].owner).scale(Stat::TrainTime, ELITE_TRAIN_TICKS as u32).max(1);
        self.penguins
            .iter()
            .filter(|p| p.order == Order::Train { building: b } && p.act != Act::Frozen)
            .map(|p| p.train_t as f32 / ticks as f32)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
    }

    /// A worker's errand for a crystal: to the nearest deposit with one
    /// left, and mine it. False if there is none anywhere.
    fn fetch_crystal(&mut self, i: usize) -> bool {
        let pos = self.penguins[i].pos;
        let ok = |r: &Resource| r.kind == ResourceKind::Crystal && r.amount > 0 && !r.spots.is_empty();
        let r = self.penguins[i].gather_res.filter(|&r| ok(&self.resources[r as usize])).or_else(|| {
            self.resources.iter().filter(|r| ok(r)).min_by(|a, b| a.pos.dist2(pos).partial_cmp(&b.pos.dist2(pos)).unwrap()).map(|r| r.id)
        });
        let Some(r) = r else { return false };
        self.penguins[i].gather_res = Some(r);
        let res = &self.resources[r as usize];
        let rpos = res.pos;
        let spot = *res.spots.iter().min_by(|a, b| a.dist2(pos).partial_cmp(&b.dist2(pos)).unwrap()).unwrap();
        if self.travel(i, spot, 0.25) {
            let p = &mut self.penguins[i];
            p.act = Act::Mine;
            p.act_t = 0;
            p.anim_seq = p.anim_seq.wrapping_add(1);
            p.facing = (rpos - p.pos).angle();
        }
        true
    }

    /// With armies mined-out crystal grows back slowly, so no map runs dry.
    pub(super) fn regrow_crystal(&mut self) {
        if !self.tick.is_multiple_of(CRYSTAL_REGROW_TICKS) {
            return;
        }
        for r in self.resources.iter_mut().filter(|r| r.kind == ResourceKind::Crystal && r.amount < r.max) {
            r.amount += 1;
        }
    }

    /// Once a second, with armies: every camp (and Bunkhouse) with fewer
    /// trainees than the tribe wants calls in the nearest idle worker of its
    /// tribe (never a fisher), if the tribe can pay for the training and,
    /// for a specialist, there is crystal to mine.
    pub(super) fn fill_camps(&mut self) {
        for b in 0..self.buildings.len() {
            let bd = &self.buildings[b];
            let Some(kind) = bd.kind.trains().filter(|_| bd.alive && bd.done() && bd.owner != NEUTRAL && bd.kind.offered()) else { continue };
            let (owner, at, want) = (bd.owner, bd.center(), bd.want.min(bd.slots) as usize);
            if self.camp_filled(b as u16) >= want || !self.can_train(owner, kind) {
                continue;
            }
            // Only as many trainees on their way to a crystal as there is crystal.
            let crystal: u32 = self.resources.iter().filter(|r| r.kind == ResourceKind::Crystal).map(|r| r.amount as u32).sum();
            let seeking = self.penguins.iter().filter(|p| p.owner == owner && p.kind == Kind::Worker && p.carry != Carry::Crystal && matches!(p.order, Order::Train { building } if self.buildings[building as usize].kind.trains() != Some(Kind::Snowballer))).count() as u32;
            if kind != Kind::Snowballer && crystal <= seeking {
                continue;
            }
            let st = kind.stats();
            let rich = self.igloos.iter().filter(|g| g.owner == owner).map(|g| g.fish).max().unwrap_or(0);
            let (fish, wood) = price(st.fish, st.wood);
            if rich < fish || self.wood_of(owner) < wood as u32 {
                continue;
            }
            let pick = (0..self.penguins.len())
                // Idle workers only: a camp never takes the tribe's fishers.
                .filter(|&i| {
                    let p = &self.penguins[i];
                    p.owner == owner && p.kind == Kind::Worker && p.order == Order::None && p.fishery.is_none() && !p.shore_pin && p.is_out() && p.act != Act::Frozen && !p.cold && !p.routed && p.carrying.is_none()
                })
                .min_by(|&x, &y| self.penguins[x].pos.dist2(at).partial_cmp(&self.penguins[y].pos.dist2(at)).unwrap());
            // Nobody to call: a newcomer swims in from the sea, if the tribe
            // can pay for it and its training too.
            let pick = pick.or_else(|| if self.may_call_sea(owner) { self.call_from_sea(owner, at, fish).map(|id| id as usize) } else { None });
            if let Some(i) = pick {
                // Paid now, so the fish is there when the trainee is.
                let Some(ig) = self.fish_igloo(owner, NEUTRAL, fish) else { continue };
                self.igloos[ig].fish -= fish;
                self.spend_wood(owner, wood);
                self.give_order(i as u16, Order::Train { building: b as u16 });
                self.penguins[i].train_t = 0;
            }
        }
    }

    /// Start building a camp, Bunkhouse or Sushi House up to more slots: its
    /// tribe's builders come to do the work, and the new room is there once
    /// it is done (filled only as far as the tribe wants).
    pub(super) fn upgrade_building(&mut self, player: u8, building: u16) {
        let Some(b) = self.buildings.get(building as usize) else { return };
        if !b.alive || !b.done() || b.owner != player || b.upgrading.is_some() {
            return;
        }
        if b.kind == BuildKind::Campfire {
            self.campfire_to_sauna(building);
            return;
        }
        if b.kind == BuildKind::Tower {
            let (steps, wood) = CATAPULT_UPGRADE;
            self.buildings[building as usize].upgrading = Some(BuildingUpgrade { steps, wood, to: CATAPULT_CREW, into: Some(BuildKind::Catapult) });
            return;
        }
        let Some((to, (steps, wood))) = b.kind.next_slots(b.slots) else { return };
        self.buildings[building as usize].upgrading = Some(BuildingUpgrade { steps, wood, to, into: None });
    }

    /// Where a Sauna built up from this campfire would stand: a 2 by 2 spot
    /// over the fire's tile with room for the rest of it.
    pub fn sauna_site(&self, building: u16) -> Option<(i32, i32)> {
        let b = self.buildings.get(building as usize)?;
        let (x, y) = b.tile;
        [(x, y), (x - 1, y), (x, y - 1), (x - 1, y - 1)].into_iter().find(|&at| {
            footprint(BuildKind::Sauna, at).filter(|&t| t != b.tile).all(|(tx, ty)| {
                // The fire's own tile is taken, by the fire.
                self.placement_problem(b.owner, BuildKind::Campfire, (tx, ty)).is_none()
            })
        })
    }

    /// A campfire built up becomes a Sauna site on the spot, its first step
    /// and log already done by the fire; the fire is out while it goes up.
    fn campfire_to_sauna(&mut self, building: u16) {
        let Some(at) = self.sauna_site(building) else { return };
        let fx = self.effects_of(self.buildings[building as usize].owner);
        let bd = &mut self.buildings[building as usize];
        bd.kind = BuildKind::Sauna;
        bd.tile = at;
        bd.built = BuildKind::Campfire.snow();
        bd.wood_in = BuildKind::Campfire.wood() as u8;
        bd.lit = false;
        bd.fuel = 0;
        bd.starved = false;
        let max = BuildKind::Sauna.max_hp_with(fx);
        bd.hp = ((max as u32 * bd.built as u32 / BuildKind::Sauna.snow() as u32) as u8).max(1);
        let tiles: Vec<(i32, i32)> = bd.tiles().collect();
        for (x, y) in tiles {
            let i = self.map.idx(x, y);
            self.building_at[i] = building;
            self.terrain.mark(i);
        }
        self.make_solid(building);
    }

    /// How many of building `building`'s slots its tribe wants filled.
    pub(super) fn set_want(&mut self, player: u8, building: u16, want: u8) {
        let Some(b) = self.buildings.get_mut(building as usize) else { return };
        if b.alive && b.owner == player && b.kind.slot_sizes().is_some() {
            b.want = want.min(b.slots);
        }
    }
}
