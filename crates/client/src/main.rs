//! Snowballz client.
//!
//!     snowballz [--connect HOST[:PORT] | --host] [--name NAME]
//!               [--quickstart MAP] [--screenshot FILE [--shot-after SECS]]
//!               [--selftest]

// No console window behind the game on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod advice;
mod assets;
mod audio;
mod effects;
mod font;
mod game;
mod gfx;
mod gpu;
mod host;
mod icons;
mod juice;
mod llm;
mod mathx;
mod menu;
mod meshgen;
mod model;
mod net;
mod platform;
mod post;
mod render3d;
mod scene;
mod selftest;
mod settings;
mod skin;
mod ui;
mod view;
#[cfg(target_arch = "wasm32")]
mod web;

use std::net::SocketAddr;
use std::path::PathBuf;

use crate::platform::{date, window, KeyCode, KeyMods, MouseButton, TouchPhase};
use sim::ai::AiLevel;
use sim::game::{Game, PlayerSetup};
use sim::map::{Ground, Map, Obstacle, ResourceKind, IGLOO_SIZE};
use sim::proto::{ClientMsg, GameInfo, LobbyState, ServerMsg, SlotKind, SnapshotEncoder, PROTOCOL_VERSION};

use crate::font::Font;
use crate::game::{ChatLine, GameAction, GameScreen};
use crate::gfx::Batch;
use crate::mathx::v3;
use crate::menu::MenuAction;
use crate::net::{Connecting, Discovery, Net};
use crate::render3d::Render3D;
use crate::scene::WorldRenderer;
use crate::settings::Settings;
use crate::ui::{Input, Ui};
use crate::view::GameView;

#[derive(Clone, Default)]
struct Args {
    connect: Option<String>,
    name: Option<String>,
    quickstart: Option<String>,
    practice: bool,
    screenshot: Option<PathBuf>,
    shot_after: f32,
    windowed: Option<(i32, i32)>,
    msaa: bool,
    selftest: bool,
    host: bool,
    /// Screenshot camera: map x, y and a zoom factor (<1 = closer).
    look: Option<(f32, f32, f32)>,
}

fn parse_args() -> Args {
    let mut a = Args { shot_after: 6.0, msaa: true, ..Default::default() };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--connect" | "-c" => a.connect = it.next(),
            "--name" => a.name = it.next(),
            "--quickstart" => a.quickstart = it.next(),
            "--practice" => a.practice = true,
            "--screenshot" => a.screenshot = it.next().map(PathBuf::from),
            "--shot-after" => a.shot_after = it.next().and_then(|s| s.parse().ok()).unwrap_or(6.0),
            "--no-msaa" => a.msaa = false,
            "--selftest" => a.selftest = true,
            "--host" => a.host = true,
            "--look" => {
                a.look = it.next().and_then(|v| {
                    let n: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                    (n.len() >= 2).then(|| (n[0], n[1], n.get(2).copied().unwrap_or(1.0)))
                })
            }
            "--size" => {
                a.windowed = it.next().and_then(|s| {
                    let (w, h) = s.split_once('x')?;
                    Some((w.parse().ok()?, h.parse().ok()?))
                })
            }
            "-h" | "--help" => {
                println!(
                    "usage: snowballz [--connect HOST[:PORT] | --host] [--name NAME] [--size WxH] [--no-msaa]\n\
                     \x20                [--quickstart MAP] [--screenshot FILE [--shot-after SECS]] [--selftest]\n                     \x20                [--look X,Y[,ZOOM]] (X < 0: zoom only)"
                );
                std::process::exit(0);
            }
            other => {
                eprintln!("unknown argument {other} (try --help)");
                std::process::exit(2);
            }
        }
    }
    if a.selftest {
        a.quickstart.get_or_insert_with(|| "aurora basin".into());
        a.windowed.get_or_insert((1280, 800));
        // The self-test drives the interface with the map's whole start (it
        // needs snowballers on screen), not the small start. Set before any
        // thread starts.
        std::env::set_var("SNOWBALLZ_START", "map");
    }
    a
}

#[derive(PartialEq, Clone, Copy)]
enum Screen {
    Main,
    Join,
    Settings,
    Lobby,
    Game,
}

/// Attract mode: computer players battling behind the main menu.
struct Demo {
    game: Game,
    enc: SnapshotEncoder,
    view: GameView,
    acc: f32,
    cam: f32,
    look: Option<sim::math::Vec2>,
    /// Screen shake on or off, so it survives a demo restart.
    shake_on: bool,
}

impl Demo {
    fn new(assets: &std::path::Path, shake_on: bool) -> Option<Demo> {
        let map = Map::parse(&assets::read(&assets.join("maps/aurora-basin.sbmap")).ok()?).ok()?;
        let setups = (0..3)
            .map(|i| PlayerSetup { name: format!("Bot {i}"), color: [1, 0, 3][i], team: i as u8, ai: Some(AiLevel::Hard) })
            .collect();
        let mut game = Game::new(map, setups, 7);
        let mut enc = SnapshotEncoder::new();
        // Skip the quiet opening so there's action straight away.
        for _ in 0..sim::TICK_RATE * 50 {
            game.tick();
            game.events.clear();
        }
        let mut view = GameView::new(GameInfo::from_game(&game, sim::game::NEUTRAL));
        view.juice.set_shake(shake_on);
        let snap = enc.encode(&game, &[]);
        view.apply(&snap);
        Some(Demo { game, enc, view, acc: 0.0, cam: 0.0, look: None, shake_on })
    }

    fn update(&mut self, dt: f32) {
        self.acc += dt;
        while self.acc >= sim::DT {
            self.acc -= sim::DT;
            self.game.tick();
            let ev = std::mem::take(&mut self.game.events);
            let snap = self.enc.encode(&self.game, &ev);
            self.view.apply(&snap);
            if self.game.winner.is_some() || self.game.tick > sim::TICK_RATE * 60 * 12 {
                // Start over with a fresh battle.
                let map = self.game.map.clone();
                let setups = self
                    .game
                    .players
                    .iter()
                    .map(|p| PlayerSetup { name: p.name.clone(), color: p.color, team: p.team, ai: p.ai })
                    .collect();
                self.game = Game::new(map, setups, self.game.tick as u64);
                self.enc = SnapshotEncoder::new();
                self.view = GameView::new(GameInfo::from_game(&self.game, sim::game::NEUTRAL));
                self.view.juice.set_shake(self.shake_on);
            }
        }
        self.view.update(dt);
        self.cam += dt;
        // Drift towards wherever the snowballs are flying.
        let fights: Vec<sim::math::Vec2> = self.view.flying.iter().map(|s| s.to).collect();
        let want = if fights.is_empty() {
            let ig = &self.view.world.info.igloos[(self.cam / 20.0) as usize % self.view.world.info.igloos.len()];
            ig.door
        } else {
            fights.iter().fold(sim::math::Vec2::ZERO, |a, &p| a + p) * (1.0 / fights.len() as f32)
        };
        let cur = self.look.unwrap_or(want);
        self.look = Some(cur.lerp(want, (dt * 0.25).min(1.0)));
    }
}

