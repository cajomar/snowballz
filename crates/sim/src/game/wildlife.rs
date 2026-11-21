//! Wildlife: leopard seals lurk under the ice at fish holes and on open ice
//! out in the contested ground. A seal surfaces when a lone penguin comes
//! near and snatches it, dragging it to the water's edge half frozen. It
//! will not touch a group, a few snowballs drive it back under, and after
//! any of that it keeps away for a while.

use super::*;

/// One seal per this many map tiles (at least two, at most [`MAX_SEALS`]).
const TILES_PER_SEAL: f32 = 6000.0;
pub const MAX_SEALS: usize = 8;
/// Seals keep this far from igloos and crystal, and from each other.
const IGLOO_CLEAR: f32 = 14.0;
const CRYSTAL_CLEAR: f32 = 6.0;
const SEAL_SPACING: f32 = 16.0;

/// A lurking seal surfaces for a penguin this close.
pub const SEAL_NOTICE: f32 = 3.2;
/// How far its lunge reaches once it is up.
pub const SEAL_REACH: f32 = 2.6;
/// A seal can be hit by a snowball landing this close.
const SEAL_BODY: f32 = 0.9;
/// Snowball hits it takes to drive a surfaced seal back under.
pub const SEAL_HP: u8 = 2;
/// Friends this close (counting itself) make a penguin safe if there are
/// [`GROUP_SIZE`] of them.
pub const GROUP_RADIUS: f32 = 4.0;
pub const GROUP_SIZE: usize = 3;
/// Time between the seal breaking the surface and its lunge.
pub const SEAL_WARN_TICKS: u16 = secs(1.4);
/// Time under before it comes back after a strike, a scare or a hit.
pub const SEAL_COOLDOWN: u16 = secs(45.0);
/// A seal rests this long after a catch before it may strike again, even if
/// it is back at the surface sooner.
pub const SEAL_SNATCH_COOLDOWN: u16 = secs(30.0);
/// A miss costs it less time.
const SEAL_MISS_COOLDOWN: u16 = secs(12.0);
/// A seal that struck this recently counts as dangerous ground for the AI.
pub const SEAL_RECENT: u32 = secs(150.0) as u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SealState {
    /// Under the ice, out of sight, waiting.
    Lurking = 0,
    /// Up on the ice with its jaws open.
    Surfaced = 1,
    /// Dived and keeping away until its cooldown ends.
    Gone = 2,
}

