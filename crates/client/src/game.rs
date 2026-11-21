//! The in-game screen: selection, orders, camera control and the HUD.

use std::collections::HashMap;

use crate::gfx::TextureWrap;
use crate::gpu::TextureId;
use crate::platform::KeyCode;
use sim::game::{Act, BuildKind, Command, Event, Kind, NEUTRAL};
use sim::map::{Ground, Obstacle};
use sim::math::{v2, Vec2};
use sim::proto::{ClientMsg, GameInfo};

use crate::audio::Sfx;
use crate::font::Align;
use crate::skin::{INK, INK_DIM};
use crate::gfx::{self, with_alpha, Color, Image};
use crate::mathx::v3;
use crate::scene::{BuildingDraw, Marker, MarkerKind, WorldRenderer};
use crate::ui::{rect, theme, Rect, Ui, MOUSE_L, MOUSE_M, MOUSE_R};
use crate::view::{player_color, GameView};

mod hud;
pub use hud::Console;

pub struct ChatLine {
    pub from: String,
    pub color: Color,
    pub text: String,
    pub private: bool,
    pub age: f32,
}

/// Build hotkeys (after B), chosen to stay clear of the camera and other keys.
pub const BUILD_KEYS: [(BuildKind, KeyCode, &str); 13] = [
    (BuildKind::Campfire, KeyCode::M, "M"),
    (BuildKind::Sauna, KeyCode::T, "T"),
    (BuildKind::Tower, KeyCode::C, "C"),
    (BuildKind::Store, KeyCode::F, "F"),
    (BuildKind::Bunkhouse, KeyCode::R, "R"),
    (BuildKind::Workshop, KeyCode::N, "N"),
    (BuildKind::Track, KeyCode::G, "G"),
    (BuildKind::Spa, KeyCode::K, "K"),
    (BuildKind::Palisade, KeyCode::J, "J"),
    (BuildKind::SkiLodge, KeyCode::Y, "Y"),
    (BuildKind::SlingRange, KeyCode::O, "O"),
    (BuildKind::PackerYard, KeyCode::X, "X"),
    (BuildKind::Igloo, KeyCode::P, "P"),
];

/// The War Cry hotkey, with a Chieftain selected.
pub const WAR_CRY_KEY: KeyCode = KeyCode::Y;

/// Seconds the pointer rests on a penguin before its card shows.
const UNIT_TIP_DELAY: f64 = 0.35;

/// A unit's card: its name, whose it is, and what it does, beats and fears.
pub(crate) fn unit_tip(kind: Kind, owner: u8, info: &GameInfo) -> (String, String, String) {
    let st = kind.stats();
    let whose = info.players.get(owner as usize).map_or(String::new(), |p| p.name.clone());
    let mut body = st.blurb.to_string();
    if !st.beats.is_empty() {
        body += &format!(" Good against: {}.", st.beats);
    }
    if !st.beaten_by.is_empty() {
        body += &format!(" Watch out for: {}.", st.beaten_by);
    }
    (st.name.to_string(), whose, body)
}

/// SNOWBALLZ_SHOW, the screenshot helper (see `GameScreen::update`).
static SHOW: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// Placing a building: the kind, and where a wall drag started.
#[derive(Clone, Copy)]
pub struct Placing {
    pub kind: BuildKind,
    pub drag: Option<(i32, i32)>,
}

/// How long a finger rests before it means "give an order here".
const HOLD: f32 = 0.4;

/// A finger on the ground, waiting to turn out a tap, a pan or a hold.
#[derive(Clone, Copy)]
struct Finger {
    start: [f32; 2],
    held: f32,
    pan: bool,
}

#[derive(PartialEq)]
pub enum GameAction {
    None,
    Leave,
    BackToLobby,
}

pub struct GameScreen {
    pub view: GameView,
    pub selected: Vec<u16>,
    sel_igloo: Option<u8>,
    sel_building: Option<u16>,
    /// The build menu is open (B): letter keys pick a building.
    pub build_menu: bool,
    pub placing: Option<Placing>,
    /// The penguin under the pointer, and since when (for its card).
    hover: Option<(u16, f64)>,
    /// Which page of the command card shows, and for which selection.
    card_page: u8,
    card_for: (Option<u8>, Option<u16>, Option<u16>, usize),
    last_rout_notice: f32,
    /// Ground under the mouse this frame.
    cursor: Option<Vec2>,
    groups: Vec<Vec<u16>>,
    last_group_key: (usize, f32),
    drag: Option<[f32; 2]>,
    mid: Option<([f32; 2], bool)>,
    minimap_drag: bool,
    finger: Option<Finger>,
    markers: Vec<Marker>,
    pub chat: Vec<ChatLine>,
    /// What each computer said it was like when the game ended, by name.
    reveals: Vec<(String, String)>,
    chat_open: bool,
    chat_input: String,
    last_from: Option<String>,
    last_to: Option<String>,
    notices: Vec<(String, Color, f32)>,
    /// What the computer players are thinking (sent to spectators), and
    /// which one the panel shows.
    ai_notes: Vec<(u8, Vec<String>)>,
    ai_shown: usize,
    /// The panel of what the computers think (F4): the self-test hides it, as
    /// it covers the left of the screen.
    pub(crate) ai_panel: bool,
    pub out: Vec<ClientMsg>,
    /// Interface sounds (alerts, fanfares) for the app to play.
    pub sounds: Vec<Sfx>,
    /// Length of the last frame, for camera speed.
    frame_dt: f32,
    show_help: bool,
    /// Which page of the help is open.
    help_page: usize,
    menu_open: bool,
    confirm_concede: bool,
    night_noticed: bool,
    /// Texture, fog and terrain versions, and when it was drawn.
    minimap: Option<(TextureId, u32, u32, f32)>,
    /// Where the latest alert was, and when: Space jumps back to it.
    alert: Option<(Vec2, f32)>,
    /// Rate limit for "your igloo is under attack!", so a long siege
    /// doesn't fill the screen with notices.
    last_attack_notice: f32,
    /// Rate limit for the frozen penguin notice, so a losing battle
    /// doesn't fill the screen with notices.
    last_freeze_notice: f32,
    /// When a cold penguin with no fire in reach was last reported.
    last_cold_notice: f32,
    /// When a newcomer from the sea was last reported.
    last_swim_notice: f32,
    /// The hp of my igloos in the previous snapshot: a drop is a siege.
    igloo_hp: HashMap<u8, u16>,
    /// When each igloo last got the siege notice: at most one per 20 s.
    last_siege_notice: HashMap<u8, f32>,
    /// Big centred line for an igloo changing hands: text, colour, age.
    banner: Option<(String, Color, f32)>,
    /// The end screen is up (it can be dismissed with "Watch on").
    show_score: bool,
    /// `SNOWBALLZ_ENDSCREEN`: pretend the game is over, for screenshots.
    fake_over: bool,
    pub is_host: bool,
    pub ping: f32,
    focused: bool,
    focus: Option<u64>,
    over_since: Option<f32>,
}

impl GameScreen {
    pub fn new(info: GameInfo, is_host: bool) -> GameScreen {
        let mut view = GameView::new(info);
        // Dev switch: show the end screen straight away with made-up numbers.
        let fake_over = std::env::var_os("SNOWBALLZ_ENDSCREEN").is_some();
        let mut reveals = Vec::new();
        if fake_over {
            view.stats.fill_fake();
            let info = view.info();
            let made_up = ["attacks early with small groups, breaks truces when it smells a win, builds a big economy, hardly builds towers, talks a lot, loves slingers", "says little", "plays it down the middle"];
            for (i, pl) in info.players.iter().enumerate().filter(|(i, _)| *i as u8 != info.you) {
                reveals.push((pl.name.clone(), made_up[i % made_up.len()].to_string()));
            }
        }
        GameScreen {
            view,
            selected: Vec::new(),
            sel_igloo: None,
            sel_building: None,
            build_menu: false,
            placing: None,
            hover: None,
            card_page: hud::PAGE_MAIN,
            card_for: (None, None, None, 0),
            last_rout_notice: -99.0,
            cursor: None,
            groups: vec![Vec::new(); 10],
            last_group_key: (99, 0.0),
            drag: None,
            mid: None,
            minimap_drag: false,
            finger: None,
            markers: Vec::new(),
            chat: Vec::new(),
            reveals,
            chat_open: false,
            chat_input: String::new(),
            last_from: None,
            last_to: None,
            notices: Vec::new(),
            ai_notes: Vec::new(),
            ai_shown: 0,
            ai_panel: true,
            out: Vec::new(),
            sounds: Vec::new(),
            frame_dt: 1.0 / 60.0,
            show_help: false,
            help_page: 0,
            menu_open: false,
            confirm_concede: false,
            night_noticed: false,
            minimap: None,
            alert: None,
            last_attack_notice: -99.0,
            last_freeze_notice: -99.0,
            last_cold_notice: -99.0,
            last_swim_notice: -99.0,
            igloo_hp: HashMap::new(),
            last_siege_notice: HashMap::new(),
            banner: None,
            show_score: true,
            fake_over,
            is_host,
            ping: 0.0,
            focused: false,
            focus: None,
            over_since: None,
        }
    }

    pub fn selected_igloo(&self) -> Option<u8> {
        self.sel_igloo
    }

    pub fn selected_building(&self) -> Option<u16> {
        self.sel_building
    }

    /// Self-test: select one of your buildings, as clicking it would.
    pub(crate) fn st_select_building(&mut self, b: u16) {
        self.selected.clear();
        self.sel_igloo = None;
        self.sel_building = Some(b);
    }

    /// Self-test: just right of the `i`th stamp (in the gap) of the card on a `w` by `h` screen.
    pub(crate) fn st_card_cell(&self, w: f32, h: f32, i: usize) -> [f32; 2] {
        let map = &self.view.info().map;
        let r = hud::Console::new(w, h, map.w, map.h, false).cell(i);
        [r.x + r.w + 3.0, r.y + r.h / 2.0]
    }

    fn workers_selected(&self) -> bool {
        self.selected.iter().any(|&id| self.view.world.penguins.get(id as usize).is_some_and(|p| p.kind == Kind::Worker))
    }

    /// Building is possible now: workers are picked, or nobody is and the
    /// site calls its own crew.
    fn can_build(&self) -> bool {
        self.workers_selected() || (self.selected.is_empty() && self.sel_igloo.is_none())
    }

    fn throwers_selected(&self) -> bool {
        self.selected.iter().any(|&id| self.view.world.penguins.get(id as usize).is_some_and(|p| p.kind.fights()))
    }

    /// Top-left tile for a building of `kind` centred under the cursor.
    pub(crate) fn site_at(kind: BuildKind, p: Vec2) -> (i32, i32) {
        let h = kind.size() as f32 / 2.0;
        ((p.x - h + 0.5).floor() as i32, (p.y - h + 0.5).floor() as i32)
    }

    /// Tiles a placement covers: a line of walls while dragging, else one site.
    fn placement_tiles(&self) -> Vec<(i32, i32)> {
        let (Some(pl), Some(c)) = (self.placing, self.cursor) else { return Vec::new() };
        let end = Self::site_at(pl.kind, c);
        let Some(start) = pl.drag.filter(|_| pl.kind == BuildKind::Palisade) else { return vec![end] };
        let (dx, dy) = (end.0 - start.0, end.1 - start.1);
        let n = dx.abs().max(dy.abs()).min(40);
        (0..=n)
            .map(|k| {
                let f = if n == 0 { 0.0 } else { k as f32 / n as f32 };
                (start.0 + (dx as f32 * f).round() as i32, start.1 + (dy as f32 * f).round() as i32)
            })
            .collect()
    }

    /// The client's best guess at whether a site is allowed (the server decides).
    /// Whether a campfire at `tile` has room to be built up into a Sauna
    /// (as `Game::sauna_site` sees it).
    pub(crate) fn sauna_room(&self, tile: (i32, i32)) -> bool {
        let (x, y) = tile;
        [(x, y), (x - 1, y), (x, y - 1), (x - 1, y - 1)]
            .into_iter()
            .any(|at| sim::game::footprint(BuildKind::Sauna, at).filter(|&t| t != tile).all(|t| self.site_ok(BuildKind::Campfire, t)))
    }

    pub(crate) fn site_ok(&self, kind: BuildKind, tile: (i32, i32)) -> bool {
        let info = self.view.info();
        let map = &info.map;
        let w = &self.view.world;
        for (x, y) in sim::game::footprint(kind, tile) {
            if !map.in_bounds(x, y) || map.obstacle_at(x, y).blocks() {
                return false;
            }
            let c = v2(x as f32 + 0.5, y as f32 + 0.5);
            let s = sim::map::IGLOO_SIZE;
            if info.igloos.iter().any(|ig| x >= ig.tile.0 && y >= ig.tile.1 && x < ig.tile.0 + s && y < ig.tile.1 + s || ig.door.dist(c) < 1.6) {
                return false;
            }
            if info.resources.iter().any(|r| r.pos.dist(c) < 1.2) {
                return false;
            }
            if w.buildings.iter().any(|b| b.alive && b.contains(c)) {
                return false;
            }
            // Blocks of ice are in the way too, like the server says.
            if w.penguins.iter().any(|p| p.act == Act::Frozen && p.pos.tile() == (x, y)) {
                return false;
            }
        }
        // A new igloo goes a good way from every other.
        if kind == BuildKind::Igloo {
            let c = sim::game::igloo_site_center(tile);
            let near = |at: sim::math::Vec2| at.dist(c) < sim::game::IGLOO_SPACING;
            if info.igloos.iter().any(|ig| near(ig.center)) || w.buildings.iter().any(|b| b.alive && b.kind == BuildKind::Igloo && near(b.center())) {
                return false;
            }
        }
        true
    }

