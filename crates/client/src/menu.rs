//! Main menu, join, settings and lobby screens.

use sim::ai::AiLevel;
use sim::game::PLAYER_COLORS;
use sim::proto::{Beacon, LobbyState, SlotKind};

use crate::font::Align;
use crate::game::{rgba3, ChatLine};
use crate::gfx::with_alpha;
use crate::settings::Settings;
use crate::ui::{rect, theme, Ui};

/// The browser build: no hosting, no LAN, no local language model.
const WEB: bool = cfg!(target_arch = "wasm32");

pub enum MenuAction {
    None,
    /// Join the server this page came from (browser only).
    PlayOnline,
    SinglePlayer,
    Host,
    OpenJoin,
    OpenSettings,
    Quit,
    Connect(String),
    Back,
    ToggleFullscreen,
    ToggleScreenShake,
    // Lobby
    SetMap(u8),
    SetSlot(u8, SlotKind),
    SetTeam(u8, u8),
    SetColor(u8, u8),
    TakeSlot(u8),
    Start,
    Chat(String),
    /// Ask the local model for a new map from this request.
    MakeMap(String),
}

/// The host's "make a map" box in the lobby.
#[derive(Default)]
pub struct MapMaker {
    pub open: bool,
    pub prompt: String,
    /// What's happening, in plain English.
    pub status: String,
    pub busy: bool,
    /// First map shown in the list, when there are too many to fit.
    pub scroll: usize,
    /// The selected map last frame.
    pub shown: Option<usize>,
}

fn title(ui: &mut Ui, y: f32) {
    let t = ui.time as f32;
    let bob = (t * 1.3).sin() * 4.0;
    ui.text_shadow("Snowballz", ui.w / 2.0, y + bob, 92.0, [255, 255, 255, 255], Align::Center);
    ui.text_shadow("a penguin snowball fight", ui.w / 2.0, y + 104.0, 22.0, [200, 225, 255, 255], Align::Center);
}

pub fn main_menu(ui: &mut Ui, settings: &mut Settings, error: &Option<String>) -> MenuAction {
    let mut act = MenuAction::None;
    title(ui, ui.h * 0.12);
    let w = 340.0;
    let x = ui.w / 2.0 - w / 2.0;
    let mut y = ui.h * 0.12 + 160.0;
    let p = rect(x - 24.0, y - 20.0, w + 48.0, if WEB { 280.0 } else { 340.0 });
    ui.panel(p);
    ui.text("Your name", x, y, 16.0, theme::TEXT_DIM, Align::Left);
    y += 22.0;
    ui.text_field(10, rect(x, y, w, 36.0), &mut settings.name, 24, "Penguin");
    y += 52.0;
    let bh = 46.0;
    if WEB {
        // Everyone meets on the server that hosts this page; computer
        // players are added in its lobby.
        if ui.button(rect(x, y, w, bh), "Play Online") {
            act = MenuAction::PlayOnline;
        }
        y += bh + 10.0;
        if ui.button(rect(x, y, w, bh), "Join Another Server") {
            act = MenuAction::OpenJoin;
        }
        y += bh + 10.0;
        if ui.button(rect(x, y, w, bh), "Settings") {
            act = MenuAction::OpenSettings;
        }
    } else {
        if ui.button(rect(x, y, w, bh), "Play vs Computer") {
            act = MenuAction::SinglePlayer;
        }
        y += bh + 10.0;
        if ui.button(rect(x, y, w, bh), "Host LAN / Internet Game") {
            act = MenuAction::Host;
        }
        y += bh + 10.0;
        if ui.button(rect(x, y, w, bh), "Join Game") {
            act = MenuAction::OpenJoin;
        }
        y += bh + 10.0;
        let half = (w - 10.0) / 2.0;
        if ui.button(rect(x, y, half, bh), "Settings") {
            act = MenuAction::OpenSettings;
        }
        if ui.button(rect(x + half + 10.0, y, half, bh), "Quit") {
            act = MenuAction::Quit;
        }
    }
    if let Some(e) = error {
        let lines = ui.font.wrap(e, 17.0, w + 200.0);
        let mut ey = p.y + p.h + 14.0;
        for l in lines {
            ui.text_shadow(&l, ui.w / 2.0, ey, 17.0, theme::BAD, Align::Center);
            ey += 22.0;
        }
    }
    ui.text_shadow(
        "Original Snowballz by Joey Marshall, Matthew Marshall & friends (2007). Rewritten 2026.",
        ui.w / 2.0,
        ui.h - 30.0,
        14.0,
        with_alpha(theme::TEXT, 0.75),
        Align::Center,
    );
    act
}

