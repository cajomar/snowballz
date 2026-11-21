//! Buildings. Workers put them up a step at a time, quarrying a layer of
//! snow from the ground around the site for every step (so building digs
//! the land bare), and the wood is carried to the site by the builders.
//! Snowballs wear buildings down; walls crumble back into snow.

use super::*;

pub const NO_BUILDING: u16 = u16::MAX;
/// Building ids are never reused, so a game can hold only this many in all.
pub const MAX_BUILDINGS: usize = 60000;
/// Most unfinished building sites one tribe may have at once.
pub const MAX_OPEN_SITES: usize = 64;
/// Time for one construction or repair step.
pub const BUILD_TICKS: u16 = secs(1.4);
/// Snow is quarried from tiles this close to a site.
pub const QUARRY_RADIUS: i32 = 3;
/// Warming huts warm friendly penguins this close by.
pub const SAUNA_RADIUS: f32 = 3.5;
/// Throwers a watchtower holds, and how much farther they throw from it.
pub const TOWER_SLOTS: usize = 4;
pub const TOWER_RANGE: f32 = 3.5;
/// How high the roof stands over the ground, for drawing and throwing.
pub const TOWER_DECK: f32 = 1.6;
/// A garrisoned thrower gets a snowball handed up this often.
pub const TOWER_RESTOCK: u32 = secs(2.0) as u32;
/// Builders look this far from a site for a tree to fetch its wood from.
pub const WOOD_FETCH_RADIUS: i32 = 20;
/// Workers who finish a site move on to unfinished ones this close by.
pub const BUILD_CONTINUE_RADIUS: f32 = 10.0;
/// A new igloo keeps at least this far (centre to centre) from every other.
pub const IGLOO_SPACING: f32 = 30.0;
/// Igloo ids are a byte, and some are kept free.
pub const MAX_IGLOOS: usize = 200;