    /// The placement grid: tile edges within `GRID_REACH` of the cursor,
    /// fading with distance, and blocked tiles shaded.
    fn draw_grid(&self, ui: &mut Ui, r: &dyn WorldRenderer, scale: f32, cursor: Vec2) {
        const GRID_REACH: i32 = 7;
        let map = &self.view.info().map;
        let (cx, cy) = cursor.tile();
        let corner = |x: i32, y: i32| -> Option<[f32; 2]> {
            let p = v2(x as f32, y as f32);
            let s = r.world_to_screen(v3(p.x, p.y, map.height_at(p) + 0.03))?;
            Some([s[0] / scale, s[1] / scale])
        };
        let fade = |x: f32, y: f32| -> f32 {
            let d = ((x - cursor.x).powi(2) + (y - cursor.y).powi(2)).sqrt() / GRID_REACH as f32;
            (1.0 - d).clamp(0.0, 1.0)
        };
        for y in cy - GRID_REACH..=cy + GRID_REACH {
            for x in cx - GRID_REACH..=cx + GRID_REACH {
                if !map.in_bounds(x, y) {
                    continue;
                }
                let k = fade(x as f32 + 0.5, y as f32 + 0.5);
                if k <= 0.0 {
                    continue;
                }
                let (Some(a), Some(b), Some(c), Some(d)) = (corner(x, y), corner(x + 1, y), corner(x + 1, y + 1), corner(x, y + 1)) else { continue };
                if !self.site_ok(BuildKind::Palisade, (x, y)) {
                    let (white, uv) = (ui.batch.white, ui.batch.white_uv);
                    ui.batch.quad_pts(ui.ctx, white, [a, b, c, d], uv, [255, 70, 60, (60.0 * k) as u8], [0; 4]);
                }
                // Each tile draws its top and left edges; the far edges come
                // from its neighbours (or the last row and column).
                let line = [255, 255, 255, (190.0 * k) as u8];
                ui.batch.line(ui.ctx, a, b, 1.0, line);
                ui.batch.line(ui.ctx, a, d, 1.0, line);
                if y == cy + GRID_REACH {
                    ui.batch.line(ui.ctx, d, c, 1.0, line);
                }
                if x == cx + GRID_REACH {
                    ui.batch.line(ui.ctx, b, c, 1.0, line);
                }
            }
        }
        // The footprint of what is being placed, outlined bold.
        let Some(pl) = self.placing else { return };
        let n = pl.kind.size();
        for t in self.placement_tiles() {
            let col = if self.site_ok(pl.kind, t) { [120, 255, 140, 230] } else { [255, 80, 70, 230] };
            let (x0, y0, x1, y1) = (t.0, t.1, t.0 + n, t.1 + n);
            let edge: Vec<(i32, i32)> = (x0..x1).map(|x| (x, y0)).chain((y0..y1).map(|y| (x1, y))).chain((x0 + 1..=x1).rev().map(|x| (x, y1))).chain((y0 + 1..=y1).rev().map(|y| (x0, y))).collect();
            for i in 0..edge.len() {
                let (a, b) = (edge[i], edge[(i + 1) % edge.len()]);
                if let (Some(a), Some(b)) = (corner(a.0, a.1), corner(b.0, b.1)) {
                    ui.batch.line(ui.ctx, a, b, 2.5, col);
                }
            }
        }
    }

    /// Placement previews for the renderer.
    pub fn ghosts(&self) -> Vec<BuildingDraw> {
        let Some(pl) = self.placing else { return Vec::new() };
        let tiles = self.placement_tiles();
        let color = player_color(self.view.info(), self.you());
        let map = &self.view.info().map;
        let n = tiles.len();
        tiles
            .iter()
            .enumerate()
            .map(|(i, &t)| {
                let h = pl.kind.size() as f32 / 2.0;
                let center = v2(t.0 as f32 + h, t.1 as f32 + h);
                // Walls in a line face along it.
                let along = if n > 1 {
                    let (a, b) = (tiles[i.saturating_sub(1)], tiles[(i + 1).min(n - 1)]);
                    (b.1 - a.1).abs() > (b.0 - a.0).abs()
                } else {
                    false
                };
                BuildingDraw {
                    kind: pl.kind,
                    center,
                    z: map.height_at(center),
                    yaws: vec![if along { std::f32::consts::FRAC_PI_2 } else { 0.0 }],
                    color,
                    progress: 1.0,
                    variant: ((t.0 * 7 + t.1 * 13) % 3) as u8,
                    selected: false,
                    ghost: Some(self.site_ok(pl.kind, t)),
                }
            })
            .collect()
    }

    fn pick_building(&self, ground: Vec2) -> Option<u16> {
        self.view.world.buildings.iter().position(|b| b.alive && b.contains(ground)).map(|i| i as u16)
    }

    fn start_placing(&mut self, kind: BuildKind) {
        self.build_menu = false;
        self.placing = Some(Placing { kind, drag: None });
    }

    fn place(&mut self, keep: bool) {
        let Some(pl) = self.placing else { return };
        let tiles: Vec<(i32, i32)> = self.placement_tiles().into_iter().filter(|&t| self.site_ok(pl.kind, t)).collect();
        if tiles.is_empty() {
            self.notice("Can't build there", theme::BAD);
            self.placing = Some(Placing { drag: None, ..pl });
            return;
        }
        let units = self.selected.clone();
        let h = pl.kind.size() as f32 / 2.0;
        let first = tiles[0];
        self.markers.push(Marker { pos: v2(first.0 as f32 + h, first.1 as f32 + h), age: 0.0, kind: MarkerKind::Gather });
        self.send(Command::Build { units, kind: pl.kind, tiles });
        self.placing = keep.then_some(Placing { drag: None, ..pl });
    }

    pub fn markers(&self) -> Vec<Marker> {
        self.markers.clone()
    }

    fn you(&self) -> u8 {
        self.view.info().you
    }

    fn spectating(&self) -> bool {
        self.you() == NEUTRAL
    }

    fn notice(&mut self, text: impl Into<String>, color: Color) {
        self.notices.push((text.into(), color, 0.0));
        if self.notices.len() > 4 {
            self.notices.remove(0);
        }
    }

    /// My Chieftain, if one of them is selected.
    fn selected_chief(&self) -> Option<u16> {
        let you = self.you();
        self.selected.iter().copied().find(|&id| {
            self.view.world.penguins.get(id as usize).is_some_and(|p| p.owner == you && p.kind == Kind::Chieftain)
        })
    }

    pub fn on_snapshot(&mut self, snap: &[u8]) {
        let events = self.view.apply(snap);
        let you = self.you();
        for e in events {
            let info = self.view.info();
            let name = |p: u8| info.players.get(p as usize).map_or("Nobody".to_string(), |pl| pl.name.clone());
            match e {
                Event::TakeoverStart { igloo, by } => {
                    let owner = self.view.world.igloos[igloo as usize].owner;
                    if owner == you && you != NEUTRAL {
                        let n = name(by);
                        let c = self.view.info().igloos[igloo as usize].center;
                        self.alert = Some((c, self.view.time));
                        // Rate-limited: a long siege would spam this.
                        if self.view.time - self.last_attack_notice > 8.0 {
                            self.last_attack_notice = self.view.time;
                            self.notice(format!("Your igloo is under attack! ({n} is taking it over; Space to look)"), theme::BAD);
                            self.sounds.push(Sfx::Alarm);
                        }
                    } else if by == you {
                        let text = if owner == NEUTRAL {
                            "Founding an igloo — keep workers inside."
                        } else {
                            "Takeover started — hold the igloo!"
                        };
                        self.notice(text, theme::ACCENT);
                    }
                }
                Event::TakeoverFailed { igloo } => {
                    if self.view.world.igloos[igloo as usize].owner == you {
                        self.notice("Igloo defended!", theme::GOOD);
                        self.sounds.push(Sfx::Defended);
                    }
                }
                Event::Captured { from, to, .. } => {
                    let (nf, nt) = (name(from), name(to));
                    let text = if to == you {
                        self.sounds.push(Sfx::Captured);
                        "Igloo captured — its penguins follow you".to_string()
                    } else if from == you {
                        self.sounds.push(Sfx::Lost);
                        format!("{nt} took one of your igloos!")
                    } else if from == NEUTRAL {
                        format!("{nt} claimed an empty igloo")
                    } else {
                        format!("{nt} took an igloo from {nf}")
                    };
                    self.banner = Some((text, rgba3(player_color(self.view.info(), to)), 0.0));
                }
                Event::Frozen { penguin } => {
                    if let Some(p) = self.view.world.penguins.get(penguin as usize) {
                        // The freeze already cleared ownership, so a penguin
                        // is yours if it came from one of your igloos.
                        let mine = you != NEUTRAL
                            && (p.owner == you
                                || self.view.world.igloos.get(p.home as usize).is_some_and(|ig| ig.owner == you));
                        if mine {
                            self.alert = Some((p.pos, self.view.time));
                            if p.kind == Kind::Chieftain {
                                self.notice("Your Chieftain froze! The tribe loses heart.", theme::BAD);
                                self.sounds.push(Sfx::Lost);
                            }
                            if self.view.time - self.last_freeze_notice > 15.0 {
                                self.last_freeze_notice = self.view.time;
                                self.notice("Your penguin froze. Send a Warmer before someone else does.", theme::BAD);
                            }
                        }
                    }
                }
                Event::SwamIn { penguin } => {
                    let mine = self.view.world.penguins.get(penguin as usize).is_some_and(|p| p.owner == you);
                    if mine && you != NEUTRAL && self.view.time - self.last_swim_notice > 30.0 {
                        self.last_swim_notice = self.view.time;
                        self.notice(format!("A newcomer swam in from the sea to fill a slot ({} fish).", sim::game::SEA_RECRUIT_FISH), theme::GOOD);
                    }
                }
                Event::NoFire { penguin } => {
                    let Some(p) = self.view.world.penguins.get(penguin as usize) else { continue };
                    if p.owner == you && you != NEUTRAL && self.view.time - self.last_cold_notice > 20.0 {
                        self.last_cold_notice = self.view.time;
                        self.alert = Some((p.pos, self.view.time));
                        let who = p.kind.stats().name.to_lowercase();
                        self.notice(format!("A cold {who} has no fire nearby and is walking home. Build a campfire (B, M) out there."), theme::BAD);
                    }
                }
                Event::Imprisoned { penguin, igloo } => {
                    let captor = self.view.world.igloos[igloo as usize].owner;
                    let victim = self.view.world.penguins.get(penguin as usize).map_or(NEUTRAL, |p| p.owner);
                    if victim == you && you != NEUTRAL {
                        let n = name(captor);
                        self.alert = Some((self.view.info().igloos[igloo as usize].center, self.view.time));
let chief = self.view.world.penguins.get(penguin as usize).is_some_and(|p| p.kind == Kind::Chieftain);
                        if chief {
                            self.notice(format!("{n} carried off your Chieftain! Take the igloo back to win it back."), theme::BAD);
                        } else {
                            self.notice(format!("{n} carried off one of your penguins! Take the igloo back to free it."), theme::BAD);
                        }
                        self.sounds.push(Sfx::Alarm);
                    } else if captor == you {
                        self.notice("Prisoner taken: it will join you soon.", theme::GOOD);
                    }
                }
                Event::Thawed { by, .. } => {
                    if by == you && you != NEUTRAL {
                        self.notice("Your Warmer thawed a frozen penguin: it's one of yours now.", theme::GOOD);
                    }
                }
                Event::TakenOver { from, by, .. } => {
                    if by == you && you != NEUTRAL {
                        self.notice("Your workers took over an enemy catapult: it's yours now.", theme::GOOD);
                    } else if from == you && you != NEUTRAL {
                        self.notice("The enemy took over your catapult: its crew were all frozen.", theme::BAD);
                    }
                }
                Event::BuildingLost { building } => {
                    let tower = self.view.world.buildings.get(building as usize).is_some_and(|b| b.kind == BuildKind::Tower && b.owner == you);
                    if tower {
                        self.notice("A watchtower fell, and its garrison tumbled down.", theme::BAD);
                    }
                }
                Event::Collapsed { owner, .. } => {
                    if you != NEUTRAL && owner == you {
                        self.notice("One of your igloos fell in!", theme::BAD);
                    } else if you != NEUTRAL && owner != NEUTRAL {
                        self.notice("You knocked down an enemy igloo.", theme::GOOD);
                    }
                }
                Event::Persuaded { by, .. } => {
                    if by == you {
                        self.notice("A wild penguin joined your tribe.", theme::GOOD);
                        self.sounds.push(Sfx::Chime);
                    }
                }
                Event::Converted { to, .. } => {
                    if to == you {
                        self.notice("A prisoner has joined your tribe!", theme::GOOD);
                        self.sounds.push(Sfx::Chime);
                    }
                }
                Event::Routed { penguin } => {
                    let mine = self.view.world.penguins.get(penguin as usize).is_some_and(|p| p.owner == you);
                    if mine && self.view.time - self.last_rout_notice > 8.0 {
                        self.last_rout_notice = self.view.time;
                        self.notice("Your penguins are losing heart and fleeing!", theme::BAD);
                    }
                }
                Event::SealSnatched { seal, penguin } => {
                    let victim = self.view.world.penguins.get(penguin as usize).map_or(NEUTRAL, |p| p.owner);
                    let at = self.view.world.seals.get(seal as usize).map(|s| s.pos);
                    if victim == you && you != NEUTRAL {
                        self.alert = at.map(|a| (a, self.view.time));
                        self.notice("A leopard seal took one of your workers!", theme::BAD);
                        self.sounds.push(Sfx::Alarm);
                    }
                }
                Event::SealDriven { by, .. } => {
                    if by == you && by != NEUTRAL {
                        self.notice("Bold snowballs drove the seal off!", theme::GOOD);
                    }
                }
                Event::SeasonChanged { season } => {
                    self.notice(format!("{} — {}", season.name(), season.blurb()), theme::ACCENT);
                }
                Event::Blizzard { on } => {
                    if on {
                        self.notice("A blizzard! Throws fall short and the cold bites. Saunas shelter.", [190, 220, 255, 255]);
                    } else {
                        self.notice("The blizzard has passed.", theme::TEXT_DIM);
                    }
                }
                Event::Victory { team } => {
                    let info = self.view.info();
                    if you != NEUTRAL {
                        let won = info.players[you as usize].team == team;
                        self.sounds.push(if won { Sfx::Victory } else { Sfx::Defeat });
                    }
                }
                Event::Upgraded { igloo, tier } => {
                    if self.view.world.igloos[igloo as usize].owner == you {
                        let t = sim::game::tier(tier).name;
                        self.notice(format!("Your igloo is now a {t}!"), theme::GOOD);
                        self.sounds.push(Sfx::Chime);
                    }
                }
                Event::ResearchDone { player, branch, level } => {
                    if player == you {
                        let b = &sim::game::BRANCH_TABLE[branch as usize];
                        let learned = b.levels.get(level.saturating_sub(1) as usize).map_or("", |l| l.name);
                        self.notice(format!("Your tribe has learnt {learned} ({})!", b.name), theme::GOOD);
                        self.sounds.push(Sfx::Built);
                    }
                }
                Event::WarCry { penguin, .. } => {
                    if self.view.world.penguins.get(penguin as usize).is_some_and(|p| p.owner == you) {
                        self.notice("Your Chieftain lets out a War Cry!", theme::ACCENT);
                    }
                }
                Event::PlayerOut { player } => {
                    let n = name(player);
                    if player == you {
                        self.notice("You've lost all your igloos.", theme::BAD);
                    } else {
                        self.notice(format!("{n} is out!"), theme::TEXT_DIM);
                    }
                }
                _ => {}
            }
        }
        // A drop in one of my igloos' hp means it is under siege.
        if you != NEUTRAL {
            let mut hit: Vec<u8> = Vec::new();
            for (i, ig) in self.view.world.igloos.iter().enumerate() {
                let id = i as u8;
                if ig.owner != you {
                    // Not mine: forget it, so a later recapture starts fresh.
                    self.igloo_hp.remove(&id);
                    self.last_siege_notice.remove(&id);
                    continue;
                }
                if self.igloo_hp.get(&id).is_some_and(|&prev| ig.hp < prev) {
                    hit.push(id);
                }
                self.igloo_hp.insert(id, ig.hp);
            }
            for id in hit {
                let last = self.last_siege_notice.get(&id).copied().unwrap_or(-99.0);
                if self.view.time - last > 20.0 {
                    self.last_siege_notice.insert(id, self.view.time);
                    self.alert = Some((self.view.info().igloos[id as usize].center, self.view.time));
                    self.notice("Your igloo is under siege!", theme::BAD);
                }
            }
        }
    }