pub fn join_menu(ui: &mut Ui, addr: &mut String, found: &[(std::net::SocketAddr, Beacon)], status: &str, lan_ok: bool) -> MenuAction {
    let mut act = MenuAction::None;
    title(ui, ui.h * 0.08);
    let w = 520.0;
    let x = ui.w / 2.0 - w / 2.0;
    let mut y = ui.h * 0.08 + 150.0;
    let p = rect(x - 24.0, y - 20.0, w + 48.0, if WEB { 280.0 } else { 470.0 });
    ui.panel(p);
    if !WEB {
        ui.text("Games on your network", x, y, 20.0, theme::TEXT, Align::Left);
        y += 34.0;
    }
    if found.is_empty() && !WEB {
        let msg = if lan_ok { "Looking for games..." } else { "LAN discovery unavailable (port in use)" };
        ui.text(msg, x, y + 8.0, 16.0, theme::TEXT_DIM, Align::Left);
        y += 44.0;
    }
    for (a, b) in found.iter().take(5) {
        let label = format!(
            "{} — {} — {}/{} players{}",
            b.name,
            b.map,
            b.players,
            b.max,
            if b.in_game { " (in game)" } else { "" }
        );
        if ui.button(rect(x, y, w, 40.0), &label) {
            act = MenuAction::Connect(a.to_string());
        }
        y += 46.0;
    }
    y = if WEB { y + 10.0 } else { y.max(p.y + 260.0) };
    let (label, example) = if WEB {
        ("Server address (host:port, or ws://... / wss://...)", "game.example.com")
    } else {
        ("Or enter an address (host or host:port)", "192.168.1.20")
    };
    ui.text(label, x, y, 16.0, theme::TEXT_DIM, Align::Left);
    y += 24.0;
    let submit = ui.text_field(20, rect(x, y, w - 130.0, 40.0), addr, 80, example);
    if (ui.button(rect(x + w - 120.0, y, 120.0, 40.0), "Connect") || submit) && !addr.trim().is_empty() {
        act = MenuAction::Connect(addr.trim().to_string());
    }
    y += 54.0;
    ui.text(status, x, y, 16.0, theme::ACCENT, Align::Left);
    if ui.button(rect(x, p.y + p.h - 64.0, 140.0, 44.0), "Back") {
        act = MenuAction::Back;
    }
    act
}

