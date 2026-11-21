//! Armies (the `armies` rule, DESIGN.md 0.1). Every Great Igloo has one
//! Chieftain, and the soldiers who live there are his army: with nothing
//! else to do they follow him and form up around him, facing the enemies
//! near him. You move the army by moving its Chieftain. A soldier sent off
//! on its own keeps that order; one picked together with its Chieftain
//! falls back in. A frozen Chieftain leaves his crown at home, and after a
//! while the igloo raises a new one from its own penguins. Watchtower roofs
//! fill themselves with as many sentries from the armies as the tribe wants.

use super::*;

/// How long an igloo waits to raise a new Chieftain after losing one.
pub const CHIEF_RETURN_TICKS: u32 = 60 * crate::TICK_RATE;
/// Soldiers each igloo starts with besides its Chieftain (the rest of the
/// map's starting soldiers start as workers).
pub const START_SOLDIERS: u8 = 2;
/// With armies a tribe starts small (DESIGN.md 0.2): its Chieftain and this
/// many workers, whatever the map says.
pub const START_WORKERS: u8 = 2;
/// Room between soldiers in the army's ranks: snowballs splash, so a clump
/// takes more hits.
pub const ARMY_SPACING: f32 = 1.7;
/// The army faces enemies this close to its Chieftain.
pub const ARMY_LOOK: f32 = 16.0;
/// Shieldbearers stand this far ahead of their Chieftain...
pub const WALL_AHEAD: f32 = 2.6;
/// ...in pairs this far apart, with this much room between pairs for a
/// Skier or a throw to get through.
pub const WALL_PAIR: f32 = 1.1;
pub const WALL_GAP: f32 = 2.0;
/// Slingers look this many tiles about their place for a tree or wall to stand behind.
pub const COVER_REACH: i32 = 2;
/// A Chieftain answering an alarm stops this far short of it.
pub const CHIEF_STANDOFF: f32 = 5.0;
/// Sentries a new watchtower posts by itself (the tribe may want more or
/// fewer, up to its four places; more may be sent by hand).
pub const TOWER_SENTRIES: usize = 2;
/// Sentries come from soldiers this close to the tower.
pub const SENTRY_REACH: f32 = 40.0;

impl Game {
    /// The Chieftain leading the army of igloo `home`, if he is up and about.
    pub fn chief_of(&self, owner: u8, home: u8) -> Option<u16> {
        self.penguins.iter().find(|p| p.kind == Kind::Chieftain && p.owner == owner && p.home == home && p.act != Act::Frozen).map(|p| p.id)
    }

    /// The army penguin `i` belongs to: its igloo's Chieftain, or if that
    /// one is down, the tribe's nearest.
    pub fn army_of(&self, i: usize) -> Option<u16> {
        let p = &self.penguins[i];
        if p.kind == Kind::Worker || p.kind == Kind::Chieftain || p.owner == NEUTRAL {
            return None;
        }
        self.chief_of(p.owner, p.home).or_else(|| {
            self.penguins
                .iter()
                .filter(|c| c.kind == Kind::Chieftain && c.owner == p.owner && c.act != Act::Frozen && c.is_out())
                .min_by(|a, b| a.pos.dist2(p.pos).partial_cmp(&b.pos.dist2(p.pos)).unwrap())
                .map(|c| c.id)
        })
    }

    /// A Chieftain froze: the crown goes home (the frozen penguin is a plain
    /// snowballer now) and the igloo raises another one later.
    pub(super) fn chief_down(&mut self, v: u16) {
        let home = self.penguins[v as usize].home as usize;
        if let Some(ig) = self.igloos.get_mut(home) {
            ig.chief_at = self.tick + CHIEF_RETURN_TICKS;
        }
        let p = &mut self.penguins[v as usize];
        p.kind = Kind::Snowballer;
    }

