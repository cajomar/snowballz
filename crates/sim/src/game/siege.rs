//! Knocking bases down (DESIGN.md section 0): with the `siege` rule,
//! soldiers pelt enemy igloos and buildings in range when there's nobody
//! to throw at, an igloo pelted down to nothing collapses into a ruin
//! anyone may claim, and a tribe left with no igloos and no buildings is
//! out.

use super::*;

/// How much an igloo of each tier can take.
pub const IGLOO_HP: [u16; 3] = [120, 200, 300];
/// Snowballs that hit an igloo or building do this much (Packers more).
pub const SIEGE_DAMAGE: u16 = 1;
pub const PACKER_SIEGE_DAMAGE: u16 = 3;

/// The most an igloo of this tier can take.
pub fn igloo_max_hp(tier: u8) -> u16 {
    IGLOO_HP[(tier as usize).min(IGLOO_HP.len() - 1)]
}

/// Idle workers this close to an igloo's walls patch them up.
pub const IGLOO_REPAIR_RADIUS: f32 = 2.0;
/// At most this many workers can patch one igloo at once.
pub const IGLOO_REPAIRERS: usize = 4;

impl Game {
    /// Each idle worker beside its tribe's battered igloo adds a point of
    /// hp every second, up to [`IGLOO_REPAIRERS`]. Ruins stay ruins.
    pub(super) fn update_igloo_repairs(&mut self) {
        if !self.tick.is_multiple_of(crate::TICK_RATE) {
            return;
        }
        let reach = IGLOO_SIZE as f32 / 2.0 + IGLOO_REPAIR_RADIUS;
        let ring = self.night_ring();
        for ig in 0..self.igloos.len() {
            let (owner, hp, center, tier) = {
                let g = &self.igloos[ig];
                (g.owner, g.hp, g.center, g.tier)
            };
            let max = igloo_max_hp(tier);
            // Nothing mends out in the Long Night's cold.
            let cold = ring.is_some_and(|(mid, r)| center.dist(mid) > r);
            if owner == NEUTRAL || hp == 0 || hp >= max || cold {
                continue;
            }
            let team = self.team(owner);
            let mut crew = 0;
            for p in &self.penguins {
                if crew == IGLOO_REPAIRERS {
                    break;
                }
                if p.kind != Kind::Worker
                    || p.inside.is_some()
                    || p.act != Act::Idle
                    || p.cold
                    || p.routed
                    || self.team(p.owner) != team
                {
                    continue;
                }
                if p.pos.dist(center) <= reach {
                    crew += 1;
                }
            }
            if crew > 0 {
                let igloo = &mut self.igloos[ig];
                igloo.hp = (igloo.hp + crew as u16).min(max);
            }
        }
    }

    /// The igloo covering this tile, if any.
    pub fn igloo_on(&self, x: i32, y: i32) -> Option<u8> {
        self.igloos
            .iter()
            .find(|ig| x >= ig.tile.0 && y >= ig.tile.1 && x < ig.tile.0 + IGLOO_SIZE && y < ig.tile.1 + IGLOO_SIZE)
            .map(|ig| ig.id)
    }

    /// The nearest enemy igloo or building this soldier can reach with a
    /// throw, as a spot to aim at.
    pub(super) fn find_structure(&self, i: usize) -> Option<Vec2> {
        let p = &self.penguins[i];
        let reach = |at: Vec2, slack: f32| p.pos.dist(at) <= self.throw_range(p.kind, p.owner, p.pos, at) + slack;
        let mut best: Option<(f32, Vec2)> = None;
        let mut consider = |at: Vec2| {
            let d = p.pos.dist(at);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, at));
            }
        };
        for ig in &self.igloos {
            // An igloo is three tiles across: in range of its near side is enough.
            if ig.owner != NEUTRAL && self.hostile(p.owner, ig.owner) && reach(ig.center, 1.2) {
                consider(ig.center);
            }
        }
        for b in &self.buildings {
            if b.alive && b.solid() && self.hostile(p.owner, b.owner) && reach(b.center(), 0.5) {
                consider(b.center());
            }
        }
        best.map(|(_, at)| at)
    }

    /// Wind up a throw at a spot on an enemy structure.
    pub(super) fn start_structure_throw(&mut self, i: usize, at: Vec2) {
        let p = &mut self.penguins[i];
        p.target = None;
        p.seal_aim = None;
        p.wall_aim = Some(at);
        p.act = Act::Throw;
        p.act_t = 0;
        p.anim_seq = p.anim_seq.wrapping_add(1);
        p.facing = (at - p.pos).angle();
    }

    /// A snowball came down on an igloo.
    pub(super) fn hit_igloo(&mut self, ig: u8, team: u8, kind: Kind) {
        let igloo = &self.igloos[ig as usize];
        if igloo.owner == NEUTRAL || self.team(igloo.owner) == team {
            return;
        }
        let dmg = if kind == Kind::Packer { PACKER_SIEGE_DAMAGE } else { SIEGE_DAMAGE };
        let igloo = &mut self.igloos[ig as usize];
        igloo.hp = igloo.hp.saturating_sub(dmg);
        if igloo.hp == 0 {
            self.collapse(ig);
        }
    }

    /// An igloo pelted to nothing falls in: everyone inside tumbles out,
    /// its stores are lost, and it is left an empty ruin that
    /// anyone may move into.
    pub(super) fn collapse(&mut self, ig: u8) {
        let owner = self.igloos[ig as usize].owner;
        for id in self.igloos[ig as usize].occupants.clone() {
            self.exit_igloo(id);
        }
        self.release_prisoners(ig as usize, None);
        let igloo = &mut self.igloos[ig as usize];
        igloo.owner = NEUTRAL;
        igloo.takeover = None;
        igloo.upgrade = None;
        igloo.research = None;
        igloo.fish = 0;
        igloo.wood = 0;
        igloo.crystal = 0;
        // Whoever lived here moves to the tribe's nearest other igloo.
        let at = igloo.center;
        let new_home = self
            .igloos
            .iter()
            .filter(|o| o.owner == owner && o.id != ig)
            .min_by(|a, b| a.center.dist2(at).partial_cmp(&b.center.dist2(at)).unwrap())
            .map(|o| o.id);
        if let Some(h) = new_home {
            for p in &mut self.penguins {
                if p.home == ig && p.owner == owner {
                    p.home = h;
                }
            }
        }
        self.events.push(Event::Collapsed { igloo: ig, owner });
    }
}