/// The centre of an igloo site whose top-left tile is `tile`.
pub fn igloo_site_center(tile: (i32, i32)) -> Vec2 {
    v2(tile.0 as f32 + IGLOO_SIZE as f32 / 2.0, tile.1 as f32 + IGLOO_SIZE as f32 / 2.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum BuildKind {
    /// Warms friendly penguins nearby; cold ones run here instead of home.
    Sauna = 0,
    /// Workers drop fish, wood and crystal here instead of walking home.
    Store = 4,
    /// Training camp: workers come out Shieldbearers.
    Workshop = 6,
    /// Training camp: workers come out Sled Lancers.
    Track = 7,
    /// Training camp: workers come out Warmers.
    Spa = 8,
    /// A taller, tougher snow wall built in a line.
    Palisade = 9,
    Campfire = 10,
    /// Trains skiers for fast travel across snow.
    SkiLodge = 12,
    /// Trains long-range Slingers.
    SlingRange = 14,
    /// Trains heavy Packer throwers.
    PackerYard = 15,
    /// Defenders stand on its roof: out of reach until it falls.
    Tower = 17,
    /// Bunks for ten more snowballers (the `armies` rule).
    Bunkhouse = 19,
    /// A new igloo going up (the `armies` rule): once built it is a real
    /// igloo of the tribe, with its own Chieftain, guards and fishing.
    Igloo = 20,
    /// A Watchtower built up (the `armies` rule): workers crew a catapult on
    /// its roof that lobs big snowballs at enemy buildings far off.
    Catapult = 21,
}

impl BuildKind {
    pub const ALL: [BuildKind; 14] = [
        BuildKind::Sauna,
        BuildKind::Store,
        BuildKind::Workshop,
        BuildKind::Track,
        BuildKind::Spa,
        BuildKind::Palisade,
        BuildKind::Campfire,
        BuildKind::SkiLodge,
        BuildKind::SlingRange,
        BuildKind::PackerYard,
        BuildKind::Tower,
        BuildKind::Bunkhouse,
        BuildKind::Igloo,
        BuildKind::Catapult,
    ];

    pub fn from_u8(v: u8) -> BuildKind {
        match v {
            4 => BuildKind::Store,
            6 => BuildKind::Workshop,
            7 => BuildKind::Track,
            8 => BuildKind::Spa,
            9 => BuildKind::Palisade,
            10 => BuildKind::Campfire,
            12 => BuildKind::SkiLodge,
            14 => BuildKind::SlingRange,
            15 => BuildKind::PackerYard,
            17 => BuildKind::Tower,
            19 => BuildKind::Bunkhouse,
            20 => BuildKind::Igloo,
            21 => BuildKind::Catapult,
            _ => BuildKind::Sauna,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            BuildKind::Sauna => "Sauna",
            BuildKind::Store => "Sushi House",
            BuildKind::Workshop => "Ice Workshop",
            BuildKind::Track => "Sled Track",
            BuildKind::Spa => "Warming Spa",
            BuildKind::Palisade => "Snow Palisade",
            BuildKind::Campfire => "Campfire",
            BuildKind::SkiLodge => "Ski Lodge",
            BuildKind::SlingRange => "Sling Range",
            BuildKind::PackerYard => "Packer Yard",
            BuildKind::Tower => "Watchtower",
            BuildKind::Catapult => "Catapult Tower",
            BuildKind::Bunkhouse => "Bunkhouse",
            BuildKind::Igloo => "Igloo",
        }
    }
    pub fn blurb(self) -> &'static str {
        match self {
            BuildKind::Sauna => "Penguins bathe here to warm up. A worker must tend the fire, which burns wood: with armies it calls its own stoker, who fetches logs from the trees nearby.",
            BuildKind::Store => "Built by the water, it has fishing slots (2, upgraded to 4 then 8): each calls in an idle worker to fish the banks beside it. Workers drop off fish, wood and crystal here too.",
            BuildKind::Workshop => "Workers sent here train into Shieldbearers.",
            BuildKind::Track => "Workers sent here train into Sled Lancers.",
            BuildKind::Spa => "Workers sent here train into Warmers.",
            BuildKind::Palisade => "A tall, sturdy snow wall. Drag to build a line.",
            BuildKind::Campfire => "A log fire: penguins warm only right beside it. Burns out after a minute, unless (with armies) its stoker keeps fetching logs.",
            BuildKind::SkiLodge => "Workers sent here train into Skier raiders.",
            BuildKind::SlingRange => "Workers sent here train into long-range Slingers.",
            BuildKind::PackerYard => "Workers sent here train into heavy Packers.",
            BuildKind::Tower => "Four throwers stand on its roof and throw far. Enemies must knock it down to reach them.",
            BuildKind::Catapult => "A Watchtower built up: three workers crew a catapult that lobs big snowballs at enemy buildings far off.",
            BuildKind::Bunkhouse => "Bunks for snowballers: each empty one drafts an idle worker into a Snowballer.",
            BuildKind::Igloo => "A new igloo, a good way (30 tiles) from any other. Once built it raises its own Chieftain, posts guards and has fishing slots. Its builders live there.",
        }
    }
    /// What it does: camps fill their own slots, Sushi Houses take no wood
    /// and towers man themselves.
    pub fn describe(self) -> String {
        if self == BuildKind::Bunkhouse {
            return format!(
                "Bunks for snowballers ({}, built up to {} then {}). Each empty bunk drafts an idle worker into a Snowballer, for fish.",
                BUNK_SLOTS[0], BUNK_SLOTS[1], BUNK_SLOTS[2]
            );
        }
        if let Some(kind) = self.trains() {
            return format!(
                "Houses {} ({} slot, built up to {} then {}). Each empty slot calls in an idle worker, who mines a crystal and trains here.",
                kind.stats().plural,
                CAMP_SLOTS[0],
                CAMP_SLOTS[1],
                CAMP_SLOTS[2]
            );
        }
        match self {
            BuildKind::Store => "Built by the water, it has fishing slots (2, built up to 4 then 8): each calls in an idle worker to fish the banks beside it. Workers drop off fish and crystal here too.".into(),
            BuildKind::Catapult => format!(
                "Three workers crew its catapult, which lobs a big snowball at an enemy building or igloo up to {} tiles off every few seconds (slower with fewer crew). It picks the nearest by itself; right-click one to aim at it. If its crew all freeze, anyone's workers can take it over.",
                CATAPULT_RANGE as u32
            ),
            BuildKind::Tower => format!("Sees far over the fog. Up to four soldiers stand on its roof and throw far; {TOWER_SENTRIES} come from your armies by themselves. Enemies must knock it down to reach them."),
            BuildKind::Sauna => "A campfire built up: penguins bathe here to warm up, six at once. Its fire burns wood: it calls its own stoker, who fetches logs from the trees nearby.".into(),
            BuildKind::Campfire => "A log fire: penguins warm only right beside it. Its stoker keeps fetching logs to keep it burning. Build it up into a Sauna, where six bathe at once.".into(),
            _ => self.blurb().to_string(),
        }
    }

    /// The slots it has when first built (with armies): fishers, trainees,
    /// bunks or roof places; and how much each step up takes.
    pub fn slot_sizes(self) -> Option<(&'static [u8], &'static [(u8, u8)])> {
        match self {
            BuildKind::Store => Some((&FISH_SLOTS, &FISH_UPGRADE)),
            BuildKind::Bunkhouse => Some((&BUNK_SLOTS, &CAMP_UPGRADE)),
            BuildKind::Tower => Some((&[TOWER_SLOTS as u8], &[])),
            BuildKind::Catapult => Some((&[CATAPULT_CREW], &[])),
            k if k.trains().is_some() => Some((&CAMP_SLOTS, &CAMP_UPGRADE)),
            _ => None,
        }
    }
    pub fn first_slots(self) -> u8 {
        self.slot_sizes().map_or(1, |(s, _)| s[0])
    }
    /// A new building fills all its first slots, except a tower, which
    /// posts a couple of sentries and leaves the rest for you to send.
    pub fn first_want(self) -> u8 {
        if self == BuildKind::Tower {
            TOWER_SENTRIES as u8
        } else {
            self.first_slots()
        }
    }
    /// The next size up from `slots` and the work it takes, if there is one.
    pub fn next_slots(self, slots: u8) -> Option<(u8, (u8, u8))> {
        let (sizes, work) = self.slot_sizes()?;
        let k = sizes.iter().position(|&s| s == slots)?;
        Some((*sizes.get(k + 1)?, *work.get(k)?))
    }

    /// Made of snow, so it slumps in the thaw.
    pub fn snowy(self) -> bool {
        matches!(self, BuildKind::Sauna | BuildKind::Palisade)
    }
    /// Width and height in tiles.
    pub fn size(self) -> i32 {
        match self {
            BuildKind::Sauna | BuildKind::Store | BuildKind::Workshop | BuildKind::Track | BuildKind::Spa | BuildKind::SkiLodge | BuildKind::SlingRange | BuildKind::PackerYard | BuildKind::Tower | BuildKind::Catapult | BuildKind::Bunkhouse => 2,
            BuildKind::Igloo => IGLOO_SIZE,
            _ => 1,
        }
    }
    /// Construction steps; each one quarries a layer of snow.
    pub fn snow(self) -> u8 {
        match self {
            BuildKind::Sauna => 6,
            BuildKind::Palisade => 5,
            BuildKind::Campfire => 1,
            BuildKind::Tower | BuildKind::Catapult => 6,
            BuildKind::Store | BuildKind::Bunkhouse => 4,
            BuildKind::Workshop | BuildKind::Track | BuildKind::Spa | BuildKind::SkiLodge | BuildKind::SlingRange | BuildKind::PackerYard => 5,
            BuildKind::Igloo => 16,
        }
    }
    /// Wood the builders carry to the site.
    pub fn wood(self) -> u16 {
        match self {
            BuildKind::Sauna => 4,
            // Snowballs must be trainable on maps without trees too.
            BuildKind::Palisade => 0,
            BuildKind::Campfire => 1,
            BuildKind::SkiLodge => 8,
            BuildKind::SlingRange | BuildKind::PackerYard => 8,
            BuildKind::Tower | BuildKind::Catapult => 4,
            BuildKind::Store => 6,
            BuildKind::Bunkhouse => 4,
            BuildKind::Workshop | BuildKind::Track => 8,
            BuildKind::Spa => 6,
            BuildKind::Igloo => 6,
        }
    }
    pub fn max_hp(self) -> u8 {
        match self {
            BuildKind::Sauna => 14,
            BuildKind::Palisade => 18,
            BuildKind::Campfire => 4,
            BuildKind::Tower | BuildKind::Catapult => 30,
            BuildKind::Store | BuildKind::Bunkhouse => 12,
            BuildKind::Workshop | BuildKind::Track | BuildKind::Spa | BuildKind::SkiLodge | BuildKind::SlingRange | BuildKind::PackerYard => 12,
            BuildKind::Igloo => 20,
        }
    }
    /// Offered to build. The Sauna is a campfire built up, the Catapult a
    /// Watchtower built up; the rest are built from scratch.
    pub fn offered(self) -> bool {
        !matches!(self, BuildKind::Sauna | BuildKind::Catapult)
    }

    pub fn trains(self) -> Option<Kind> {
        match self {
            BuildKind::Bunkhouse => Some(Kind::Snowballer),
            BuildKind::Workshop => Some(Kind::Shield),
            BuildKind::Track => Some(Kind::Sledder),
            BuildKind::Spa => Some(Kind::Warmer),
            BuildKind::SkiLodge => Some(Kind::Skier),
            BuildKind::SlingRange => Some(Kind::Slinger),
            BuildKind::PackerYard => Some(Kind::Packer),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Building {
    pub id: u16,
    pub kind: BuildKind,
    pub owner: u8,
    /// Top-left tile of the footprint.
    pub tile: (i32, i32),
    pub hp: u8,
    /// Construction steps done; finished at `kind.snow()`.
    pub built: u8,
    /// Wood carried to the site so far; building only goes as far as it covers.
    pub wood_in: u8,
    pub alive: bool,
    /// Saunas: ticks of fuel in the fire, whether it is lit, and the tick a
    /// worker last tended it.
    pub fuel: u16,
    pub lit: bool,
    pub tended: u32,
    /// No snow left nearby to build or repair with.
    pub starved: bool,
    /// Camps: where trained penguins walk to once they are done.
    pub rally: Option<Vec2>,
    /// Buildings with slots (the `armies` rule): camps, Bunkhouses, Sushi
    /// Houses and towers. How many it has room for...
    pub slots: u8,
    /// ...and how many of them the tribe wants filled. Building up adds
    /// room, never fills it by itself.
    pub want: u8,
    /// More slots being built on (with armies).
    pub upgrading: Option<BuildingUpgrade>,
    /// Catapults: ticks until the next shot.
    pub reload: u16,
}

/// Work still to do on a building's upgrade: builders carry in the logs
/// first, then build the steps, and the building has `to` slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingUpgrade {
    pub steps: u8,
    pub wood: u8,
    pub to: u8,
    /// What it becomes once built up, if not more of the same.
    pub into: Option<BuildKind>,
}

impl Building {
    pub fn done(&self) -> bool {
        self.built >= self.kind.snow()
    }
    /// A site where no work has been done yet is only staked out: it doesn't
    /// block the way or stop snowballs, and nobody throws at it. It becomes
    /// solid with the first step of building.
    pub fn solid(&self) -> bool {
        self.built > 0
    }
    /// Steps the wood delivered so far pays for.
    pub fn steps_allowed(&self) -> u8 {
        let (snow, wood) = (self.kind.snow() as u16, self.kind.wood() as u16);
        if wood == 0 {
            return snow as u8;
        }
        (snow * self.wood_in.min(wood as u8) as u16 / wood) as u8
    }
    /// Still waiting for wood to be carried in.
    pub fn needs_wood(&self) -> bool {
        (!self.done() && self.wood_in < self.kind.wood() as u8) || self.upgrading.is_some_and(|u| u.wood > 0)
    }
    /// No step can be built until more wood comes.
    pub fn waiting_for_wood(&self) -> bool {
        (!self.done() && self.built >= self.steps_allowed()) || self.upgrading.is_some_and(|u| u.wood > 0)
    }
    /// Going up, or being built on: builders have work here.
    pub fn is_site(&self) -> bool {
        !self.done() || self.upgrading.is_some()
    }
    /// A log carried in.
    fn take_log(&mut self) {
        match &mut self.upgrading {
            Some(u) if u.wood > 0 => u.wood -= 1,
            _ => self.wood_in += 1,
        }
    }
    pub fn center(&self) -> Vec2 {
        let s = self.kind.size() as f32 / 2.0;
        v2(self.tile.0 as f32 + s, self.tile.1 as f32 + s)
    }
    pub fn tiles(&self) -> impl Iterator<Item = (i32, i32)> {
        footprint(self.kind, self.tile)
    }
    /// Distance from `p` to the nearest edge of the footprint.
    pub fn dist_to(&self, p: Vec2) -> f32 {
        let s = self.kind.size() as f32;
        let (x0, y0) = (self.tile.0 as f32, self.tile.1 as f32);
        let dx = (x0 - p.x).max(p.x - (x0 + s)).max(0.0);
        let dy = (y0 - p.y).max(p.y - (y0 + s)).max(0.0);
        (dx * dx + dy * dy).sqrt()
    }
}

pub fn footprint(kind: BuildKind, tile: (i32, i32)) -> impl Iterator<Item = (i32, i32)> {
    let s = kind.size();
    (0..s * s).map(move |i| (tile.0 + i % s, tile.1 + i / s))
}

impl Game {
    pub fn building_at(&self, x: i32, y: i32) -> Option<&Building> {
        if !self.map.in_bounds(x, y) {
            return None;
        }
        match self.building_at[self.map.idx(x, y)] {
            NO_BUILDING => None,
            b => Some(&self.buildings[b as usize]),
        }
    }

    /// True if snowballs stop at this tile (trees, buildings, and with the
    /// new core, frozen penguins).
    pub fn blocks_snowballs(&self, x: i32, y: i32) -> bool {
        self.map.obstacle_at(x, y) == Obstacle::Tree
            || self.building_at(x, y).is_some_and(|b| b.solid())
            || (self.frozen_at(x, y))
    }

    /// Wood across all of a player's igloos.
    pub fn wood_of(&self, player: u8) -> u32 {
        self.igloos.iter().filter(|ig| ig.owner == player).map(|ig| ig.wood as u32).sum()
    }

    /// Penguins with a tribe count toward its food capacity.
    pub fn population(&self, player: u8) -> usize {
        self.penguins.iter().filter(|p| p.owner == player && p.act != Act::Frozen).count()
    }

    /// Fish across the tribe's igloos.
    pub fn fish_of(&self, player: u8) -> u32 {
        self.igloos.iter().filter(|ig| ig.owner == player).map(|ig| ig.fish as u32).sum()
    }

    /// Spend pooled fish, richest igloo first.
    pub(super) fn spend_fish(&mut self, player: u8, mut n: u16) -> bool {
        if self.fish_of(player) < n as u32 {
            return false;
        }
        while n > 0 {
            let Some(ig) = self.igloos.iter_mut().filter(|ig| ig.owner == player).max_by_key(|ig| ig.fish) else { return false };
            let take = ig.fish.min(n);
            ig.fish -= take;
            n -= take;
        }
        true
    }

    /// Take wood from a player's igloos, richest first. False if they can't afford it.
    pub(super) fn spend_wood(&mut self, player: u8, mut n: u16) -> bool {
        if self.wood_of(player) < n as u32 {
            return false;
        }
        while n > 0 {
            let Some(ig) = self.igloos.iter_mut().filter(|ig| ig.owner == player).max_by_key(|ig| ig.wood) else { return false };
            let take = ig.wood.min(n);
            ig.wood -= take;
            n -= take;
        }
        true
    }

    /// A free top-left tile for `kind` near `p` (spiral search), if any.
    pub fn find_site(&self, player: u8, kind: BuildKind, p: Vec2, max_r: i32) -> Option<(i32, i32)> {
        let (cx, cy) = p.tile();
        for r in 0..=max_r {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let t = (cx + dx, cy + dy);
                    if self.placement_problem(player, kind, t).is_none() {
                        return Some(t);
                    }
                }
            }
        }
        None
    }

    /// Why a building can't go here, or None if it can.
    pub fn placement_problem(&self, _player: u8, kind: BuildKind, tile: (i32, i32)) -> Option<&'static str> {
        // Building ids are never reused, so a game has only so many in it.
        if self.buildings.len() >= MAX_BUILDINGS {
            return Some("too many buildings");
        }
        for (x, y) in footprint(kind, tile) {
            if !self.map.in_bounds(x, y) {
                return Some("off the map");
            }
            if !self.grid.walkable(x, y) || self.building_at(x, y).is_some() {
                return Some("something is in the way");
            }
            let c = v2(x as f32 + 0.5, y as f32 + 0.5);
            if self.igloos.iter().any(|ig| ig.door.dist(c) < 1.6) {
                return Some("too close to an igloo door");
            }
            if self.resources.iter().any(|r| r.pos.dist(c) < 1.2) {
                return Some("too close to a fish hole or crystal");
            }
        }
        // A new igloo goes a good way from every other, and so never on an
        // enemy's doorstep.
        if kind == BuildKind::Igloo {
            let c = igloo_site_center(tile);
            let near = |at: Vec2| at.dist(c) < IGLOO_SPACING;
            if self.igloos.iter().any(|ig| near(ig.center)) || self.buildings.iter().any(|b| b.alive && b.kind == BuildKind::Igloo && near(b.center())) {
                return Some("too close to another igloo");
            }
            if self.igloos.len() >= MAX_IGLOOS {
                return Some("too many igloos");
            }
        }
        None
    }

    pub(super) fn place_buildings(&mut self, player: u8, units: Vec<u16>, kind: BuildKind, tiles: Vec<(i32, i32)>) {
        if !kind.offered() {
            return;
        }
        // Somebody has to build it, and a tribe can only have so many
        // unfinished sites going at once.
        let units: Vec<u16> = units.into_iter().filter(|&u| self.controllable(player, u)).collect();
        // A tribe may place sites with nobody picked: its foreman sends a
        // crew. Otherwise the picked workers build.
        let crewed = self.penguins.iter().any(|p| p.owner == player && p.kind == Kind::Worker);
        if !crewed && !units.iter().any(|&u| self.penguins[u as usize].kind == Kind::Worker) {
            return;
        }
        let mut open = self.buildings.iter().filter(|b| b.alive && b.owner == player && b.built < b.kind.snow()).count();
        let mut placed = Vec::new();
        for tile in tiles.into_iter().take(64) {
            if open >= MAX_OPEN_SITES || self.placement_problem(player, kind, tile).is_some() {
                continue;
            }
            open += 1;
            placed.push(self.add_building(player, kind, tile, false));
        }
        let Some(&first) = placed.first() else { return };
        let mut fighters = Vec::new();
        for u in units {
            if self.penguins[u as usize].kind == Kind::Worker {
                self.give_order(u, Order::Build { building: first });
            } else {
                fighters.push(u);
            }
        }
        if !fighters.is_empty() {
            let at = self.buildings[first as usize].center();
            self.command(player, Command::Move { units: fighters, target: at, run: false, queue: false });
        }
    }

    /// For looking at the art: one building of every kind round the
    /// player's first igloo (walls and palisades as short lines), where
    /// there is room, `built` (0..1) of the way up. Not used in play.
    pub fn showcase(&mut self, player: u8, built: f32) {
        let Some(door) = self.igloos.iter().find(|i| i.owner == player).map(|i| i.door) else { return };
        for kind in BuildKind::ALL {
            // (An igloo site can't go near home, and looks like the igloo.)
            if kind == BuildKind::Igloo {
                continue;
            }
            'find: for r in 3..14 {
                for k in 0..24 {
                    let t = (door + Vec2::from_angle(k as f32 / 24.0 * std::f32::consts::TAU) * r as f32).tile();
                    let line = if kind == BuildKind::Palisade { 4 } else { 1 };
                    let tiles: Vec<(i32, i32)> = (0..line).map(|d| (t.0 + d, t.1)).collect();
                    if tiles.iter().all(|&t| self.placement_problem(player, kind, t).is_none()) {
                        for t in tiles {
                            let b = self.add_building(player, kind, t, true) as usize;
                            let b = &mut self.buildings[b];
                            b.built = ((kind.snow() as f32 * built).ceil() as u8).clamp(1, kind.snow());
                        }
                        break 'find;
                    }
                }
            }
        }
    }

    pub(super) fn add_building(&mut self, owner: u8, kind: BuildKind, tile: (i32, i32), finished: bool) -> u16 {
        let id = self.buildings.len() as u16;
        let mut b = Building { id, kind, owner, tile, hp: 1, built: 0, wood_in: 0, alive: true, fuel: 0, lit: false, tended: 0, starved: false, rally: None, slots: kind.first_slots(), want: kind.first_want(), upgrading: None, reload: 0 };
        if finished {
            b.built = kind.snow();
            b.wood_in = kind.wood() as u8;
            b.hp = kind.max_hp_with(self.effects_of(owner));
        }
        let tiles: Vec<(i32, i32)> = b.tiles().collect();
        let solid = b.solid();
        self.buildings.push(b);
        for (x, y) in tiles {
            let i = self.map.idx(x, y);
            self.building_at[i] = id;
            self.terrain.mark(i);
        }
        if solid {
            self.make_solid(id);
        }
        self.events.push(Event::Placed { building: id });
        id
    }

    /// The site's tiles stop walkers, and anyone standing on it steps aside
    /// (blocks of ice stay).
    pub(super) fn make_solid(&mut self, id: u16) {
        let tiles: Vec<(i32, i32)> = self.buildings[id as usize].tiles().collect();
        for (x, y) in tiles {
            let i = self.map.idx(x, y);
            self.set_walk(i, false);
        }
        for p in &mut self.penguins {
            if p.inside.is_none() && p.blocks.is_none() && !self.grid.walkable_at(p.pos) {
                if let Some((x, y)) = self.grid.nearest_walkable(p.pos.tile(), 4) {
                    p.pos = v2(x as f32 + 0.5, y as f32 + 0.5);
                    p.path.clear();
                    p.goal = None;
                }
            }
        }
    }

    pub(super) fn repair(&mut self, player: u8, units: Vec<u16>, building: u16) {
        let Some(b) = self.buildings.get(building as usize) else { return };
        if !b.alive || self.team(b.owner) != self.team(player) {
            return;
        }
        for u in units {
            if self.controllable(player, u) && self.penguins[u as usize].kind == Kind::Worker {
                self.give_order(u, Order::Build { building });
            }
        }
    }

    pub(super) fn demolish(&mut self, player: u8, building: u16) {
        if self.buildings.get(building as usize).is_some_and(|b| b.alive && b.owner == player) {
            self.destroy(building as usize);
        }
    }

    pub(super) fn destroy(&mut self, b: usize) {
        self.buildings[b].alive = false;
        self.eject_garrison(b as u16);
        let kind = self.buildings[b].kind;
        let tiles: Vec<(i32, i32)> = self.buildings[b].tiles().collect();
        for (x, y) in tiles {
            let i = self.map.idx(x, y);
            self.building_at[i] = NO_BUILDING;
            self.set_walk(i, true);
            self.terrain.mark(i);
            // Walls crumble back into the snow they were made of.
            if kind == BuildKind::Palisade {
                let s = (self.terrain.snow[i] + 2).min(SNOW_DEPTH + 2);
                self.set_snow(i, s);
            }
        }
        self.events.push(Event::BuildingLost { building: b as u16 });
    }

    /// A snowball hit a building.
    pub fn damage_building(&mut self, b: usize, dmg: u8) {
        let hp = self.buildings[b].hp.saturating_sub(dmg);
        self.buildings[b].hp = hp;
        if hp == 0 {
            self.destroy(b);
        }
    }

    /// A builder whose site is short of wood fetches a log: it chops the
    /// nearest tree and carries the log to the site, or takes one from the
    /// tribe's store when there are no trees about. True while it is at that.
    fn site_wood_job(&mut self, i: usize, building: u16) -> bool {
        let b = &self.buildings[building as usize];
        let (owner, carry, pos) = (self.penguins[i].owner, self.penguins[i].carry, self.penguins[i].pos);
        if !b.alive || self.team(b.owner) != self.team(owner) || !b.needs_wood() {
            return false;
        }
        let (center, blocked) = (b.center(), b.waiting_for_wood());
        if carry == Carry::Wood {
            let Some(spot) = self.build_spot(building, pos) else { return false };
            if self.travel(i, spot, 0.3) {
                self.buildings[building as usize].take_log();
                let p = &mut self.penguins[i];
                p.carry = Carry::None;
                p.facing = (center - p.pos).angle();
            }
            return true;
        }
        // Others are building; fetch only when the work has run dry.
        if carry != Carry::None || !blocked {
            return false;
        }
        let tree = self.penguins[i].chop_tree.filter(|&t| self.terrain.wood[t as usize] > 0).or_else(|| {
            self.tree_walkable_from(center, WOOD_FETCH_RADIUS).map(|(x, y)| self.map.idx(x, y) as u32)
        });
        if let Some(t) = tree {
            self.chop_tree_job(i, t);
            return true;
        }
        if self.wood_of(owner) == 0 {
            return false;
        }
        let door = self.igloos[self.penguins[i].home as usize].door;
        if self.travel(i, door, 0.8) && self.spend_wood(owner, 1) {
            self.penguins[i].carry = Carry::Wood;
        }
        true
    }

    /// Worker job loop for building or repairing.
    pub(super) fn build_job(&mut self, i: usize, building: u16) {
        if self.site_wood_job(i, building) || self.deliver(i) {
            return;
        }
        let b = &self.buildings[building as usize];
        let team = self.team(self.penguins[i].owner);
        let needs_work = b.alive && self.team(b.owner) == team && (b.is_site() || b.hp < self.max_hp(b));
        if !needs_work && self.penguins[i].crew {
            self.penguins[i].order = self.crew_next(i);
            return;
        }
        if !needs_work {
            // Move on to the next unfinished site nearby, if any.
            let pos = self.penguins[i].pos;
            let owner = self.penguins[i].owner;
            let next = self
                .buildings
                .iter()
                .filter(|o| o.alive && o.owner == owner && o.is_site() && !o.starved && o.dist_to(pos) < BUILD_CONTINUE_RADIUS)
                .min_by(|a, c| a.dist_to(pos).partial_cmp(&c.dist_to(pos)).unwrap())
                .map(|o| o.id);
            self.penguins[i].order = match next {
                Some(n) => Order::Build { building: n },
                None => Order::None,
            };
            return;
        }
        // A new site waits for its wood; the next step is covered by a log.
        let b = &self.buildings[building as usize];
        if b.waiting_for_wood() {
            return;
        }
        let pos = self.penguins[i].pos;
        let Some(spot) = self.build_spot(building, pos) else {
            self.penguins[i].order = Order::None;
            return;
        };
        if self.travel(i, spot, 0.3) {
            let c = self.buildings[building as usize].center();
            let p = &mut self.penguins[i];
            p.act = Act::Build;
            p.act_t = 0;
            p.anim_seq = p.anim_seq.wrapping_add(1);
            p.facing = (c - p.pos).angle();
            p.build_site = Some(building);
        }
    }

    /// A walkable spot next to the footprint, nearest to `from`.
    pub(super) fn build_spot(&self, building: u16, from: Vec2) -> Option<Vec2> {
        let b = &self.buildings[building as usize];
        self.edge_spot(b.tile, b.kind.size(), from)
    }

    /// A walkable tile next to the `s` x `s` square at `tile`, nearest `from`.
    pub(super) fn edge_spot(&self, tile: (i32, i32), s: i32, from: Vec2) -> Option<Vec2> {
        let (x0, y0) = tile;
        let mut best: Option<(f32, Vec2)> = None;
        for y in y0 - 1..=y0 + s {
            for x in x0 - 1..=x0 + s {
                let edge = x == x0 - 1 || y == y0 - 1 || x == x0 + s || y == y0 + s;
                if !edge || !self.grid.walkable(x, y) {
                    continue;
                }
                let p = v2(x as f32 + 0.5, y as f32 + 0.5);
                let d = p.dist2(from);
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, p));
                }
            }
        }
        best.map(|(_, p)| p)
    }

    /// Take one layer of snow from the ground near a building. False if there's none.
    fn quarry(&mut self, building: usize) -> bool {
        self.quarry_at(self.buildings[building].center())
    }

    /// Take one layer of snow from the ground near `c`. False if there's none.
    pub(super) fn quarry_at(&mut self, c: Vec2) -> bool {
        let (cx, cy) = c.tile();
        let mut best: Option<(f32, usize)> = None;
        for y in cy - QUARRY_RADIUS..=cy + QUARRY_RADIUS {
            for x in cx - QUARRY_RADIUS..=cx + QUARRY_RADIUS {
                if !self.map.in_bounds(x, y) || self.building_at(x, y).is_some() || self.igloo_on(x, y).is_some() {
                    continue;
                }
                let i = self.map.idx(x, y);
                if self.terrain.snow[i] == 0 || self.map.obstacle[i].blocks() {
                    continue;
                }
                // Dig the deepest, closest snow first so pits grow evenly.
                let d = v2(x as f32 + 0.5, y as f32 + 0.5).dist(c) - self.terrain.snow[i] as f32 * 0.6;
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, i));
                }
            }
        }
        let Some((_, i)) = best else { return false };
        let s = self.terrain.snow[i];
        self.set_snow(i, s - 1);
        true
    }

    /// One construction or repair step finished.
    pub(super) fn build_step(&mut self, i: usize) {
        let Some(b) = self.penguins[i].build_site.take() else { return };
        let bi = b as usize;
        if !self.buildings[bi].alive {
            return;
        }
        // Several builders may start a step at once; the wood covers only so many.
        if self.buildings[bi].waiting_for_wood() {
            return;
        }
        if !self.quarry(bi) {
            self.buildings[bi].starved = true;
            self.penguins[i].order = Order::None;
            return;
        }
        let fx = self.effects_of(self.buildings[bi].owner);
        let bd = &mut self.buildings[bi];
        bd.starved = false;
        let max = bd.kind.max_hp_with(fx);
        if let Some(u) = &mut bd.upgrading {
            u.steps = u.steps.saturating_sub(1);
            if u.steps == 0 {
                let into = u.into;
                bd.slots = u.to;
                bd.upgrading = None;
                if let Some(kind) = into {
                    bd.kind = kind;
                    bd.built = kind.snow();
                    bd.want = kind.first_want();
                    bd.rally = None;
                    // The sentries climb down to make room on the roof.
                    self.eject_garrison(b);
                }
                self.events.push(Event::Built { building: b });
            }
        } else if bd.done() {
            bd.hp = (bd.hp + 3).min(max);
        } else {
            bd.built += 1;
            if bd.built == 1 {
                self.make_solid(b);
            }
            let bd = &mut self.buildings[bi];
            let steps = bd.kind.snow();
            bd.hp = ((max as u32 * bd.built as u32 / steps as u32) as u8).max(1);
            if bd.done() && bd.kind == BuildKind::Igloo {
                self.found_igloo(bi);
                return;
            }
            let bd = &mut self.buildings[bi];
            if bd.done() {
                bd.hp = max;
                if bd.kind == BuildKind::Campfire {
                    // Lit as soon as it is built, and it burns down by itself.
                    bd.fuel = CAMPFIRE_TICKS;
                    bd.lit = true;
                }
                self.events.push(Event::Built { building: b });
            }
        }
    }

    /// A finished igloo site becomes a real igloo of its tribe (the site's
    /// tiles stay shut, now as the igloo's), and its builders live there. It
    /// raises its own Chieftain from them.
    fn found_igloo(&mut self, b: usize) {
        let (owner, tile) = (self.buildings[b].owner, self.buildings[b].tile);
        self.buildings[b].alive = false;
        let id = self.igloos.len() as u8;
        let mut igloo = Igloo::new(id, tile, igloo_site_center(tile), igloo_door(&self.grid, tile));
        igloo.owner = owner;
        igloo.fish = 0;
        igloo.chief_at = self.tick;
        self.igloos.push(igloo);
        self.map.igloos.push((tile.0 as u16, tile.1 as u16));
        for p in self.penguins.iter_mut().filter(|p| p.owner == owner && p.order == (Order::Build { building: b as u16 })) {
            p.home = id;
        }
        self.events.push(Event::Built { building: b as u16 });
    }

    /// The nearest finished friendly building of a kind (with its distance).
    pub(super) fn nearest_building(&self, owner: u8, kind: BuildKind, p: Vec2) -> Option<(f32, usize)> {
        let team = self.team(owner);
        self.buildings
            .iter()
            .filter(|b| b.alive && b.done() && b.kind == kind && self.team(b.owner) == team)
            .map(|b| (b.dist_to(p), b.id as usize))
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
    }

    /// Extra throwing range from standing on a watchtower's roof.
    pub(super) fn tower_bonus(&self, owner: u8, p: Vec2) -> f32 {
        if self.on_tower(owner, p) {
            TOWER_RANGE
        } else {
            0.0
        }
    }
}