    /// Once a second: raise Chieftains where they are missing, man the
    /// watchtowers, and give every following soldier its place in the ranks.
    pub(super) fn update_armies(&mut self) {
        if !(self.tick + 7).is_multiple_of(crate::TICK_RATE) {
            return;
        }
        self.raise_chiefs();
        self.post_sentries();
        // Food first: the fishing slots pick from the idle workers before the
        // fires and the camps do.
        self.fill_fisheries();
        self.fill_stokers();
        self.fill_crews();
        self.fill_camps();
        for p in &mut self.penguins {
            p.slot = None;
        }
        let chiefs: Vec<u16> = self.penguins.iter().filter(|c| c.kind == Kind::Chieftain && c.owner != NEUTRAL && c.act != Act::Frozen && c.is_out()).map(|c| c.id).collect();
        let mut armies: Vec<Vec<u16>> = vec![Vec::new(); chiefs.len()];
        for i in 0..self.penguins.len() {
            if !self.following(i) {
                continue;
            }
            if let Some(c) = self.army_of(i) {
                if let Some(k) = chiefs.iter().position(|&x| x == c) {
                    armies[k].push(i as u16);
                }
            }
        }
        for (k, &c) in chiefs.iter().enumerate() {
            self.form_up(c, &armies[k]);
        }
    }

    /// Is penguin `i` free to follow its Chieftain: a soldier out in the
    /// field with no order of its own?
    pub(super) fn following(&self, i: usize) -> bool {
        let p = &self.penguins[i];
        p.kind != Kind::Worker
            && p.kind != Kind::Chieftain
            && p.owner != NEUTRAL
            && p.order == Order::None
            && p.is_out()
            && p.act != Act::Frozen
            && p.post.is_none()
            && p.carrying.is_none()
            && p.carried_by.is_none()
            && !p.cold
            && !p.routed
    }

    /// Places for the army of Chieftain `c`. Facing one way: Shieldbearers
    /// in a wall just ahead of him, in pairs with gaps between; throwers in
    /// staggered ranks behind the wall; Skiers out on the flanks; Slingers
    /// behind the throwers, in cover where there is some; Warmers and the
    /// rest at the back. Facing enemies all round: a ring of shields with
    /// the throwers just inside and the rest in the middle. Each soldier
    /// takes the free place of its sort nearest it.
    fn form_up(&mut self, c: u16, army: &[u16]) {
        if army.is_empty() {
            return;
        }
        let chief = &self.penguins[c as usize];
        let (at, owner, facing) = (chief.pos, chief.owner, chief.facing);
        let foes: Vec<Vec2> = self
            .penguins
            .iter()
            .filter(|q| q.owner != NEUTRAL && self.hostile(owner, q.owner) && q.kind.fights() && !q.wild && q.is_out() && q.act != Act::Frozen && q.pos.dist(at) < ARMY_LOOK)
            .map(|q| q.pos)
            .collect();
        // Face the middle of them: one line across their front.
        let mid = foes.iter().fold(Vec2::ZERO, |a, &b| a + b) * (1.0 / foes.len().max(1) as f32);
        let ahead = if !foes.is_empty() && mid.dist(at) > 0.1 { (mid - at).norm() } else { Vec2::from_angle(facing) };
        let mut groups: [Vec<u16>; 5] = Default::default();
        for &id in army {
            let p = &self.penguins[id as usize];
            // The hurt swap to the back, beside the Warmers (`tactics.rs`).
            let rank = if falls_back(p) && rank_of(p.kind) != Rank::Back { Rank::Rear } else { rank_of(p.kind) };
            groups[rank as usize].push(id);
        }
        let ring = surrounded(at, &foes);
        let places = if ring { ring_places(at, ahead, &groups) } else { line_places(at, ahead, &groups) };
        let mut used: Vec<Vec2> = Vec::new();
        for (g, mut spots) in places.into_iter().enumerate() {
            for spot in spots.iter_mut() {
                let out = if g == Rank::Back as usize {
                    // A Slinger stands behind a tree or a wall if there is one about.
                    let toward = if ring && spot.dist(at) > 0.1 { (*spot - at).norm() } else { ahead };
                    self.cover_near(*spot, toward, &used)
                } else {
                    None
                };
                *spot = match out {
                    Some(c) => c,
                    None if self.grid.walkable_at(*spot) => *spot,
                    None => self.grid.nearest_walkable(spot.tile(), 3).map_or(at, |(x, y)| v2(x as f32 + 0.5, y as f32 + 0.5)),
                };
                used.push(*spot);
            }
            // Nearest soldier of the sort to each place, front first.
            let mut left = std::mem::take(&mut groups[g]);
            for place in spots {
                let Some(k) = (0..left.len()).min_by(|&a, &b| {
                    let da = self.penguins[left[a] as usize].pos.dist2(place);
                    let db = self.penguins[left[b] as usize].pos.dist2(place);
                    da.partial_cmp(&db).unwrap()
                }) else {
                    break;
                };
                let id = left.swap_remove(k);
                self.penguins[id as usize].slot = Some(place);
            }
        }
    }