struct App {
    /// Fingers currently down, in interface units.
    touches: Vec<(u64, [f32; 2])>,
    ctx: gpu::Gpu,
    audio: audio::Audio,
    /// Volumes last sent to the mixer.
    volume: (u8, u8),
    batch: Batch,
    font: Font,
    assets: PathBuf,
    settings: Settings,
    /// Screen shake on or off, for this session only.
    shake_on: bool,
    args: Args,
    r3d: Option<Render3D>,
    r3d_error: Option<String>,
    /// Which GameView/map the renderer was last prepared for.
    prepared: u64,
    view_ids: u64,
    input: Input,
    focus: Option<u64>,
    screen: Screen,
    net: Option<Net>,
    connecting: Option<Connecting>,
    /// The server we started ourselves, if we're hosting.
    hosted: Option<host::Hosted>,
    conn_id: u32,
    /// Where we last connected, and the rejoin token that server gave us.
    server_addr: String,
    session: Option<(String, u64)>,
    lobby: Option<LobbyState>,
    lobby_chat: Vec<ChatLine>,
    lobby_input: String,
    map_maker: menu::MapMaker,
    /// The local model working on a map for the lobby.
    map_job: Option<llm::MapRequest>,
    single_player: bool,
    auto_filled: bool,
    auto_start: bool,
    game: Option<GameScreen>,
    game_id: u64,
    demo: Option<Demo>,
    demo_id: u64,
    discovery: Option<Discovery>,
    join_addr: String,
    status: String,
    error: Option<String>,
    last: f64,
    start: f64,
    shot_done: bool,
    fps: (u32, f64, u32),
    frame_times: [f64; 60],
    frame_idx: usize,
    frame_count: u32,
    show_frame_time: bool,
    selftest: Option<selftest::SelfTest>,
    look_zoomed: bool,
    /// The page has been told the first frame is up.
    #[cfg(target_arch = "wasm32")]
    announced: bool,
}

impl App {
    fn new(args: Args, mut ctx: gpu::Gpu) -> App {
        let assets = assets::dir();
        let mut settings = Settings::load();
        if let Some(n) = &args.name {
            settings.name = n.clone();
        }
        let mut batch = Batch::new(&mut ctx);
        let font = Font::load(&mut ctx, &assets.join("fonts/ui")).unwrap_or_else(|e| fatal(&format!("can't load font: {e}")));
        batch.white = font.tex;
        batch.white_uv = font.white_uv;

        // Automated runs (screenshots, self-tests, baking) stay silent.
        let audio = if args.screenshot.is_some() || args.selftest {
            audio::Audio::silent()
        } else {
            audio::Audio::new(&assets, settings.music_volume(), settings.sound_volume())
        };
        let volume = (settings.music, settings.sound);
        let mut app = App {
            touches: Vec::new(),
            ctx,
            audio,
            volume,
            batch,
            font,
            assets,
            settings,
            shake_on: true,
            args: args.clone(),
            r3d: None,
            r3d_error: None,
            prepared: 0,
            view_ids: 1,
            input: Input::default(),
            focus: None,
            screen: Screen::Main,
            net: None,
            connecting: None,
            hosted: None,
            conn_id: 0,
            server_addr: String::new(),
            session: None,
            lobby: None,
            lobby_chat: Vec::new(),
            lobby_input: String::new(),
            map_maker: menu::MapMaker::default(),
            map_job: None,
            single_player: false,
            auto_filled: false,
            auto_start: false,
            game: None,
            game_id: 0,
            demo: None,
            demo_id: 0,
            discovery: None,
            join_addr: String::new(),
            status: String::new(),
            error: None,
            last: date::now(),
            start: date::now(),
            shot_done: false,
            fps: (0, date::now(), 0),
            frame_times: [0.0; 60],
            frame_idx: 0,
            frame_count: 0,
            show_frame_time: std::env::var_os("SNOWBALLZ_FRAMETIME").is_some(),
            selftest: args.selftest.then(selftest::SelfTest::default),
            look_zoomed: false,
            #[cfg(target_arch = "wasm32")]
            announced: false,
        };
        app.join_addr = app.settings.last_server.clone();
        #[cfg(target_arch = "wasm32")]
        if app.join_addr.is_empty() {
            app.join_addr = web::server_url();
        }
        app.ensure_renderer();

        if let Some(addr) = args.connect.clone() {
            app.connect(&addr);
        } else if args.practice {
            app.start_local(true);
        } else if args.host {
            app.start_local(false);
        } else if let Some(map) = args.quickstart.clone() {
            app.start_local(true);
            app.auto_start = true;
            app.status = map;
        } else {
            app.demo = Demo::new(&app.assets, app.shake_on);
            app.view_ids += 1;
            app.demo_id = app.view_ids;
        }
        app
    }

    /// Create the renderer. There is only the 3D one, so no GPU, no game.
    fn ensure_renderer(&mut self) {
        if self.r3d.is_none() && self.r3d_error.is_none() {
            match Render3D::new(&mut self.ctx, &self.assets) {
                Ok(r) => self.r3d = Some(r),
                Err(e) => {
                    eprintln!("3D renderer unavailable: {e}");
                    fatal(&format!("no renderer available: {e}"));
                }
            }
        }
    }

    /// Save settings, but not one-off command-line overrides, and never
    /// from automated runs.
    fn save_settings(&self) {
        if self.args.screenshot.is_some() || self.args.selftest {
            return;
        }
        let mut s = self.settings.clone();
        let stored = Settings::load();
        if self.args.name.as_ref() == Some(&s.name) {
            s.name = stored.name;
        }
        s.save();
    }