    pub fn on_ai_notes(&mut self, notes: Vec<(u8, Vec<String>)>) {
        self.ai_notes = notes;
    }

    pub fn on_chat(&mut self, from: String, color: u8, text: String, private: bool) {
        let c = if color == 255 { theme::TEXT_DIM } else { rgba3(sim::game::PLAYER_COLORS[color as usize % 10]) };
        if private {
            if let Some((a, _)) = from.split_once(" → ") {
                if a != self.my_name() {
                    self.last_from = Some(a.to_string());
                }
            }
        }
        if from.split(" → ").next() != Some(self.my_name().as_str()) {
            self.sounds.push(Sfx::Chat);
        }
        if let Some(what) = text.strip_prefix(sim::ai::REVEAL) {
            self.reveals.push((from.clone(), what.trim_end_matches('.').to_string()));
        }
        self.chat.push(ChatLine { from, color: c, text, private, age: 0.0 });
        if self.chat.len() > 60 {
            self.chat.remove(0);
        }
    }

    fn my_name(&self) -> String {
        let info = self.view.info();
        info.players.get(info.you as usize).map_or(String::new(), |p| p.name.clone())
    }

    pub fn update(&mut self, dt: f32) {
        self.frame_dt = dt;
        // Screenshot helper (see README): keep our own igloo selected so the panel shows.
        static KEEP_IGLOO: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *KEEP_IGLOO.get_or_init(|| std::env::var_os("SNOWBALLZ_SELECT_IGLOO").is_some()) && self.sel_igloo.is_none() {
            let you = self.you();
            if let Some(i) = self.view.world.igloos.iter().position(|ig| ig.owner == you) {
                self.sel_igloo = Some(i as u8);
            }
        }
        // Screenshot helper (see README): select the workers and open the build menu.
        static SHOW_BUILD: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *SHOW_BUILD.get_or_init(|| std::env::var_os("SNOWBALLZ_SHOW_BUILD").is_some()) && !self.build_menu {
            let you = self.you();
            self.selected = self.view.world.penguins.iter().filter(|p| p.owner == you && p.kind == Kind::Worker).map(|p| p.id).take(3).collect();
            self.build_menu = !self.selected.is_empty();
        }
        // Screenshot helper: select every soldier.
        static SELECT_ARMY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *SELECT_ARMY.get_or_init(|| std::env::var_os("SNOWBALLZ_SELECT_ARMY").is_some()) && self.selected.is_empty() {
            let you = self.you();
            self.selected = self.view.world.penguins.iter().filter(|p| p.owner == you && p.kind.soldier() && !p.inside).map(|p| p.id).collect();
        }
        static SHOW_IGLOO: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *SHOW_IGLOO.get_or_init(|| std::env::var_os("SNOWBALLZ_SHOW_IGLOO").is_some()) && self.sel_igloo.is_none() {
            self.sel_igloo = self.view.my_igloos().first().copied();
        }
        // Screenshot helper: open the help ("help") or the game menu ("menu"),
        // or place a sauna in the middle of the view ("place").
        match SHOW.get_or_init(|| std::env::var("SNOWBALLZ_SHOW").ok()).as_deref() {
            Some("help") => self.show_help = true,
            Some(h) if h.starts_with("help:") && !self.show_help => {
                self.show_help = true;
                self.help_page = h[5..].parse().unwrap_or(0);
            }
            Some("menu") => self.menu_open = true,
            Some("place") if self.placing.is_none() => self.placing = Some(Placing { kind: BuildKind::Sauna, drag: None }),
            _ => {}
        }
        self.view.update(dt);
        for m in &mut self.markers {
            m.age += dt;
        }
        self.markers.retain(|m| m.age < 0.8);
        for n in &mut self.notices {
            n.2 += dt;
        }
        self.notices.retain(|n| n.2 < 6.0);
        let mut banner_done = false;
        if let Some(b) = &mut self.banner {
            b.2 += dt;
            banner_done = b.2 > 3.6;
        }
        if banner_done {
            self.banner = None;
        }
        for c in &mut self.chat {
            c.age += dt;
        }
        // Drop penguins that went inside or changed hands.
        let you = self.you();
        let w = &self.view.world;
        self.selected.retain(|&id| w.penguins.get(id as usize).is_some_and(|p| p.owner == you && !p.inside));
        if self.fake_over && self.over_since.is_none() {
            self.over_since = Some(self.view.time);
        }
        if self.view.world.winner.is_some() && self.over_since.is_none() {
            self.over_since = Some(self.view.time);
        }
    }

    fn send(&mut self, cmd: Command) {
        self.out.push(ClientMsg::Cmd(cmd));
    }

    // ---------------------------------------------------------------------
    // Input

    fn penguin_screen(&self, r: &dyn WorldRenderer, id: u16) -> Option<[f32; 2]> {
        let (pos, _) = self.view.world.penguin_at(id, self.view.clock as f32);
        let z = self.view.info().map.height_at(pos);
        r.world_to_screen(v3(pos.x, pos.y, z + 0.45))
    }