    /// A walkable spot within reach of `place` with a tree or a wall right
    /// in front of it (the side `toward` the enemy), not already taken.
    pub fn cover_near(&self, place: Vec2, toward: Vec2, taken: &[Vec2]) -> Option<Vec2> {
        let (px, py) = place.tile();
        let r = COVER_REACH;
        let mut best: Option<(f32, Vec2)> = None;
        for y in py - r..=py + r {
            for x in px - r..=px + r {
                if !self.grid.walkable(x, y) {
                    continue;
                }
                let c = v2(x as f32 + 0.5, y as f32 + 0.5);
                let (fx, fy) = (c + toward * 1.0).tile();
                if !self.blocks_snowballs(fx, fy) || taken.iter().any(|t| t.dist(c) < 1.0) {
                    continue;
                }
                let d = c.dist(place);
                if d <= r as f32 + 0.5 && best.is_none_or(|b| d < b.0) {
                    best = Some((d, c));
                }
            }
        }
        best.map(|b| b.1)
    }

    /// A following soldier walks to its place in the ranks. False if it has
    /// no army to follow (it stays about its igloo instead).
    pub(super) fn follow_chief(&mut self, i: usize) -> bool {
        let Some(place) = self.penguins[i].slot else { return false };
        // A few paces out of the ranks, if that brings an enemy into reach.
        let place = self.stand_at(i, place);
        if self.penguins[i].pos.dist(place) > 0.5 {
            self.travel(i, place, 0.2);
        } else {
            self.idle(i);
            // Face the nearest enemy, or the way the army faces.
            let f = match self.enemy_fighters_near(i, ARMY_LOOK).first() {
                Some(&(_, e)) => Some((self.penguins[e as usize].pos - self.penguins[i].pos).angle()),
                None => self.army_of(i).map(|c| self.penguins[c as usize].facing),
            };
            if let Some(f) = f {
                let p = &mut self.penguins[i];
                if p.act == Act::Idle {
                    p.facing = turn_towards(p.facing, f, 0.1);
                }
            }
        }
        true
    }

    /// Every igloo of a tribe has a Chieftain: one that has none (newly
    /// founded, or its Chieftain froze a while ago) raises one of its own
    /// penguins, a soldier if it has one.
    fn raise_chiefs(&mut self) {
        for ig in 0..self.igloos.len() {
            let (owner, door, at) = (self.igloos[ig].owner, self.igloos[ig].door, self.igloos[ig].chief_at);
            if owner == NEUTRAL || self.tick < at || self.chief_of(owner, ig as u8).is_some() {
                continue;
            }
            let pick = self
                .penguins
                .iter()
                .filter(|p| p.owner == owner && p.home == ig as u8 && p.is_out() && p.act != Act::Frozen && p.carrying.is_none() && p.carried_by.is_none() && p.kind != Kind::Chieftain)
                .min_by(|a, b| {
                    let key = |p: &Penguin| (p.kind == Kind::Worker) as u8 as f32 * 1000.0 + p.pos.dist(door);
                    key(a).partial_cmp(&key(b)).unwrap()
                })
                .map(|p| p.id);
            if let Some(id) = pick {
                self.make_chief(id);
            }
        }
    }

    fn make_chief(&mut self, id: u16) {
        self.give_order(id, Order::None);
        let max = self.max_warmth_of(Kind::Chieftain);
        let ammo = self.effects_of(self.penguins[id as usize].owner).ammo(Kind::Chieftain);
        let p = &mut self.penguins[id as usize];
        p.kind = Kind::Chieftain;
        p.max_warmth = max;
        p.warmth = max;
        p.snowballs = p.snowballs.min(ammo);
        p.courage = MAX_COURAGE;
        p.anim_seq = p.anim_seq.wrapping_add(1);
    }

