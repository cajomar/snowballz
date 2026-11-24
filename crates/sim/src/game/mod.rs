//! Authoritative game state and rules.
//!
//! Rules in brief (see README for the player-facing version):
//! * Workers fish holes and banks anywhere they can reach and bring
//!   fish home. Fish warm penguins; newcomers swim in for fish.
//! * Snowballers automatically throw at enemies in range, scoop new snowballs
//!   while standing on snow, and are more accurate up close and from high ground.
//! * Hits cost warmth. Cold penguins run home to warm up; at zero warmth they
//!   freeze solid for a while first.
//! * A warm snowballer entering an enemy igloo starts a takeover. If no warm
//!   defender gets inside before the timer runs out, the igloo and
//!   every penguin that lives there switch sides.
//! * The ground wears down. Every snow tile holds a few layers of snow:
//!   scooping snowballs and heavy foot traffic use them up until bare ice
//!   shows (quick to walk on, useless for ammo), and snowfall slowly brings
//!   them back. Workers chop trees for wood, leaving stumps and new paths.

use crate::ai::{Ai, AiLevel, Directive};
use crate::map::{Ground, Map, Obstacle, ResourceKind, IGLOO_SIZE};
use crate::math::{turn_towards, v2, Vec2};
use crate::path::{Grid, PathFinder};
use crate::rng::Rng;
use crate::{secs, DT};

mod build;
mod camp;
mod core;
mod frozen;
mod heroes;
mod armies;
mod jobs;
mod longnight;
mod morale;
mod outpost;
mod research;
mod roles;
mod tactics;
mod sea;
mod sauna;
mod season;
mod shore;
mod siege;
mod terrain;
mod tower;
mod tiers;
mod catapult;
pub use catapult::*;
mod warmer;
mod units;
mod warmth;
mod wildlife;
pub use build::*;
pub use core::*;
pub use frozen::*;
pub use heroes::*;
pub use armies::*;
pub use outpost::{fish_upgrade, Fishery, FISH_SLOTS, FISH_UPGRADE, IGLOO_FISH_SLOTS, STORE_REACH};
pub use camp::{camp_upgrade, BUNK_SLOTS, CAMP_SLOTS, CAMP_UPGRADE, CRYSTAL_REGROW_TICKS, ELITE_TRAIN_TICKS, TRAIN_REACH};
pub use longnight::*;
pub use morale::*;
pub use research::*;
pub use sauna::*;
pub use season::*;
pub use shore::*;
pub use siege::*;
pub use terrain::{initial_snow, Terrain};
pub use tiers::*;
pub use units::*;
pub use warmer::*;
pub use warmth::*;
pub use wildlife::*;
pub use tactics::{falls_back, DODGE_CHANCE, STEP_UP};
pub use sea::{SEA_CALL_SECONDS, SEA_RECRUIT_FISH};

pub const NEUTRAL: u8 = 255;
pub const MAX_PLAYERS: usize = 10;
/// Most new paths planned in one tick: bigger armies take a few ticks to set off.
pub const PLANS_PER_TICK: u16 = 40;

/// Warmth at or below which a penguin gives up and runs home.
pub const COLD: u8 = 4;
/// Snowballs a penguin holds: the one it has just scooped.
pub const MAX_SNOWBALLS: u8 = 1;
pub const THROW_TICKS: u16 = secs(0.5);
pub const THROW_RELEASE: u16 = secs(0.3);
pub const SCOOP_TICKS: u16 = secs(1.6);
pub const FISH_TICKS: u16 = secs(3.5);
pub const MINE_TICKS: u16 = secs(12.0);
pub const UNLOAD_TICKS: u16 = secs(0.8);
pub const FROZEN_TICKS: u16 = secs(8.0);
pub const WARM_TICKS: u16 = secs(1.2);
pub const TAKEOVER_TICKS: u16 = secs(25.0);
pub const NEUTRAL_TAKEOVER_TICKS: u16 = secs(10.0);
pub const IGLOO_POP_CAP: usize = 40;
pub const DIRECT_RADIUS: f32 = 0.45;
pub const ALERT_RADIUS: f32 = 14.0;
/// Layers of snow on an untouched snow tile.
pub const SNOW_DEPTH: u8 = 4;
/// Penguin-ticks of walking over a tile that pack one layer of snow to ice.
pub const TRAMPLE_TICKS: u16 = secs(9.0);
/// Each worn tile regains a layer with this chance every second.
pub const SNOWFALL_CHANCE: f32 = 1.0 / 50.0;
pub const CHOP_TICKS: u16 = secs(2.5);
/// Loads of wood in a tree.
pub const TREE_WOOD: u8 = 3;
/// With armies a felled tree grows back after this long...
pub const TREE_REGROW_TICKS: u32 = secs(180.0) as u32;
/// ...or, if something is in the way, tries again after this.
pub const TREE_RETRY_TICKS: u32 = secs(20.0) as u32;
/// Workers told to chop keep clearing trees this close to where they were sent.
pub const CHOP_RADIUS: f32 = 4.5;
pub const PLAYER_COLORS: [[u8; 3]; MAX_PLAYERS] = [
    [220, 50, 47],   // red
    [38, 110, 230],  // blue
    [60, 170, 60],   // green
    [240, 200, 30],  // yellow
    [150, 70, 200],  // purple
    [245, 130, 30],  // orange
    [30, 190, 200],  // cyan
    [240, 110, 180], // pink
    [120, 80, 50],   // brown
    [60, 60, 70],    // charcoal
];

/// What a penguin is visibly doing; drives animation on clients.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Act {
    Idle = 0,
    Walk = 1,
    Throw = 2,
    Scoop = 3,
    Fish = 4,
    Mine = 5,
    Unload = 6,
    Inside = 7,
    Frozen = 8,
    Chop = 9,
    Build = 10,
    /// A Warmer blowing hot air on someone.
    Warm = 11,
    /// Bathing in a lit sauna.
    Bathe = 12,
    /// Knocked flat by a Skier's charge.
    Stunned = 13,
}