impl SealState {
    pub fn from_u8(v: u8) -> SealState {
        match v {
            1 => SealState::Surfaced,
            2 => SealState::Gone,
            _ => SealState::Lurking,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Seal {
    pub id: u8,
    pub pos: Vec2,
    pub state: SealState,
    /// Snowball hits left before it dives.
    pub hp: u8,
    /// Ticks until the lunge (surfaced) or the return (gone).
    timer: u16,
    /// Ticks left before it may snatch again.
    pub cooldown: u16,
    /// Tick of its last snatch, if it has made one.
    pub struck: Option<u32>,
}

impl Seal {
    /// How far the seal is from the straight line `a` to `b`.
    pub fn route_dist(&self, a: Vec2, b: Vec2) -> f32 {
        let ab = b - a;
        let t = (((self.pos - a).x * ab.x + (self.pos - a).y * ab.y) / ab.len2().max(1e-4)).clamp(0.0, 1.0);
        self.pos.dist(a + ab * t)
    }

    /// Has it made a catch in the last couple of minutes?
    pub fn recently_struck(&self, now: u32) -> bool {
        self.struck.is_some_and(|t| now.saturating_sub(t) < SEAL_RECENT)
    }
}

fn mix(x: i32, y: i32) -> u32 {
    let h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
    (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D)
}

/// Spots for seals: fish holes and open ice well away from every igloo. Spread by a fixed hash of the position, so the same map always
/// gives the same seals; no randomness, no map changes needed.
pub(super) fn place_seals(map: &Map, grid: &Grid, igloos: &[Igloo], resources: &[Resource]) -> Vec<Seal> {
    let want = ((map.w * map.h) as f32 / TILES_PER_SEAL).round().clamp(2.0, MAX_SEALS as f32) as usize;
    let ok = |p: Vec2| {
        igloos.iter().all(|ig| ig.center.dist(p) >= IGLOO_CLEAR)
            && resources.iter().all(|r| r.kind != ResourceKind::Crystal || r.pos.dist(p) >= CRYSTAL_CLEAR)
    };
    let mut holes: Vec<(u32, Vec2)> = resources
        .iter()
        .filter(|r| r.kind == ResourceKind::Fish && !r.spots.is_empty() && ok(r.pos))
        .map(|r| (mix(r.tile.0, r.tile.1), r.pos))
        .collect();
    let mut ice: Vec<(u32, Vec2)> = Vec::new();
    for y in (2..map.h - 2).step_by(2) {
        for x in (2..map.w - 2).step_by(2) {
            let p = v2(x as f32 + 0.5, y as f32 + 0.5);
            if map.ground_at(x, y) == Ground::Ice
                && (-1..=1).all(|dy| (-1..=1).all(|dx| grid.walkable(x + dx, y + dy)))
                && ok(p)
            {
                ice.push((mix(x, y), p));
            }
        }
    }
    holes.sort_by_key(|c| c.0);
    ice.sort_by_key(|c| c.0);

    // Two holes for every stretch of open ice, as far as each list lasts.
    let mut out: Vec<Seal> = Vec::new();
    let (mut h, mut i) = (0, 0);
    while out.len() < want && (h < holes.len() || i < ice.len()) {
        let from_ice = (h >= holes.len() || (out.len() % 3 == 2)) && i < ice.len();
        let p = if from_ice {
            i += 1;
            ice[i - 1].1
        } else {
            h += 1;
            holes[h - 1].1
        };
        if out.iter().all(|s| s.pos.dist(p) >= SEAL_SPACING) {
            out.push(Seal { id: out.len() as u8, pos: p, state: SealState::Lurking, hp: SEAL_HP, timer: 0, cooldown: 0, struck: None });
        }
    }
    out
}

impl Game {
    /// Penguins out on the ice, not frozen, within `r` of `pos`.
    fn standing_near(&mut self, pos: Vec2, r: f32) -> Vec<u16> {
        let mut near = std::mem::take(&mut self.scratch);
        self.buckets.query(pos, r, &mut near);
        let out = near
            .iter()
            .copied()
            .filter(|&o| {
                let q = &self.penguins[o as usize];
                q.inside.is_none() && q.act != Act::Frozen && q.pos.dist(pos) <= r
            })
            .collect();
        self.scratch = near;
        out
    }

    /// Fewer than [`GROUP_SIZE`] of its own side within [`GROUP_RADIUS`].
    fn is_lone(&mut self, id: u16) -> bool {
        let p = &self.penguins[id as usize];
        let (pos, team) = (p.pos, self.team(p.owner));
        let friends = self.standing_near(pos, GROUP_RADIUS);
        friends.iter().filter(|&&o| self.team(self.penguins[o as usize].owner) == team).count() < GROUP_SIZE
    }

    /// The lone penguin nearest `pos` within `r` that a seal could take.
    fn seal_prey(&mut self, pos: Vec2, r: f32) -> Option<u16> {
        let mut near = self.standing_near(pos, r);
        near.retain(|&o| self.penguins[o as usize].owner != NEUTRAL && self.penguins[o as usize].carried_by.is_none());
        near.sort_by(|&a, &b| {
            let (da, db) = (self.penguins[a as usize].pos.dist2(pos), self.penguins[b as usize].pos.dist2(pos));
            da.partial_cmp(&db).unwrap().then(a.cmp(&b))
        });
        near.into_iter().find(|&o| self.is_lone(o))
    }

    pub(super) fn update_seals(&mut self) {
        for i in 0..self.seals.len() {
            let pos = self.seals[i].pos;
            self.seals[i].cooldown = self.seals[i].cooldown.saturating_sub(1);
            match self.seals[i].state {
                SealState::Lurking => {
                    // Look about a few times a second, once it has rested.
                    if self.seals[i].cooldown > 0
                        || !(self.tick + i as u32 * 3).is_multiple_of(4)
                        || self.seal_prey(pos, SEAL_NOTICE).is_none()
                    {
                        continue;
                    }
                    let s = &mut self.seals[i];
                    s.state = SealState::Surfaced;
                    s.timer = SEAL_WARN_TICKS;
                    s.hp = SEAL_HP;
                    self.events.push(Event::SealSurfaced { seal: i as u8 });
                }
                SealState::Surfaced => {
                    // A crowd frightens it off before it can strike.
                    if self.standing_near(pos, GROUP_RADIUS).len() >= GROUP_SIZE {
                        self.dive(i, NEUTRAL, SEAL_COOLDOWN);
                        continue;
                    }
                    self.seals[i].timer = self.seals[i].timer.saturating_sub(1);
                    if self.seals[i].timer > 0 {
                        continue;
                    }
                    match self.seal_prey(pos, SEAL_REACH) {
                        Some(v) => self.snatch(i, v),
                        // Nobody left to take: back under, but quicker to try again.
                        None => {
                            let s = &mut self.seals[i];
                            s.state = SealState::Gone;
                            s.timer = SEAL_MISS_COOLDOWN;
                        }
                    }
                }
                SealState::Gone => {
                    let s = &mut self.seals[i];
                    s.timer = s.timer.saturating_sub(1);
                    if s.timer == 0 {
                        s.state = SealState::Lurking;
                        s.hp = SEAL_HP;
                    }
                }
            }
        }
    }

    /// Back under the ice for `cooldown` ticks; `by` is whoever drove it off,
    /// or [`NEUTRAL`] if a crowd did.
    fn dive(&mut self, seal: usize, by: u8, cooldown: u16) {
        let s = &mut self.seals[seal];
        s.state = SealState::Gone;
        s.timer = cooldown;
        self.events.push(Event::SealDriven { seal: seal as u8, by });
    }

    /// The seal drags `victim` to the water's edge, frozen stiff, and dives.
    /// It drops whatever the victim carried; the victim thaws after a few
    /// seconds and walks home cold, or is carried off if an enemy gets there.
    fn snatch(&mut self, seal: usize, victim: u16) {
        let spos = self.seals[seal].pos;
        self.drop_captive(victim);
        let from = self.penguins[victim as usize].pos;
        // Pulled in to the edge of the hole, as far as the ice allows.
        let to = spos + (from - spos).norm() * 1.0f32.max(from.dist(spos) * 0.4);
        let walkable = self.grid.walkable_at(to);
        let p = &mut self.penguins[victim as usize];
        if walkable {
            p.pos = to;
        }
        p.warmth = 0;
        p.act = Act::Frozen;
        p.act_t = 0;
        p.cold = true;
        p.order = Order::None;
        p.goal = None;
        p.path.clear();
        p.target = None;
        p.seal_aim = None;
        p.carry = Carry::None;
        p.gather_res = None;
        p.shore = None;
        p.chop_tree = None;
        p.build_site = None;
        let (pos, owner) = (p.pos, p.owner);
        let team = self.team(owner);
        let s = &mut self.seals[seal];
        s.state = SealState::Gone;
        s.timer = SEAL_COOLDOWN;
        s.cooldown = SEAL_SNATCH_COOLDOWN;
        s.struck = Some(self.tick);
        self.events.push(Event::SealSnatched { seal: seal as u8, penguin: victim });
        self.morale_shock(pos, team);
        if self.penguins[victim as usize].kind == Kind::Chieftain {
            self.chieftain_fell(pos, team);
        }
        // Whatever the victim carried as a powerup lies where it fell.
        if let Some(pw) = self.penguins[victim as usize].power.take() {
            self.unequip(victim, pw);
            self.powerups[pw as usize].pos = pos;
        }
        self.on_frozen(victim);
    }

    /// The nearest surfaced seal this snowballer can reach with a throw.
    pub(super) fn find_seal(&self, i: usize) -> Option<u8> {
        let p = &self.penguins[i];
        self.seals
            .iter()
            .filter(|s| s.state == SealState::Surfaced && s.pos.dist(p.pos) <= self.throw_range(p.kind, p.owner, p.pos, s.pos))
            .min_by(|a, b| a.pos.dist2(p.pos).partial_cmp(&b.pos.dist2(p.pos)).unwrap())
            .map(|s| s.id)
    }

    /// Point a throw at a surfaced seal.
    pub(super) fn start_seal_throw(&mut self, i: usize, seal: u8) {
        let spos = self.seals[seal as usize].pos;
        let p = &mut self.penguins[i];
        p.target = None;
        p.seal_aim = Some(seal);
        p.act = Act::Throw;
        p.act_t = 0;
        p.anim_seq = p.anim_seq.wrapping_add(1);
        p.facing = (spos - p.pos).angle();
    }

    /// A snowball thrown by `thrower` lands at `at`: a surfaced seal there
    /// takes the hit and dives once it has had enough. True if one was hit.
    pub(super) fn seal_hit(&mut self, at: Vec2, thrower: u16) -> bool {
        let hit = self.seals.iter().position(|s| s.state == SealState::Surfaced && s.pos.dist(at) <= SEAL_BODY);
        let Some(i) = hit else { return false };
        self.seals[i].hp = self.seals[i].hp.saturating_sub(1);
        if self.seals[i].hp == 0 {
            let by = self.penguins.get(thrower as usize).map_or(NEUTRAL, |p| p.owner);
            self.dive(i, by, SEAL_COOLDOWN);
        }
        true
    }
}