    fn renderer(&mut self) -> Option<&mut dyn WorldRenderer> {
        self.r3d.as_mut().map(|r| r as &mut dyn WorldRenderer)
    }

    fn scale(&self) -> f32 {
        // Many 4K setups report no DPI scaling, and small Pi screens need
        // the interface shrunk to fit, so also scale with window height.
        let dpi = window::dpi_scale();
        let h = window::screen_size().1 / dpi;
        let fit = if h > 1100.0 {
            h / 1100.0
        } else if h < 720.0 {
            (h / 720.0).max(0.6)
        } else {
            1.0
        };
        dpi * fit * self.settings.ui_scale
    }

    // ---------------------------------------------------------------------
    // Networking

    fn start_local(&mut self, single: bool) {
        self.leave();
        match host::start(&self.settings.name, single, &self.assets) {
            Ok(h) => {
                let port = h.port();
                self.hosted = Some(h);
                self.single_player = single;
                self.auto_filled = false;
                self.connect(&format!("127.0.0.1:{port}"));
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn connect(&mut self, addr: &str) {
        self.status = format!("Connecting to {addr}...");
        self.server_addr = addr.to_string();
        self.connecting = Some(Connecting::start(addr));
        if self.hosted.is_none() {
            self.settings.last_server = addr.to_string();
            self.save_settings();
        }
    }

    fn leave(&mut self) {
        self.net = None;
        self.connecting = None;
        if let Some(h) = self.hosted.take() {
            h.close_in_background();
        }
        self.lobby = None;
        self.game = None;
        self.lobby_chat.clear();
        self.single_player = false;
        self.auto_start = false;
        if self.screen != Screen::Join && self.screen != Screen::Settings {
            self.screen = Screen::Main;
        }
        if self.demo.is_none() && self.args.quickstart.is_none() {
            self.demo = Demo::new(&self.assets, self.shake_on);
            self.view_ids += 1;
            self.demo_id = self.view_ids;
        }
    }

    fn send(&mut self, m: ClientMsg) {
        if let Some(n) = &mut self.net {
            n.send(m);
        }
    }

    fn poll_net(&mut self) {
        if let Some(c) = &self.connecting {
            if let Some(res) = c.poll() {
                self.connecting = None;
                match res {
                    Ok(mut n) => {
                        // The token only goes back to the server that issued it.
                        let token = self.session.as_ref().filter(|(a, _)| *a == self.server_addr).map_or(0, |s| s.1);
                        n.send(ClientMsg::Hello { version: PROTOCOL_VERSION, name: self.settings.name.clone(), token });
                        self.net = Some(n);
                        self.status = "Joining...".into();
                        self.screen = Screen::Lobby;
                    }
                    Err(e) => {
                        self.status = String::new();
                        self.error = Some(format!("Couldn't connect: {e}"));
                        self.hosted = None;
                    }
                }
            }
        }
        let Some(net) = &mut self.net else { return };
        let msgs = match net.poll() {
            Ok(m) => m,
            Err(e) => {
                let in_game = self.game.is_some();
                self.leave();
                self.error = Some(if in_game { format!("Connection lost ({e})") } else { e });
                return;
            }
        };
        let ping = net.ping_ms;
        for m in msgs {
            match m {
                ServerMsg::Welcome { conn, token } => {
                    self.conn_id = conn;
                    self.session = Some((self.server_addr.clone(), token));
                }
                ServerMsg::Reject(r) => {
                    self.leave();
                    self.error = Some(format!("Server said: {r}"));
                    return;
                }
                ServerMsg::Lobby(l) => {
                    if !l.in_game && self.game.is_some() {
                        self.game = None;
                        self.screen = Screen::Lobby;
                    }
                    if self.single_player && !self.auto_filled && l.host == self.conn_id {
                        self.auto_filled = true;
                        let mut level = AiLevel::Normal;
                        if let Some(map) = &self.args.quickstart {
                            if let Some(i) = l.maps.iter().position(|m| m.name.to_lowercase().replace(' ', "") == map.to_lowercase().replace(' ', "")) {
                                if i as u8 != l.map {
                                    // Pick the map first; slots get reset by the server.
                                    self.send(ClientMsg::SetMap(i as u8));
                                    self.auto_filled = false;
                                    self.lobby = Some(l);
                                    continue;
                                }
                            }
                            // The self-test checks the controls, not whether a rush gets through.
                            level = if self.args.selftest { AiLevel::Easy } else { AiLevel::Hard };
                        }
                        for (i, s) in l.slots.iter().enumerate() {
                            if s.kind == SlotKind::Open {
                                self.send(ClientMsg::SetSlot { slot: i as u8, kind: SlotKind::Ai(level) });
                            }
                        }
                        if self.auto_start {
                            self.send(ClientMsg::Start);
                            self.auto_start = false;
                        }
                    }
                    self.lobby = Some(l);
                    if self.screen != Screen::Game {
                        self.screen = Screen::Lobby;
                    }
                }
                ServerMsg::GameStart(info) => {
                    let host = self.lobby.as_ref().is_some_and(|l| l.host == self.conn_id);
                    let mut g = GameScreen::new(*info, host);
                    g.view.juice.set_shake(self.shake_on);
                    self.game = Some(g);
                    self.view_ids += 1;
                    self.game_id = self.view_ids;
                    self.screen = Screen::Game;
                    self.demo = None;
                }
                ServerMsg::Snapshot(b) => {
                    if let Some(g) = &mut self.game {
                        g.on_snapshot(&b);
                    }
                }
                ServerMsg::Chat { from, color, text, private } => {
                    if let Some(g) = &mut self.game {
                        g.on_chat(from, color, text, private);
                    } else {
                        let c = if color == 255 { ui::theme::TEXT_DIM } else { game::rgba3(sim::game::PLAYER_COLORS[color as usize % 10]) };
                        if from != self.settings.name {
                            self.audio.play(audio::Sfx::Chat);
                        }
                        self.lobby_chat.push(ChatLine { from, color: c, text, private, age: 0.0 });
                    }
                }
                ServerMsg::AiNotes(notes) => {
                    if let Some(g) = &mut self.game {
                        g.on_ai_notes(notes);
                    }
                }
                ServerMsg::Pong(_) => {}
            }
        }
        if let Some(g) = &mut self.game {
            g.ping = ping;
            let out = std::mem::take(&mut g.out);
            for m in out {
                self.send(m);
            }
        }
    }

    // ---------------------------------------------------------------------
    // Frame

    fn frame(&mut self) {
        let now = date::now();
        let dt = ((now - self.last) as f32).clamp(0.0, 0.1);
        // Raw frame time for the F3 readout.
        self.frame_times[self.frame_idx] = (now - self.last).max(0.0) * 1000.0;
        self.frame_idx = (self.frame_idx + 1) % self.frame_times.len();
        if self.frame_count < self.frame_times.len() as u32 {
            self.frame_count += 1;
        }
        self.last = now;
        self.poll_net();
        self.poll_map_job();
        if self.screen == Screen::Join {
            let d = self.discovery.get_or_insert_with(Discovery::new);
            d.poll();
        } else {
            self.discovery = None;
        }
        if let Some(g) = &mut self.game {
            g.update(dt);
        } else if let Some(d) = &mut self.demo {
            d.update(dt);
        }
        self.selftest_frame(dt);

        let (pw, ph) = window::screen_size();
        let scale = self.scale();
        let (lw, lh) = (pw / scale, ph / scale);

        // Screenshots render into an offscreen target we can read back.
        let shoot = self.args.screenshot.is_some() && !self.shot_done && (now - self.start) as f32 >= self.args.shot_after
            && (self.args.quickstart.is_none() || self.game.as_ref().is_some_and(|g| g.view.world.tick > 0));
        if !self.ctx.begin_frame(shoot) {
            self.input.end_frame();
            return;
        }
        if let Some(r) = &mut self.r3d {
            r.shadows = self.settings.shadows;
        }

        // World.
        self.batch.draw_calls = 0;
        self.draw_world(dt, pw, ph);
        self.update_audio(pw, ph);

        // UI.
        self.batch.begin(gfx::screen_proj(lw, lh));
        let mut action_game = GameAction::None;
        let mut action_menu = MenuAction::None;
        // Frame-time readout (F3): average of the last 60 frames.
        let frame_line = self.show_frame_time.then(|| {
            let n = self.frame_count as usize;
            let sum: f64 = if n == self.frame_times.len() {
                self.frame_times.iter().sum()
            } else {
                self.frame_times[..n].iter().sum()
            };
            let avg = sum / n.max(1) as f64;
            let fps = if avg > 0.001 { 1000.0 / avg } else { 0.0 };
            format!("{avg:.1} ms, {fps:.0} fps")
        });
        {
            let mut ui = Ui {
                ctx: &mut self.ctx,
                batch: &mut self.batch,
                font: &self.font,
                input: &mut self.input,
                focus: &mut self.focus,
                time: now - self.start,
                w: lw,
                h: lh,
            };
            match self.screen {
                Screen::Game => {
                    if let Some(g) = &mut self.game {
                        let renderer = self.r3d.as_mut().map(|r| r as &mut dyn WorldRenderer);
                        if let Some(r) = renderer {
                            action_game = g.draw_hud(&mut ui, r, scale);
                            if action_game == GameAction::None {
                                action_game = g.handle_input(&mut ui, r, scale, self.settings.edge_scroll);
                            }
                        }
                    }
                }
                Screen::Main => {
                    vignette(&mut ui);
                    action_menu = menu::main_menu(&mut ui, &mut self.settings, &self.error);
                    if !self.status.is_empty() && self.connecting.is_some() {
                        ui.text_shadow(&self.status, lw / 2.0, lh - 70.0, 18.0, ui::theme::ACCENT, font::Align::Center);
                    }
                }
                Screen::Join => {
                    vignette(&mut ui);
                    let found: Vec<(SocketAddr, sim::proto::Beacon)> =
                        self.discovery.as_ref().map_or(Vec::new(), |d| d.found.iter().map(|(a, b, _)| (*a, b.clone())).collect());
                    let lan_ok = self.discovery.as_ref().is_none_or(|d| d.available());
                    let status = if let Some(e) = &self.error { e.clone() } else { self.status.clone() };
                    action_menu = menu::join_menu(&mut ui, &mut self.join_addr, &found, &status, lan_ok);
                }
                Screen::Settings => {
                    vignette(&mut ui);
                    action_menu = menu::settings_menu(&mut ui, &mut self.settings, self.shake_on);
                }
                Screen::Lobby => {
                    vignette(&mut ui);
                    if let Some(l) = &self.lobby {
                        let join_info = self.hosted.as_ref().map_or(Vec::new(), |h| h.join_info());
                        action_menu = menu::lobby(&mut ui, l, self.conn_id, &self.lobby_chat, &mut self.lobby_input, &join_info, &mut self.map_maker);
                    } else {
                        ui.text_shadow(&self.status, lw / 2.0, lh / 2.0, 26.0, ui::theme::TEXT, font::Align::Center);
                        if ui.button(ui::rect(lw / 2.0 - 80.0, lh / 2.0 + 50.0, 160.0, 44.0), "Cancel") {
                            action_menu = MenuAction::Back;
                        }
                    }
                }
            }
            // Frame-time readout: F3 toggles it.
            if ui.input.hit(KeyCode::F3) {
                self.show_frame_time = !self.show_frame_time;
            }
            if let Some(line) = &frame_line {
                // Top-left, below the top bar in a game.
                let y = if self.screen == Screen::Game { 44.0 } else { 10.0 };
                ui.text_shadow(line, 10.0, y, 16.0, ui::theme::ACCENT, font::Align::Left);
            }
        }
        self.batch.render(&mut self.ctx);
        if let Some(img) = self.ctx.end_frame() {
            self.save_screenshot(img);
        }
        #[cfg(target_arch = "wasm32")]
        if !self.announced {
            self.announced = true;
            web::ready();
        }
        if self.input.clicked {
            self.audio.play(audio::Sfx::Click);
        }
        self.input.end_frame();
        self.audio.end_frame();

        self.fps.0 += 1;
        if now - self.fps.1 >= 1.0 {
            self.fps.2 = self.fps.0;
            self.fps.0 = 0;
            self.fps.1 = now;
        }

        match action_game {
            GameAction::Leave => self.leave(),
            GameAction::BackToLobby => self.send(ClientMsg::BackToLobby),
            GameAction::None => {}
        }
        self.apply_menu(action_menu);
    }

    /// Music for the current screen, and the game's sounds placed by where
    /// they happen on screen.
    fn update_audio(&mut self, pw: f32, ph: f32) {
        if (self.settings.music, self.settings.sound) != self.volume {
            self.volume = (self.settings.music, self.settings.sound);
            self.audio.set_volume(self.settings.music_volume(), self.settings.sound_volume());
        }
        let in_game = self.screen == Screen::Game && self.game.is_some();
        self.audio.music(Some(if in_game { audio::Music::Battle } else { audio::Music::Menu }));
        if let Some(d) = &mut self.demo {
            d.view.sounds.clear();
        }
        let Some(g) = &mut self.game else { return };
        for s in std::mem::take(&mut g.sounds) {
            self.audio.play(s);
        }
        let sounds = std::mem::take(&mut g.view.sounds);
        if !in_game {
            return;
        }
        let renderer = self.r3d.as_ref().map(|r| r as &dyn WorldRenderer);
        let Some(r) = renderer else { return };
        let map = &g.view.world.info.map;
        // Zoomed out, everything is a bit further away.
        let zoom = (r.pixels_per_tile(r.center()) / 40.0).clamp(0.35, 1.0);
        for (sfx, pos) in sounds {
            let Some(sp) = r.world_to_screen(v3(pos.x, pos.y, map.height_at(pos))) else { continue };
            let (x, y) = (sp[0] / pw * 2.0 - 1.0, sp[1] / ph * 2.0 - 1.0);
            // Full volume on screen, fading out over a screen's width beyond.
            let off = (x.abs().max(y.abs()) - 1.0).max(0.0);
            let gain = (1.0 - off).max(0.0) * zoom;
            self.audio.play_at(sfx, gain, x * 0.7);
        }
    }

    /// Self-tests and screenshots ignore the real mouse and keyboard, so
    /// whatever the pointer happens to do doesn't change the result.
    fn automated(&self) -> bool {
        self.selftest.is_some() || self.args.screenshot.is_some()
    }

    fn world_renderer(&self) -> Option<&dyn WorldRenderer> {
        self.r3d.as_ref().map(|r| r as &dyn WorldRenderer)
    }

    /// Render the game (or the attract-mode demo) behind the UI.
    fn draw_world(&mut self, dt: f32, pw: f32, ph: f32) -> bool {
        let (view, id, sel, sel_ig, markers): (&GameView, u64, Vec<u16>, Option<u8>, Vec<scene::Marker>) =
            if let (Screen::Game, Some(g)) = (self.screen, &self.game) {
                (&g.view, self.game_id, g.selected.clone(), None, Vec::new())
            } else if let Some(d) = &self.demo {
                (&d.view, self.demo_id, Vec::new(), None, Vec::new())
            } else {
                return false;
            };
        if view.world.penguins.is_empty() {
            return false;
        }
        let ctx = &mut self.ctx;
        let Some(renderer) = self.r3d.as_mut() else { return false };
        if self.prepared != id {
            renderer.prepare(ctx, &view.world.info.map);
            self.prepared = id;
        }
        let renderer: &mut dyn WorldRenderer = renderer;
        renderer.set_viewport(pw, ph);
        if let Some((x, y, zoom)) = self.args.look {
            if self.screen == Screen::Game {
                // A negative X keeps the camera where the game puts it (on
                // your igloo) and only zooms.
                if x >= 0.0 {
                    renderer.focus(sim::math::v2(x, y));
                }
                if self.selftest.is_none() && !self.look_zoomed {
                    renderer.zoom_at(zoom, [pw / 2.0, ph / 2.0]);
                    self.look_zoomed = true;
                }
            }
        }
        if self.screen != Screen::Game {
            if let Some(d) = &self.demo {
                if let Some(look) = d.look {
                    renderer.focus(look);
                }
                renderer.rotate(dt * 0.04);
            }
        }
        let (sel_ig, sel_b, ghosts, markers) = match &self.game {
            Some(g) if self.screen == Screen::Game => (g_selected_igloo(g), g.selected_building(), g.ghosts(), g_markers(g)),
            _ => (sel_ig, None, Vec::new(), markers),
        };
        let mut scene = view.scene(dt, &sel, sel_ig, sel_b, &markers);
        scene.buildings.extend(ghosts);
        renderer.draw(ctx, &scene, &mut self.batch);
        true
    }

    fn save_screenshot(&mut self, img: gfx::Image) {
        let path = self.args.screenshot.clone().unwrap();
        match gfx::save_png(&path, &img) {
            Ok(()) => {
                let penguins = self.game.as_ref().map_or(0, |g| g.view.world.penguins.len());
                println!("saved screenshot {} ({} fps, {penguins} penguins)", path.display(), self.fps.2)
            }
            Err(e) => eprintln!("screenshot failed: {e}"),
        }
        self.shot_done = true;
        window::order_quit();
    }

    fn apply_menu(&mut self, a: MenuAction) {
        match a {
            MenuAction::None => {}
            MenuAction::SinglePlayer => {
                self.error = None;
                self.save_settings();
                self.start_local(true);
            }
            MenuAction::PlayOnline => {
                self.error = None;
                self.save_settings();
                #[cfg(target_arch = "wasm32")]
                self.connect(&web::server_url());
            }
            MenuAction::Host => {
                self.error = None;
                self.save_settings();
                self.start_local(false);
            }
            MenuAction::OpenJoin => {
                self.error = None;
                self.status.clear();
                self.screen = Screen::Join;
            }
            MenuAction::OpenSettings => self.screen = Screen::Settings,
            MenuAction::Quit => window::order_quit(),
            MenuAction::Connect(addr) => {
                self.error = None;
                self.connect(&addr);
            }
            MenuAction::Back => {
                if self.screen == Screen::Settings {
                    self.save_settings();
                }
                if self.screen == Screen::Lobby {
                    self.leave();
                }
                self.screen = Screen::Main;
                self.status.clear();
            }
            MenuAction::ToggleFullscreen => {
                self.settings.fullscreen = !self.settings.fullscreen;
                window::set_fullscreen(self.settings.fullscreen);
                self.save_settings();
            }
            MenuAction::ToggleScreenShake => {
                self.shake_on = !self.shake_on;
                if let Some(d) = &mut self.demo {
                    d.shake_on = self.shake_on;
                    d.view.juice.set_shake(self.shake_on);
                }
                if let Some(g) = &mut self.game {
                    g.view.juice.set_shake(self.shake_on);
                }
            }
            MenuAction::SetMap(m) => self.send(ClientMsg::SetMap(m)),
            MenuAction::SetSlot(s, k) => self.send(ClientMsg::SetSlot { slot: s, kind: k }),
            MenuAction::SetTeam(s, t) => self.send(ClientMsg::SetTeam { slot: s, team: t }),
            MenuAction::SetColor(s, c) => self.send(ClientMsg::SetColor { slot: s, color: c }),
            MenuAction::TakeSlot(s) => self.send(ClientMsg::TakeSlot(s)),
            MenuAction::Start => self.send(ClientMsg::Start),
            MenuAction::Chat(t) => self.send(ClientMsg::Cmd(sim::game::Command::Chat { to: None, text: t })),
            MenuAction::MakeMap(prompt) => {
                let seed = date::now().to_bits();
                self.map_maker.busy = true;
                self.map_maker.status = "Looking for the local model...".into();
                self.map_job = Some(llm::request_map(llm::LlmConfig::from_settings(&self.settings), prompt, seed));
            }
        }
    }

    /// Follow the local model's map; when it's done, build the map and hand
    /// it to the server, which adds it to the lobby and selects it.
    fn poll_map_job(&mut self) {
        // Dev switch for headless runs: SNOWBALLZ_MAKE_MAP="..." asks for a
        // map as soon as we're hosting a lobby.
        if self.map_job.is_none() && self.map_maker.status.is_empty() && self.lobby.as_ref().is_some_and(|l| l.host == self.conn_id) {
            if let Ok(prompt) = std::env::var("SNOWBALLZ_MAKE_MAP") {
                self.map_maker.open = true;
                self.map_maker.prompt = prompt.clone();
                self.apply_menu(MenuAction::MakeMap(prompt));
            }
        }
        let Some(job) = self.map_job.as_mut() else { return };
        match job.poll() {
            Some(llm::MapJob::Working(s)) => self.map_maker.status = s,
            Some(llm::MapJob::Done { spec, from_model, note }) => {
                self.map_job = None;
                self.map_maker.busy = false;
                let map = sim::mapgen::generate(&spec);
                let how = if from_model { "The local AI made" } else { "Made" };
                self.map_maker.status = format!("{how} \"{}\". {note}", map.name).trim().to_string();
                self.send(ClientMsg::AddMap(map.encode()));
            }
            None => {}
        }
    }
}

fn g_selected_igloo(g: &GameScreen) -> Option<u8> {
    g.selected_igloo()
}

fn g_markers(g: &GameScreen) -> Vec<scene::Marker> {
    g.markers()
}

/// Darken the edges behind menus so text stays readable over the demo.
fn vignette(ui: &mut Ui) {
    ui.batch.rect(ui.ctx, 0.0, 0.0, ui.w, ui.h, [10, 25, 50, 70]);
}

fn fatal(msg: &str) -> ! {
    // There's no process to exit in a browser; the panic hook logs it.
    #[cfg(target_arch = "wasm32")]
    panic!("{msg}");
    #[cfg(not(target_arch = "wasm32"))]
    {
        eprintln!("snowballz: {msg}");
        std::process::exit(1);
    }
}

impl App {
    fn mouse_motion_event(&mut self, x: f32, y: f32) {
        if self.automated() {
            return;
        }
        let s = self.scale();
        let m = [x / s, y / s];
        self.input.touch = false;
        self.input.mouse_delta[0] += m[0] - self.input.mouse[0];
        self.input.mouse_delta[1] += m[1] - self.input.mouse[1];
        self.input.mouse = m;
        self.input.inside = true;
    }

    fn mouse_leave_event(&mut self) {
        if self.automated() {
            return;
        }
        self.input.inside = false;
    }

    fn mouse_wheel_event(&mut self, _x: f32, y: f32) {
        if self.automated() {
            return;
        }
        self.input.wheel += y.signum();
    }

    fn mouse_button_down_event(&mut self, b: MouseButton, x: f32, y: f32) {
        if self.automated() {
            return;
        }
        self.mouse_motion_event(x, y);
        let i = match b {
            MouseButton::Left => 0,
            MouseButton::Right => 1,
            MouseButton::Middle => 2,
            MouseButton::Unknown => return,
        };
        self.input.down[i] = true;
        self.input.pressed[i] = true;
        if i == 0 {
            let now = date::now();
            let m = self.input.mouse;
            let lp = self.input.last_click_pos;
            if now - self.input.last_click < 0.35 && (m[0] - lp[0]).abs() + (m[1] - lp[1]).abs() < 8.0 {
                self.input.double_click = true;
            }
            self.input.last_click = now;
            self.input.last_click_pos = m;
        }
    }

    fn mouse_button_up_event(&mut self, b: MouseButton, x: f32, y: f32) {
        if self.automated() {
            return;
        }
        self.mouse_motion_event(x, y);
        let i = match b {
            MouseButton::Left => 0,
            MouseButton::Right => 1,
            MouseButton::Middle => 2,
            MouseButton::Unknown => return,
        };
        self.input.down[i] = false;
        self.input.released[i] = true;
    }

    /// Fingers: one acts as the left button (the game turns that into tap,
    /// drag-to-pan and press-and-hold gestures); two pinch and pan.
    fn touch_event(&mut self, phase: TouchPhase, id: u64, x: f32, y: f32) {
        if self.automated() {
            return;
        }
        let s = self.scale();
        let p = [x / s, y / s];
        match phase {
            TouchPhase::Started => {
                if self.touches.is_empty() {
                    self.touches.push((id, p));
                    self.mouse_button_down_event(MouseButton::Left, x, y);
                } else if self.touches.len() == 1 {
                    // Second finger: abandon the first one's tap or drag.
                    self.touches.push((id, p));
                    self.input.down[0] = false;
                    self.input.cancel = true;
                }
            }
            TouchPhase::Moved => {
                let Some(i) = self.touches.iter().position(|t| t.0 == id) else { return };
                if self.touches.len() == 1 {
                    self.mouse_motion_event(x, y);
                } else if self.touches.len() == 2 {
                    let spread = |t: &[(u64, [f32; 2])]| ((t[0].1[0] - t[1].1[0]).powi(2) + (t[0].1[1] - t[1].1[1]).powi(2)).sqrt();
                    let mid = |t: &[(u64, [f32; 2])]| [(t[0].1[0] + t[1].1[0]) / 2.0, (t[0].1[1] + t[1].1[1]) / 2.0];
                    let (d0, m0) = (spread(&self.touches), mid(&self.touches));
                    self.touches[i].1 = p;
                    let (d1, m1) = (spread(&self.touches), mid(&self.touches));
                    if d0 > 8.0 && d1 > 8.0 {
                        self.input.pinch = if self.input.pinch > 0.0 { self.input.pinch } else { 1.0 } * d1 / d0;
                    }
                    self.input.pan2[0] += m1[0] - m0[0];
                    self.input.pan2[1] += m1[1] - m0[1];
                    self.input.mouse = m1;
                }
                if let Some(t) = self.touches.iter_mut().find(|t| t.0 == id) {
                    t.1 = p;
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let Some(i) = self.touches.iter().position(|t| t.0 == id) else { return };
                let alone = self.touches.len() == 1;
                self.touches.remove(i);
                if alone {
                    if phase == TouchPhase::Ended {
                        self.mouse_button_up_event(MouseButton::Left, x, y);
                    } else {
                        self.input.down[0] = false;
                        self.input.cancel = true;
                    }
                }
            }
        }
        self.input.touch = true;
        self.input.inside = false;
    }

    fn char_event(&mut self, c: char, _mods: KeyMods, _repeat: bool) {
        if self.automated() {
            return;
        }
        self.input.chars.push(c);
    }

    fn key_down_event(&mut self, k: KeyCode, mods: KeyMods, repeat: bool) {
        if self.automated() {
            return;
        }
        self.input.mods = mods;
        self.input.keys_down.insert(k);
        self.input.keys_pressed.push((k, repeat));
        if k == KeyCode::F11 || (k == KeyCode::Enter && mods.alt) {
            self.settings.fullscreen = !self.settings.fullscreen;
            window::set_fullscreen(self.settings.fullscreen);
        }
    }

    fn key_up_event(&mut self, k: KeyCode, mods: KeyMods) {
        if self.automated() {
            return;
        }
        self.input.mods = mods;
        self.input.keys_down.remove(&k);
    }

    fn quit_requested_event(&mut self) {
        self.save_settings();
        // Close the router port before the process exits.
        drop(self.hosted.take());
    }
}

/// Dev switch for headless runs: `SNOWBALLZ_MAP_PREVIEW="1"` or
/// `SNOWBALLZ_MAP_PREVIEW="1 frozen islands with a volcano"` generates a map
/// from those words and writes a top-down PNG (one pixel per tile: snow
/// white, ice pale blue, the water of fish holes dark blue, trees green,
/// ice pinnacles grey, igloo spots red) to `/tmp/map-<seed>.png`, then exits
/// without opening a window.
fn map_preview(value: &str) -> Result<(), String> {
    let mut it = value.split_whitespace();
    let seed = it
        .next()
        .ok_or("SNOWBALLZ_MAP_PREVIEW needs the map seed (a number, then optional map words)")?
        .parse::<u64>()
        .map_err(|_| "SNOWBALLZ_MAP_PREVIEW starts with the map seed, e.g. \"1 frozen islands\"")?;
    let prompt = it.collect::<Vec<_>>().join(" ");
    let spec = sim::mapgen::MapSpec::from_words(&prompt, seed);
    let map = sim::mapgen::generate(&spec);
    let in_igloo = |x: i32, y: i32| {
        map.igloos
            .iter()
            .any(|&(gx, gy)| x >= gx as i32 && x < gx as i32 + IGLOO_SIZE && y >= gy as i32 && y < gy as i32 + IGLOO_SIZE)
    };
    let mut rgba = vec![0u8; (map.w * map.h * 4) as usize];
    for y in 0..map.h {
        for x in 0..map.w {
            let i = map.idx(x, y);
            let c = if in_igloo(x, y) {
                [220, 60, 70, 255] // igloo spots
            } else if map.obstacle[i] == Obstacle::Tree {
                [46, 110, 55, 255] // trees
            } else if map.obstacle[i] == Obstacle::Pinnacle {
                [128, 130, 136, 255] // rock
            } else if map
                .resources
                .iter()
                .any(|r| r.kind == ResourceKind::Fish && r.x as i32 == x && r.y as i32 == y)
            {
                [38, 104, 148, 255] // water (the fish holes' open sea)
            } else if map.ground[i] == Ground::Water {
                [38, 104, 148, 255] // open water
            } else if map.ground[i] == Ground::Ice {
                [168, 214, 240, 255] // frozen sea and lakes
            } else {
                [255, 255, 255, 255] // snow
            };
            let p = ((y * map.w + x) * 4) as usize;
            rgba[p..p + 4].copy_from_slice(&c);
        }
    }
    let path = format!("/tmp/map-{seed}.png");
    let img = gfx::Image { w: map.w as u32, h: map.h as u32, rgba };
    gfx::save_png(std::path::Path::new(&path), &img).map_err(|e| format!("map preview failed: {e}"))?;
    println!("saved map preview \"{}\" to {} ({}x{} tiles)", map.name, path, map.w, map.h);
    Ok(())
}

fn main() {
    // Headless map preview: no window, no lobby, just the PNG.
    if let Ok(v) = std::env::var("SNOWBALLZ_MAP_PREVIEW") {
        if !v.trim().is_empty() {
            if let Err(e) = map_preview(&v) {
                eprintln!("{e}");
                std::process::exit(2);
            }
            return;
        }
    }
    // Panics would otherwise vanish into an "unreachable" trap.
    #[cfg(target_arch = "wasm32")]
    std::panic::set_hook(Box::new(|info| web::log_error(&info.to_string())));
    let args = parse_args();
    let event_loop = winit::event_loop::EventLoop::new().unwrap_or_else(|e| fatal(&format!("no window system: {e}")));
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
    let runner = Runner { args: Some(args), app: None, cursor: [0.0; 2], mods: KeyMods::default(), pending: Default::default() };
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(runner);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut runner = runner;
        if let Err(e) = event_loop.run_app(&mut runner) {
            fatal(&format!("the window closed badly: {e}"));
        }
    }
}

/// Opens the window, then hands winit's events to the [`App`].
struct Runner {
    args: Option<Args>,
    app: Option<App>,
    /// Where the pointer is, in physical pixels.
    cursor: [f32; 2],
    mods: KeyMods,
    /// The GPU, once the browser has handed it over (it is asynchronous there).
    pending: std::rc::Rc<std::cell::RefCell<Option<Result<gpu::Gpu, String>>>>,
}

impl winit::application::ApplicationHandler for Runner {
    fn resumed(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        if window::STATE.with(|s| s.borrow().window.is_some()) {
            return;
        }
        let Some(args) = &self.args else { return };
        let settings = Settings::load();
        let (w, h) = args.windowed.unwrap_or((1280, 800));
        let full = settings.fullscreen && args.screenshot.is_none() && args.windowed.is_none();
        #[allow(unused_mut)]
        let mut attrs = winit::window::Window::default_attributes()
            .with_title("Snowballz")
            .with_fullscreen(full.then_some(winit::window::Fullscreen::Borderless(None)));
        // In a browser the page's CSS sizes the canvas (a size here would pin
        // it with an inline style, and it would never follow the window).
        #[cfg(not(target_arch = "wasm32"))]
        {
            attrs = attrs.with_inner_size(winit::dpi::LogicalSize::new(w, h));
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (w, h);
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            use winit::platform::web::WindowAttributesExtWebSys;
            let canvas = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id("glcanvas"))
                .and_then(|c| c.dyn_into::<web_sys::HtmlCanvasElement>().ok());
            attrs = attrs.with_canvas(canvas).with_prevent_default(true);
        }
        #[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
        {
            use winit::platform::x11::WindowAttributesExtX11;
            attrs = attrs.with_name("snowballz", "snowballz");
        }
        let win = match el.create_window(attrs) {
            Ok(w) => std::sync::Arc::new(w),
            Err(e) => fatal(&format!("can't open a window: {e}")),
        };
        let size = win.inner_size();
        window::STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.window = Some(win.clone());
            s.size = (size.width.max(1) as f32, size.height.max(1) as f32);
            s.dpi = win.scale_factor() as f32;
        });
        let (pw, ph) = (size.width.max(1), size.height.max(1));
        #[cfg(not(target_arch = "wasm32"))]
        {
            *self.pending.borrow_mut() = Some(gpu::block_on(gpu::Gpu::new(win.clone(), pw, ph)));
        }
        #[cfg(target_arch = "wasm32")]
        {
            let pending = self.pending.clone();
            let w2 = win.clone();
            wasm_bindgen_futures::spawn_local(async move {
                *pending.borrow_mut() = Some(gpu::Gpu::new(w2.clone(), pw, ph).await);
                w2.request_redraw();
            });
        }
        win.request_redraw();
    }