    /// Fill empty places on the tribe's watchtower roofs with the nearest
    /// soldiers following an army.
    fn post_sentries(&mut self) {
        let towers: Vec<(u16, u8, Vec2, usize)> = self
            .buildings
            .iter()
            .filter(|b| b.alive && b.done() && b.kind == BuildKind::Tower && b.owner != NEUTRAL)
            .map(|b| (b.id, b.owner, b.center(), b.want.min(TOWER_SLOTS as u8) as usize))
            .collect();
        for (t, owner, at, want) in towers {
            let manned = self.penguins.iter().filter(|p| p.post == Some(t) || p.order == Order::Garrison { building: t }).count();
            for _ in manned..want {
                let pick = (0..self.penguins.len())
                    .filter(|&i| {
                        let p = &self.penguins[i];
                        p.owner == owner && p.kind.fights() && p.kind != Kind::Chieftain && self.following(i) && p.pos.dist(at) < SENTRY_REACH
                    })
                    .min_by(|&a, &b| self.penguins[a].pos.dist2(at).partial_cmp(&self.penguins[b].pos.dist2(at)).unwrap());
                let Some(i) = pick else { break };
                self.give_order(i as u16, Order::Garrison { building: t });
            }
        }
    }

    /// Of the units in a move order, those that belong to a Chieftain who is
    /// also in it fall in behind him rather than walk to the spot.
    pub(super) fn fall_in(&mut self, units: Vec<u16>) -> Vec<u16> {
        let chiefs: Vec<u16> = units.iter().copied().filter(|&u| self.penguins[u as usize].kind == Kind::Chieftain).collect();
        if chiefs.is_empty() {
            return units;
        }
        let mut walk = Vec::new();
        for u in units {
            let p = &self.penguins[u as usize];
            let mine = p.kind != Kind::Chieftain && p.kind != Kind::Worker && self.army_of(u as usize).is_some_and(|c| chiefs.contains(&c));
            if mine {
                if p.order != Order::None && p.post.is_none() {
                    self.give_order(u, Order::None);
                }
            } else {
                walk.push(u);
            }
        }
        walk
    }
}

/// Where a kind stands in an army.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Rank {
    /// Shieldbearers: the wall between the army and the enemy.
    Wall = 0,
    /// Throwers: the body of the army, behind the wall.
    Line = 1,
    /// Skiers: out to the sides, to go round.
    Flank = 2,
    /// Slingers: behind the throwers, in cover.
    Back = 3,
    /// Warmers, Sleds and the like: at the back, out of the way.
    Rear = 4,
}

pub fn rank_of(k: Kind) -> Rank {
    match k {
        Kind::Shield => Rank::Wall,
        Kind::Skier => Rank::Flank,
        Kind::Slinger => Rank::Back,
        Kind::Warmer | Kind::Sledder => Rank::Rear,
        _ => Rank::Line,
    }
}

/// Enemies on every side of `at` rather than off one way: their directions
/// mostly cancel out.
fn surrounded(at: Vec2, foes: &[Vec2]) -> bool {
    let dirs: Vec<Vec2> = foes.iter().filter(|f| f.dist(at) > 0.5).map(|&f| (f - at).norm()).collect();
    if dirs.len() < 3 {
        return false;
    }
    let sum = dirs.iter().fold(Vec2::ZERO, |a, &b| a + b);
    sum.len() / (dirs.len() as f32) < 0.4
}

/// Places across the line from `at`, `across` to the side and `fwd` ahead.
fn spot(at: Vec2, ahead: Vec2, across: f32, fwd: f32) -> Vec2 {
    at + ahead * fwd + v2(-ahead.y, ahead.x) * across
}

/// `n` places in staggered ranks `width` wide, the first rank `fwd` ahead.
fn ranks(at: Vec2, ahead: Vec2, n: usize, width: usize, gap: f32, fwd: f32) -> Vec<Vec2> {
    (0..n)
        .map(|k| {
            let (row, col) = (k / width, k % width);
            let in_row = (n - row * width).min(width);
            let across = (col as f32 - (in_row as f32 - 1.0) / 2.0) * gap;
            // Every other rank is staggered, so the back can throw through the front.
            let stagger = if row % 2 == 1 { gap * 0.5 } else { 0.0 };
            spot(at, ahead, across + stagger, fwd - row as f32 * ARMY_SPACING)
        })
        .collect()
}