pub fn settings_menu(ui: &mut Ui, s: &mut Settings, shake_on: bool) -> MenuAction {
    let mut act = MenuAction::None;
    title(ui, ui.h * 0.08);
    let w = 460.0;
    let x = ui.w / 2.0 - w / 2.0;
    let mut y = ui.h * 0.08 + 150.0;
    let p = rect(x - 24.0, y - 20.0, w + 48.0, 540.0);
    ui.panel(p);
    let row = |ui: &mut Ui, y: f32, label: &str| ui.text(label, x, y + 9.0, 18.0, theme::TEXT, Align::Left);

    row(ui, y, "Name");
    ui.text_field(30, rect(x + 200.0, y, w - 200.0, 36.0), &mut s.name, 24, "Penguin");
    y += 50.0;
    row(ui, y, "Shadows");
    if ui.button(rect(x + 200.0, y, w - 200.0, 36.0), if s.shadows { "On" } else { "Off" }) {
        s.shadows = !s.shadows;
    }
    y += 50.0;
    row(ui, y, "Edge scrolling");
    if ui.button(rect(x + 200.0, y, w - 200.0, 36.0), if s.edge_scroll { "On" } else { "Off" }) {
        s.edge_scroll = !s.edge_scroll;
    }
    y += 50.0;
    row(ui, y, "Screen shake");
    if ui.button(rect(x + 200.0, y, w - 200.0, 36.0), if shake_on { "On" } else { "Off" }) {
        act = MenuAction::ToggleScreenShake;
    }
    y += 50.0;
    row(ui, y, "Interface size");
    if ui.button(rect(x + 200.0, y, 60.0, 36.0), "-") {
        s.ui_scale = ((s.ui_scale - 0.1) * 10.0).round() / 10.0;
        s.ui_scale = s.ui_scale.max(0.6);
    }
    ui.text_in(&format!("{:.0}%", s.ui_scale * 100.0), rect(x + 260.0, y, w - 320.0, 36.0), 18.0, theme::TEXT, Align::Center);
    if ui.button(rect(x + w - 60.0, y, 60.0, 36.0), "+") {
        s.ui_scale = ((s.ui_scale + 0.1) * 10.0).round() / 10.0;
        s.ui_scale = s.ui_scale.min(2.5);
    }
    y += 50.0;
    for (label, v) in [("Music", &mut s.music), ("Sound effects", &mut s.sound)] {
        row(ui, y, label);
        if ui.button(rect(x + 200.0, y, 60.0, 36.0), "-") {
            *v = v.saturating_sub(1);
        }
        let text = if *v == 0 { "Off".to_string() } else { format!("{}%", *v as u32 * 10) };
        ui.text_in(&text, rect(x + 260.0, y, w - 320.0, 36.0), 18.0, theme::TEXT, Align::Center);
        if ui.button(rect(x + w - 60.0, y, 60.0, 36.0), "+") {
            *v = (*v + 1).min(10);
        }
        y += 50.0;
    }
    row(ui, y, "Fullscreen");
    if ui.button(rect(x + 200.0, y, w - 200.0, 36.0), if s.fullscreen { "On" } else { "Off" }) {
        act = MenuAction::ToggleFullscreen;
    }
    if ui.button(rect(x, p.y + p.h - 64.0, 140.0, 44.0), "Back") {
        act = MenuAction::Back;
    }
    act
}

fn slot_kind_label(k: SlotKind) -> String {
    match k {
        SlotKind::Open => "Open".into(),
        SlotKind::Closed => "Closed".into(),
        SlotKind::Human => "Human".into(),
        SlotKind::Ai(l) => format!("Computer ({})", l.name()),
    }
}