    fn about_to_wait(&mut self, _el: &winit::event_loop::ActiveEventLoop) {
        window::STATE.with(|s| {
            if let Some(w) = &s.borrow().window {
                w.request_redraw();
            }
        });
    }

    fn window_event(&mut self, el: &winit::event_loop::ActiveEventLoop, _id: winit::window::WindowId, event: winit::event::WindowEvent) {
        use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
        // Sizes can change before the GPU is ready (always, in a browser).
        if let WindowEvent::Resized(size) = &event {
            window::STATE.with(|s| s.borrow_mut().size = (size.width.max(1) as f32, size.height.max(1) as f32));
        }
        if self.app.is_none() {
            if let Some(r) = self.pending.borrow_mut().take() {
                match r {
                    Ok(mut gpu) => {
                        let (w, h) = window::screen_size();
                        gpu.resize(w as u32, h as u32);
                        self.app = Some(App::new(self.args.take().unwrap(), gpu));
                    }
                    Err(e) => fatal(&e),
                }
            }
        }
        let Some(app) = &mut self.app else { return };
        match event {
            WindowEvent::CloseRequested => {
                app.quit_requested_event();
                el.exit();
            }
            WindowEvent::Resized(size) => app.ctx.resize(size.width, size.height),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                window::STATE.with(|s| s.borrow_mut().dpi = scale_factor as f32);
            }
            WindowEvent::RedrawRequested => {
                app.frame();
                if window::STATE.with(|s| s.borrow().quit) {
                    app.quit_requested_event();
                    el.exit();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = [position.x as f32, position.y as f32];
                app.mouse_motion_event(self.cursor[0], self.cursor[1]);
            }
            WindowEvent::CursorLeft { .. } => app.mouse_leave_event(),
            WindowEvent::MouseWheel { delta, .. } => {
                let y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32,
                };
                if y != 0.0 {
                    app.mouse_wheel_event(0.0, y);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let b = match button {
                    winit::event::MouseButton::Left => MouseButton::Left,
                    winit::event::MouseButton::Right => MouseButton::Right,
                    winit::event::MouseButton::Middle => MouseButton::Middle,
                    _ => MouseButton::Unknown,
                };
                let [x, y] = self.cursor;
                match state {
                    ElementState::Pressed => app.mouse_button_down_event(b, x, y),
                    ElementState::Released => app.mouse_button_up_event(b, x, y),
                }
            }
            WindowEvent::Touch(t) => {
                let phase = match t.phase {
                    winit::event::TouchPhase::Started => TouchPhase::Started,
                    winit::event::TouchPhase::Moved => TouchPhase::Moved,
                    winit::event::TouchPhase::Ended => TouchPhase::Ended,
                    winit::event::TouchPhase::Cancelled => TouchPhase::Cancelled,
                };
                app.touch_event(phase, t.id, t.location.x as f32, t.location.y as f32);
            }
            WindowEvent::ModifiersChanged(m) => {
                let st = m.state();
                self.mods = KeyMods { shift: st.shift_key(), ctrl: st.control_key(), alt: st.alt_key(), logo: st.super_key() };
                app.input.mods = self.mods;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let winit::keyboard::PhysicalKey::Code(code) = event.physical_key {
                    if let Some(k) = platform::key(code) {
                        match event.state {
                            ElementState::Pressed => app.key_down_event(k, self.mods, event.repeat),
                            ElementState::Released => app.key_up_event(k, self.mods),
                        }
                    }
                }
                if event.state == ElementState::Pressed {
                    if let Some(text) = &event.text {
                        for c in text.chars().filter(|c| !c.is_control()) {
                            app.char_event(c, self.mods, event.repeat);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