/// The army in a line facing `ahead`, one list of places per rank.
fn line_places(at: Vec2, ahead: Vec2, groups: &[Vec<u16>; 5]) -> Vec<Vec<Vec2>> {
    let n = |r: Rank| groups[r as usize].len();
    let line = n(Rank::Line);
    // The wall: pairs side by side, a gap between pairs.
    let walls = n(Rank::Wall);
    let wall_span = |k: usize| (k / 2) as f32 * (WALL_PAIR + WALL_GAP) + (k % 2) as f32 * WALL_PAIR;
    let wall_width = if walls > 0 { wall_span(walls - 1) } else { 0.0 };
    let wall: Vec<Vec2> = (0..walls).map(|k| spot(at, ahead, wall_span(k) - wall_width / 2.0, WALL_AHEAD)).collect();
    // Throwers as wide as the wall, so it covers them.
    let width = ((line as f32 * 2.0).sqrt().ceil() as usize).max(3).max((wall_width / ARMY_SPACING) as usize + 1);
    let body = ranks(at, ahead, line, width, ARMY_SPACING, 1.5);
    let rows = line.div_ceil(width).max(1);
    let half = ((width.min(line.max(1)) as f32 - 1.0) / 2.0 * ARMY_SPACING).max(wall_width / 2.0);
    // Skiers out to both sides, level with the front.
    let flank: Vec<Vec2> = (0..n(Rank::Flank))
        .map(|k| {
            let side = if k % 2 == 0 { 1.0 } else { -1.0 };
            spot(at, ahead, side * (half + 2.0 + (k / 2) as f32 * 1.5), 1.5 - (k / 2) as f32 * 0.8)
        })
        .collect();
    // Slingers just behind the throwers, spread wider; then everyone else.
    let behind = 1.5 - rows as f32 * ARMY_SPACING - 0.4;
    let back_n = n(Rank::Back);
    let back = ranks(at, ahead, back_n, ((back_n as f32 * 2.0).sqrt().ceil() as usize).max(3), 2.2, behind);
    let back_rows = back_n.div_ceil(3).max(if back_n > 0 { 1 } else { 0 });
    let rear = ranks(at, ahead, n(Rank::Rear), 3, ARMY_SPACING, behind - back_rows as f32 * ARMY_SPACING - 0.4);
    vec![wall, body, flank, back, rear]
}

/// The army in a ring about `at`, for when enemies are all round: shields
/// outermost, throwers just inside, everyone else in the middle.
fn ring_places(at: Vec2, ahead: Vec2, groups: &[Vec<u16>; 5]) -> Vec<Vec<Vec2>> {
    use std::f32::consts::TAU;
    let n = |r: Rank| groups[r as usize].len();
    let walls = n(Rank::Wall);
    let throwers = n(Rank::Line) + n(Rank::Flank);
    let start = ahead.y.atan2(ahead.x);
    let circle = |r: f32, k: usize, of: usize| at + Vec2::from_angle(start + TAU * k as f32 / of.max(1) as f32) * r;
    let wall_r = (walls as f32 * WALL_PAIR * 1.2 / TAU).max(WALL_AHEAD);
    let wall: Vec<Vec2> = (0..walls).map(|k| circle(wall_r, k, walls)).collect();
    // Rings from just inside the wall inwards, then outwards past it if
    // there are too many, filled in order: throwers, Skiers, Slingers, the rest.
    let first = if walls > 0 { wall_r - 1.1 } else { (throwers as f32 * ARMY_SPACING / TAU).max(2.2) };
    let mut radii: Vec<f32> = Vec::new();
    let mut r = first;
    while r >= 1.0 {
        radii.push(r);
        r -= ARMY_SPACING;
    }
    let mut r = first.max(wall_r) + ARMY_SPACING;
    while radii.len() < 12 {
        radii.push(r);
        r += ARMY_SPACING;
    }
    let mut seats = radii.iter().flat_map(|&r| {
        let fit = ((TAU * r / ARMY_SPACING) as usize).max(1);
        (0..fit).map(move |k| (r, k, fit))
    });
    let mut take = |count: usize| -> Vec<Vec2> { (0..count).filter_map(|_| seats.next()).map(|(r, k, fit)| circle(r, k, fit)).collect() };
    let line = take(n(Rank::Line));
    let flank = take(n(Rank::Flank));
    let back = take(n(Rank::Back));
    let rear = take(n(Rank::Rear));
    vec![wall, line, flank, back, rear]
}