impl Act {
    pub fn from_u8(v: u8) -> Act {
        match v {
            1 => Act::Walk,
            2 => Act::Throw,
            3 => Act::Scoop,
            4 => Act::Fish,
            5 => Act::Mine,
            6 => Act::Unload,
            7 => Act::Inside,
            8 => Act::Frozen,
            9 => Act::Chop,
            10 => Act::Build,
            11 => Act::Warm,
            12 => Act::Bathe,
            13 => Act::Stunned,
            _ => Act::Idle,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Carry {
    None = 0,
    Fish = 1,
    Crystal = 2,
    Wood = 3,
}

impl Carry {
    pub fn from_u8(v: u8) -> Carry {
        match v {
            1 => Carry::Fish,
            2 => Carry::Crystal,
            3 => Carry::Wood,
            _ => Carry::None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PowerKind {
    /// +4 max warmth and direct hits hurt less.
    Defense = 0,
    /// Every throw launches two snowballs.
    Double = 1,
}

impl PowerKind {
    pub fn from_u8(v: u8) -> PowerKind {
        if v == 1 {
            PowerKind::Double
        } else {
            PowerKind::Defense
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Order {
    /// Hang around the home igloo (workers: fish automatically).
    None,
    /// Walk somewhere. `run` ignores enemies; `auto` orders come from alerts.
    Move { target: Vec2, run: bool, auto: bool },
    /// Stay near a spot, fighting anything in range.
    Hold { pos: Vec2 },
    /// Go into an igloo: rehome, defend, or take it over.
    Enter { igloo: u8 },
    /// Workers: gather from a specific resource.
    Gather { resource: u16 },
    /// Workers: chop down the trees around a tile.
    Chop { at: (i32, i32) },
    /// Workers: build or repair a building.
    Build { building: u16 },
    /// Workers: stand by a sauna and keep its fire fed.
    Tend { building: u16 },
    /// Throwers: climb onto a watchtower's roof and defend from there.
    Garrison { building: u16 },
    /// Pick up a frozen enemy and carry it to the nearest igloo.
    Capture { penguin: u16 },
    /// Warmers: thaw a frozen penguin into the tribe.
    Thaw { penguin: u16 },
    /// Workers: train at a camp until they are its specialist.
    Train { building: u16 },
}

#[derive(Clone, Debug)]
pub struct Penguin {
    pub id: u16,
    pub kind: Kind,
    pub owner: u8,
    pub home: u8,
    pub pos: Vec2,
    pub facing: f32,
    pub act: Act,
    pub act_t: u16,
    /// Bumped whenever a one-shot animation (throw, scoop...) restarts.
    pub anim_seq: u8,
    pub warmth: u8,
    pub max_warmth: u8,
    pub snowballs: u8,
    pub carry: Carry,
    pub power: Option<u8>,
    pub inside: Option<u8>,
    pub cold: bool,
    pub order: Order,
    /// Where to walk after the current move order: queued waypoints and whether to run.
    waypoints: Vec<(Vec2, bool)>,
    pub target: Option<u16>,
    /// 0..=MAX_COURAGE; at 0 the penguin flees.
    pub courage: u8,
    /// Fleeing to safety; won't take orders until brave again.
    pub routed: bool,
    /// The frozen penguin this one is carrying off.
    pub carrying: Option<u16>,
    /// Who's carrying this (frozen) penguin.
    pub carried_by: Option<u16>,
    /// The surfaced seal this penguin is throwing at.
    seal_aim: Option<u8>,
    /// Aiming at an enemy igloo or building (the `siege` rule).
    wall_aim: Option<Vec2>,
    /// Prisoners: ticks until they change sides.
    pub prison_t: u16,
    /// Warmth taken by blizzard wind; given back once the storm passes.
    pub chill: u8,
    /// Who it belonged to before it froze (new core: freezing ends ownership).
    pub was: u8,
    /// Thawed out with no tribe: anyone may persuade it to join.
    pub wild: bool,
    /// Ticks towards thawing out by itself.
    thaw_t: u16,
    /// Ticks spent training at a camp.
    train_t: u16,
    /// The tile (index) this frozen penguin holds as a block of ice. Stays
    /// put while frozen even if the penguin is somehow moved.
    blocks: Option<usize>,
    /// The tribe persuading this wild penguin, and for how long.
    persuade: Option<(u8, u16)>,
    /// Warmers: who they are warming or thawing.
    pub tend: Option<u16>,
    /// Ticks spent out in the cold; every period of them costs a warmth.
    expose_t: u16,
    /// Warmth taken by the cold; given back in shelter.
    frost: u8,
    /// Ticks left of a War Cry's speed and throwing bonus.
    pub cry: u16,
    /// Ticks left lying flat after a Skier's charge.
    pub stun: u16,
    /// Skiers: ticks until it can charge again.
    pub charge_cd: u16,
    /// Chieftains: ticks until the next War Cry.
    pub cry_cd: u16,
    /// Workers: preferred resource kind when choosing what to gather.
    pub prefer: ResourceKind,
    pub vel: Vec2,
    // Navigation.
    goal: Option<Vec2>,
    path: Vec<Vec2>,
    path_i: usize,
    stuck_t: u16,
    repaths: u8,
    cooldown: u16,
    wander_t: u16,
    gather_res: Option<u16>,
    /// The sauna being bathed in, and how long the bath has run.
    bath_at: Option<u16>,
    bath_t: u16,
    /// Workers: the bank (index into `Game::shores`) being fished.
    shore: Option<u16>,
    /// The bank was picked by hand: stay on it even when a hole is free.
    shore_pin: bool,
    /// On its job because the tribe's foreman sent it (see [`jobs`]), so it
    /// may be moved to another.
    crew: bool,
    /// The job it was pulled off to build, to go back to afterwards.
    back_to: Option<Order>,
    /// Its place in its Chieftain's ranks, while it follows him.
    slot: Option<Vec2>,
    /// The camp whose slot it fills (the `armies` rule): trained there, or
    /// on its way to train.
    pub camp: Option<u16>,
    /// The fishing slot it fills (the `armies` rule).
    pub fishery: Option<Fishery>,
    /// A recruit on its way to its first meal (the `armies` rule): it serves
    /// nobody and wears no colours until it has eaten at its igloo.
    pub dining: bool,
    /// On the roof of this watchtower, if any.
    pub post: Option<u16>,
    /// Workers: the tree (tile index) being chopped.
    chop_tree: Option<u32>,
    /// Sidestepping a snowball: ticks left, and which way (`tactics.rs`).
    dodge_t: u16,
    dodge_dir: Vec2,
    /// Damage already on its way to it, thrown at it and still in the air.
    incoming: u8,
    /// Its owner has been told it is cold with no fire in reach.
    told_cold: bool,
    /// Workers: the building the current build step is for.
    build_site: Option<u16>,
}

impl Penguin {
    pub fn is_out(&self) -> bool {
        self.inside.is_none()
    }
    pub fn panicking(&self, game: &Game) -> bool {
        self.kind.soldier()
            && !self.cold
            && !self.routed
            && game.igloos[self.home as usize]
                .takeover
                .is_some_and(|t| game.team(t.by) != game.team(self.owner))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Takeover {
    pub by: u8,
    pub ticks_left: u16,
    pub total: u16,
}

#[derive(Clone, Debug)]
pub struct Igloo {
    pub id: u8,
    pub tile: (i32, i32),
    pub center: Vec2,
    pub door: Vec2,
    pub owner: u8,
    pub fish: u16,
    pub crystal: u16,
    /// Burned to warm penguins inside before any fish is eaten.
    pub wood: u16,
    /// Igloo, Snow Hall or Ice Palace (see [`tiers`]).
    pub tier: u8,
    /// The upgrade under way, if any.
    pub upgrade: Option<Upgrade>,
    /// The research under way, if any.
    pub research: Option<Research>,
    pub takeover: Option<Takeover>,
    /// How much more pelting it can take (the `siege` rule).
    pub hp: u16,
    /// Captured enemy penguins waiting to change sides.
    pub prisoners: Vec<u16>,
    pub occupants: Vec<u16>,
    /// Not before this tick does the igloo raise a new Chieftain (armies rule).
    pub chief_at: u32,
    /// How many of its fishing slots the tribe wants filled (armies rule;
    /// see `outpost`), out of `IGLOO_FISH_SLOTS`.
    pub fish_want: u8,
}

impl Igloo {
    /// A new, empty igloo nobody holds yet.
    fn new(id: u8, tile: (i32, i32), center: Vec2, door: Vec2) -> Igloo {
        Igloo {
            id,
            tile,
            center,
            door,
            owner: NEUTRAL,
            fish: 3,
            crystal: 0,
            wood: 0,
            tier: TIER_IGLOO,
            upgrade: None,
            research: None,
            takeover: None,
            hp: igloo_max_hp(TIER_IGLOO),
            prisoners: Vec::new(),
            occupants: Vec::new(),
            chief_at: 0,
            fish_want: IGLOO_FISH_SLOTS,
        }
    }
}

/// Where the door of an igloo at `tile` goes: south when possible.
fn igloo_door(grid: &Grid, tile: (i32, i32)) -> Vec2 {
    let (ix, iy) = tile;
    let mid = IGLOO_SIZE / 2;
    let candidates = [(ix + mid, iy + IGLOO_SIZE), (ix + mid - 1, iy + IGLOO_SIZE), (ix + IGLOO_SIZE, iy + mid), (ix - 1, iy + mid), (ix + mid, iy - 1)];
    let door = candidates
        .iter()
        .copied()
        .find(|&(x, y)| grid.walkable(x, y))
        .or_else(|| grid.nearest_walkable((ix + mid, iy + IGLOO_SIZE), 6))
        .unwrap_or((ix + mid, iy + IGLOO_SIZE));
    v2(door.0 as f32 + 0.5, door.1 as f32 + 0.5)
}

#[derive(Clone, Debug)]
pub struct Resource {
    pub id: u16,
    pub tile: (i32, i32),
    pub pos: Vec2,
    pub kind: ResourceKind,
    pub amount: u8,
    pub max: u8,
    /// Walkable tile centers next to the resource where workers stand.
    pub spots: Vec<Vec2>,
}

#[derive(Clone, Debug)]
pub struct Powerup {
    pub id: u8,
    pub kind: PowerKind,
    pub pos: Vec2,
    pub holder: Option<u16>,
}

#[derive(Clone, Debug)]
pub struct Snowball {
    pub id: u32,
    pub from: Vec2,
    pub from_z: f32,
    pub to: Vec2,
    pub to_z: f32,
    pub start_tick: u32,
    pub flight: u16,
    pub thrower: u16,
    pub team: u8,
    /// The thrower's kind: decides how hard and wide it hits.
    pub kind: Kind,
    /// Lobbed by a catapult: wrecks buildings.
    pub big: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HitKind {
    Miss = 0,
    Splash = 1,
    Direct = 2,
    Tree = 3,
    Building = 4,
}

impl HitKind {
    pub fn from_u8(v: u8) -> HitKind {
        match v {
            1 => HitKind::Splash,
            2 => HitKind::Direct,
            3 => HitKind::Tree,
            4 => HitKind::Building,
            _ => HitKind::Miss,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Event {
    /// A penguin threw a snowball.
    Thrown(Snowball),
    /// A snowball came down at `pos`; `hit` says what it struck.
    Landed { id: u32, pos: Vec2, hit: HitKind },
    /// A penguin was frozen solid by a snowball hit.
    Frozen { penguin: u16 },
    /// A tribe began taking the igloo over.
    TakeoverStart { igloo: u8, by: u8 },
    /// An igloo was pelted until it fell in; it's an empty ruin now.
    Collapsed { igloo: u8, owner: u8 },
    /// A Skier's charge knocked a penguin flat...
    KnockedDown { penguin: u16, by: u16 },
    /// ...or it skied into a Shieldbearer's slab and went down itself.
    Bounced { skier: u16, shield: u16 },
    /// A cold penguin found no lit sauna or campfire in reach and is
    /// walking all the way home to warm up. Once per cold spell.
    NoFire { penguin: u16 },
    /// A newcomer swam in from the sea to fill a slot (with armies).
    SwamIn { penguin: u16 },
    /// Workers took over an enemy catapult nobody crewed.
    TakenOver { building: u16, from: u8, by: u8 },
    /// A worker finished training at a camp.
    Trained { building: u16, penguin: u16 },
    /// The takeover of the igloo ended without a capture.
    TakeoverFailed { igloo: u8 },
    /// The igloo changed hands from `from` to `to`.
    Captured { igloo: u8, from: u8, to: u8 },
    /// A frozen penguin woke up, joining `by` (or wild, for [`NEUTRAL`]).
    Thawed { penguin: u16, by: u8 },
    /// A wild penguin joined the tribe that kept it company.
    Persuaded { penguin: u16, by: u8 },
    /// A penguin picked up a powerup.
    PowerupTaken { powerup: u8, penguin: u16 },
    /// The tree on the tile was chopped down.
    TreeFelled { tile: (i32, i32) },
    /// A building was placed at the site.
    Placed { building: u16 },
    /// A building was finished.
    Built { building: u16 },
    /// A building was destroyed or torn down.
    BuildingLost { building: u16 },
    /// A penguin broke, fleeing the fight.
    Routed { penguin: u16 },
    /// A penguin was caught and is being carried off.
    PickedUp { penguin: u16, by: u16 },
    /// A carrier dropped its captive in the snow.
    Dropped { penguin: u16 },
    /// A penguin was locked up in the igloo's prison.
    Imprisoned { penguin: u16, igloo: u8 },
    /// A prisoner changed sides to `to`.
    Converted { penguin: u16, to: u8 },
    /// A prisoner was set free at the door.
    Released { penguin: u16 },
    /// The season turned to `season`.
    SeasonChanged { season: Season },
    /// A blizzard began (or ended).
    Blizzard { on: bool },
    /// An igloo finished being built up into a higher tier.
    Upgraded { igloo: u8, tier: u8 },
    /// A player's tribe finished a piece of research.
    ResearchDone { player: u8, branch: u8, level: u8 },
    /// A Chieftain let out its War Cry.
    WarCry { penguin: u16, pos: Vec2 },
    /// A leopard seal broke the surface looking for a lone penguin.
    SealSurfaced { seal: u8 },
    /// A seal dragged a penguin under; it comes back frozen.
    SealSnatched { seal: u8, penguin: u16 },
    /// A seal dived because snowballs hit it (`by` threw them) or a crowd
    /// scared it ([`NEUTRAL`]).
    SealDriven { seal: u8, by: u8 },
    /// A player is out of the game: it lost its last igloo.
    PlayerOut { player: u8 },
    /// The team won the match.
    Victory { team: u8 },
    /// A player sent a chat message to `to` (or everyone).
    Chat { from: u8, to: Option<u8>, text: String },
}

#[derive(Clone, Debug)]
pub struct PlayerSetup {
    pub name: String,
    pub color: u8,
    pub team: u8,
    pub ai: Option<AiLevel>,
}

#[derive(Clone, Debug)]
pub struct Player {
    pub id: u8,
    pub name: String,
    pub color: u8,
    pub team: u8,
    pub ai: Option<AiLevel>,
    pub out: bool,
    pub alerts: Vec<(Vec2, u16)>,
    /// What the tribe has learnt (see [`research`]).
    pub research: Levels,
}

#[derive(Clone, Debug)]
pub enum Command {
    /// Walk units to a point (run: sprint, queue: add the leg to the current route).
    Move { units: Vec<u16>, target: Vec2, run: bool, queue: bool },
    /// Send units into an igloo.
    Enter { units: Vec<u16>, igloo: u8 },
    /// Gather from a resource (workers gather, the others just walk over).
    Gather { units: Vec<u16>, resource: u16 },
    /// Chop down the tree on a tile (workers chop, the others just walk over).
    Chop { units: Vec<u16>, tile: (i32, i32) },
    /// Workers stand by a finished sauna and keep its fire fed.
    Tend { units: Vec<u16>, building: u16 },
    /// Throwers man a watchtower.
    Garrison { units: Vec<u16>, building: u16 },
    /// Give the game up: the tribe freezes and is out.
    Concede,
    /// Fish the bank nearest a tile of water (workers; the others just walk over).
    Fish { units: Vec<u16>, tile: (i32, i32) },
    /// Place building sites (several for walls) and send workers to build them.
    Build { units: Vec<u16>, kind: BuildKind, tiles: Vec<(i32, i32)> },
    /// Send workers to repair a building.
    Repair { units: Vec<u16>, building: u16 },
    /// Take a building down.
    Demolish { building: u16 },
    /// Carry off a frozen enemy.
    Capture { units: Vec<u16>, penguin: u16 },
    /// Let an igloo's prisoners go.
    Release { igloo: u8 },
    /// Cancel the units' orders: they stand where they are.
    Stop { units: Vec<u16> },
    /// Stand here: throw at what comes in range, but don't walk after it.
    Hold { units: Vec<u16> },
    /// Build an igloo up into the next tier.
    Upgrade { igloo: u8 },
    /// Start research in a branch (Snowcraft, Icecraft or Fishery) at an igloo.
    StartResearch { igloo: u8, branch: u8 },
    /// A Chieftain shouts its War Cry.
    WarCry { unit: u16 },
    /// Send Warmers to thaw a frozen penguin into the tribe.
    Thaw { units: Vec<u16>, penguin: u16 },
    /// Set where a camp sends its graduates (pointing at the camp clears it).
    SetRally { building: u16, at: Vec2 },
    /// Send a chat message to one player, or to everyone.
    Chat { to: Option<u8>, text: String },
    /// Builders build a building with slots (a camp, Bunkhouse or Sushi
    /// House) up to more of them (the `armies` rule).
    UpgradeBuilding { building: u16 },
    /// How many of a building's slots to fill (the `armies` rule): its
    /// fishers, trainees, bunks or sentries.
    SetWant { building: u16, want: u8 },
    /// How many of an igloo's fishing slots to fill.
    SetIglooFishers { igloo: u8, want: u8 },
}

/// Uniform grid of penguin ids for neighbour queries.
struct Buckets {
    w: i32,
    h: i32,
    size: f32,
    cells: Vec<Vec<u16>>,
}

const BUCKET: f32 = 2.0;
/// Target finding only needs the nearby cells, so its grid is coarse.
const TARGET_BUCKET: f32 = 8.0;

impl Buckets {
    fn new(map_w: i32, map_h: i32, size: f32) -> Buckets {
        let w = (map_w as f32 / size).ceil() as i32;
        let h = (map_h as f32 / size).ceil() as i32;
        Buckets { w, h, size, cells: vec![Vec::new(); (w * h) as usize] }
    }
    fn clear(&mut self) {
        self.cells.iter_mut().for_each(|c| c.clear());
    }
    fn cell(&self, p: Vec2) -> (i32, i32) {
        (
            ((p.x / self.size) as i32).clamp(0, self.w - 1),
            ((p.y / self.size) as i32).clamp(0, self.h - 1),
        )
    }
    fn insert(&mut self, p: Vec2, id: u16) {
        let (x, y) = self.cell(p);
        self.cells[(y * self.w + x) as usize].push(id);
    }
    fn query(&self, p: Vec2, r: f32, out: &mut Vec<u16>) {
        out.clear();
        let (x0, y0) = self.cell(p - v2(r, r));
        let (x1, y1) = self.cell(p + v2(r, r));
        for y in y0..=y1 {
            for x in x0..=x1 {
                out.extend_from_slice(&self.cells[(y * self.w + x) as usize]);
            }
        }
    }
}

pub struct Game {
    pub map: Map,
    pub tick: u32,
    pub players: Vec<Player>,
    pub penguins: Vec<Penguin>,
    pub igloos: Vec<Igloo>,
    pub resources: Vec<Resource>,
    pub powerups: Vec<Powerup>,
    pub snowballs: Vec<Snowball>,
    pub events: Vec<Event>,
    pub winner: Option<u8>,
    pub grid: Grid,
    /// Banks workers can fish from (see `shore.rs`).
    pub shores: Vec<Shore>,
    /// Where newcomers swim in from the sea (with armies; see `sea.rs`).
    pub sea: Vec<Vec2>,
    /// The tick the Long Night begins (see `longnight.rs`).
    pub long_night_at: u32,
    pub terrain: Terrain,
    pub buildings: Vec<Building>,
    /// Frozen penguins on each tile (new core: they block it).
    frozen_on_tile: Vec<u8>,
    /// Whether each tile was walkable before frozen penguins blocked it.
    frozen_walk: Vec<bool>,
    pub seals: Vec<Seal>,
    pub weather: Weather,
    /// The few switches this game plays by.
    pub core: CoreRules,
    /// Building id per tile, or [`NO_BUILDING`].
    building_at: Vec<u16>,
    pf: PathFinder,
    buckets: Buckets,
    /// Coarse grid for target finding, rebuilt each tick with `buckets`.
    targets: Buckets,
    scratch: Vec<u16>,
    /// Snowballs in the air thrown at a penguin: (ball, target, damage).
    aims: Vec<(u32, u16, u8)>,
    /// Paths that may still be planned this tick (see `travel`).
    plans_left: u16,
    rng: Rng,
    next_snowball: u32,
    ais: Vec<Ai>,
}

impl Game {
    pub fn new(map: Map, setups: Vec<PlayerSetup>, seed: u64) -> Game {
        Game::new_with(map, setups, seed, CoreRules::default())
    }

    /// A game with the given switches.
    pub fn new_with(mut map: Map, setups: Vec<PlayerSetup>, seed: u64, core: CoreRules) -> Game {
        // No fish holes: they become ponds to fish from the bank.
        map.holes_to_ponds();
        let mut rng = Rng::new(seed);
        let n = (map.w * map.h) as usize;
        let mut walk = vec![true; n];
        let mut cost = vec![1.0; n];
        for y in 0..map.h {
            for x in 0..map.w {
                let i = map.idx(x, y);
                walk[i] = !map.obstacle[i].blocks() && map.ground[i] != Ground::Water;
                if map.ground[i] == Ground::Ice {
                    cost[i] = 0.75;
                }
            }
        }
        for &(ix, iy) in &map.igloos {
            for y in iy as i32..iy as i32 + IGLOO_SIZE {
                for x in ix as i32..ix as i32 + IGLOO_SIZE {
                    if map.in_bounds(x, y) {
                        walk[map.idx(x, y)] = false;
                    }
                }
            }
        }
        for r in &map.resources {
            if map.in_bounds(r.x as i32, r.y as i32) {
                walk[map.idx(r.x as i32, r.y as i32)] = false;
            }
        }
        let grid = Grid { w: map.w, h: map.h, walk, cost };
        let shores = shore::find_shores(&map, &grid);

        let mut igloos = Vec::new();
        for (i, &(ix, iy)) in map.igloos.iter().enumerate() {
            let tile = (ix as i32, iy as i32);
            igloos.push(Igloo::new(i as u8, tile, map.igloo_center(i), igloo_door(&grid, tile)));
        }

        let resources: Vec<Resource> = map
            .resources
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let (x, y) = (r.x as i32, r.y as i32);
                let mut spots = Vec::new();
                for (dx, dy) in [(0, 1), (1, 0), (-1, 0), (0, -1), (1, 1), (-1, 1), (1, -1), (-1, -1)] {
                    if grid.walkable(x + dx, y + dy) {
                        spots.push(v2(x as f32 + 0.5 + dx as f32 * 0.8, y as f32 + 0.5 + dy as f32 * 0.8));
                    }
                }
                Resource {
                    id: i as u16,
                    tile: (x, y),
                    pos: v2(x as f32 + 0.5, y as f32 + 0.5),
                    kind: r.kind,
                    amount: r.amount,
                    max: r.amount,
                    spots,
                }
            })
            .collect();

        let powerups = map
            .powerups
            .iter()
            .enumerate()
            .filter(|(_, &(_, x, y))| grid.walkable(x as i32, y as i32))
            .map(|(i, &(k, x, y))| Powerup {
                id: i as u8,
                kind: PowerKind::from_u8(k),
                pos: v2(x as f32 + 0.5, y as f32 + 0.5),
                holder: None,
            })
            .collect();

        let players: Vec<Player> = setups
            .iter()
            .take(MAX_PLAYERS.min(map.igloos.len()))
            .enumerate()
            .map(|(i, s)| Player {
                id: i as u8,
                name: s.name.clone(),
                color: s.color,
                team: s.team,
                ai: s.ai,
                out: false,
                alerts: Vec::new(),
                research: [0; BRANCHES],
            })
            .collect();

        let seals = wildlife::place_seals(&map, &grid, &igloos, &resources);
        let mut game = Game {
            terrain: Terrain::new(&map),
            buildings: Vec::new(),
            frozen_on_tile: vec![0; n],
            frozen_walk: vec![false; n],
            seals,
            weather: Weather::default(),
            core,
            building_at: vec![NO_BUILDING; n],
            buckets: Buckets::new(map.w, map.h, BUCKET),
            targets: Buckets::new(map.w, map.h, TARGET_BUCKET),
            pf: PathFinder::new(n),
            shores,
            sea: Vec::new(),
            long_night_at: LONG_NIGHT_START,
            grid,
            map,
            tick: 0,
            players,
            penguins: Vec::new(),
            igloos,
            resources,
            powerups,
            snowballs: Vec::new(),
            events: Vec::new(),
            winner: None,
            scratch: Vec::new(),
            aims: Vec::new(),
            plans_left: PLANS_PER_TICK,
            rng: Rng::new(rng.next_u64()),
            next_snowball: 1,
            ais: Vec::new(),
        };

        // Hand out igloos so players start as far apart as possible.
        let mut taken: Vec<usize> = Vec::new();
        for p in 0..game.players.len() {
            let pick = if taken.is_empty() {
                rng.below(game.igloos.len() as u32) as usize
            } else {
                (0..game.igloos.len())
                    .filter(|i| !taken.contains(i))
                    .max_by(|&a, &b| {
                        let d = |i: usize| {
                            taken
                                .iter()
                                .map(|&t| game.igloos[t].center.dist(game.igloos[i].center))
                                .fold(f32::MAX, f32::min)
                        };
                        d(a).partial_cmp(&d(b)).unwrap()
                    })
                    .unwrap()
            };
            taken.push(pick);
            game.igloos[pick].owner = p as u8;
        }

        game.sea = game.sea_beaches();
        let rules = game.map.rules.clone();
        for i in 0..game.igloos.len() {
            // Penguins aren't made, only won: an igloo nobody holds is a site
            // to found, not a barracks full of free penguins.
            if game.igloos[i].owner == NEUTRAL {
                continue;
            }
            // A tribe starts small: its Chieftain and two workers (or, without
            // the small start, a couple of soldiers and the rest of the map's
            // people as workers).
            let small = game.core.small_start;
            let soldiers = if small { 0 } else { rules.snowballers.min(START_SOLDIERS) };
            let workers = if small { START_WORKERS } else { rules.workers + (rules.snowballers - soldiers) };
            for _ in 0..workers {
                game.spawn_penguin(i as u8, Kind::Worker);
            }
            for _ in 0..soldiers {
                game.spawn_penguin(i as u8, Kind::Snowballer);
            }
            game.spawn_penguin(i as u8, Kind::Chieftain);
        }

        for p in &game.players {
            if let Some(level) = p.ai {
                game.ais.push(Ai::new(p.id, level));
            }
        }
        game
    }

    pub fn team(&self, player: u8) -> u8 {
        if player == NEUTRAL {
            NEUTRAL
        } else {
            self.players.get(player as usize).map_or(NEUTRAL, |p| p.team)
        }
    }

    /// True if `a`'s and `b`'s owners would fight each other.
    pub fn hostile(&self, a: u8, b: u8) -> bool {
        a != NEUTRAL && b != NEUTRAL && self.team(a) != self.team(b)
    }

    /// Most warmth a penguin of this kind can hold on this map.
    pub fn max_warmth_of(&self, kind: Kind) -> u8 {
        match kind {
            Kind::Worker => self.map.rules.worker_warmth,
            k => (self.map.rules.snowballer_warmth as f32 * k.stats().warmth).round().max(4.0) as u8,
        }
    }

    fn spawn_penguin(&mut self, igloo: u8, kind: Kind) -> u16 {
        let ig = &self.igloos[igloo as usize];
        let id = self.penguins.len() as u16;
        let count = self.penguins.iter().filter(|p| p.home == igloo).count();
        // Fan new penguins out in a ring in front of the door.
        let mut pos = ig.door;
        for attempt in 0..24 {
            let a = (count + attempt) as f32 * 2.4;
            let r = 0.8 + ((count + attempt) as f32).sqrt() * 0.55;
            let p = ig.door + Vec2::from_angle(a) * r;
            if self.grid.walkable_at(p) {
                pos = p;
                break;
            }
        }
        let max_warmth = self.max_warmth_of(kind);
        let owner = ig.owner;
        self.penguins.push(Penguin {
            id,
            kind,
            owner,
            home: igloo,
            pos,
            facing: std::f32::consts::FRAC_PI_2 + self.rng.range(-1.0, 1.0),
            act: Act::Idle,
            act_t: 0,
            anim_seq: 0,
            warmth: max_warmth,
            max_warmth,
            snowballs: self.effects_of(owner).ammo(kind),
            carry: Carry::None,
            power: None,
            inside: None,
            cold: false,
            order: Order::None,
            waypoints: Vec::new(),
            target: None,
            courage: MAX_COURAGE,
            routed: false,
            carrying: None,
            seal_aim: None,
            wall_aim: None,
            carried_by: None,
            prison_t: 0,
            chill: 0,
            was: owner,
            wild: false,
            thaw_t: 0,
            train_t: 0,
            blocks: None,
            persuade: None,
            tend: None,
            expose_t: 0,
            frost: 0,
            cry: 0,
            stun: 0,
            charge_cd: 0,
            cry_cd: 0,
            prefer: ResourceKind::Fish,
            vel: Vec2::ZERO,
            goal: None,
            path: Vec::new(),
            path_i: 0,
            stuck_t: 0,
            repaths: 0,
            cooldown: 0,
            wander_t: self.rng.below(100) as u16,
            gather_res: None,
            bath_at: None,
            bath_t: 0,
            shore: None,
            shore_pin: false,
            crew: false,
            back_to: None,
            slot: None,
            camp: None,
            fishery: None,
            dining: false,
            post: None,
            chop_tree: None,
            dodge_t: 0,
            dodge_dir: Vec2::ZERO,
            incoming: 0,
            told_cold: false,
            build_site: None,
        });
        id
    }

    // ---------------------------------------------------------------------
    // Commands

    fn controllable(&self, player: u8, id: u16) -> bool {
        match self.penguins.get(id as usize) {
            Some(p) => p.owner == player && !p.cold && !p.routed && !p.dining && p.act != Act::Frozen && !p.panicking(self),
            None => false,
        }
    }

    pub fn command(&mut self, player: u8, cmd: Command) {
        if self.players.get(player as usize).is_none_or(|p| p.out) {
            if let Command::Chat { to, text } = cmd {
                self.chat(player, to, text);
            }
            return;
        }
        match cmd {
            Command::Move { units, target, run, queue } => {
                // A target off the map is the nearest point on it.
                if !target.x.is_finite() || !target.y.is_finite() {
                    return;
                }
                let target = v2(target.x.clamp(0.5, self.map.w as f32 - 0.5), target.y.clamp(0.5, self.map.h as f32 - 0.5));
                let units: Vec<u16> =
                    units.into_iter().filter(|&u| self.controllable(player, u)).collect();
                let units = self.fall_in(units);
                let slots = self.formation(target, units.len());
                // Closest penguins take the slots nearest the target.
                let mut sorted = units.clone();
                sorted.sort_by(|&a, &b| {
                    let da = self.penguins[a as usize].pos.dist2(target);
                    let db = self.penguins[b as usize].pos.dist2(target);
                    da.partial_cmp(&db).unwrap()
                });
                for (u, slot) in sorted.into_iter().zip(slots) {
                    if queue {
                        // Shift-click: walk this leg once the current one is done.
                        self.penguins[u as usize].waypoints.push((slot, run));
                        if !matches!(self.penguins[u as usize].order, Order::Move { .. }) {
                            self.start_waypoints(u as usize);
                        }
                    } else {
                        self.penguins[u as usize].waypoints.clear();
                        self.give_order(u, Order::Move { target: slot, run, auto: false });
                    }
                }
            }
            Command::Enter { units, igloo } => {
                if igloo as usize >= self.igloos.len() {
                    return;
                }
                for u in units {
                    if self.controllable(player, u) && self.penguins[u as usize].is_out() {
                        self.give_order(u, Order::Enter { igloo });
                    }
                }
            }
            Command::Gather { units, resource } => {
                if resource as usize >= self.resources.len() {
                    return;
                }
                let kind = self.resources[resource as usize].kind;
                let pos = self.resources[resource as usize].pos;
                let mut fighters = Vec::new();
                for u in units {
                    if !self.controllable(player, u) {
                        continue;
                    }
                    if self.penguins[u as usize].kind == Kind::Worker {
                        self.penguins[u as usize].prefer = kind;
                        self.give_order(u, Order::Gather { resource });
                    } else {
                        fighters.push(u);
                    }
                }
                if !fighters.is_empty() {
                    self.command(player, Command::Move { units: fighters, target: pos, run: false, queue: false });
                }
            }
            Command::Fish { units, tile } => self.order_fish(player, units, tile),
            Command::Tend { units, building } => self.order_tend(player, units, building),
            Command::Garrison { units, building } => self.order_garrison(player, units, building),
            Command::Concede => self.concede(player),
            Command::Chop { units, tile } => {
                if self.map.obstacle_at(tile.0, tile.1) != Obstacle::Tree {
                    return;
                }
                let pos = v2(tile.0 as f32 + 0.5, tile.1 as f32 + 0.5);
                let mut fighters = Vec::new();
                for u in units {
                    if !self.controllable(player, u) {
                        continue;
                    }
                    if self.penguins[u as usize].kind == Kind::Worker {
                        self.give_order(u, Order::Chop { at: tile });
                    } else {
                        fighters.push(u);
                    }
                }
                if !fighters.is_empty() {
                    self.command(player, Command::Move { units: fighters, target: pos, run: false, queue: false });
                }
            }
            Command::Build { units, kind, tiles } => self.place_buildings(player, units, kind, tiles),
            Command::Repair { units, building } => self.repair(player, units, building),
            Command::Demolish { building } => self.demolish(player, building),
            Command::Capture { units, penguin } => self.order_capture(player, units, penguin),
            Command::Release { igloo } => self.release(player, igloo),
            Command::Stop { units } => {
                for u in units {
                    if self.controllable(player, u) {
                        self.penguins[u as usize].prefer = ResourceKind::Fish;
                        self.give_order(u, Order::None);
                    }
                }
            }
            Command::Hold { units } => {
                for u in units {
                    if self.controllable(player, u) {
                        let pos = self.penguins[u as usize].pos;
                        self.give_order(u, Order::Hold { pos });
                    }
                }
            }
            Command::Upgrade { igloo } => self.upgrade_igloo(player, igloo),
            Command::StartResearch { igloo, branch } => self.start_research(player, igloo, branch),
            Command::WarCry { unit } => self.war_cry(player, unit),
            Command::Thaw { units, penguin } => self.order_thaw(player, units, penguin),
            Command::SetRally { building, at } => {
                if self.buildings.get(building as usize).is_some_and(|b| b.kind == BuildKind::Catapult) {
                    self.aim_catapult(player, building, at);
                } else {
                    self.set_rally(player, building, at);
                }
            }
            Command::Chat { to, text } => self.chat(player, to, text),
            Command::UpgradeBuilding { building } => self.upgrade_building(player, building),
            Command::SetWant { building, want } => self.set_want(player, building, want),
            Command::SetIglooFishers { igloo, want } => {
                if let Some(ig) = self.igloos.get_mut(igloo as usize).filter(|ig| ig.owner == player) {
                    ig.fish_want = want.min(IGLOO_FISH_SLOTS);
                }
            }
        }
    }

    fn chat(&mut self, from: u8, to: Option<u8>, text: String) {
        let text: String = text.chars().filter(|c| !c.is_control()).take(200).collect();
        if text.trim().is_empty() {
            return;
        }
        // A private message to a seat that doesn't exist goes nowhere.
        if to.is_some_and(|t| t as usize >= self.players.len()) {
            return;
        }
        self.events.push(Event::Chat { from, to, text });
    }

    fn give_order(&mut self, id: u16, order: Order) {
        if !matches!(order, Order::Capture { .. }) {
            self.drop_captive(id);
        }
        self.leave_post(id as usize);
        let p = &mut self.penguins[id as usize];
        // Any order but a move leg drops whatever was queued behind it.
        if !matches!(order, Order::Move { .. }) {
            p.waypoints.clear();
        }
        p.order = order;
        p.goal = None;
        p.path.clear();
        p.repaths = 0;
        p.target = None;
        p.gather_res = None;
        p.shore = None;
        p.shore_pin = false;
        p.fishery = None;
        p.bath_at = None;
        if p.act == Act::Bathe {
            p.act = Act::Idle;
        }
        p.chop_tree = None;
        p.build_site = None;
        p.crew = false;
        p.back_to = None;
        // Running cancels whatever the penguin was busy with.
        let urgent = matches!(order, Order::Move { run: true, .. } | Order::Enter { .. });
        if (urgent || p.kind == Kind::Worker)
            && matches!(p.act, Act::Scoop | Act::Fish | Act::Mine | Act::Chop | Act::Build)
        {
            p.act = Act::Idle;
        }
    }

    /// Walk the next queued waypoint, or return false when the queue is empty.
    fn start_waypoints(&mut self, i: usize) -> bool {
        if self.penguins[i].waypoints.is_empty() {
            return false;
        }
        let (target, run) = self.penguins[i].waypoints.remove(0);
        self.give_order(i as u16, Order::Move { target, run, auto: false });
        true
    }

    /// Spread `n` destinations around `center` on walkable ground.
    fn formation(&self, center: Vec2, n: usize) -> Vec<Vec2> {
        let mut out = Vec::with_capacity(n);
        let spacing = 0.85;
        let mut ring = 0;
        if self.grid.walkable_at(center) {
            out.push(center);
        }
        while out.len() < n && ring < 40 {
            ring += 1;
            let r = ring as f32 * spacing;
            let count = (std::f32::consts::TAU * r / spacing).floor() as usize;
            for k in 0..count {
                if out.len() >= n {
                    break;
                }
                let a = k as f32 / count as f32 * std::f32::consts::TAU + ring as f32 * 0.5;
                let p = center + Vec2::from_angle(a) * r;
                if self.grid.walkable_at(p) {
                    out.push(p);
                }
            }
        }
        while out.len() < n {
            out.push(center);
        }
        out
    }

    // ---------------------------------------------------------------------
    // Simulation

    pub fn tick(&mut self) {
        self.tick += 1;
        self.plans_left = PLANS_PER_TICK;
        self.terrain.clear_dirty();
        if self.winner.is_none() {
            self.run_ai();
        }
        self.update_weather();
        self.update_igloos();
        self.update_igloo_repairs();
        self.update_long_night();
        self.update_upgrades();
        self.update_research();
                self.rebuild_buckets();
        self.update_jobs();
        self.update_armies();
        self.update_saunas();
        self.update_heroes();
        if self.tick.is_multiple_of(crate::TICK_RATE) {
            self.update_courage();
        }
        for i in 0..self.penguins.len() {
            self.update_penguin(i);
        }
        self.update_exposure();
        self.separate();
        for i in 0..self.penguins.len() {
            if self.penguins[i].carried_by.is_some() {
                self.follow_carrier(i);
            }
        }
        self.update_seals();
        self.update_frozen();
        self.update_wild();
        self.update_catapults();
        self.update_snowballs();
        self.update_powerups();
        if self.tick.is_multiple_of(crate::TICK_RATE) {
            self.snowfall();
            self.regrow_trees();
            self.regrow_crystal();
        }
        for p in &mut self.players {
            p.alerts.retain_mut(|(_, ttl)| {
                *ttl = ttl.saturating_sub(1);
                *ttl > 0
            });
        }
        if self.tick.is_multiple_of(10) {
            self.check_victory();
        }
    }

    fn run_ai(&mut self) {
        let mut ais = std::mem::take(&mut self.ais);
        let mut cmds = Vec::new();
        for ai in &mut ais {
            ai.update(self, &mut cmds);
        }
        // Offers between computers, said in public with their answers.
        let mut deals = Vec::new();
        for ai in &mut ais {
            if let Some(text) = ai.outbox_text.take() {
                deals.push((ai.player, text, std::mem::take(&mut ai.outbox)));
            }
        }
        self.ais = ais;
        // (Only in the logs: computers don't chat their stock lines.)
        for (from, text, offers) in deals {
            self.ai_think(from, None, text);
            for (to, offer) in offers {
                let mut ais = std::mem::take(&mut self.ais);
                let answer = ais.iter_mut().find(|a| a.player == to).map(|a| a.consider(self, from, offer));
                if let Some((Some(minutes), _)) = answer {
                    for a in ais.iter_mut() {
                        if a.player == to {
                            a.keep_peace(self, from, minutes);
                        } else if a.player == from {
                            a.keep_peace(self, to, minutes);
                        }
                    }
                }
                self.ais = ais;
                if let Some((_, reply)) = answer {
                    self.ai_think(to, None, reply);
                }
            }
        }
        for (player, cmd) in cmds {
            self.command(player, cmd);
        }
    }

    /// Steer a computer player. A target that is not a living enemy is
    /// dropped; returns false if `player` is not a computer.
    pub fn set_ai_directive(&mut self, player: u8, mut d: Directive) -> bool {
        d.target = d.target.filter(|&t| t != player && self.players.get(t as usize).is_some_and(|p| !p.out) && self.hostile(player, t));
        // A raid or siege on someone who can't be attacked is no plan.
        d.plan = d.plan.filter(|p| p.target().is_none_or(|t| d.target == Some(t)));
        match self.ais.iter_mut().find(|a| a.player == player) {
            Some(ai) => {
                if ai.directive != d {
                    let target = d.target.and_then(|t| self.players.get(t as usize)).map_or("nobody".to_string(), |p| p.name.clone());
                    let research = d.research.map_or("none in particular", crate::ai::branch_name);
                    ai.note(
                        self.tick,
                        format!("Advisor says {:?}; pick on {target}; more {}; learn {research}", d.posture, d.favour.map_or("of whatever", |k| k.stats().plural)),
                    );
                }
                if let Some(plan) = d.plan {
                    ai.advise_plan(plan, None);
                }
                ai.directive = d;
                true
            }
            None => false,
        }
    }

    /// Have a computer player take up a plan, saying `said` if given (else
    /// its own words). The plan's target must be a living enemy.
    pub fn set_ai_plan(&mut self, player: u8, plan: crate::ai::Plan, said: Option<String>) -> bool {
        if plan.target().is_some_and(|t| t == player || self.players.get(t as usize).is_none_or(|p| p.out) || !self.hostile(player, t)) {
            return false;
        }
        match self.ais.iter_mut().find(|a| a.player == player) {
            Some(ai) => {
                ai.advise_plan(plan, said);
                true
            }
            None => false,
        }
    }

    /// Give every computer player a character for this game, from `seed`:
    /// traits that change how it plays (see `ai::Temper`), a title that hints
    /// at the strongest ("Tux the Reckless"), and fresh dice, so no two games
    /// with the same seats play out alike. Without this they play plain.
    pub fn roll_ai_tempers(&mut self, seed: u64) {
        let mut rng = Rng::new(seed ^ 0x7e3a_5ca1_ab1e);
        for i in 0..self.ais.len() {
            let temper = crate::ai::Temper::roll(&mut rng);
            let player = self.ais[i].player;
            self.set_ai_temper(player, temper);
            self.ais[i].reseed(rng.next_u64());
        }
    }

    /// Set what a computer player is like, and title it to match. The title
    /// replaces a trailing " Bot" (and any earlier title).
    pub fn set_ai_temper(&mut self, player: u8, temper: crate::ai::Temper) {
        let Some(ai) = self.ais.iter_mut().find(|a| a.player == player) else { return };
        ai.temper = temper;
        let name = &mut self.players[player as usize].name;
        let base = name.split(" the ").next().unwrap_or(name).trim_end_matches(" Bot").to_string();
        *name = format!("{base} {}", temper.title());
    }

    /// How much computer player `ai` holds against `rival`, for tests and debugging.
    pub fn ai_rivalry(&self, ai: u8, rival: u8) -> f32 {
        self.ais.iter().find(|a| a.player == ai).map_or(0.0, |a| a.rivalry(rival))
    }

    /// What each computer player is thinking, as lines of text, for people
    /// watching a game (see `ServerMsg::AiNotes`).
    pub fn ai_notes(&self) -> Vec<(u8, Vec<String>)> {
        self.ais.iter().map(|a| (a.player, a.notes(self))).collect()
    }

    /// Each computer's summary, and its log from tick `since` on, for the
    /// admin page.
    pub fn ai_admin(&self, since: u32) -> Vec<(u8, Vec<String>, Vec<(u32, String)>)> {
        self.ais
            .iter()
            .map(|a| {
                let log = a.log();
                let from = log.partition_point(|(t, _)| *t < since);
                (a.player, a.notes_last(self, 0), log[from..].to_vec())
            })
            .collect()
    }

    /// Add a line to a computer player's log.
    pub fn ai_log(&mut self, player: u8, text: impl Into<String>) {
        let tick = self.tick;
        if let Some(ai) = self.ais.iter_mut().find(|a| a.player == player) {
            ai.note(tick, text);
        }
    }

    /// Whether computer player `ai` is holding off a person in the opening
    /// grace period (see `AiLevel::grace_minutes`).
    pub fn ai_grace_holds(&self, ai: u8, person: u8) -> bool {
        self.ais.iter().find(|a| a.player == ai).is_some_and(|a| a.grace_holds(self, person))
    }

    /// Per computer player: its number and the report to base a directive on.
    /// The advice a computer player is following, if it is a computer.
    pub fn ai_directive(&self, player: u8) -> Option<Directive> {
        self.ais.iter().find(|a| a.player == player).map(|a| a.directive)
    }

    pub fn ai_briefings(&self) -> Vec<(u8, String)> {
        self.ais.iter().filter(|a| !self.players[a.player as usize].out).map(|a| (a.player, a.briefing(self))).collect()
    }

    pub fn ai_debug(&self) -> Vec<String> {
        self.ais.iter().map(|a| a.debug()).collect()
    }

    /// Without a language model only binding answers (deals, questions
    /// about the game) are said; small talk stays in the log.
    pub fn deliver_chat_to_ai(&mut self, from: u8, to: Option<u8>, text: &str) {
        for (ai, r) in self.ai_replies_full(from, to, text) {
            if r.binding {
                self.ai_say(ai, r.to, r.text);
            } else {
                self.ai_think(ai, r.to, r.text);
            }
        }
    }

    /// What the computer players answer to a chat message, as (who, to whom,
    /// what). Their moods and deals change as they hear it; nothing is said
    /// until the caller passes the reply to `ai_say`.
    pub fn ai_replies(&mut self, from: u8, to: Option<u8>, text: &str) -> Vec<(u8, Option<u8>, String)> {
        self.ai_replies_full(from, to, text).into_iter().map(|(p, r)| (p, r.to, r.text)).collect()
    }

    /// As [`Game::ai_replies`], saying which replies are binding. A computer
    /// spoken to privately or named in the message always answers; a public
    /// remark gets small talk from one computer at most, so they don't all
    /// pile in at once.
    pub fn ai_replies_full(&mut self, from: u8, to: Option<u8>, text: &str) -> Vec<(u8, crate::ai::Reply)> {
        let low = text.to_lowercase();
        let named = |p: &Player| p.name.split_whitespace().next().is_some_and(|first| low.contains(&first.to_lowercase()));
        let mut ais = std::mem::take(&mut self.ais);
        let mut replies = Vec::new();
        for ai in &mut ais {
            if Some(ai.player) == to || (to.is_none() && ai.player != from) {
                let addressed = Some(ai.player) == to || self.players.get(ai.player as usize).is_some_and(named);
                if let Some(r) = ai.hear_full(self, from, text, addressed) {
                    replies.push((ai.player, r, addressed));
                }
            }
        }
        self.ais = ais;
        let named_any = replies.iter().any(|(_, _, a)| *a);
        let mut chatted = false;
        replies
            .into_iter()
            .filter(|(_, r, addressed)| {
                if r.binding || *addressed {
                    return true;
                }
                // Unaddressed small talk: one voice, and none if someone was named.
                let ok = !named_any && !chatted;
                chatted |= ok;
                ok
            })
            .map(|(p, r, _)| (p, r))
            .collect()
    }

    /// A stock line a computer would once have said: logged, not chatted.
    pub fn ai_think(&mut self, player: u8, to: Option<u8>, text: String) {
        self.ai_log(player, format!("Thinks{}: \"{text}\"", to.and_then(|t| self.players.get(t as usize)).map_or(String::new(), |p| format!(" to {}", p.name))));
    }

    /// A computer player says something.
    pub fn ai_say(&mut self, player: u8, to: Option<u8>, text: String) {
        self.ai_log(player, format!("Says{}: \"{text}\"", to.and_then(|t| self.players.get(t as usize)).map_or(String::new(), |p| format!(" to {}", p.name))));
        self.chat(player, to, text);
    }

    fn rebuild_buckets(&mut self) {
        self.buckets.clear();
        self.targets.clear();
        for p in &self.penguins {
            if p.inside.is_none() {
                self.buckets.insert(p.pos, p.id);
                self.targets.insert(p.pos, p.id);
            }
        }
    }

    fn update_igloos(&mut self) {
        for ig in 0..self.igloos.len() {
            // Takeover countdown.
            if let Some(mut t) = self.igloos[ig].takeover {
                let invaders = self.igloos[ig]
                    .occupants
                    .iter()
                    .any(|&o| self.team(self.penguins[o as usize].owner) == self.team(t.by));
                if !invaders {
                    self.igloos[ig].takeover = None;
                    self.events.push(Event::TakeoverFailed { igloo: ig as u8 });
                } else {
                    t.ticks_left = t.ticks_left.saturating_sub(1);
                    self.igloos[ig].takeover = Some(t);
                    if t.ticks_left == 0 {
                        self.capture(ig as u8, t.by);
                    }
                }
            }

            // Warm up occupants; let warm ones out.
            let occupants = self.igloos[ig].occupants.clone();
            let locked = self.igloos[ig].takeover.is_some();
            let owner = self.igloos[ig].owner;
            // A grander igloo has a bigger fire in it.
            let warm = tier(self.igloos[ig].tier).warm;
            for id in occupants {
                let p = &mut self.penguins[id as usize];
                p.act_t = p.act_t.saturating_add(1);
                // Fish and wood warm nobody: the igloo is only slow shelter.
                // The fast way is a sauna.
                if p.act_t.is_multiple_of(WARM_TICKS) && p.warmth < p.max_warmth {
                    p.warmth = (p.warmth + warm).min(p.max_warmth);
                }
                if p.warmth >= p.max_warmth {
                    p.chill = 0;
                    p.frost = 0;
                }
                if p.warmth >= p.max_warmth && !locked && p.owner == owner && !p.routed {
                    self.exit_igloo(id);
                }
            }

            self.update_prisoners(ig);
        }
    }

    fn capture(&mut self, ig: u8, by: u8) {
        let from = self.igloos[ig as usize].owner;
        let igloo = &mut self.igloos[ig as usize];
        igloo.owner = by;
        igloo.takeover = None;
        // A ruin is patched up by whoever moves in.
        igloo.hp = igloo_max_hp(igloo.tier);
        // Half-built work is lost with the old owners.
        igloo.upgrade = None;
        igloo.research = None;
        let igloo = &self.igloos[ig as usize];
        let occupants = igloo.occupants.clone();
        // Prisoners from the winning side walk free; the rest stay locked up.
        let team = self.team(by);
        self.release_prisoners(ig as usize, Some(team));
        for p in &mut self.penguins {
            // (Prisoners still waiting to change sides keep theirs.)
            if p.home == ig && p.prison_t == 0 {
                p.owner = by;
                p.order = Order::None;
                p.goal = None;
                p.path.clear();
                p.target = None;
            }
        }
        // Invaders move in.
        for id in occupants {
            let p = &mut self.penguins[id as usize];
            if p.owner == by {
                p.home = ig;
            }
        }
        self.events.push(Event::Captured { igloo: ig, from, to: by });
    }

    fn exit_igloo(&mut self, id: u16) {
        let p = &self.penguins[id as usize];
        let Some(ig) = p.inside else { return };
        let door = self.igloos[ig as usize].door;
        self.igloos[ig as usize].occupants.retain(|&o| o != id);
        let jitter = v2(self.rng.range(-0.3, 0.3), self.rng.range(0.0, 0.3));
        let fx = self.effects_of(p.owner);
        let p = &mut self.penguins[id as usize];
        p.inside = None;
        p.pos = door + jitter;
        p.act = Act::Idle;
        p.act_t = 0;
        p.cold = false;
        p.snowballs = fx.ammo(p.kind);
        p.goal = None;
        p.path.clear();
        p.facing = std::f32::consts::FRAC_PI_2;
    }

    /// Throw an invader back out into the snow, cold.
    fn kick_out(&mut self, id: u16) {
        self.exit_igloo(id);
        let p = &mut self.penguins[id as usize];
        p.warmth = p.warmth.min(COLD);
        p.cold = true;
        p.order = Order::None;
    }

    fn enter_igloo(&mut self, id: u16, ig: u8) {
        let p = self.penguins[id as usize].clone();
        let igloo = self.igloos[ig as usize].clone();
        let friendly = !self.hostile(p.owner, igloo.owner) && igloo.owner != NEUTRAL;
        if friendly {
            if igloo.owner == p.owner && p.home != ig {
                self.penguins[id as usize].home = ig;
            }
            // A warm defender throws the invaders out.
            if let Some(t) = igloo.takeover {
                if !p.cold && p.kind.fights() && self.hostile(t.by, p.owner) {
                    for o in igloo.occupants.clone() {
                        if self.hostile(self.penguins[o as usize].owner, igloo.owner) {
                            self.kick_out(o);
                        }
                    }
                    self.igloos[ig as usize].takeover = None;
                    self.events.push(Event::TakeoverFailed { igloo: ig });
                }
            }
        } else {
            // Core expansion sites need workers to found them; soldiers do
            // not claim empty igloos by walking inside.
            let founding = igloo.owner == NEUTRAL;
            let barred = igloo.owner != NEUTRAL;
            if p.cold || (!p.kind.fights() && !founding) || barred || (founding && p.kind != Kind::Worker) {
                self.penguins[id as usize].order = Order::None;
                return;
            }
            match igloo.takeover {
                Some(t) if !self.hostile(t.by, p.owner) => {}
                other => {
                    if founding {
                        // Someone else is already founding it: walking in would
                        // restart their clock and keep their payment.
                        if other.is_some() {
                            self.penguins[id as usize].order = Order::None;
                            return;
                        }
                        let (cost_fish, cost_wood) = price(GREAT_IGLOO_FISH, GREAT_IGLOO_WOOD);
                        let Some(source) = self
                            .igloos
                            .iter()
                            .filter(|i| i.owner == p.owner && i.fish >= cost_fish)
                            .max_by_key(|i| i.fish)
                            .map(|i| i.id as usize)
                        else {
                            self.penguins[id as usize].order = Order::None;
                            return;
                        };
                        if self.wood_of(p.owner) < cost_wood as u32 {
                            self.penguins[id as usize].order = Order::None;
                            return;
                        }
                        self.igloos[source].fish -= cost_fish;
                        self.spend_wood(p.owner, cost_wood);
                    }
                    if other.is_some() {
                        for o in igloo.occupants.clone() {
                            if self.hostile(self.penguins[o as usize].owner, p.owner) {
                                self.kick_out(o);
                            }
                        }
                    }
                    let total = takeover_ticks(igloo.tier, igloo.owner == NEUTRAL);
                    self.igloos[ig as usize].takeover =
                        Some(Takeover { by: p.owner, ticks_left: total, total });
                    self.events.push(Event::TakeoverStart { igloo: ig, by: p.owner });
                }
            }
        }
        let p = &mut self.penguins[id as usize];
        p.inside = Some(ig);
        p.act = Act::Inside;
        p.act_t = 0;
        p.order = Order::None;
        p.goal = None;
        p.path.clear();
        p.target = None;
        self.igloos[ig as usize].occupants.push(id);
    }

    fn update_penguin(&mut self, i: usize) {
        if self.penguins[i].inside.is_some() {
            return;
        }
        let act = self.penguins[i].act;
        self.penguins[i].act_t = self.penguins[i].act_t.saturating_add(1);
        if !self.penguins[i].cold {
            self.penguins[i].told_cold = false;
        }
        self.penguins[i].vel = Vec2::ZERO;

        if act == Act::Frozen {
            if self.penguins[i].carried_by.is_some() {
                // Moved along with its carrier after everyone's had a turn.
                self.penguins[i].act_t = 0;
                return;
            }
            // With the new core, an ownerless frozen penguin waits for a thaw.
            let waits = self.penguins[i].owner == NEUTRAL;
            let p = &mut self.penguins[i];
            if p.act_t >= FROZEN_TICKS && !waits {
                p.act = Act::Idle;
                p.warmth = 1;
                p.cold = true;
            }
            return;
        }

        // Wild penguins stand about, waiting for a tribe to take them in.
        if self.penguins[i].wild {
            let p = &mut self.penguins[i];
            p.act = Act::Idle;
            p.goal = None;
            p.path.clear();
            return;
        }

        // Panicking defenders rush home no matter what they were told.
        if self.penguins[i].panicking(self) {
            let home = self.penguins[i].home;
            if self.penguins[i].order != (Order::Enter { igloo: home }) {
                self.give_order(i as u16, Order::Enter { igloo: home });
            }
        }

        if self.penguins[i].owner == NEUTRAL {
            // Unclaimed igloos' penguins just potter about waiting for a leader.
            self.wander_home(i);
            return;
        }
        if self.penguins[i].act == Act::Bathe {
            self.bathe(i);
            return;
        }
        if self.stunned(i) {
            return;
        }
        self.skier_charge(i);
        match self.penguins[i].kind {
            Kind::Worker => self.update_worker(i),
            _ => self.update_snowballer(i),
        }
    }

    fn update_snowballer(&mut self, i: usize) {
        let p = &self.penguins[i];
        match p.act {
            Act::Throw => {
                if p.act_t == THROW_RELEASE {
                    self.release_snowball(i);
                }
                if self.penguins[i].act_t >= THROW_TICKS {
                    let p = &mut self.penguins[i];
                    p.act = Act::Idle;
                    p.cooldown = p.kind.stats().cooldown;
                }
                return;
            }
            Act::Scoop => {
                if p.act_t >= self.effects_of(p.owner).scale(Stat::ScoopTime, SCOOP_TICKS as u32) as u16 {
                    let (x, y) = p.pos.tile();
                    let cap = self.effects_of(p.owner).ammo(p.kind);
                    let p = &mut self.penguins[i];
                    p.snowballs = cap;
                    p.act = Act::Idle;
                    if self.map.in_bounds(x, y) {
                        let t = self.map.idx(x, y);
                        let s = self.terrain.snow[t];
                        if s > 0 {
                            self.set_snow(t, s - 1);
                        }
                    }
                }
                return;
            }
            _ => {}
        }

        if p.post.is_some() {
            self.update_posted(i);
            return;
        }
        if p.cold || p.routed {
            self.go_home(i);
            return;
        }
        if self.dodging(i) {
            return;
        }

        let p = &mut self.penguins[i];
        // A War Cry makes the cooldown run down faster.
        p.cooldown = p.cooldown.saturating_sub(if p.cry > 0 { 1 + WAR_CRY_THROW } else { 1 });
        let order = p.order;
        // Running and carrying off prisoners come before fighting and scooping.
        let running = matches!(order, Order::Move { run: true, .. } | Order::Enter { .. } | Order::Capture { .. });

        // Fight.
        if !running && p.snowballs > 0 && p.cooldown == 0 && (self.tick as usize + i).is_multiple_of(3) {
            // A Packer goes for walls, towers and igloos before penguins.
            if self.penguins[i].kind == Kind::Packer {
                if let Some(at) = self.find_structure(i) {
                    self.start_structure_throw(i, at);
                    return;
                }
            }
            if let Some(t) = self.find_target(i) {
                self.start_throw(i, t);
                return;
            }
            if let Some(s) = self.find_seal(i) {
                self.start_seal_throw(i, s);
                return;
            }
            if let Some(at) = self.find_structure(i) {
                self.start_structure_throw(i, at);
                return;
            }
        }

        let moving_order = matches!(order, Order::Move { .. } | Order::Enter { .. });
        let p = &self.penguins[i];
        let ammo = self.effects_of(p.owner).ammo(p.kind);
        let fights = p.kind.fights();
        let on_snow = self.snow_at(self.penguins[i].pos) > 0;
        let p = &self.penguins[i];
        // Standing on scraped-bare ice with nothing to throw: find some snow.
        if !running && !on_snow && !moving_order && p.snowballs == 0 && p.kind.fights() {
            if let Some(spot) = self.nearest_snow(p.pos, 6) {
                self.travel(i, spot, 0.3);
                return;
            }
        }
        // Out of ammo: make more, even mid-march. Topping up only while idle.
        if fights && !running && on_snow && (p.snowballs == 0 || (!moving_order && p.snowballs < ammo)) {
            let p = &mut self.penguins[i];
            p.act = Act::Scoop;
            p.act_t = 0;
            p.anim_seq = p.anim_seq.wrapping_add(1);
            return;
        }

        if p.kind == Kind::Warmer && self.warmer_job(i) {
            return;
        }
        if self.role_job(i) {
            return;
        }
        match order {
            Order::Move { target, auto, .. } => {
                if self.travel(i, target, 0.2) && !self.start_waypoints(i) {
                    self.penguins[i].order =
                        if auto { Order::None } else { Order::Hold { pos: target } };
                }
            }
            Order::Hold { pos } => {
                if self.penguins[i].pos.dist(pos) > 1.2 {
                    self.travel(i, pos, 0.3);
                } else {
                    self.idle(i);
                }
            }
            Order::Enter { igloo } => {
                let door = self.igloos[igloo as usize].door;
                if self.travel(i, door, 0.5) {
                    self.enter_igloo(i as u16, igloo);
                }
            }
            Order::Garrison { building } => self.garrison_job(i, building),
            Order::Capture { penguin } => self.capture_job(i, penguin),
            // A snowballer called to fill a camp's slot.
            Order::Train { building } => self.train_job(i, building),
            Order::Thaw { .. } => self.penguins[i].order = Order::None,
            Order::None | Order::Gather { .. } | Order::Chop { .. } | Order::Build { .. } | Order::Tend { .. } => {
                // A soldier in an army keeps its place in the ranks.
                if self.penguins[i].slot.is_some() && order == Order::None && self.follow_chief(i) {
                    return;
                }
                if !self.respond_to_alerts(i) {
                    // Hurt and idle: a lit sauna nearby is the place to be.
                    let hurt = self.penguins[i].warmth * 2 < self.penguins[i].max_warmth;
                    if !(hurt && self.seek_bath(i, SEEK_HURT)) {
                        if self.penguins[i].kind == Kind::Chieftain && order == Order::None {
                            // A Chieftain with nothing to do goes home and stands
                            // there, so his army isn't forever reshuffling.
                            let door = self.igloos[self.penguins[i].home as usize].door;
                            if self.penguins[i].pos.dist(door) > 6.0 {
                                self.travel(i, door, 3.0);
                            } else {
                                self.idle(i);
                            }
                        } else {
                            self.wander_home(i);
                        }
                    }
                }
            }
        }
    }

    fn respond_to_alerts(&mut self, i: usize) -> bool {
        let p = &self.penguins[i];
        let alerts = &self.players[p.owner as usize].alerts;
        let best = alerts
            .iter()
            .map(|&(pos, _)| pos)
            .filter(|&pos| pos.dist(p.pos) < ALERT_RADIUS)
            .min_by(|a, b| a.dist2(p.pos).partial_cmp(&b.dist2(p.pos)).unwrap());
        if let Some(pos) = best {
            if p.kind == Kind::Chieftain {
                // A Chieftain stops short, so his army forms up facing the
                // trouble rather than walking into it.
                let d = pos.dist(p.pos);
                if d > CHIEF_STANDOFF + 1.0 {
                    let target = pos + (p.pos - pos).norm() * CHIEF_STANDOFF;
                    self.give_order(i as u16, Order::Move { target, run: false, auto: true });
                } else {
                    self.idle(i);
                }
                return true;
            }
            let target = pos + Vec2::from_angle(self.rng.range(0.0, std::f32::consts::TAU)) * self.rng.range(0.0, 1.5);
            self.give_order(i as u16, Order::Move { target, run: false, auto: true });
            true
        } else {
            false
        }
    }

    fn update_worker(&mut self, i: usize) {
        let p = &self.penguins[i];
        match p.act {
            Act::Fish | Act::Mine => {
                let fish_ticks = self.effects_of(p.owner).scale(Stat::FishTime, FISH_TICKS as u32) as u16;
                let need = if p.act == Act::Fish { fish_ticks } else { MINE_TICKS };
                let Some(r) = p.gather_res else {
                    if p.act == Act::Fish && p.shore.is_some() {
                        self.shore_catch(i);
                    } else {
                        self.penguins[i].act = Act::Idle;
                    }
                    return;
                };
                if self.resources[r as usize].amount == 0 {
                    let p = &mut self.penguins[i];
                    p.act = Act::Idle;
                    p.gather_res = None;
                    return;
                }
                if p.act_t >= need {
                    let res = &mut self.resources[r as usize];
                    res.amount -= 1;
                    let carry = if res.kind == ResourceKind::Fish { Carry::Fish } else { Carry::Crystal };
                    let p = &mut self.penguins[i];
                    p.carry = carry;
                    p.act = Act::Idle;
                    p.gather_res = None;
                }
                return;
            }
            Act::Chop => {
                let Some(t) = p.chop_tree else {
                    self.penguins[i].act = Act::Idle;
                    return;
                };
                if self.terrain.wood[t as usize] == 0 {
                    let p = &mut self.penguins[i];
                    p.act = Act::Idle;
                    p.chop_tree = None;
                    return;
                }
                if p.act_t >= CHOP_TICKS {
                    self.terrain.wood[t as usize] -= 1;
                    if self.terrain.wood[t as usize] == 0 {
                        self.fell_tree(t as usize);
                    }
                    let p = &mut self.penguins[i];
                    p.carry = Carry::Wood;
                    p.act = Act::Idle;
                    p.chop_tree = None;
                }
                return;
            }
            Act::Build => {
                if p.act_t >= self.effects_of(p.owner).scale(Stat::BuildTime, BUILD_TICKS as u32) as u16 {
                    self.penguins[i].act = Act::Idle;
                    self.build_step(i);
                }
                return;
            }
            Act::Unload => {
                if p.act_t >= UNLOAD_TICKS {
                    let p = &self.penguins[i];
                    let ig = &mut self.igloos[p.home as usize];
                    match p.carry {
                        Carry::Fish => ig.fish = (ig.fish + 1).min(999),
                        Carry::Crystal => ig.crystal += 1,
                        Carry::Wood => ig.wood = (ig.wood + 1).min(999),
                        Carry::None => {}
                    }
                    let p = &mut self.penguins[i];
                    p.carry = Carry::None;
                    p.act = Act::Idle;
                }
                return;
            }
            _ => {}
        }

        if p.cold || p.routed {
            self.go_home(i);
            return;
        }

        match p.order {
            Order::Capture { penguin } => self.capture_job(i, penguin),
            Order::Thaw { .. } => self.penguins[i].order = Order::None,
            Order::Train { building } => self.train_job(i, building),
            Order::Move { target, .. } => {
                if self.travel(i, target, 0.2) && !self.start_waypoints(i) {
                    self.penguins[i].order = Order::Hold { pos: target };
                }
            }
            Order::Hold { pos } => {
                if self.penguins[i].pos.dist(pos) > 1.2 {
                    self.travel(i, pos, 0.3);
                } else {
                    self.idle(i);
                }
            }
            Order::Enter { igloo } => {
                let (door, owner) = (self.igloos[igloo as usize].door, self.igloos[igloo as usize].owner);
                let own = owner == self.penguins[i].owner;
                if self.travel(i, door, 0.5) {
                    if owner == NEUTRAL {
                        self.enter_igloo(i as u16, igloo);
                    } else if own {
                        self.penguins[i].home = igloo;
                        self.penguins[i].order = Order::None;
                        // A recruit's first meal: it sits down to eat, and then serves.
                        if self.penguins[i].dining {
                            let p = &mut self.penguins[i];
                            p.dining = false;
                            p.act = Act::Unload;
                            p.act_t = 0;
                            p.anim_seq = p.anim_seq.wrapping_add(1);
                        }
                    } else {
                        self.penguins[i].order = Order::None;
                    }
                }
            }
            Order::Gather { resource } => {
                if self.resources[resource as usize].amount == 0 && p.carry == Carry::None {
                    self.penguins[i].order = Order::None;
                } else {
                    self.work(i, Some(resource));
                }
            }
            Order::Chop { at } => self.chop(i, at),
            Order::Build { building } => self.build_job(i, building),
            Order::Tend { building } if self.buildings[building as usize].kind == BuildKind::Catapult => self.crew_job(i, building),
            Order::Tend { building } => self.tend_job(i, building),
            Order::None | Order::Garrison { .. } => self.work(i, None),
        }
    }

    /// Carry whatever we're holding home. Returns false if empty-handed.
    fn deliver(&mut self, i: usize) -> bool {
        let p = &self.penguins[i];
        if p.carry == Carry::None {
            return false;
        }
        // With armies wood is never stored: a log nobody needs is left.
        if p.carry == Carry::Wood {
            self.penguins[i].carry = Carry::None;
            return false;
        }
        let home = p.home as usize;
        // Home, or a fish store if one is closer.
        let (mut drop, mut face, mut radius) = (self.igloos[home].door, self.igloos[home].center, 0.6);
        // A Sushi House's fishers always take the catch to it.
        let near = match self.fishery_of(i) {
            Some((Fishery::House(b), _)) => Some((f32::NEG_INFINITY, b as usize)),
            _ => self.nearest_building(p.owner, BuildKind::Store, p.pos),
        };
        if let Some((d, b)) = near {
            if d + 1.0 < self.penguins[i].pos.dist(drop) {
                let c = self.buildings[b].center();
                face = c;
                drop = c;
                radius = 1.0 + BuildKind::Store.size() as f32 * 0.5;
            }
        }
        if self.travel(i, drop, radius) {
            let p = &mut self.penguins[i];
            p.act = Act::Unload;
            p.act_t = 0;
            p.anim_seq = p.anim_seq.wrapping_add(1);
            p.facing = (face - p.pos).angle();
        }
        true
    }

    /// Worker job loop for clearing a patch of forest around `at`.
    fn chop(&mut self, i: usize, at: (i32, i32)) {
        if self.deliver(i) {
            return;
        }
        let pos = self.penguins[i].pos;
        let tree = self.penguins[i]
            .chop_tree
            .filter(|&t| self.terrain.wood[t as usize] > 0)
            .or_else(|| self.choose_tree(pos, at));
        let Some(t) = tree else {
            // Nothing left to chop here.
            self.penguins[i].order = Order::None;
            return;
        };
        self.chop_tree_job(i, t);
    }

    /// Walk to tree `t` and start chopping it.
    pub(super) fn chop_tree_job(&mut self, i: usize, t: u32) {
        let pos = self.penguins[i].pos;
        self.penguins[i].chop_tree = Some(t);
        let (tx, ty) = ((t as i32) % self.map.w, (t as i32) / self.map.w);
        let Some(spot) = self.chop_spot(tx, ty, pos) else {
            self.penguins[i].chop_tree = None;
            return;
        };
        if self.travel(i, spot, 0.25) {
            let p = &mut self.penguins[i];
            p.act = Act::Chop;
            p.act_t = 0;
            p.anim_seq = p.anim_seq.wrapping_add(1);
            p.facing = (v2(tx as f32 + 0.5, ty as f32 + 0.5) - p.pos).angle();
        }
    }

    /// The reachable tree near `at` that's closest to `from`.
    fn choose_tree(&self, from: Vec2, at: (i32, i32)) -> Option<u32> {
        let r = CHOP_RADIUS.ceil() as i32;
        let mut best: Option<(f32, u32)> = None;
        for y in at.1 - r..=at.1 + r {
            for x in at.0 - r..=at.0 + r {
                if !self.map.in_bounds(x, y) || self.map.obstacle_at(x, y) != Obstacle::Tree {
                    continue;
                }
                let c = v2(x as f32 + 0.5, y as f32 + 0.5);
                let dc = c.dist(v2(at.0 as f32 + 0.5, at.1 as f32 + 0.5));
                if dc > CHOP_RADIUS || self.chop_spot(x, y, from).is_none() {
                    continue;
                }
                // Mostly nearest to the worker, but work outward from the target.
                let d = c.dist(from) + dc * 0.5;
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, self.map.idx(x, y) as u32));
                }
            }
        }
        best.map(|(_, t)| t)
    }

    /// The nearest tree to `from` (within `r` tiles) that a worker can reach.
    pub fn tree_near(&self, from: Vec2, r: i32) -> Option<(i32, i32)> {
        self.tree_near_where(from, r, |_| true)
    }

    /// The tree within `r` tiles of `from` nearest by walking from there,
    /// not across water or a cliff: where builders and stokers fetch logs.
    pub fn tree_walkable_from(&self, from: Vec2, r: i32) -> Option<(i32, i32)> {
        let walk = self.grid.distances(from.tile());
        let (fx, fy) = from.tile();
        let mut best: Option<(f32, (i32, i32))> = None;
        for y in fy - r..=fy + r {
            for x in fx - r..=fx + r {
                if self.map.obstacle_at(x, y) != Obstacle::Tree {
                    continue;
                }
                let Some(spot) = self.chop_spot(x, y, from) else { continue };
                let (sx, sy) = spot.tile();
                let d = walk[self.map.idx(sx, sy)];
                if d.is_finite() && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, (x, y)));
                }
            }
        }
        best.map(|(_, t)| t)
    }

    /// As [`Game::tree_near`], among the trees `ok` allows.
    fn tree_near_where(&self, from: Vec2, r: i32, ok: impl Fn((i32, i32)) -> bool) -> Option<(i32, i32)> {
        let (fx, fy) = from.tile();
        let mut best: Option<(i32, (i32, i32))> = None;
        for y in fy - r..=fy + r {
            for x in fx - r..=fx + r {
                if self.map.obstacle_at(x, y) != Obstacle::Tree || !ok((x, y)) || self.chop_spot(x, y, from).is_none() {
                    continue;
                }
                let d = (x - fx).pow(2) + (y - fy).pow(2);
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, (x, y)));
                }
            }
        }
        best.map(|(_, t)| t)
    }

    /// Where to stand to chop the tree at (x, y).
    fn chop_spot(&self, x: i32, y: i32, from: Vec2) -> Option<Vec2> {
        [(0, 1), (1, 0), (-1, 0), (0, -1)]
            .iter()
            .filter(|&&(dx, dy)| self.grid.walkable(x + dx, y + dy))
            .map(|&(dx, dy)| v2(x as f32 + 0.5 + dx as f32 * 0.75, y as f32 + 0.5 + dy as f32 * 0.75))
            .min_by(|a, b| a.dist2(from).partial_cmp(&b.dist2(from)).unwrap())
    }

    /// Worker job loop: fetch a resource and bring it home.
    fn work(&mut self, i: usize, forced: Option<u16>) {
        if self.deliver(i) {
            return;
        }
        let p = &self.penguins[i];
        // Only fishers in a slot fish; the rest wait for work.
        if forced.is_none() && p.order == Order::None && p.fishery.is_none() && !p.shore_pin && p.kind == Kind::Worker {
            self.wander_home(i);
            return;
        }
        let pinned = forced.is_none() && (p.shore_pin && p.shore.is_some() || p.fishery.is_some());
        let res = forced
            .filter(|&r| self.resources[r as usize].amount > 0)
            .or(p.gather_res.filter(|&r| self.resources[r as usize].amount > 0))
            .or_else(|| if pinned { None } else { self.choose_resource(i) });
        let Some(r) = res else {
            // No hole to go to: fish a bank instead.
            if self.penguins[i].prefer == ResourceKind::Fish && self.fish_shore(i) {
                return;
            }
            self.wander_home(i);
            return;
        };
        self.penguins[i].gather_res = Some(r);
        self.penguins[i].shore = None;
        let pos = self.penguins[i].pos;
        let resource = &self.resources[r as usize];
        let Some(&spot) = resource
            .spots
            .iter()
            .min_by(|a, b| a.dist2(pos).partial_cmp(&b.dist2(pos)).unwrap())
        else {
            self.penguins[i].gather_res = None;
            self.wander_home(i);
            return;
        };
        let rpos = resource.pos;
        let kind = resource.kind;
        if self.travel(i, spot, 0.25) {
            let p = &mut self.penguins[i];
            p.act = if kind == ResourceKind::Fish { Act::Fish } else { Act::Mine };
            p.act_t = 0;
            p.anim_seq = p.anim_seq.wrapping_add(1);
            p.facing = (rpos - p.pos).angle();
        }
    }

    fn choose_resource(&mut self, i: usize) -> Option<u16> {
        let p = &self.penguins[i];
        let mut best: Option<(f32, u16)> = None;
        for r in &self.resources {
            if r.amount == 0 || r.kind != p.prefer || r.spots.is_empty() {
                continue;
            }
            // Spread workers out: a little randomness in the distance.
            let d = r.pos.dist(p.pos) + self.rng.range(0.0, 6.0);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, r.id));
            }
        }
        best.map(|(_, id)| id)
    }

    fn go_home(&mut self, i: usize) {
        let home = self.penguins[i].home;
        let door = self.igloos[home as usize].door;
        // A lit sauna in reach beats the walk home.
        if self.seek_bath(i, SEEK_COLD) || self.seek_fire(i) {
            return;
        }
        if self.penguins[i].cold && !self.penguins[i].told_cold {
            self.penguins[i].told_cold = true;
            self.events.push(Event::NoFire { penguin: i as u16 });
        }
        if self.travel(i, door, 0.5) {
            self.enter_igloo(i as u16, home);
        }
    }

    fn idle(&mut self, i: usize) {
        let p = &mut self.penguins[i];
        if p.act == Act::Walk {
            p.act = Act::Idle;
        }
    }

    /// Mill about near the home igloo.
    fn wander_home(&mut self, i: usize) {
        let home = self.penguins[i].home as usize;
        let center = self.igloos[home].door;
        let p = &mut self.penguins[i];
        if let Some(goal) = p.goal {
            if goal.dist(center) < 7.0 {
                if self.travel(i, goal, 0.3) {
                    self.penguins[i].goal = None;
                }
                return;
            }
        }
        if p.pos.dist(center) > 7.0 {
            self.travel(i, center, 2.0);
            return;
        }
        self.idle(i);
        let p = &mut self.penguins[i];
        p.wander_t = p.wander_t.saturating_sub(1);
        if p.wander_t == 0 {
            p.wander_t = secs(3.0) + self.rng.below(secs(6.0) as u32) as u16;
            let target = center
                + Vec2::from_angle(self.rng.range(0.0, std::f32::consts::TAU))
                    * self.rng.range(1.5, 5.5);
            if self.grid.walkable_at(target) {
                let p = &mut self.penguins[i];
                p.goal = Some(target);
                p.path.clear();
                p.path_i = 0;
                self.plan(i, target);
            }
        }
    }

    // ---------------------------------------------------------------------
    // Movement

    fn speed(&self, i: usize, dir: Vec2) -> f32 {
        let p = &self.penguins[i];
        let (x, y) = p.pos.tile();
        let mut s = p.kind.stats().speed;
        if self.map.ground_at(x, y) == Ground::Ice {
            if p.kind == Kind::Skier {
                s *= SKIER_ICE_SPEED; // skis don't glide on bare ice
            } else {
                s *= 1.35; // belly slide!
                s = self.effects_of(p.owner).scale_f(Stat::IceSpeed, s);
            }
        }
        if p.cold || p.routed {
            s *= 1.1;
        }
        if p.cry > 0 {
            s *= WAR_CRY_SPEED;
        }
        if p.carrying.is_some() {
            s *= if p.kind == Kind::Sledder { 0.85 } else { CARRY_SPEED };
        }
        let h0 = self.map.height_at(p.pos);
        let h1 = self.map.height_at(p.pos + dir * 0.5);
        let slope = (h1 - h0) / 0.5;
        s * (1.0 - slope * 0.6).clamp(0.55, 1.25)
    }

    fn plan(&mut self, i: usize, goal: Vec2) {
        let start = self.penguins[i].pos.tile();
        let mut goal_tile = goal.tile();
        if !self.grid.walkable(goal_tile.0, goal_tile.1) {
            goal_tile = self.grid.nearest_walkable(goal_tile, 8).unwrap_or(goal_tile);
        }
        let tiles = self.pf.find(&self.grid, start, goal_tile, 12000);
        let mut path: Vec<Vec2> =
            tiles.iter().map(|&(x, y)| v2(x as f32 + 0.5, y as f32 + 0.5)).collect();
        let reached = tiles.last().copied().unwrap_or(start) == goal_tile;
        if reached && self.grid.walkable_at(goal) {
            if let Some(last) = path.last_mut() {
                *last = goal;
            } else {
                path.push(goal);
            }
        }
        let p = &mut self.penguins[i];
        p.path = path;
        p.path_i = 0;
        p.goal = Some(goal);
        p.stuck_t = 0;
    }

    /// Walk towards `goal`. Returns true once within `radius` (or hopelessly stuck).
    fn travel(&mut self, i: usize, goal: Vec2, radius: f32) -> bool {
        let p = &self.penguins[i];
        if p.pos.dist(goal) <= radius {
            let p = &mut self.penguins[i];
            if p.act == Act::Walk {
                p.act = Act::Idle;
            }
            p.goal = None;
            p.path.clear();
            return true;
        }
        if p.goal.is_none_or(|g| g.dist2(goal) > 0.01) {
            // A whole army ordered at once would plan every path in one
            // tick; spread them over the next few instead.
            if self.plans_left == 0 {
                return false;
            }
            self.plans_left -= 1;
            self.plan(i, goal);
        }
        let p = &self.penguins[i];
        if p.path.is_empty() {
            // Nowhere to go (goal unreachable from here).
            if p.pos.dist(goal) < 2.5 || p.repaths > 2 {
                let p = &mut self.penguins[i];
                p.act = Act::Idle;
                return true;
            }
            self.penguins[i].repaths += 1;
            if self.plans_left > 0 {
                self.plans_left -= 1;
                self.plan(i, goal);
            }
            return false;
        }

        // String-pull: skip waypoints we can see past.
        let mut pi = p.path_i;
        let mut look = 0;
        while pi + 1 < p.path.len() && look < 4 && self.grid.line_clear(p.pos, p.path[pi + 1]) && crate::path::clear_of_trees(&self.map, p.pos, p.path[pi + 1]) {
            pi += 1;
            look += 1;
        }
        let wp = p.path[pi];
        let last = pi + 1 == p.path.len();
        let to = wp - p.pos;
        let dist = to.len();
        let dir = to.norm();
        let speed = self.speed(i, dir);
        let step = (speed * DT).min(dist);
        let next = p.pos + dir * step;

        let p = &mut self.penguins[i];
        p.path_i = pi;
        if dist < 0.15 && !last {
            p.path_i += 1;
        }
        let ok = self.grid.walkable_at(next)
            || !self.grid.walkable_at(p.pos) /* already stuck inside: let it out */;
        if ok {
            p.vel = dir * speed;
            p.pos = next;
        } else {
            // Slide along whichever axis is free.
            let nx = v2(next.x, p.pos.y);
            let ny = v2(p.pos.x, next.y);
            if self.grid.walkable_at(nx) {
                p.pos = nx;
            } else if self.grid.walkable_at(ny) {
                p.pos = ny;
            }
        }
        if dir.len2() > 0.0 {
            p.facing = turn_towards(p.facing, dir.angle(), 14.0 * DT);
        }
        p.act = Act::Walk;

        if ok {
            let (x, y) = next.tile();
            self.trample(x, y);
        }
        let p = &mut self.penguins[i];
        // Stuck detection.
        if step < speed * DT * 0.3 || !ok {
            p.stuck_t += 1;
            if p.stuck_t > secs(1.0) {
                p.repaths = p.repaths.saturating_add(1);
                if p.repaths > 3 {
                    p.act = Act::Idle;
                    p.goal = None;
                    p.path.clear();
                    return true;
                }
                self.plan(i, goal);
            }
        } else {
            p.stuck_t = 0;
        }
        false
    }

    /// Gently push overlapping penguins apart.
    fn separate(&mut self) {
        const R: f32 = 0.55;
        let mut near = std::mem::take(&mut self.scratch);
        for i in 0..self.penguins.len() {
            let p = &self.penguins[i];
            // A block of ice stays where it fell.
            if p.inside.is_some() || p.post.is_some() || p.carried_by.is_some() || p.blocks.is_some() {
                continue;
            }
            let pos = p.pos;
            self.buckets.query(pos, R, &mut near);
            let mut push = Vec2::ZERO;
            for &o in &near {
                if o as usize == i {
                    continue;
                }
                let q = &self.penguins[o as usize];
                if q.inside.is_some() || q.carried_by.is_some() {
                    continue;
                }
                let d = pos - q.pos;
                let l = d.len();
                if l < R {
                    let dir = if l > 1e-4 { d * (1.0 / l) } else { Vec2::from_angle(i as f32) };
                    push += dir * ((R - l) * 0.35);
                }
            }
            if push.len2() > 0.0 {
                let busy = matches!(self.penguins[i].act, Act::Fish | Act::Mine | Act::Chop | Act::Build | Act::Unload | Act::Frozen);
                let push = if busy { push * 0.2 } else { push };
                let np = pos + push;
                if self.grid.walkable_at(np) {
                    self.penguins[i].pos = np;
                }
            }
        }
        self.scratch = near;
    }

    // ---------------------------------------------------------------------
    // Combat

    /// How far this penguin can throw, with its owner's research.
    pub fn throw_range(&self, kind: Kind, owner: u8, from: Vec2, to: Vec2) -> f32 {
        let dh = self.map.height_at(from) - self.map.height_at(to);
        self.effects_of(owner).scale_f(Stat::Range, kind.stats().range) + (dh * 1.2).clamp(-1.5, 2.5) + self.tower_bonus(owner, from) - self.weather_range_penalty()
    }

    fn find_target(&mut self, i: usize) -> Option<u16> {
        let p = &self.penguins[i];
        let pos = p.pos;
        let owner = p.owner;
        let mut near = std::mem::take(&mut self.scratch);
        let kind = p.kind;
        let range = self.effects_of(owner).scale_f(Stat::Range, kind.stats().range);
        self.targets.query(pos, range + 2.5 + TOWER_RANGE, &mut near);
        // These do not depend on the candidate, so ask once, not per penguin.
        let from_h = self.map.height_at(pos);
        let tower = self.tower_bonus(owner, pos);
        let weather = self.weather_range_penalty();
        let last = self.penguins[i].target;
        let direct = kind.stats().direct;
        let mut best: Option<(f32, u16)> = None;
        for &o in &near {
            let q = &self.penguins[o as usize];
            if !self.hostile(owner, q.owner) || q.inside.is_some() || q.post.is_some() || q.act == Act::Frozen {
                continue;
            }
            let d = q.pos.dist(pos);
            if kind == Kind::Slinger && d < roles::SLINGER_MIN_RANGE {
                continue;
            }
            let dh = from_h - self.map.height_at(q.pos);
            if d > range + (dh * 1.2).clamp(-1.5, 2.5) + tower - weather {
                continue;
            }
            // Prefer snowballers, then whoever's closest; stick with the last
            // target; and pick sensibly between them (`tactics.rs`).
            let mut score = d + self.target_penalty(q, pos, direct);
            if !q.kind.fights() {
                score += 4.0;
            }
            if q.cold {
                score += 3.0;
            }
            if last == Some(o) {
                score -= 1.5;
            }
            if best.is_none_or(|(s, _)| score < s) {
                best = Some((score, o));
            }
        }
        self.scratch = near;
        best.map(|(_, id)| id)
    }

    fn start_throw(&mut self, i: usize, target: u16) {
        let tpos = self.penguins[target as usize].pos;
        let p = &mut self.penguins[i];
        p.target = Some(target);
        p.seal_aim = None;
        p.wall_aim = None;
        p.act = Act::Throw;
        p.act_t = 0;
        p.anim_seq = p.anim_seq.wrapping_add(1);
        p.facing = (tpos - p.pos).angle();
    }

    fn release_snowball(&mut self, i: usize) {
        let p = &self.penguins[i];
        if p.snowballs == 0 {
            return;
        }
        // At a penguin, or at a seal that's still up.
        let (tpos, tvel) = match (p.target, p.seal_aim) {
            (Some(t), _) => {
                let tq = &self.penguins[t as usize];
                if tq.inside.is_some() {
                    return;
                }
                (tq.pos, tq.vel)
            }
            (None, Some(s)) if self.seals[s as usize].state == SealState::Surfaced => (self.seals[s as usize].pos, Vec2::ZERO),
            (None, None) if p.wall_aim.is_some() => (p.wall_aim.unwrap(), Vec2::ZERO),
            _ => return,
        };
        let from = p.pos + Vec2::from_angle(p.facing) * 0.25;
        let dist = from.dist(tpos);
        let flight_s = 0.3 + dist * 0.07;
        let double = p.power.is_some_and(|pw| self.powerups[pw as usize].kind == PowerKind::Double);
        let shots = if double { 2 } else { 1 };
        let team = self.team(p.owner);
        let lead = tvel * (flight_s * 0.75);
        let aim = tpos + lead;
        let thrower = p.id;
        let kind = p.kind;
        let from_z = self.map.height_at(from) + 0.75 * kind.stats().scale + if p.post.is_some() { TOWER_DECK } else { 0.0 };
        let spread = self.effects_of(p.owner).scale_f(Stat::Spread, kind.stats().spread);
        self.penguins[i].snowballs -= 1;
        for _ in 0..shots {
            let sigma = 0.12 + dist * 0.055 * spread;
            let to = aim + v2(self.rng.gauss(), self.rng.gauss()) * sigma;
            let to = v2(to.x.clamp(0.0, self.map.w as f32 - 0.01), to.y.clamp(0.0, self.map.h as f32 - 0.01));
            // From a roof the ball arcs over whatever stands between.
            let to = if self.penguins[i].post.is_some() { to } else { self.cover(from, to) };
            let sb = Snowball {
                id: self.next_snowball,
                from,
                from_z,
                to,
                to_z: self.map.height_at(to) + if self.building_at(to.tile().0, to.tile().1).is_some() { 0.6 } else { 0.0 },
                start_tick: self.tick,
                flight: ((flight_s / DT).round() as u16).max(2),
                thrower,
                team,
                kind,
                big: false,
            };
            self.next_snowball += 1;
            self.events.push(Event::Thrown(sb.clone()));
            self.snowballs.push(sb);
            if let Some(t) = self.penguins[i].target.filter(|_| self.penguins[i].seal_aim.is_none() && self.penguins[i].wall_aim.is_none()) {
                self.aimed_at(self.next_snowball - 1, t, kind.stats().direct, from);
            }
        }
    }

    /// Where a throw from `from` to `to` really lands: walls and trees just in
    /// front of the target stop it. Cover right next to the thrower doesn't.
    fn cover(&self, from: Vec2, to: Vec2) -> Vec2 {
        let d = to - from;
        let len = d.len();
        if len < 1.6 {
            return to;
        }
        let dir = d * (1.0 / len);
        let mut t = (len - 1.6).max(1.3);
        while t < len - 0.3 {
            let p = from + dir * t;
            let (x, y) = p.tile();
            if self.blocks_snowballs(x, y) {
                return p;
            }
            t += 0.25;
        }
        to
    }

    fn update_snowballs(&mut self) {
        let tick = self.tick;
        let mut landed = Vec::new();
        self.snowballs.retain(|s| {
            if tick >= s.start_tick + s.flight as u32 {
                landed.push(s.clone());
                false
            } else {
                true
            }
        });
        for s in landed {
            self.land(s);
        }
    }

    fn land(&mut self, s: Snowball) {
        self.ball_landed(s.id);
        if s.big && self.catapult_landed(&s) {
            self.events.push(Event::Landed { id: s.id, pos: s.to, hit: HitKind::Building });
            return;
        }
        let (tx, ty) = s.to.tile();
        if self.map.obstacle_at(tx, ty) == Obstacle::Tree {
            self.events.push(Event::Landed { id: s.id, pos: s.to, hit: HitKind::Tree });
            return;
        }
        if let Some(ig) = self.igloo_on(tx, ty) {
            self.events.push(Event::Landed { id: s.id, pos: s.to, hit: HitKind::Building });
            self.hit_igloo(ig, s.team, s.kind);
            return;
        }
        if let Some(b) = self.building_at(tx, ty).filter(|b| b.solid()) {
            let (b, friendly) = (b.id as usize, self.team(b.owner) == s.team);
            self.events.push(Event::Landed { id: s.id, pos: s.to, hit: HitKind::Building });
            if !friendly {
                self.damage_building(b, if s.kind == Kind::Packer { 3 } else { 1 });
            }
            return;
        }
        let st = s.kind.stats();
        let radius = self.effects_of(self.penguins[s.thrower as usize].owner).scale_f(Stat::Splash, st.splash_radius);
        let mut near = std::mem::take(&mut self.scratch);
        self.buckets.query(s.to, radius, &mut near);
        let mut victims: Vec<(f32, u16)> = near
            .iter()
            .filter_map(|&o| {
                let q = &self.penguins[o as usize];
                let hostile = q.owner != NEUTRAL && self.team(q.owner) != s.team;
                if !hostile || q.inside.is_some() || q.post.is_some() || q.act == Act::Frozen {
                    return None;
                }
                let d = q.pos.dist(s.to);
                (d <= radius).then_some((d, o))
            })
            .collect();
        self.scratch = near;
        victims.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let mut kind = HitKind::Miss;
        for (n, &(d, v)) in victims.iter().enumerate() {
            let direct = n == 0 && d <= DIRECT_RADIUS;
            let defense = self.penguins[v as usize]
                .power
                .is_some_and(|pw| self.powerups[pw as usize].kind == PowerKind::Defense);
            let mut dmg = if direct { st.direct } else { st.splash };
            if direct && (defense || self.shielded(v, s.from)) {
                dmg = dmg.saturating_sub(1).max(1);
            }
            kind = if direct { HitKind::Direct } else { kind.max_hit(HitKind::Splash) };
            self.hurt(v, dmg, s.thrower, s.from);
        }
        if self.seal_hit(s.to, s.thrower) {
            kind = HitKind::Direct;
        }
        self.events.push(Event::Landed { id: s.id, pos: s.to, hit: kind });
    }

    /// A friendly Shieldbearer (or the penguin itself) is covering `v`
    /// against a throw from `from`: the slab is between them.
    fn shielded(&self, v: u16, from: Vec2) -> bool {
        let p = &self.penguins[v as usize];
        if p.kind == Kind::Shield {
            return true;
        }
        let team = self.team(p.owner);
        let mut near = Vec::new();
        self.buckets.query(p.pos, SHIELD_RADIUS, &mut near);
        near.iter().any(|&o| {
            let q = &self.penguins[o as usize];
            let covering = q.kind == Kind::Shield && q.act != Act::Frozen && q.act != Act::Stunned && !q.cold && self.team(q.owner) == team && q.pos.dist(p.pos) <= SHIELD_RADIUS;
            // In front: the shield is nearer the thrower than `v` is.
            covering && q.pos.dist(from) < p.pos.dist(from)
        })
    }

    fn hurt(&mut self, v: u16, dmg: u8, thrower: u16, from: Vec2) {
        let owner = self.penguins[v as usize].owner;
        let pos = self.penguins[v as usize].pos;
        if let Some(pl) = self.players.get_mut(owner as usize) {
            if !pl.alerts.iter().any(|(a, _)| a.dist(from) < 3.0) {
                pl.alerts.push((from, secs(4.0)));
            }
            let _ = pos;
        }
                let p = &mut self.penguins[v as usize];
        p.warmth = p.warmth.saturating_sub(dmg);
        if p.warmth <= COLD {
            self.drop_captive(v);
        }
        let p = &mut self.penguins[v as usize];
        if p.warmth == 0 {
            p.act = Act::Frozen;
            p.act_t = 0;
            p.cold = true;
            p.order = Order::None;
            p.goal = None;
            p.path.clear();
            p.carry = Carry::None;
            p.gather_res = None;
            p.shore = None;
            self.events.push(Event::Frozen { penguin: v });
            let team = self.team(owner);
            self.morale_shock(pos, team);
            if self.penguins[v as usize].kind == Kind::Chieftain {
                self.chieftain_fell(pos, team);
            }
            // Whoever froze them steals their powerup.
            if let Some(pw) = self.penguins[v as usize].power.take() {
                self.unequip(v, pw);
                let t = &self.penguins[thrower as usize];
                if t.power.is_none() && !t.cold && t.inside.is_none() && t.kind.fights() {
                    self.equip(thrower, pw);
                } else {
                    self.powerups[pw as usize].pos = pos;
                }
            }
            self.on_frozen(v);
        } else if p.warmth <= COLD && !p.cold {
            p.cold = true;
            p.order = Order::None;
            p.goal = None;
            p.path.clear();
            p.target = None;
            if matches!(p.act, Act::Throw | Act::Scoop | Act::Fish | Act::Mine | Act::Chop | Act::Build) {
                p.act = Act::Idle;
            }
        }
    }

    fn equip(&mut self, id: u16, pw: u8) {
        let kind = self.powerups[pw as usize].kind;
        self.powerups[pw as usize].holder = Some(id);
        let p = &mut self.penguins[id as usize];
        p.power = Some(pw);
        if kind == PowerKind::Defense {
            p.max_warmth += 4;
            p.warmth += 4;
        }
        self.events.push(Event::PowerupTaken { powerup: pw, penguin: id });
    }

    fn unequip(&mut self, id: u16, pw: u8) {
        let kind = self.powerups[pw as usize].kind;
        self.powerups[pw as usize].holder = None;
        let p = &mut self.penguins[id as usize];
        if kind == PowerKind::Defense {
            p.max_warmth = p.max_warmth.saturating_sub(4);
            p.warmth = p.warmth.min(p.max_warmth);
        }
    }

    fn update_powerups(&mut self) {
        for pw in 0..self.powerups.len() {
            match self.powerups[pw].holder {
                Some(h) => {
                    let p = &self.penguins[h as usize];
                    if p.inside.is_none() {
                        self.powerups[pw].pos = p.pos;
                    }
                }
                None => {
                    let pos = self.powerups[pw].pos;
                    let mut near = std::mem::take(&mut self.scratch);
                    self.buckets.query(pos, 0.8, &mut near);
                    let taker = near.iter().copied().find(|&o| {
                        let q = &self.penguins[o as usize];
                        q.kind.fights()
                            && q.power.is_none()
                            && !q.cold
                            && q.inside.is_none()
                            && q.owner != NEUTRAL
                            && q.pos.dist(pos) < 0.8
                    });
                    self.scratch = near;
                    if let Some(t) = taker {
                        self.equip(t, pw as u8);
                    }
                }
            }
        }
    }

    fn check_victory(&mut self) {
        if self.winner.is_some() || self.players.is_empty() {
            return;
        }
        for pi in 0..self.players.len() {
            // Buildings keep a tribe in the game too. Only finished ones: a
            // tribe can't hide behind empty building sites.
            let standing = self.buildings.iter().any(|b| b.alive && b.owner == pi as u8 && b.done());
            if !self.players[pi].out && !standing && !self.igloos.iter().any(|ig| ig.owner == pi as u8) {
                self.players[pi].out = true;
                self.events.push(Event::PlayerOut { player: pi as u8 });
            }
        }
        let mut teams: Vec<u8> = self.players.iter().filter(|p| !p.out).map(|p| p.team).collect();
        teams.sort();
        teams.dedup();
        let started_teams = {
            let mut t: Vec<u8> = self.players.iter().map(|p| p.team).collect();
            t.sort();
            t.dedup();
            t.len()
        };
        if teams.len() == 1 && started_teams > 1 {
            self.winner = Some(teams[0]);
        }
        if let Some(team) = self.winner {
            self.events.push(Event::Victory { team });
            // Now it can be told: what each computer was really like.
            let reveals: Vec<(u8, String)> = self.ais.iter().filter(|a| a.temper.rolled).map(|a| (a.player, format!("{}{}.", crate::ai::REVEAL, a.temper.describe()))).collect();
            for (player, text) in reveals {
                self.ai_think(player, None, text);
            }
        }
    }
}

impl HitKind {
    fn max_hit(self, o: HitKind) -> HitKind {
        if (o as u8) > (self as u8) {
            o
        } else {
            self
        }
    }
}

/// Hooks for tests in `crates/sim/tests`, which can't reach private methods.
impl Game {
    #[doc(hidden)]
    pub fn retrain_for_test(&mut self, id: u16, kind: Kind) {
        self.retrain(id, kind);
    }
    #[doc(hidden)]
    pub fn ai_knows_all_for_test(&mut self, player: u8) {
        let n = self.igloos.len();
        for ai in self.ais.iter_mut().filter(|a| a.player == player) {
            ai.know_all(n);
        }
    }
    #[doc(hidden)]
    pub fn spawn_for_test(&mut self, igloo: u8, kind: Kind) -> u16 {
        self.spawn_penguin(igloo, kind)
    }
    #[doc(hidden)]
    pub fn place_finished_for_test(&mut self, owner: u8, kind: BuildKind, tile: (i32, i32)) -> u16 {
        self.add_building(owner, kind, tile, true)
    }
    #[doc(hidden)]
    pub fn aim_for_test(&mut self, v: u16, dmg: u8, from: Vec2) {
        self.aimed_at(u32::MAX, v, dmg, from);
    }
    #[doc(hidden)]
    pub fn slot_for_test(&self, v: u16) -> Option<Vec2> {
        self.penguins[v as usize].slot
    }
    #[doc(hidden)]
    pub fn hurt_for_test(&mut self, v: u16, dmg: u8, thrower: u16) {
        let pos = self.penguins[thrower as usize].pos;
        self.hurt(v, dmg, thrower, pos);
    }
    #[doc(hidden)]
    pub fn thaw_for_test(&mut self, v: u16, by: u8) {
        self.thaw(v, by);
    }
}