    fn pick_penguin(&self, r: &dyn WorldRenderer, at: [f32; 2], mine_only: bool) -> Option<u16> {
        let ground = r.screen_to_ground(at)?;
        let radius = (r.pixels_per_tile(ground) * 0.55).max(10.0);
        let you = self.you();
        let mut best: Option<(f32, u16)> = None;
        for p in &self.view.world.penguins {
            if p.inside || (mine_only && p.owner != you) {
                continue;
            }
            let Some(s) = self.penguin_screen(r, p.id) else { continue };
            let d = ((s[0] - at[0]).powi(2) + (s[1] - at[1]).powi(2)).sqrt();
            // Slight preference for our own penguins.
            let d = if p.owner == you { d * 0.8 } else { d };
            if d < radius && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, p.id));
            }
        }
        best.map(|(_, id)| id)
    }

    fn pick_igloo(&self, ground: Vec2) -> Option<u8> {
        self.view
            .info()
            .igloos
            .iter()
            .position(|ig| ig.center.dist(ground) < 2.4 || ig.door.dist(ground) < 0.9)
            .map(|i| i as u8)
    }

    fn pick_resource(&self, ground: Vec2) -> Option<u16> {
        self.view.info().resources.iter().position(|r| r.pos.dist(ground) < 0.9).map(|i| i as u16)
    }

    /// A tree under the cursor, if the selection has a worker to chop it.
    fn pick_tree(&self, ground: Vec2) -> Option<(i32, i32)> {
        let workers = self.selected.iter().any(|&id| self.view.world.penguins.get(id as usize).is_some_and(|p| p.kind == Kind::Worker));
        let (x, y) = ground.tile();
        (workers && self.view.info().map.obstacle_at(x, y) == Obstacle::Tree).then_some((x, y))
    }

    /// The selected building, if it is one of our finished training camps.
    /// A selected camp (right-click sets its rally point) or Catapult Tower
    /// (right-click aims it).
    fn selected_camp(&self) -> Option<u16> {
        let id = self.sel_building?;
        let b = self.view.world.buildings.get(id as usize)?;
        (b.alive && b.done() && b.owner == self.you() && (b.kind.trains().is_some() || b.kind == BuildKind::Catapult)).then_some(id)
    }

    /// The water tile under the pointer, if workers are selected to fish it.
    fn pick_water(&self, ground: Vec2) -> Option<(i32, i32)> {
        let (x, y) = ground.tile();
        (self.workers_selected() && self.view.info().map.ground_at(x, y) == Ground::Water).then_some((x, y))
    }

    fn order_at(&mut self, ground: Vec2, run: bool, queue: bool) {
        if self.selected.is_empty() {
            return;
        }
        let units = self.selected.clone();
        let you = self.you();
        let frozen_enemy = self.view.world.penguins.iter().filter(|p| {
            !p.inside && p.act == Act::Frozen && !p.carried && self.view.world.hostile(you, p.owner) && p.pos.dist(ground) < 0.8
        }).min_by(|a, b| a.pos.dist2(ground).partial_cmp(&b.pos.dist2(ground)).unwrap()).map(|p| p.id);
        // Warmers thaw whoever is frozen there: nobody's, or our own.
        let warmers: Vec<u16> =
            units.iter().copied().filter(|&u| self.view.world.penguins.get(u as usize).is_some_and(|p| p.kind == Kind::Warmer)).collect();
        let thaw = self.view.world.penguins.iter().filter(|p| {
            !p.inside && p.act == Act::Frozen && !p.carried && (p.owner == NEUTRAL || p.owner == you) && p.pos.dist(ground) < 0.8
        }).min_by(|a, b| a.pos.dist2(ground).partial_cmp(&b.pos.dist2(ground)).unwrap()).map(|p| p.id);
        if let (Some(penguin), false, false) = (thaw, warmers.is_empty(), run) {
            let pos = self.view.world.penguins[penguin as usize].pos;
            self.markers.push(Marker { pos, age: 0.0, kind: MarkerKind::Enter });
            self.send(Command::Thaw { units: warmers, penguin });
            return;
        }
        let (cmd, kind) = if let (Some(penguin), false) = (frozen_enemy, run) {
            (Command::Capture { units, penguin }, MarkerKind::Enter)
        } else if let Some(ig) = self.pick_igloo(ground) {
            (Command::Enter { units, igloo: ig }, MarkerKind::Enter)
        } else if let Some(res) = self.pick_resource(ground) {
            (Command::Gather { units, resource: res }, MarkerKind::Gather)
        } else if let (Some(tile), false) = (self.pick_tree(ground), run) {
            (Command::Chop { units, tile }, MarkerKind::Gather)
        } else if let (Some(tile), false) = (self.pick_water(ground), run) {
            (Command::Fish { units, tile }, MarkerKind::Gather)
        } else if let Some(b) = self.pick_building(ground).filter(|&b| {
            let v = &self.view.world.buildings[b as usize];
            !run && self.throwers_selected() && v.owner == self.you() && v.done() && v.kind == BuildKind::Tower
        }) {
            (Command::Garrison { units, building: b }, MarkerKind::Enter)
        } else if let Some(b) = self.pick_building(ground).filter(|&b| {
            let v = &self.view.world.buildings[b as usize];
            // (An enemy's too: if nobody crews it, the workers take it over.)
            let catapult = v.kind == BuildKind::Catapult;
            !run && self.workers_selected() && v.done() && (v.owner == self.you() && v.kind == BuildKind::Sauna && v.hp >= v.kind.max_hp() || catapult)
        }) {
            (Command::Tend { units, building: b }, MarkerKind::Enter)
        } else if let Some(b) = self.pick_building(ground).filter(|&b| {
            let v = &self.view.world.buildings[b as usize];
            !run && self.workers_selected() && v.owner == self.you() && (!v.done() || v.hp < v.kind.max_hp())
        }) {
            (Command::Repair { units, building: b }, MarkerKind::Gather)
        } else {
            (Command::Move { units, target: ground, run, queue }, if run { MarkerKind::Run } else { MarkerKind::Move })
        };
        let pos = match &cmd {
            Command::Enter { igloo, .. } => self.view.info().igloos[*igloo as usize].door,
            Command::Gather { resource, .. } => self.view.info().resources[*resource as usize].pos,
            Command::Chop { tile, .. } => v2(tile.0 as f32 + 0.5, tile.1 as f32 + 0.5),
            Command::Repair { building, .. } | Command::Tend { building, .. } | Command::Garrison { building, .. } => self.view.world.buildings[*building as usize].center(),
            Command::Capture { penguin, .. } => self.view.world.penguins[*penguin as usize].pos,
            _ => ground,
        };
        self.markers.push(Marker { pos, age: 0.0, kind });
        self.send(cmd);
    }

    fn box_select(&mut self, r: &dyn WorldRenderer, a: [f32; 2], b: [f32; 2], add: bool) {
        let (x0, x1) = (a[0].min(b[0]), a[0].max(b[0]));
        let (y0, y1) = (a[1].min(b[1]), a[1].max(b[1]));
        let you = self.you();
        let mut found: Vec<(u16, Kind)> = Vec::new();
        for p in &self.view.world.penguins {
            if p.owner != you || p.inside {
                continue;
            }
            if let Some(s) = self.penguin_screen(r, p.id) {
                if s[0] >= x0 && s[0] <= x1 && s[1] >= y0 && s[1] <= y1 {
                    found.push((p.id, p.kind));
                }
            }
        }
        // Prefer snowballers: workers are usually busy fishing.
        if found.iter().any(|(_, k)| k.soldier()) {
            found.retain(|(_, k)| k.soldier());
        }
        if !add {
            self.selected.clear();
        }
        for (id, _) in found {
            if !self.selected.contains(&id) {
                self.selected.push(id);
            }
        }
        if !self.selected.is_empty() {
            self.sel_igloo = None;
            self.sel_building = None;
        }
    }

    fn select_same_kind_on_screen(&mut self, r: &dyn WorldRenderer, kind: Kind, w: f32, h: f32) {
        self.box_select(r, [0.0, 0.0], [w, h], false);
        let world = &self.view.world;
        self.selected.retain(|&id| world.penguins[id as usize].kind == kind);
        if self.selected.is_empty() {
            // box_select dropped workers; collect them directly.
            let you = self.you();
            for p in &self.view.world.penguins {
                if p.owner == you && !p.inside && p.kind == kind {
                    if let Some(s) = self.penguin_screen(r, p.id) {
                        if s[0] >= 0.0 && s[0] <= w && s[1] >= 0.0 && s[1] <= h {
                            self.selected.push(p.id);
                        }
                    }
                }
            }
        }
    }

    fn centroid(&self, ids: &[u16]) -> Option<Vec2> {
        if ids.is_empty() {
            return None;
        }
        let s = ids.iter().fold(Vec2::ZERO, |a, &id| a + self.view.world.penguins[id as usize].pos);
        Some(s * (1.0 / ids.len() as f32))
    }

    /// Camera, selection and orders. Runs after the HUD so widgets get the mouse first.
    pub fn handle_input(&mut self, ui: &mut Ui, r: &mut dyn WorldRenderer, scale: f32, edge_scroll: bool) -> GameAction {
        let inp = &mut *ui.input;
        let phys = [inp.mouse[0] * scale, inp.mouse[1] * scale];
        let (pw, ph) = (ui.w * scale, ui.h * scale);
        let dt = self.frame_dt;

        // Wait for the first snapshot: preparing the renderer for the map resets its camera.
        if !self.focused && !self.view.world.penguins.is_empty() {
            self.focused = true;
            let home = self.view.my_igloos().first().map(|&i| self.view.info().igloos[i as usize].door);
            let c = home.unwrap_or(v2(self.view.info().map.w as f32 / 2.0, self.view.info().map.h as f32 / 2.0));
            r.reset_rotation();
            r.focus(c);
        }

        // Keyboard camera (not while Ctrl is held: Ctrl+A selects).
        if !inp.keyboard_used && !self.chat_open && !inp.ctrl() {
            let speed = 900.0 * dt * scale;
            let mut d = [0.0f32, 0.0];
            if inp.key(KeyCode::Left) || inp.key(KeyCode::A) {
                d[0] += speed;
            }
            if inp.key(KeyCode::Right) || inp.key(KeyCode::D) {
                d[0] -= speed;
            }
            if inp.key(KeyCode::Up) || inp.key(KeyCode::W) {
                d[1] += speed;
            }
            if inp.key(KeyCode::Down) || inp.key(KeyCode::S) {
                d[1] -= speed;
            }
            if d != [0.0, 0.0] {
                r.pan_pixels(d);
            }
            if inp.key(KeyCode::Q) {
                r.rotate(-1.8 * dt);
            }
            if inp.key(KeyCode::E) {
                r.rotate(1.8 * dt);
            }
            if inp.hit(KeyCode::PageUp) || inp.hit(KeyCode::Equal) || inp.hit(KeyCode::KpAdd) {
                r.zoom_at(0.8, [pw / 2.0, ph / 2.0]);
            }
            if inp.hit(KeyCode::PageDown) || inp.hit(KeyCode::Minus) || inp.hit(KeyCode::KpSubtract) {
                r.zoom_at(1.25, [pw / 2.0, ph / 2.0]);
            }
        }
        if edge_scroll && inp.inside && self.drag.is_none() {
            let m = 4.0;
            let speed = 700.0 * dt * scale;
            let mut d = [0.0f32, 0.0];
            if inp.mouse[0] < m {
                d[0] += speed;
            }
            if inp.mouse[0] > ui.w - m {
                d[0] -= speed;
            }
            if inp.mouse[1] < m {
                d[1] += speed;
            }
            if inp.mouse[1] > ui.h - m {
                d[1] -= speed;
            }
            if d != [0.0, 0.0] {
                r.pan_pixels(d);
            }
        }

        // Hotkeys.
        if !inp.keyboard_used && !self.chat_open {
            if inp.hit(KeyCode::Escape) {
                if self.show_help {
                    self.show_help = false;
                } else if self.placing.is_some() || self.build_menu {
                    self.placing = None;
                    self.build_menu = false;
                } else if !self.selected.is_empty() || self.sel_igloo.is_some() || self.sel_building.is_some() {
                    self.selected.clear();
                    self.sel_igloo = None;
                    self.sel_building = None;
                } else {
                    self.menu_open = !self.menu_open;
                }
            }
            if inp.hit(KeyCode::F1) || (inp.hit(KeyCode::Slash) && inp.mods.shift) {
                self.show_help = !self.show_help;
            }
            if inp.hit(KeyCode::Enter) || inp.hit(KeyCode::KpEnter) {
                self.chat_open = true;
                self.focus = Some(1);
                inp.keyboard_used = true;
                inp.chars.clear();
            }
            if inp.hit(KeyCode::Period) {
                if let Some(n) = self.last_from.clone() {
                    self.chat_open = true;
                    self.focus = Some(1);
                    self.chat_input = format!(">{n} ");
                    inp.chars.clear();
                }
            }
            if inp.hit(KeyCode::Comma) {
                if let Some(n) = self.last_to.clone() {
                    self.chat_open = true;
                    self.focus = Some(1);
                    self.chat_input = format!(">{n} ");
                    inp.chars.clear();
                }
            }
            if inp.hit(KeyCode::C) && !self.build_menu {
                self.chat.iter_mut().for_each(|c| c.age = 99.0);
            }
            // One key does one thing. What a key changes (closing the build
            // menu, picking an igloo) only counts from the next frame, or the
            // same press would also hit the keys that mean something else in
            // the new mode (B then H, O or Y).
            let menu_open = self.build_menu;
            let igloo_picked = self.sel_igloo;
            // B with no workers picked: a site calls its own crew, so nobody
            // need be picked.
            if inp.hit(KeyCode::B) && !self.workers_selected() && self.sel_igloo.is_none() {
                self.selected.clear();
                self.sel_building = None;
            }
            if self.can_build() {
                if inp.hit(KeyCode::B) {
                    self.build_menu = !self.build_menu;
                    self.placing = None;
                }
                if menu_open {
                    for (kind, key, _) in BUILD_KEYS {
                        if inp.hit(key) && kind.offered() {
                            self.start_placing(kind);
                        }
                    }
                }
            } else {
                self.build_menu = false;
                self.placing = None;
            }
            if (inp.hit(KeyCode::O) || inp.hit(KeyCode::Backspace)) && !self.selected.is_empty() && !menu_open {
                let units = self.selected.clone();
                self.send(Command::Stop { units });
            }
            // T is the build menu's sauna key too, so only hold when not building.
            if inp.hit(KeyCode::T) && !self.selected.is_empty() && !menu_open && self.placing.is_none() && igloo_picked.is_none() {
                let units = self.selected.clone();
                self.send(Command::Hold { units });
            }
            if inp.hit(KeyCode::Space) {
                // The alert sticks around, so Space can come back to it.
                if let Some((a, _)) = self.alert {
                    r.focus(a);
                } else if let Some(c) = self.centroid(&self.selected) {
                    r.focus(c);
                } else if let Some(&ig) = self.view.my_igloos().first() {
                    r.focus(self.view.info().igloos[ig as usize].door);
                }
            }
            if inp.hit(KeyCode::I) {
                if inp.mods.shift {
                    self.next_idle_fighter(r);
                } else {
                    self.next_idle_worker(r);
                }
            }
            // H picks your first igloo; with one picked it does nothing, so
            // Tab is what steps on to the next igloo.
            let cycle = inp.hit(KeyCode::Tab) || (inp.hit(KeyCode::H) && !menu_open && igloo_picked.is_none());
            if cycle {
                let mine = self.view.my_igloos();
                if !mine.is_empty() {
                    let cur = igloo_picked.and_then(|s| mine.iter().position(|&m| m == s));
                    let next = mine[cur.map_or(0, |c| (c + 1) % mine.len())];
                    self.sel_igloo = Some(next);
                    self.selected.clear();
                    r.focus(self.view.info().igloos[next as usize].door);
                }
            }
            if inp.hit(WAR_CRY_KEY) && !menu_open {
                if let Some(unit) = self.selected_chief() {
                    self.send(Command::WarCry { unit });
                }
            }
            if inp.ctrl() && inp.hit(KeyCode::A) {
                let you = self.you();
                self.selected = self
                    .view
                    .world
                    .penguins
                    .iter()
                    .filter(|p| p.owner == you && !p.inside && p.kind.soldier())
                    .map(|p| p.id)
                    .collect();
            }
            let digits = [
                KeyCode::Key0, KeyCode::Key1, KeyCode::Key2, KeyCode::Key3, KeyCode::Key4,
                KeyCode::Key5, KeyCode::Key6, KeyCode::Key7, KeyCode::Key8, KeyCode::Key9,
            ];
            for (n, k) in digits.iter().enumerate() {
                if inp.hit(*k) {
                    if inp.ctrl() {
                        self.groups[n] = self.selected.clone();
                        self.notice(format!("Group {n} set ({} penguins)", self.selected.len()), theme::TEXT_DIM);
                    } else {
                        let you = self.you();
                        let w = &self.view.world;
                        self.groups[n].retain(|&id| w.penguins.get(id as usize).is_some_and(|p| p.owner == you));
                        self.selected = self.groups[n].iter().copied().filter(|&id| !w.penguins[id as usize].inside).collect();
                        self.sel_igloo = None;
                        let now = self.view.time;
                        if self.last_group_key.0 == n && now - self.last_group_key.1 < 0.4 {
                            if let Some(c) = self.centroid(&self.selected) {
                                r.focus(c);
                            }
                        }
                        self.last_group_key = (n, now);
                    }
                }
            }
        }

        // Zoom with the wheel.
        if inp.wheel != 0.0 && !inp.mouse_used {
            r.zoom_at(if inp.wheel > 0.0 { 0.87 } else { 1.15 }, phys);
        }

        // Fingers: pinch to zoom, two fingers drag to pan.
        if inp.cancel {
            self.drag = None;
            self.finger = None;
        }
        if inp.pinch > 0.0 {
            r.zoom_at(1.0 / inp.pinch, phys);
        }
        if inp.pan2 != [0.0, 0.0] {
            r.pan_pixels([inp.pan2[0] * scale, inp.pan2[1] * scale]);
        }
        // One finger on the ground: a tap selects, a drag pans, a hold gives an
        // order, and a hold followed by a drag boxes a group.
        let mut order_at_finger = false;
        if inp.touch && self.placing.is_none() && !self.spectating() {
            if inp.pressed[MOUSE_L] && !inp.mouse_used {
                self.finger = Some(Finger { start: inp.mouse, held: 0.0, pan: false });
            }
            if let Some(mut f) = self.finger.take() {
                let far = (inp.mouse[0] - f.start[0]).abs() + (inp.mouse[1] - f.start[1]).abs() > 14.0;
                if inp.down[MOUSE_L] {
                    if far && f.held >= HOLD && !f.pan {
                        self.drag = Some([f.start[0] * scale, f.start[1] * scale]);
                    } else {
                        if far || f.pan {
                            f.pan = true;
                            r.pan_pixels([inp.mouse_delta[0] * scale, inp.mouse_delta[1] * scale]);
                        } else {
                            f.held += dt;
                        }
                        self.finger = Some(f);
                    }
                } else if inp.released[MOUSE_L] && !f.pan {
                    if f.held >= HOLD {
                        order_at_finger = true;
                    } else {
                        self.drag = Some(phys);
                    }
                }
            }
        }

        // Middle mouse: drag to pan, click to send a running order.
        if inp.pressed[MOUSE_M] && !inp.mouse_used {
            self.mid = Some((inp.mouse, false));
        }
        if let Some((start, moved)) = &mut self.mid {
            if inp.down[MOUSE_M] {
                if (inp.mouse[0] - start[0]).abs() + (inp.mouse[1] - start[1]).abs() > 6.0 {
                    *moved = true;
                }
                if *moved {
                    r.pan_pixels([inp.mouse_delta[0] * scale, inp.mouse_delta[1] * scale]);
                }
            }
            if inp.released[MOUSE_M] {
                let was_moved = *moved;
                self.mid = None;
                if !was_moved {
                    if let Some(g) = r.screen_to_ground(phys) {
                        self.order_at(g, true, false);
                    }
                }
            }
        }

        if self.spectating() {
            return GameAction::None;
        }

        self.cursor = r.screen_to_ground(phys);
        if SHOW.get().is_some_and(|s| s.as_deref() == Some("place")) {
            self.cursor = Some(r.center() + v2(2.5, 1.5));
        }
        // Placing a building: left click (or drag, for walls) places, right click cancels.
        if let Some(pl) = self.placing {
            if inp.pressed[MOUSE_R] && !inp.mouse_used {
                self.placing = None;
                inp.mouse_used = true;
            } else if inp.pressed[MOUSE_L] && !inp.mouse_used {
                if let Some(c) = self.cursor {
                    self.placing = Some(Placing { drag: Some(Self::site_at(pl.kind, c)), ..pl });
                }
                inp.mouse_used = true;
            } else if inp.released[MOUSE_L] && pl.drag.is_some() {
                let keep = inp.mods.shift;
                self.place(keep);
            }
            return GameAction::None;
        }

        // Left: select.
        if inp.pressed[MOUSE_L] && !inp.mouse_used && !inp.touch {
            self.drag = Some(phys);
        }
        if let Some(start) = self.drag {
            if inp.released[MOUSE_L] || !inp.down[MOUSE_L] {
                self.drag = None;
                let add = inp.mods.shift;
                let moved = (phys[0] - start[0]).abs() + (phys[1] - start[1]).abs() > 8.0;
                if moved {
                    self.box_select(r, start, phys, add);
                } else if let Some(id) = self.pick_penguin(r, phys, true) {
                    if inp.double_click {
                        let kind = self.view.world.penguins[id as usize].kind;
                        self.select_same_kind_on_screen(r, kind, pw, ph);
                    } else if add {
                        if let Some(i) = self.selected.iter().position(|&s| s == id) {
                            self.selected.remove(i);
                        } else {
                            self.selected.push(id);
                        }
                    } else {
                        self.selected = vec![id];
                    }
                    self.sel_igloo = None;
                    self.sel_building = None;
                } else if let Some(ig) = r.screen_to_ground(phys).and_then(|g| self.pick_igloo(g)) {
                    self.sel_igloo = Some(ig);
                    self.sel_building = None;
                    if !add {
                        self.selected.clear();
                    }
                } else if let Some(b) = r.screen_to_ground(phys).and_then(|g| self.pick_building(g)) {
                    self.sel_building = Some(b);
                    self.sel_igloo = None;
                    if !add {
                        self.selected.clear();
                    }
                } else if !add {
                    self.selected.clear();
                    self.sel_igloo = None;
                    self.sel_building = None;
                }
            }
        }

        // Right (or a held finger): order.
        if (inp.pressed[MOUSE_R] && !inp.mouse_used) || order_at_finger {
            if let Some(g) = r.screen_to_ground(phys) {
                if let Some(camp) = self.selected_camp().filter(|_| self.selected.is_empty()) {
                    // A camp is selected: right-click sets its rally point.
                    // A catapult aims at the building or igloo under the pointer.
                    let at = self.pick_building(g).map(|b| self.view.world.buildings[b as usize].center()).unwrap_or(g);
                    self.markers.push(Marker { pos: at, age: 0.0, kind: MarkerKind::Enter });
                    self.send(Command::SetRally { building: camp, at });
                } else {
                    self.order_at(g, inp.ctrl(), inp.mods.shift);
                }
            }
        }
        GameAction::None
    }

    // ---------------------------------------------------------------------
    // HUD

    /// Draw world-anchored overlays and the HUD. Returns an action.
    pub fn draw_hud(&mut self, ui: &mut Ui, r: &mut dyn WorldRenderer, scale: f32) -> GameAction {
        let mut action = GameAction::None;
        let info_you = self.you();
        let t = self.view.clock as f32;
        // A finger resting on the ground fills a ring: when it's full, lifting
        // gives an order there.
        if let Some(f) = self.finger.filter(|f| !f.pan && f.held > 0.08) {
            let k = (f.held / HOLD).min(1.0);
            let a = if k >= 1.0 { [120, 255, 140, 110] } else { [255, 255, 255, 70] };
            ui.batch.ellipse(ui.ctx, f.start[0], f.start[1], 10.0 + 26.0 * k, 10.0 + 26.0 * k, a);
        }
        // No right button on a phone: a way out of placing a building.
        if ui.input.touch && self.placing.is_some() {
            let b = rect(ui.w / 2.0 - 60.0, 64.0, 120.0, 40.0);
            if ui.stamp_text(b, "Cancel", true) {
                self.placing = None;
            }
        }

        // Placing a building: the tile grid round the cursor, fading out,
        // with tiles nothing can go on shaded red.
        if let (Some(_), Some(c)) = (self.placing, self.cursor) {
            self.draw_grid(ui, r, scale, c);
        }

        // Drag box.
        if let Some(s) = self.drag {
            let (a, b) = ([s[0] / scale, s[1] / scale], ui.input.mouse);
            let (x, y, w, h) = (a[0].min(b[0]), a[1].min(b[1]), (a[0] - b[0]).abs(), (a[1] - b[1]).abs());
            if w + h > 8.0 {
                ui.batch.rect(ui.ctx, x, y, w, h, [120, 255, 140, 30]);
                ui.batch.rect_outline(ui.ctx, x, y, w, h, 1.5, [120, 255, 140, 200]);
            }
        }

        // Warmth bars over penguins.
        let ppt = r.pixels_per_tile(r.center()) / scale;
        // Remember where the camera is looking: only shake for what it sees.
        self.view.camera = r.center();
        self.view.view_radius = if ppt > 0.0 { ui.w / ppt * 0.75 } else { 40.0 };
        if ppt > 10.0 {
            for p in &self.view.world.penguins {
                if p.inside {
                    continue;
                }
                let sel = self.selected.contains(&p.id);
                let hurt = p.warmth < p.max_warmth && p.act != Act::Frozen;
                if !(sel || p.routed || (hurt && ppt > 16.0)) {
                    continue;
                }
                let (pos, _) = self.view.world.penguin_at(p.id, t);
                let z = self.view.info().map.height_at(pos);
                let Some(s) = r.world_to_screen(v3(pos.x, pos.y, z + 1.15)) else { continue };
                let (x, y) = (s[0] / scale, s[1] / scale);
                if p.routed {
                    // A little white flag: fleeing.
                    ui.batch.rect(ui.ctx, x + 10.0, y - 16.0, 1.5, 16.0, [90, 70, 50, 255]);
                    ui.batch.rect(ui.ctx, x + 11.5, y - 16.0, 9.0, 6.0, [250, 250, 250, 255]);
                }
                if sel {
                    let w = (ppt * 0.7).clamp(14.0, 34.0);
                    let f = p.courage as f32 / sim::game::MAX_COURAGE as f32;
                    ui.bar(rect(x - w / 2.0, y + 5.0, w, 3.0), f, [235, 190, 80, 255]);
                }
                let w = (ppt * 0.7).clamp(14.0, 34.0);
                let f = p.warmth as f32 / p.max_warmth.max(1) as f32;
                let c = if p.cold { [120, 190, 255, 255] } else { warmth_color(f) };
                ui.bar(rect(x - w / 2.0, y, w, 4.0), f, c);
                if sel && p.kind.fights() {
                    for i in 0..p.snowballs.min(4) {
                        ui.batch.ellipse(ui.ctx, x - w / 2.0 + 3.0 + i as f32 * 6.0, y - 4.0, 2.2, 2.2, [255, 255, 255, 230]);
                    }
                }
            }
        }

        // Igloo labels.
        for (i, ig) in self.view.info().igloos.iter().enumerate() {
            let v = &self.view.world.igloos[i];
            let z = self.view.info().map.height_at(ig.center);
            let Some(s) = r.world_to_screen(v3(ig.center.x, ig.center.y, z + 2.9)) else { continue };
            let (x, y) = (s[0] / scale, s[1] / scale);
            if x < -100.0 || y < -100.0 || x > ui.w + 100.0 || y > ui.h + 100.0 {
                continue;
            }
            if let Some(tk) = v.takeover {
                let secs = tk.ticks_left as f32 / sim::TICK_RATE as f32;
                let c = rgba3(player_color(self.view.info(), tk.by));
                let label = format!("Takeover {}:{:02}", secs as u32 / 60, secs.ceil() as u32 % 60);
                ui.text_shadow(&label, x, y - 22.0, 18.0, c, Align::Center);
            }
            if v.owner == info_you && ppt > 12.0 {
                let mut label = format!("Fish {}", v.fish);
                if v.wood > 0 {
                    label += &format!("  Wood {}", v.wood);
                }
                if v.crystal > 0 {
                    label += &format!("  Crystal {}", v.crystal);
                }
                ui.text_shadow(&label, x, y, 14.0, theme::TEXT, Align::Center);
            }
        }

        // The rally flag of the selected camp.
        if let Some(rally) = self.selected_camp().and_then(|id| self.view.world.buildings[id as usize].rally) {
            let z = self.view.info().map.height_at(rally);
            if let Some(s) = r.world_to_screen(v3(rally.x, rally.y, z)) {
                let (x, y) = (s[0] / scale, s[1] / scale);
                ui.batch.rect(ui.ctx, x, y - 22.0, 2.0, 22.0, [90, 70, 50, 255]);
                ui.batch.rect(ui.ctx, x + 2.0, y - 22.0, 12.0, 8.0, [235, 190, 80, 255]);
            }
        }

        // A pip per slot over your buildings that have them, so you can see
        // at a glance who is where: solid for a filled slot, filling up for
        // one in training, hollow for an empty one. Camps show their
        // specialists, stores their fishing crew, towers their sentries, and
        // igloos (in front of the door) their guards.
        if ppt > 8.0 {
            let pips = |ui: &mut Ui, x: f32, y: f32, slots: u8, filled: u8, training: u8| {
                let (pip, gap) = (10.0, 4.0);
                let x0 = x - (slots as f32 * (pip + gap) - gap) / 2.0;
                for k in 0..slots {
                    let px = x0 + k as f32 * (pip + gap);
                    ui.batch.rect(ui.ctx, px - 1.5, y - 1.5, pip + 3.0, pip + 3.0, [20, 30, 50, 220]);
                    let fill = if k < filled {
                        1.0
                    } else if k == filled && training > 0 {
                        (training - 1) as f32 / 254.0
                    } else {
                        0.0
                    };
                    ui.batch.rect(ui.ctx, px, y, pip, pip, [70, 80, 100, 230]);
                    if fill > 0.0 {
                        let col = if fill >= 1.0 { theme::ACCENT } else { [235, 190, 80, 255] };
                        ui.batch.rect(ui.ctx, px, y + pip * (1.0 - fill), pip, pip * fill, col);
                    }
                }
            };
            for b in self.view.world.buildings.iter().filter(|b| b.alive && b.owner == info_you && b.slots > 0 && b.done()) {
                let c = b.center();
                let z = self.view.info().map.height_at(c);
                // Over the top of it: a tower is tall.
                let above = if matches!(b.kind, BuildKind::Tower | BuildKind::Catapult) { 3.4 } else { 1.9 };
                let Some(s) = r.world_to_screen(v3(c.x, c.y, z + above)) else { continue };
                // Only the slots you want filled.
                let want = b.want.min(b.slots);
                pips(ui, s[0] / scale, s[1] / scale, want, b.filled.saturating_sub(b.trainees).min(want), b.training);
            }
            let info = self.view.info();
            // And the igloo's fishing slots, over its dome.
            for (i, ig) in self.view.world.igloos.iter().enumerate() {
                if ig.owner != info_you || ig.fish_want == 0 || ig.hp == 0 {
                    continue;
                }
                let Some(at) = info.igloos.get(i).map(|v| v.center) else { continue };
                let z = info.map.height_at(at);
                let Some(s) = r.world_to_screen(v3(at.x, at.y, z + 3.2)) else { continue };
                pips(ui, s[0] / scale, s[1] / scale, ig.fish_want, ig.fishers.min(ig.fish_want), 0);
            }
        }

        // Progress and strength bars over building sites and damaged buildings.
        if ppt > 8.0 {
            for (i, b) in self.view.world.buildings.iter().enumerate() {
                let sel = self.sel_building == Some(i as u16);
                if !b.alive || (b.done() && b.hp >= b.kind.max_hp() && !sel) {
                    continue;
                }
                let c = b.center();
                let z = self.view.info().map.height_at(c);
                let Some(s) = r.world_to_screen(v3(c.x, c.y, z + 1.3)) else { continue };
                let (x, y) = (s[0] / scale, s[1] / scale);
                let bw = 34.0 * b.kind.size() as f32;
                let (f, col) = if b.done() {
                    let f = b.hp as f32 / b.kind.max_hp() as f32;
                    (f, warmth_color(f))
                } else {
                    (b.built as f32 / b.kind.snow() as f32, theme::ACCENT)
                };
                ui.batch.rect(ui.ctx, x - bw / 2.0 - 1.0, y - 1.0, bw + 2.0, 6.0, [20, 30, 50, 200]);
                ui.batch.rect(ui.ctx, x - bw / 2.0, y, bw * f, 4.0, col);
                if b.starved {
                    ui.text_shadow("no snow!", x, y - 18.0, 13.0, theme::BAD, Align::Center);
                } else if b.waiting_for_wood() {
                    ui.text_shadow("needs wood", x, y - 18.0, 13.0, theme::TEXT_DIM, Align::Center);
                }
            }
        }

        // What placing a building does.
        if let Some(pl) = self.placing {
            let wall = if pl.kind == BuildKind::Palisade { ", drag for a line" } else { "" };
            let hint = format!("{}: click to place{wall}, Shift to keep placing, right-click to cancel", pl.kind.name());
            ui.text_shadow(&hint, ui.w / 2.0, 66.0, 17.0, theme::ACCENT, Align::Center);
        }

        let mm = self.draw_console(ui, r, scale);
        // The wheel over the console never zooms the map behind it.
        let c = self.console(ui);
        if ui.input.wheel != 0.0 && (c.bar.contains(ui.input.mouse) || c.frame.contains(ui.input.mouse)) {
            ui.input.wheel = 0.0;
            ui.input.mouse_used = true;
        }
        self.draw_top_bar(ui);
        let console_top = self.console(ui).frame.y;
        // Phones have no Space, Tab or Ctrl+A: buttons down the left edge.
        if ui.input.touch && !self.spectating() {
            let mut y = 64.0;
            if self.alert.is_some() && ui.stamp_text(rect(8.0, y, 74.0, 34.0), "Alert", true) {
                if let Some((a, _)) = self.alert {
                    r.focus(a);
                }
            }
            if self.alert.is_some() {
                y += 40.0;
            }
            if ui.stamp_text(rect(8.0, y, 74.0, 34.0), "Army", true) {
                let you = self.you();
                self.selected = self
                    .view
                    .world
                    .penguins
                    .iter()
                    .filter(|p| p.owner == you && !p.inside && p.kind.soldier())
                    .map(|p| p.id)
                    .collect();
                self.sel_igloo = None;
                self.sel_building = None;
            }
            y += 40.0;
            if ui.stamp_text(rect(8.0, y, 74.0, 34.0), "Home", true) {
                if let Some(&ig) = self.view.my_igloos().first() {
                    self.sel_igloo = Some(ig);
                    self.selected.clear();
                    r.focus(self.view.info().igloos[ig as usize].door);
                }
            }
            y += 40.0;
            // Control groups: "Save" files the selection under the next free
            // number, and a numbered button calls a group back (twice: look at it).
            if !self.selected.is_empty() && ui.stamp_text(rect(8.0, y, 74.0, 34.0), "Save", true) {
                let n = (1..10).find(|&n| self.groups[n].is_empty()).unwrap_or(9);
                self.groups[n] = self.selected.clone();
                self.notice(format!("Group {n} set ({} penguins)", self.selected.len()), theme::TEXT_DIM);
            }
            if !self.selected.is_empty() {
                y += 40.0;
            }
            let you = self.you();
            for n in 1..10 {
                let w = &self.view.world;
                self.groups[n].retain(|&id| w.penguins.get(id as usize).is_some_and(|p| p.owner == you));
                // Stop above the minimap; the number keys still reach the rest.
                if self.groups[n].is_empty() || y + 34.0 > console_top - 6.0 {
                    continue;
                }
                if ui.stamp_text(rect(8.0, y, 74.0, 34.0), &format!("{n} ({})", self.groups[n].len()), true) {
                    let now = self.view.time;
                    let w = &self.view.world;
                    let ids: Vec<u16> = self.groups[n].iter().copied().filter(|&id| !w.penguins[id as usize].inside).collect();
                    let again = self.last_group_key.0 == n && now - self.last_group_key.1 < 0.6;
                    self.selected = ids;
                    self.sel_igloo = None;
                    self.sel_building = None;
                    if again {
                        if let Some(c) = self.centroid(&self.selected) {
                            r.focus(c);
                        }
                    }
                    self.last_group_key = (n, now);
                }
                y += 40.0;
            }
        }
        self.draw_chat(ui);
        self.draw_ai_panel(ui);
        self.draw_notices(ui);

        // An igloo changing hands: a short banner in the new owner's colour.
        if let Some((text, c, age)) = &self.banner {
            let a = (1.0 - ((age - 2.6).max(0.0) / 1.0)).clamp(0.0, 1.0);
            let y = ui.h * 0.24;
            let tw = (ui.font.measure(text, 30.0) + 60.0).min(ui.w - 40.0);
            ui.fill(rect(ui.w / 2.0 - tw / 2.0, y - 10.0, tw, 46.0), 8.0, with_alpha([44, 24, 12, 200], a));
            ui.text_shadow(text, ui.w / 2.0, y - 2.0, 30.0, with_alpha(*c, a), Align::Center);
        }

        // Minimap interaction.
        if ui.input.pressed[MOUSE_L] && mm.contains(ui.input.mouse) {
            self.minimap_drag = true;
            ui.input.mouse_used = true;
        }
        if !ui.input.down[MOUSE_L] {
            self.minimap_drag = false;
        }
        let map = &self.view.info().map;
        let to_map = |m: [f32; 2]| v2((m[0] - mm.x) / mm.w * map.w as f32, (m[1] - mm.y) / mm.h * map.h as f32);
        if self.minimap_drag {
            r.focus(to_map(ui.input.mouse));
        }
        if ui.input.pressed[MOUSE_R] && mm.contains(ui.input.mouse) {
            ui.input.mouse_used = true;
            let g = to_map(ui.input.mouse);
            self.order_at(g, ui.input.ctrl(), ui.input.mods.shift);
        }

        let over = self.over_since.is_some_and(|since| self.fake_over || self.view.time - since > 1.5);
        if over && self.show_score {
            action = self.draw_game_over(ui);
        } else if over {
            // "Watch on" dismissed it; keep a way back to the numbers.
            if ui.stamp_text(rect(ui.w / 2.0 - 80.0, 64.0, 160.0, 32.0), "Final score", true) {
                self.show_score = true;
            }
        }
        if self.show_help {
            self.draw_help(ui);
        }
        if self.menu_open {
            let w = 300.0;
            let p = rect(ui.w / 2.0 - w / 2.0, ui.h / 2.0 - 160.0, w, 320.0);
            ui.panel(p);
            ui.text("Paused? Never!", p.x + w / 2.0, p.y + 20.0, 22.0, theme::TEXT, Align::Center);
            if ui.button(rect(p.x + 30.0, p.y + 70.0, w - 60.0, 44.0), "Resume") {
                self.menu_open = false;
            }
            if ui.button(rect(p.x + 30.0, p.y + 124.0, w - 60.0, 44.0), "Leave game") {
                action = GameAction::Leave;
            }
            // Giving up needs a second click.
            if self.view.world.info.you != NEUTRAL && !self.view.world.out.get(self.you() as usize).copied().unwrap_or(false) {
                let label = if self.confirm_concede { "Really concede? Click again" } else { "Concede" };
                if ui.button(rect(p.x + 30.0, p.y + 178.0, w - 60.0, 44.0), label) {
                    if self.confirm_concede {
                        self.send(Command::Concede);
                        self.confirm_concede = false;
                        self.menu_open = false;
                    } else {
                        self.confirm_concede = true;
                    }
                }
                ui.text("Your penguins freeze and your igloos fall in.", p.x + w / 2.0, p.y + 236.0, 13.0, theme::TEXT_DIM, Align::Center);
            }
        }
        // Tooltip for the penguin under the cursor, or the ruin under it. A
        // tribe's penguin gets a card once the pointer has rested on it:
        // what it does, what it beats and what beats it.
        let hovering = self.placing.is_none() && !self.build_menu && !self.show_help
            && !self.menu_open && !self.chat_open && !ui.input.mouse_used
            && !(over && self.show_score);
        let phys = [ui.input.mouse[0] * scale, ui.input.mouse[1] * scale];
        let under = if hovering { self.pick_penguin(r, phys, false) } else { None };
        self.hover = match (under, self.hover) {
            (Some(id), Some((h, since))) if h == id => Some((id, since)),
            (Some(id), _) => Some((id, ui.time)),
            (None, _) => None,
        };
        let mut card = None;
        let tip: Option<&str> = if hovering {
            let penguin_tip = under.and_then(|id| self.view.world.penguins.get(id as usize)).and_then(|p| {
                // Frozen wins: freezing drops the tribe, so a frozen
                // penguin also looks wild.
                if p.act == Act::Frozen {
                    Some("Frozen. Blocks the way and stops snowballs. Warmers can thaw it into your tribe; left alone it thaws by itself.")
                } else if p.owner == NEUTRAL {
                    Some("Wild. Stay close and it may join you.")
                } else {
                    if self.hover.is_some_and(|(_, since)| ui.time - since >= UNIT_TIP_DELAY) {
                        card = Some((p.kind, p.owner));
                    }
                    None
                }
            });
            if penguin_tip.is_some() || under.is_some() {
                penguin_tip
            } else {
                r.screen_to_ground(phys)
                    .and_then(|g| self.pick_igloo(g))
                    .and_then(|ig| self.view.world.igloos.get(ig as usize))
                    .filter(|ig| ig.hp == 0)
                    .map(|_| "Ruin: an igloo knocked down. Nobody can use it.")
            }
        } else {
            None
        };
        if let Some(tip) = tip {
            let tw = ui.font.measure(tip, 13.0) + 16.0;
            let mut x = ui.input.mouse[0] + 14.0;
            let mut y = ui.input.mouse[1] + 16.0;
            if x + tw > ui.w - 4.0 {
                x = (ui.input.mouse[0] - tw - 8.0).max(4.0);
            }
            if y + 30.0 > ui.h - 4.0 {
                y = (ui.input.mouse[1] - 38.0).max(4.0);
            }
            ui.paper(rect(x, y, tw, 26.0), 5.0);
            ui.text_in(tip, rect(x, y, tw, 26.0), 13.0, crate::skin::INK, Align::Center);
        }
        if let Some((kind, owner)) = card {
            let (title, note, body) = unit_tip(kind, owner, self.view.info());
            let m = ui.input.mouse;
            ui.slip(rect(m[0] - 1.0, m[1] - 24.0, 2.0, 48.0), 340.0, &title, &note, &body);
        }
        action
    }

    fn ensure_minimap(&mut self, ui: &mut Ui) -> TextureId {
        // Redraw for new fog at once, for worn ground once a second.
        let (region, terrain, now) = (self.view.tint_version, self.view.world.terrain_version, self.view.time);
        if let Some((tex, r, t, at)) = self.minimap {
            if r == region && (t == terrain || now - at < 1.0) {
                return tex;
            }
            ui.ctx.delete_texture(tex);
        }
        let map = &self.view.info().map;
        let tint = crate::render3d::tint_texture(map, self.view.fog.as_deref(), self.view.night_ring());
        let mut img = Image { w: map.w as u32, h: map.h as u32, rgba: vec![0; (map.w * map.h * 4) as usize] };
        for y in 0..map.h {
            for x in 0..map.w {
                let i = map.idx(x, y);
                let shade = 0.85 + map.height[i] / sim::map::HEIGHT_SCALE * 0.15;
                let mut c = match (map.obstacle[i], map.ground[i]) {
                    (Obstacle::Tree, _) => [70.0, 120.0, 90.0],
                    (Obstacle::Pinnacle, _) => [120.0, 170.0, 220.0],
                    (_, Ground::Water) => [40.0, 100.0, 160.0],
                    (_, Ground::Ice) => [150.0, 195.0, 230.0],
                    (_, Ground::Snow) => [235.0, 240.0, 250.0],
                };
                let (tr, tb, ta) = (tint[i * 4], tint[i * 4 + 2], tint[i * 4 + 3]);
                if ta == 255 && tb == 0 {
                    // Never seen.
                    c = [4.0, 6.0, 12.0];
                } else if tb == 255 {
                    // Remembered: dark first, then the old paper map.
                    let k = tr as f32 / 255.0;
                    let paper = [196.0, 168.0, 118.0];
                    for j in 0..3 {
                        c[j] = (c[j] * 0.35) * (1.0 - k) + paper[j] * k * 0.9;
                        c[j] *= shade;
                    }
                } else {
                    let a = ta as f32 / 255.0 * 1.6;
                    let ink = [tint[i * 4] as f32, tint[i * 4 + 1] as f32, tint[i * 4 + 2] as f32];
                    for k in 0..3 {
                        c[k] = (c[k] * (1.0 - a.min(0.8)) + ink[k] * a.min(0.8)) * shade;
                    }
                }
                img.rgba[i * 4..i * 4 + 4].copy_from_slice(&[c[0] as u8, c[1] as u8, c[2] as u8, 255]);
            }
        }
        let tex = gfx::texture(ui.ctx, &img, false, TextureWrap::Clamp);
        self.minimap = Some((tex, region, terrain, now));
        tex
    }

    /// Your workers standing about with nothing to do.
    fn idle_workers(&self) -> Vec<u16> {
        let you = self.you();
        self.view
            .world
            .penguins
            .iter()
            .filter(|p| p.owner == you && p.kind == Kind::Worker && !p.inside && p.act == Act::Idle && p.carry == sim::game::Carry::None)
            .map(|p| p.id)
            .collect()
    }

    /// Your fighters standing by with nothing to do.
    fn idle_fighters(&self) -> Vec<u16> {
        let you = self.you();
        self.view
            .world
            .penguins
            .iter()
            .filter(|p| p.owner == you && p.kind.fights() && !p.inside && p.act == Act::Idle)
            .map(|p| p.id)
            .collect()
    }

    /// Select the workers to build with: the idle ones, or if everyone is
    /// busy the three nearest your igloo, so one sauna doesn't pull every
    /// fisher off the ice.
    fn select_builders(&mut self) {
        let mut workers = self.idle_workers();
        if workers.is_empty() {
            let you = self.you();
            let home = self.view.my_igloos().first().map(|&ig| self.view.info().igloos[ig as usize].door);
            let mut busy: Vec<&_> = self
                .view
                .world
                .penguins
                .iter()
                .filter(|p| p.owner == you && p.kind == Kind::Worker && !p.inside && p.act != Act::Frozen)
                .collect();
            if let Some(h) = home {
                busy.sort_by(|a, b| a.pos.dist2(h).partial_cmp(&b.pos.dist2(h)).unwrap_or(std::cmp::Ordering::Equal));
            }
            workers = busy.into_iter().take(3).map(|p| p.id).collect();
        }
        if workers.is_empty() {
            self.notice("No workers out to build with", theme::BAD);
            return;
        }
        self.selected = workers;
        self.sel_igloo = None;
        self.build_menu = false;
    }

    /// Select the next idle worker and look at it.
    fn next_idle_worker(&mut self, r: &mut dyn WorldRenderer) {
        let idle = self.idle_workers();
        if idle.is_empty() {
            return;
        }
        let cur = self.selected.first().and_then(|s| idle.iter().position(|i| i == s));
        let next = idle[cur.map_or(0, |c| (c + 1) % idle.len())];
        self.selected = vec![next];
        self.sel_igloo = None;
        r.focus(self.view.world.penguins[next as usize].pos);
    }

    /// Select the next idle fighter and look at it.
    fn next_idle_fighter(&mut self, r: &mut dyn WorldRenderer) {
        let idle = self.idle_fighters();
        if idle.is_empty() {
            return;
        }
        let cur = self.selected.first().and_then(|s| idle.iter().position(|i| i == s));
        let next = idle[cur.map_or(0, |c| (c + 1) % idle.len())];
        self.selected = vec![next];
        self.sel_igloo = None;
        r.focus(self.view.world.penguins[next as usize].pos);
    }

    /// The minimap, drawn into `mm`.
    fn draw_minimap(&mut self, ui: &mut Ui, r: &dyn WorldRenderer, scale: f32, mm: Rect) {
        let tex = self.ensure_minimap(ui);
        let map = &self.view.info().map;
        ui.batch.quad(ui.ctx, tex, mm.x, mm.y, mm.w, mm.h, [0.0, 0.0, 1.0, 1.0], gfx::WHITE, [0; 4]);
        let sx = mm.w / map.w as f32;
        let sy = mm.h / map.h as f32;
        let info = self.view.info();
        for (i, ig) in info.igloos.iter().enumerate() {
            let v = &self.view.world.igloos[i];
            // An igloo nobody of yours has laid eyes on is not on your map.
            if v.owner != info.you && !self.view.discovered(ig.center) {
                continue;
            }
            let (x, y) = (mm.x + ig.center.x * sx, mm.y + ig.center.y * sy);
            let blink = v.takeover.is_some() && (self.view.time * 4.0).fract() < 0.5;
            ui.batch.rect(ui.ctx, x - 5.0, y - 5.0, 10.0, 10.0, if blink { theme::ACCENT } else { [20, 30, 50, 255] });
            if v.hp == 0 {
                // A pelted-down igloo: a small dark grey ruin.
                ui.batch.rect(ui.ctx, x - 2.0, y - 2.0, 4.0, 4.0, [80, 80, 80, 255]);
            } else {
                let c = if v.owner == NEUTRAL { [200, 200, 200, 255] } else { rgba3(player_color(info, v.owner)) };
                ui.batch.rect(ui.ctx, x - 4.0, y - 4.0, 8.0, 8.0, c);
                // Half pelted away: the square's far half goes dark, so a siege reads on the minimap.
                if (v.hp as f32) < sim::game::igloo_max_hp(v.tier) as f32 * 0.5 {
                    ui.batch.rect(ui.ctx, x, y - 4.0, 4.0, 8.0, [0, 0, 0, 160]);
                }
            }
        }
        let team = self.view.world.team(info.you);
        for p in &self.view.world.penguins {
            if p.inside || ((p.owner == NEUTRAL || self.view.world.team(p.owner) != team) && !self.view.in_sight(p.pos)) {
                continue;
            }
            // Frozen reads grey; a wild (unowned) one reads white.
            let c = if p.act == Act::Frozen {
                [170, 170, 170, 255]
            } else if p.owner == NEUTRAL || p.dining {
                [255, 255, 255, 255]
            } else {
                rgba3(player_color(info, p.owner))
            };
            let (x, y) = (mm.x + p.pos.x * sx, mm.y + p.pos.y * sy);
            let s = if self.selected.contains(&p.id) { 3.0 } else { 2.0 };
            ui.batch.rect(ui.ctx, x - s / 2.0, y - s / 2.0, s, s, c);
        }
        for pw in &self.view.world.powerups {
            if pw.holder.is_none() && self.view.discovered(pw.pos) {
                let (x, y) = (mm.x + pw.pos.x * sx, mm.y + pw.pos.y * sy);
                ui.batch.rect(ui.ctx, x - 1.5, y - 1.5, 3.0, 3.0, theme::ACCENT);
            }
        }
        // A pulsing ping where the latest alert is, while it is fresh.
        if let Some((at, since)) = self.alert {
            let age = self.view.time - since;
            if (0.0..8.0).contains(&age) {
                let fade = if age > 6.0 { 1.0 - (age - 6.0) / 2.0 } else { 1.0 };
                let (x, y) = (mm.x + at.x * sx, mm.y + at.y * sy);
                let pulse = (age * 1.6).fract();
                let rad = 4.0 + pulse * 14.0;
                let a = ((1.0 - pulse) * 230.0 * fade) as u8;
                ui.batch.ring(ui.ctx, x, y, rad, rad, 2.0, [255, 130, 90, a]);
                ui.batch.ring(ui.ctx, x, y, 3.0, 3.0, 1.2, with_alpha(theme::BAD, fade));
            }
        }
        // Camera view outline.
        let (pw, ph) = (ui.w * scale, ui.h * scale);
        let corners: Vec<Option<Vec2>> =
            [[0.0, 0.0], [pw, 0.0], [pw, ph], [0.0, ph]].iter().map(|&c| r.screen_to_ground(c)).collect();
        if corners.iter().all(|c| c.is_some()) {
            let pts: Vec<[f32; 2]> = corners
                .iter()
                .map(|c| {
                    let c = c.unwrap();
                    [
                        (mm.x + c.x * sx).clamp(mm.x, mm.x + mm.w),
                        (mm.y + c.y * sy).clamp(mm.y, mm.y + mm.h),
                    ]
                })
                .collect();
            for i in 0..4 {
                ui.batch.line(ui.ctx, pts[i], pts[(i + 1) % 4], 1.5, [255, 255, 255, 200]);
            }
        }
    }

    /// What each computer player is doing and why: for spectators, and for
    /// players while the server shares it (see `ai_notes_for_players`).
    fn draw_ai_panel(&mut self, ui: &mut Ui) {
        if ui.input.hit(KeyCode::F4) {
            self.ai_panel = !self.ai_panel;
        }
        if self.ai_notes.is_empty() || !self.ai_panel {
            return;
        }
        // Down the left side, over the world: the right has the tribes list.
        let (w, top) = (470.0, 84.0);
        // Tall enough to read a whole decision: the top of the console is the limit.
        let h = (self.console(ui).frame.y - top - 14.0).clamp(230.0, 760.0);
        let (x, px) = (10.0, 14.0);
        ui.batch.rect(ui.ctx, x, top, w, h, [18, 16, 14, 225]);
        ui.batch.rect_outline(ui.ctx, x, top, w, h, 1.5, [200, 170, 120, 220]);
        ui.text("Computer players  (F4 hides)", x + 10.0, top + 6.0, px, theme::TEXT_DIM, Align::Left);
        // A tab for each.
        self.ai_shown = self.ai_shown.min(self.ai_notes.len() - 1);
        let tab_w = ((w - 20.0) / self.ai_notes.len() as f32).min(150.0);
        let info = self.view.info();
        let mut pick = None;
        for (i, (p, _)) in self.ai_notes.iter().enumerate() {
            let full = info.players.get(*p as usize).map_or("?", |pl| pl.name.as_str());
            // Many tabs are narrow: "Tux the Grudge-Holder" becomes "Tux".
            let name = if ui.font.measure(full, 11.0) > tab_w - 18.0 { full.split(' ').next().unwrap_or(full) } else { full };
            if ui.button_ex(rect(x + 10.0 + i as f32 * tab_w, top + 28.0, tab_w - 4.0, 26.0), name, true, i == self.ai_shown) {
                pick = Some(i);
            }
        }
        if let Some(i) = pick {
            self.ai_shown = i;
        }
        let (player, lines) = &self.ai_notes[self.ai_shown];
        let color = info.players.get(*player as usize).map_or(theme::TEXT, |pl| rgba3(sim::game::PLAYER_COLORS[pl.color as usize % 10]));
        // Where things stand first, then what it did lately, newest last.
        let wrap = |text: &str, ui: &Ui| -> Vec<String> {
            let mut out = Vec::new();
            let mut line = String::new();
            for word in text.split(' ') {
                let next = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
                if ui.font.measure(&next, 13.0) > w - 28.0 && !line.is_empty() {
                    out.push(std::mem::replace(&mut line, format!("   {word}")));
                } else {
                    line = next;
                }
            }
            out.push(line);
            out
        };
        const STATE: usize = 5;
        let state: Vec<String> = lines.iter().take(STATE).flat_map(|l| wrap(l, ui)).collect();
        // The log keeps whole entries: when the panel is full the oldest go,
        // never half of one.
        let mut entries: Vec<Vec<String>> = lines.iter().skip(STATE).map(|l| wrap(l, ui)).collect();
        let room = ((h - 66.0) / 17.0) as usize;
        let log_room = room.saturating_sub(state.len() + 2);
        while entries.iter().map(Vec::len).sum::<usize>() > log_room && entries.len() > 1 {
            entries.remove(0);
        }
        let log: Vec<String> = entries.into_iter().flatten().collect();
        let mut y = top + 62.0;
        for (i, l) in state.iter().enumerate() {
            ui.text(l, x + 10.0, y, 13.0, if i == 0 { color } else { theme::TEXT }, Align::Left);
            y += 17.0;
        }
        y += 4.0;
        ui.text("Recently:", x + 10.0, y, 13.0, theme::TEXT_DIM, Align::Left);
        y += 17.0;
        for l in &log {
            ui.text(l, x + 10.0, y, 13.0, theme::TEXT, Align::Left);
            y += 17.0;
        }
    }

    fn draw_chat(&mut self, ui: &mut Ui) {
        let x = 12.0;
        let mut y = self.console(ui).frame.y - 14.0;
        if self.chat_open {
            let r = rect(x, y, 440.0, 30.0);
            let mut input = std::mem::take(&mut self.chat_input);
            let mut focus = self.focus;
            std::mem::swap(ui.focus, &mut focus);
            let submitted = ui.text_field(1, r, &mut input, 200, "Say something... (>name for private)");
            std::mem::swap(ui.focus, &mut focus);
            self.focus = focus;
            self.chat_input = input;
            if ui.input.hit_repeat(KeyCode::Escape) {
                self.chat_open = false;
                self.chat_input.clear();
                ui.input.keyboard_used = true;
            }
            if submitted {
                let text = std::mem::take(&mut self.chat_input);
                self.chat_open = false;
                self.focus = None;
                self.send_chat(text);
            }
            ui.input.keyboard_used = true;
            y -= 8.0;
        }
        let lines: Vec<&ChatLine> = self.chat.iter().rev().filter(|c| c.age < 14.0 || self.chat_open).take(8).collect();
        for c in lines {
            y -= 22.0;
            let a = if self.chat_open { 1.0 } else { (1.0 - (c.age - 11.0) / 3.0).clamp(0.0, 1.0) };
            let mut lx = x;
            if !c.from.is_empty() {
                lx += ui.text_shadow(&format!("{}: ", c.from), lx, y, 16.0, with_alpha(c.color, a), Align::Left);
            }
            let tc = if c.private { [255, 220, 150, 255] } else { theme::TEXT };
            ui.text_shadow(&c.text, lx, y, 16.0, with_alpha(tc, a), Align::Left);
        }
    }

    fn send_chat(&mut self, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let info = self.view.info();
        if let Some(rest) = text.strip_prefix('>') {
            let (name, msg) = rest.split_once(' ').unwrap_or((rest, ""));
            let lname = name.to_lowercase();
            let target = info
                .players
                .iter()
                .position(|p| p.name.to_lowercase() == lname)
                .or_else(|| info.players.iter().position(|p| p.name.to_lowercase().starts_with(&lname)));
            match target {
                Some(t) if !msg.trim().is_empty() => {
                    self.last_to = Some(info.players[t].name.clone());
                    self.send(Command::Chat { to: Some(t as u8), text: msg.trim().into() });
                }
                _ => self.notice(format!("No player named '{name}'"), theme::BAD),
            }
        } else {
            self.send(Command::Chat { to: None, text });
        }
    }

    fn draw_game_over(&mut self, ui: &mut Ui) -> GameAction {
        let info = self.view.info();
        let you = info.you;
        let winner = self.view.world.winner;
        let won = winner.is_some_and(|t| you != NEUTRAL && info.players[you as usize].team == t);
        let (title, c) = if you != NEUTRAL && won {
            ("Victory!", theme::ACCENT)
        } else if winner.is_some() && you != NEUTRAL {
            ("Defeat", theme::BAD)
        } else {
            ("Game over", theme::TEXT)
        };
        ui.fill(rect(0.0, 0.0, ui.w, ui.h), 0.0, [20, 12, 6, 140]);
        let rows = info.players.len().max(1);
        let pw = 740.0f32.min(ui.w - 24.0);
        // What each computer was really like, told now the game is over.
        let reveal = |name: &str| self.reveals.iter().find(|(n, _)| n == name).map(|(_, w)| w.clone());
        let revealed = info.players.iter().filter(|pl| reveal(&pl.name).is_some()).count();
        // Room for the latest turning points if the screen is tall enough.
        let base = 340.0 + rows as f32 * 26.0 + revealed as f32 * 18.0;
        let story = &self.view.stats.story;
        let shown = story.len().min(5).min(((ui.h - 24.0 - base) / 20.0).max(0.0) as usize);
        let ph = base + if shown > 0 { 30.0 + shown as f32 * 20.0 } else { 0.0 };
        let p = rect(ui.w / 2.0 - pw / 2.0, (ui.h - ph) / 2.0, pw, ph);
        ui.panel(p);
        ui.text(title, p.x + pw / 2.0, p.y + 12.0, 42.0, c, Align::Center);
        let sub = match winner {
            Some(t) => {
                let names: Vec<&str> = info.players.iter().filter(|pl| pl.team == t).map(|pl| pl.name.as_str()).collect();
                format!("{} won the snowball fight", names.join(" & "))
            }
            None => "The match is over".to_string(),
        };
        ui.text(&sub, p.x + pw / 2.0, p.y + 62.0, 18.0, theme::TEXT, Align::Center);
        let secs = self.view.world.tick / sim::TICK_RATE;
        ui.text(&format!("Game time {}:{:02}", secs / 60, secs % 60), p.x + pw - 24.0, p.y + 66.0, 15.0, theme::TEXT_DIM, Align::Right);

        // One row per player, counted from this client's events, on a
        // sheet of paper pinned to the board.
        let sheet = rect(p.x + 16.0, p.y + 96.0, pw - 32.0, ph - 96.0 - 74.0);
        ui.paper(sheet, 6.0);
        let col = |i: usize| p.x + 296.0 + i as f32 * 66.0;
        let cols = ["Trained", "Thrown", "Hits", "Frozen", "Igloos", "Prisoners"];
        let hy = p.y + 108.0;
        ui.text("Player", p.x + 30.0, hy, 14.0, INK_DIM, Align::Left);
        for (i, name) in cols.iter().enumerate() {
            ui.text(name, col(i) + 62.0, hy, 13.0, INK_DIM, Align::Right);
        }
        let st = &self.view.stats;
        let mut y = hy + 24.0;
        for (i, pl) in info.players.iter().enumerate() {
            let t = st.tally.get(i).copied().unwrap_or_default();
            if i as u8 == you {
                ui.fill(rect(p.x + 22.0, y - 4.0, pw - 44.0, 24.0), 4.0, [226, 196, 120, 120]);
            }
            ui.batch.ellipse(ui.ctx, p.x + 34.0, y + 7.0, 6.0, 6.0, rgba3(player_color(info, i as u8)));
            ui.text(&pl.name, p.x + 48.0, y, 17.0, INK, Align::Left);
            for (n, v) in [t.trained, t.thrown, t.hits, t.frozen, t.captured, t.prisoners].iter().enumerate() {
                ui.text(&v.to_string(), col(n) + 62.0, y, 17.0, INK, Align::Right);
            }
            if let Some(what) = reveal(&pl.name) {
                let mut line = format!("Was really: {what}");
                // Too long for the row: drop traits from the end.
                while ui.font.measure(&line, 14.0) > pw - 90.0 {
                    match line.rfind(", ") {
                        Some(i) => line.truncate(i),
                        None => break,
                    }
                }
                ui.text(&line, p.x + 48.0, y + 21.0, 14.0, INK_DIM, Align::Left);
                y += 18.0;
            }
            y += 26.0;
        }

        // Army sizes over the match, a sample every few seconds.
        let gy = y + 10.0;
        ui.text("Army size", p.x + 30.0, gy, 14.0, INK_DIM, Align::Left);
        let g = rect(p.x + 30.0, gy + 18.0, pw - 60.0, 80.0);
        ui.fill(g, 5.0, [222, 204, 166, 255]);
        self.draw_army_graph(ui, g);
        if shown > 0 {
            let ty = g.y + g.h + 10.0;
            ui.text("Turning points", p.x + 30.0, ty, 14.0, INK_DIM, Align::Left);
            let story = &self.view.stats.story;
            for (i, (secs, text)) in story[story.len() - shown..].iter().enumerate() {
                let line = format!("{}:{:02}  {text}", secs / 60, secs % 60);
                ui.text(&line, p.x + 30.0, ty + 22.0 + i as f32 * 20.0, 16.0, INK, Align::Left);
            }
        }

        let mut act = GameAction::None;
        let by = p.y + ph - 58.0;
        if ui.button_ex(rect(p.x + 24.0, by, 200.0, 44.0), "Back to lobby", self.is_host, false) {
            act = GameAction::BackToLobby;
        }
        if ui.button(rect(p.x + pw / 2.0 - 90.0, by, 180.0, 44.0), "Watch on") {
            self.show_score = false;
        }
        if ui.button(rect(p.x + pw - 220.0, by, 196.0, 44.0), "Main menu") {
            act = GameAction::Leave;
        }
        act
    }

    /// Small line graph of every player's army size over the match.
    fn draw_army_graph(&self, ui: &mut Ui, g: Rect) {
        let st = &self.view.stats;
        let n = st.army.len();
        if n < 2 {
            return;
        }
        let info = self.view.info();
        let peak = st.army.iter().flatten().copied().max().unwrap_or(1).max(1) as f32;
        for (i, _) in info.players.iter().enumerate() {
            let c = rgba3(player_color(info, i as u8));
            let mut prev: Option<[f32; 2]> = None;
            for (s, row) in st.army.iter().enumerate() {
                let x = g.x + g.w * s as f32 / (n - 1) as f32;
                let v = row.get(i).copied().unwrap_or(0) as f32;
                let at = [x, g.y + g.h - g.h * v / peak];
                if let Some(a) = prev {
                    ui.batch.line(ui.ctx, a, at, 2.0, c);
                }
                prev = Some(at);
            }
        }
        ui.text(&format!("{peak:.0}"), g.x + 4.0, g.y + 2.0, 12.0, INK_DIM, Align::Left);
    }

    fn draw_help(&mut self, ui: &mut Ui) {
        let mut lines: Vec<(&str, &str)> = vec![
            ("Left click / drag", "Select penguins (double-click: all of a kind)"),
            ("Right click", "Move · attack-move · enter igloo · gather"),
            ("Right click a tree", "Workers chop down the trees around it"),
            ("Right click a sauna, workers selected", "Tend its fire: it burns wood, and only a lit sauna warms bathers. Each fire calls its own stoker"),
            ("Right click a frozen enemy", "Carry it home; it joins you after a while"),
            ("Right click the frozen, Warmers selected", "Thaw them: they join your tribe"),
            ("Middle click / Ctrl+right", "Run there without stopping to fight"),
            ("Shift+right click", "Queue moves"),
            ("Middle drag, WASD, arrows", "Scroll the map"),
            ("Mouse wheel, PgUp/PgDn", "Zoom"),
            ("Q / E", "Rotate camera (3D)"),
            ("Ctrl+1..9, 1..9", "Set / recall groups (tap twice to jump)"),
            ("Ctrl+A", "Select all snowballers"),
            ("H / Tab", "Select your igloo / the next igloo"),
            ("I", "Jump to the next idle worker"),
            ("Shift+I", "Jump to the next idle fighter"),
            ("Soldiers near enemy igloos", "Pelt them when nobody's fighting; they fall in at 0"),
            ("Packers", "Knock igloos down three times faster"),
            ("Y", "War Cry, with a Chieftain selected"),
            ("O or Backspace", "Disband: penguins return to their igloo"),
            ("T", "Hold position: stand and throw, don't chase"),
            ("Space", "Jump to the latest alert (it pulses on the minimap)"),
            ("Enter, '>name msg'", "Chat, private chat ('.' reply, ',' resend)"),
            ("F3", "Frame time"),
            ("Phone: tap", "Select; tap the ground to deselect"),
            ("Phone: drag", "Pan the map; two fingers pinch to zoom"),
            ("Phone: hold", "Hold still until the ring fills, lift: order. Hold then drag: box select"),
        ];
        lines.push(("Right click a tower, soldiers selected", "Man the roof: four fit, they throw far and can't be hit until it falls. Its sentries also climb up by themselves"));
        lines.push((
            "Catapult Tower",
            "Build a Watchtower up and it gets a catapult, crewed by three workers at its foot. It lobs big snowballs that wreck enemy buildings and igloos up to 14 tiles off, the nearest first; select it and right-click one to aim. Freeze its whole crew and your workers can take it over (right-click it)",
        ));
        lines.push(("B, then M J C F P", "Workers build campfires, snow walls (drag a line), watchtowers (they see far), Sushi Houses and new igloos (30 tiles from any other). Camps and the Bunkhouse are on the next page"));
        lines.push(("Sauna", "Select a campfire and build it up: it becomes a Sauna on the spot, where six bathe at once"));
        lines.push((
            "Slots",
            "Buildings fill themselves with penguins: an igloo's two fishers, a Sushi House's fishers, \
             a Bunkhouse's snowballers, a camp's specialists, a tower's sentries and a fire's stoker. \
             You choose how many: click a slot on its card, or roll the mouse wheel over the slots. \
             Each wanted slot calls in an idle worker; with nobody idle, a newcomer swims in from the sea for 4 fish. \
             Builders build Sushi Houses, Bunkhouses and camps up to more slots with snow and logs; the new room stays empty until you want it. \
             Igloos have no guards: build a Watchtower beside one to guard it.",
        ));
        lines.push(("", "Nobody eats fish to live: fish pays for newcomers from the sea and for training"));
    
        // Every unit in this game: what it does, how to get one, and what it
        // beats and fears.
        let mut units: Vec<(String, String)> = Vec::new();
        for kind in Kind::ALL {
            let made_at = BUILD_KEYS.iter().map(|(b, _, _)| *b).find(|b| b.trains() == Some(kind) && b.offered());
            let how = match kind {
                Kind::Worker => "Swims in from the sea when a slot wants one and nobody is idle (4 fish).".to_string(),
                Kind::Snowballer => "Drafted from a worker by each bunk of a Bunkhouse.".to_string(),
                Kind::Chieftain => "Every igloo has one.".to_string(),
                _ => match made_at {
                    Some(b) => format!("An idle worker called into a slot at the {} mines a crystal, then trains.", b.name()),
                    None => continue,
                },
            };
            let (title, _, body) = unit_tip(kind, NEUTRAL, self.view.info());
            units.push((title, format!("{body} {how}")));
        }
        for (t, b) in &units {
            lines.push((t.as_str(), b.as_str()));
        }
        let tips = "Snowballers fight on their own and scoop new snowballs on snow (not ice). \
            They fight smart: they finish off the nearly frozen, sometimes sidestep a snowball, \
            step out of the ranks to reach an enemy just out of range, and the hurt swap to the back. \
            Hits make penguins cold; cold penguins run home to warm up and eat fish. \
            Higher ground throws farther. Trees block snowballs, but workers can chop them down \
            to open new paths, and builders and stokers fetch logs from them. Felled trees grow back. Scooping and busy routes wear \
            snow down to bare ice: fast to walk on, no snowballs there. Snowfall brings it back. \
            Workers build with the snow around a site, so building digs the ground bare. \
            {walls} Leave a gap and it is a weak point: defend it. \
            The seasons turn every four minutes: deep winter snows, the thaw melts snow \
            buildings, midsummer brings fish, the polar night shortens throws. Blizzards chill. \
            Watching friends freeze costs courage (less in a huddle); penguins with none left flee. \
            {take} A Chieftain steadies the tribe around it; its War Cry makes \
            nearby friends run and throw faster, and shake off their fear.";
        let take = "Igloos can't be taken: pelt their walls down. An empty igloo is founded by sending a worker inside, for fish.";
        let walls = "Snow walls are tall and sturdy.";
        let tips = tips.replace("{take}", take).replace("{walls}", walls);
        lines.push(("Good to know", &tips));

        // A sheet of paper, two columns of entries, as many pages as needed.
        let w = (ui.w - 32.0).min(980.0);
        let h = (ui.h - 32.0).min(720.0);
        let p = rect(ui.w / 2.0 - w / 2.0, (ui.h - h) / 2.0, w, h);
        let (pad, gap) = (28.0, 28.0);
        let cw = (w - 2.0 * pad - gap) / 2.0;
        let (top, bottom) = (p.y + 64.0, p.y + h - 58.0);
        let mut pages: Vec<Vec<(f32, f32, &str, Vec<String>)>> = vec![Vec::new()];
        let (mut col, mut y) = (0, top);
        for (k, d) in &lines {
            let wrapped = ui.font.wrap(d, 15.0, cw);
            let eh = if k.is_empty() { 0.0 } else { 21.0 } + wrapped.len() as f32 * 18.0 + 10.0;
            if y + eh > bottom && y > top {
                col += 1;
                y = top;
                if col == 2 {
                    pages.push(Vec::new());
                    col = 0;
                }
            }
            let x = p.x + pad + col as f32 * (cw + gap);
            pages.last_mut().unwrap().push((x, y, k, wrapped));
            y += eh;
        }
        self.help_page = self.help_page.min(pages.len() - 1);

        ui.paper(p, 8.0);
        ui.text("How to play", p.x + w / 2.0, p.y + 14.0, 30.0, INK, Align::Center);
        ui.fill(rect(p.x + pad, p.y + 54.0, w - 2.0 * pad, 1.5), 0.0, [205, 178, 130, 255]);
        for (x, mut y, k, wrapped) in &pages[self.help_page] {
            if !k.is_empty() {
                ui.text(k, *x, y, 16.0, [150, 70, 30, 255], Align::Left);
                y += 21.0;
            }
            for l in wrapped {
                ui.text(l, *x, y, 15.0, INK, Align::Left);
                y += 18.0;
            }
        }
        let by = p.y + h - 46.0;
        ui.text("F1 or Esc to close", p.x + w / 2.0, by + 9.0, 14.0, INK_DIM, Align::Center);
        if pages.len() > 1 {
            let n = pages.len();
            ui.text(&format!("Page {} of {n}", self.help_page + 1), p.x + pad, by + 9.0, 14.0, INK_DIM, Align::Left);
            if ui.stamp_text(rect(p.x + w - pad - 196.0, by, 92.0, 32.0), "Back", self.help_page > 0) {
                self.help_page -= 1;
            }
            if ui.stamp_text(rect(p.x + w - pad - 92.0, by, 92.0, 32.0), "Next", self.help_page + 1 < n) {
                self.help_page += 1;
            }
        }
    }
}

pub fn rgba3(c: [u8; 3]) -> Color {
    [c[0], c[1], c[2], 255]
}

fn warmth_color(f: f32) -> Color {
    if f > 0.6 {
        [110, 220, 120, 255]
    } else if f > 0.35 {
        [240, 200, 70, 255]
    } else {
        [120, 190, 255, 255]
    }
}