fn next_kind(k: SlotKind) -> SlotKind {
    match k {
        SlotKind::Open => SlotKind::Ai(AiLevel::Easy),
        SlotKind::Ai(AiLevel::Easy) => SlotKind::Ai(AiLevel::Normal),
        SlotKind::Ai(AiLevel::Normal) => SlotKind::Ai(AiLevel::Hard),
        SlotKind::Ai(AiLevel::Hard) => SlotKind::Closed,
        _ => SlotKind::Open,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn lobby(
    ui: &mut Ui,
    l: &LobbyState,
    me: u32,
    chat: &[ChatLine],
    chat_input: &mut String,
    join_info: &[(String, crate::gfx::Color)],
    maker: &mut MapMaker,
) -> MenuAction {
    let mut act = MenuAction::None;
    let host = l.host == me;
    let w = (ui.w - 60.0).min(1100.0);
    let x0 = (ui.w - w) / 2.0;
    let h = (ui.h - 60.0).min(760.0);
    let top = (ui.h - h) / 2.0;
    ui.panel(rect(x0, top, w, h));
    ui.text(&l.server_name, x0 + 24.0, top + 18.0, 30.0, theme::TEXT, Align::Left);
    for (i, (line, color)) in join_info.iter().enumerate() {
        ui.text(line, x0 + w - 24.0, top + 10.0 + i as f32 * 19.0, 15.0, *color, Align::Right);
    }

    // Map column.
    let mx = x0 + 24.0;
    let mut y = top + 74.0;
    let mw = 300.0;
    ui.text("Map", mx, y, 18.0, theme::TEXT_DIM, Align::Left);
    y += 26.0;
    // As many maps as fit above the description and the map maker; the
    // wheel scrolls the rest.
    let can_make = host && !WEB;
    let maker_h = if !can_make { 0.0 } else if maker.open { 160.0 } else { 46.0 };
    // Room under the list for the scroll hint and three lines of description.
    let below = 96.0;
    let list_bottom = top + h - 240.0 - below - maker_h;
    let rows = (((list_bottom - y) / 38.0).floor() as usize).max(2);
    let list = rect(mx, y, mw, rows as f32 * 38.0);
    let max_scroll = l.maps.len().saturating_sub(rows);
    if ui.hovered(list) && ui.input.wheel != 0.0 {
        maker.scroll = if ui.input.wheel > 0.0 { maker.scroll.saturating_sub(1) } else { maker.scroll + 1 };
    }
    // Bring a newly selected map into view (an added one lands at the end).
    let sel = l.map as usize;
    if maker.shown != Some(sel) && (sel < maker.scroll || sel >= maker.scroll + rows) {
        maker.scroll = sel.saturating_sub(rows - 1);
    }
    maker.shown = Some(sel);
    maker.scroll = maker.scroll.min(max_scroll);
    for (i, m) in l.maps.iter().enumerate().skip(maker.scroll).take(rows) {
        let sel = i as u8 == l.map;
        let label = format!("{} ({}p)", m.name, m.players);
        if ui.button_ex(rect(mx, y, mw, 32.0), &label, host || sel, sel) && host {
            act = MenuAction::SetMap(i as u8);
        }
        y += 38.0;
    }
    if max_scroll > 0 {
        let more = format!("Maps {}-{} of {}: scroll for more", maker.scroll + 1, (maker.scroll + rows).min(l.maps.len()), l.maps.len());
        ui.text(&more, mx, y, 13.0, theme::TEXT_DIM, Align::Left);
        y += 18.0;
    }
    if let Some(m) = l.maps.get(l.map as usize) {
        y += 4.0;
        for line in ui.font.wrap(&m.description, 15.0, mw).into_iter().take(3) {
            ui.text(&line, mx, y, 15.0, theme::TEXT, Align::Left);
            y += 20.0;
        }
    }
    if can_make {
        let y = y.max(list_bottom + below);
        if !maker.open {
            if ui.button(rect(mx, y, mw, 36.0), "Make a new map...") {
                maker.open = true;
            }
        } else {
            ui.text("Describe a map; the local AI builds it:", mx, y, 14.0, theme::TEXT_DIM, Align::Left);
            let go = ui.text_field(41, rect(mx, y + 20.0, mw, 34.0), &mut maker.prompt, 200, "frozen islands, crystal in the middle...");
            let ready = !maker.busy && !maker.prompt.trim().is_empty();
            let label = if maker.busy { "Working..." } else { "Make map" };
            if (ui.button_ex(rect(mx, y + 60.0, mw - 96.0, 34.0), label, ready, false) || go) && ready {
                act = MenuAction::MakeMap(maker.prompt.trim().to_string());
            }
            if ui.button_ex(rect(mx + mw - 88.0, y + 60.0, 88.0, 34.0), "Close", !maker.busy, false) {
                maker.open = false;
            }
            for (k, line) in ui.font.wrap(&maker.status, 14.0, mw).into_iter().take(3).enumerate() {
                ui.text(&line, mx, y + 100.0 + k as f32 * 18.0, 14.0, theme::TEXT_DIM, Align::Left);
            }
        }
    }

    // Slots.
    let sx = mx + mw + 30.0;
    let sw = (w - (sx - x0) - 24.0).min(520.0);
    let mut y = top + 74.0;
    ui.text("Players", sx, y, 18.0, theme::TEXT_DIM, Align::Left);
    y += 26.0;
    let used: Vec<u8> = l.slots.iter().map(|s| s.color).collect();
    let n_slots = l.slots.len() as u8;
    for (i, s) in l.slots.iter().enumerate() {
        let i = i as u8;
        let mine = s.conn == me && s.conn != 0;
        let editable = host || mine;
        let c = rgba3(PLAYER_COLORS[s.color as usize % PLAYER_COLORS.len()]);
        if ui.swatch(rect(sx, y + 4.0, 28.0, 28.0), c) && editable && s.kind != SlotKind::Closed {
            let next = (1..PLAYER_COLORS.len() as u8).map(|d| (s.color + d) % PLAYER_COLORS.len() as u8).find(|c| !used.contains(c));
            if let Some(nc) = next {
                act = MenuAction::SetColor(i, nc);
            }
        }
        let label = match s.kind {
            SlotKind::Human => {
                if mine {
                    format!("{} (you)", s.name)
                } else {
                    s.name.clone()
                }
            }
            SlotKind::Ai(_) => format!("{} — {}", s.name, slot_kind_label(s.kind)),
            k => slot_kind_label(k),
        };
        let bw = sw - 40.0 - 120.0;
        let clickable = (host && s.kind != SlotKind::Human) || (!host && s.kind == SlotKind::Open);
        if ui.button_ex(rect(sx + 40.0, y, bw, 36.0), &label, clickable || mine, mine) {
            if host && s.kind != SlotKind::Human {
                act = MenuAction::SetSlot(i, next_kind(s.kind));
            } else if s.kind == SlotKind::Open {
                act = MenuAction::TakeSlot(i);
            }
        }
        if s.kind != SlotKind::Closed && s.kind != SlotKind::Open {
            if ui.button_ex(rect(sx + 40.0 + bw + 8.0, y, 112.0, 36.0), &format!("Team {}", s.team + 1), editable, false) && editable {
                act = MenuAction::SetTeam(i, (s.team + 1) % n_slots.max(1));
            }
        } else if s.kind == SlotKind::Open && host && !l.slots.iter().any(|s| s.conn == me)
            && ui.button(rect(sx + 40.0 + bw + 8.0, y, 112.0, 36.0), "Sit here") {
            act = MenuAction::TakeSlot(i);
        }
        y += 42.0;
    }
    if host {
        ui.text("Click a slot to add or change a computer player.", sx, y + 4.0, 14.0, theme::TEXT_DIM, Align::Left);
    }
    y += 30.0;
    if !l.spectators.is_empty() {
        ui.text(&format!("Watching: {}", l.spectators.join(", ")), sx, y, 15.0, theme::TEXT_DIM, Align::Left);
    }

    // Chat.
    let cy = top + h - 230.0;
    let cw = w - 48.0 - 200.0;
    let mut ly = cy + 140.0;
    for c in chat.iter().rev().take(6) {
        ly -= 22.0;
        let mut lx = mx;
        if !c.from.is_empty() {
            lx += ui.text(&format!("{}: ", c.from), lx, ly, 16.0, c.color, Align::Left);
        }
        ui.text(&c.text, lx, ly, 16.0, theme::TEXT, Align::Left);
    }
    if ui.text_field(40, rect(mx, cy + 150.0, cw, 34.0), chat_input, 200, "Chat with the lobby...") {
        let t = std::mem::take(chat_input);
        if !t.trim().is_empty() {
            act = MenuAction::Chat(t);
        }
    }

    let by = top + h - 64.0;
    if ui.button(rect(x0 + w - 200.0, by - 50.0, 176.0, 44.0), "Leave") {
        act = MenuAction::Back;
    }
    if host {
        let seated = l.slots.iter().filter(|s| matches!(s.kind, SlotKind::Human | SlotKind::Ai(_))).count();
        if ui.button_ex(rect(x0 + w - 200.0, by, 176.0, 44.0), "Start!", seated > 0 && !l.in_game, false) {
            act = MenuAction::Start;
        }
    } else {
        let msg = if l.in_game { "Game in progress..." } else { "Waiting for the host to start" };
        ui.text(msg, x0 + w - 24.0, by + 12.0, 16.0, theme::TEXT_DIM, Align::Right);
    }
    act
}
