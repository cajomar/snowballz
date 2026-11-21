//! The in-game HUD in its hand-made skin: a wooden plank of stores along
//! the top, a paper sign for the season and a note of the tribes, and the
//! console along the bottom (minimap, what's selected, and a card of
//! commands for it).

use super::*;
use crate::advice::{self, Standing};
use crate::icons::{self, Icon};
use crate::skin::{self, CREAM, GOLD, INK, INK_BAD, INK_COLD, INK_DIM, WOOD_EDGE};

/// Where the bottom console's parts go.
pub struct Console {
    pub bar: Rect,
    /// The minimap's wooden frame, and the map inside it.
    pub frame: Rect,
    pub map: Rect,
    /// The paper about whatever is selected.
    pub sel: Rect,
    /// The 5 by 3 card of command stamps.
    pub card: Rect,
    cell: [f32; 2],
}

pub const CARD_COLS: usize = 5;
pub const CARD_SLOTS: usize = 15;
const GAP: f32 = 6.0;
const PAD: f32 = 8.0;

impl Console {
    pub fn new(w: f32, h: f32, map_w: i32, map_h: i32, touch: bool) -> Console {
        // Fingers need bigger stamps than a mouse does.
        let ch = if touch { (h * 0.34).clamp(140.0, 196.0) } else { (h * 0.22).clamp(130.0, 196.0) };
        let bar = rect(0.0, h - ch, w, ch);
        // The minimap's frame stands a little proud of the bar.
        let fs = ch + 20.0;
        let frame = rect(8.0, h - fs - 6.0, fs, fs);
        let inner = frame.shrink(10.0);
        let aspect = map_w as f32 / map_h.max(1) as f32;
        let (mw, mh) = if aspect > 1.0 { (inner.w, inner.h / aspect) } else { (inner.w * aspect, inner.h) };
        let map = rect(inner.x + (inner.w - mw) / 2.0, inner.y + (inner.h - mh) / 2.0, mw, mh);
        let card_h = ch - 18.0;
        let cell_h = (card_h - 2.0 * PAD - 2.0 * GAP) / 3.0;
        let cell_w = (cell_h * 1.4).min((w * 0.36 - 2.0 * PAD - 4.0 * GAP) / 5.0);
        let card_w = 5.0 * cell_w + 4.0 * GAP + 2.0 * PAD;
        let card = rect(w - card_w - 10.0, bar.y + 9.0, card_w, card_h);
        let sx = frame.x + frame.w + 10.0;
        let sel = rect(sx, bar.y + 10.0, (card.x - 10.0 - sx).max(60.0), ch - 20.0);
        Console { bar, frame, map, sel, card, cell: [cell_w, cell_h] }
    }

    /// The `i`th stamp of the card, left to right then down.
    pub fn cell(&self, i: usize) -> Rect {
        let (c, r) = ((i % CARD_COLS) as f32, (i / CARD_COLS) as f32);
        rect(self.card.x + PAD + c * (self.cell[0] + GAP), self.card.y + PAD + r * (self.cell[1] + GAP), self.cell[0], self.cell[1])
    }
}

/// A hovered thing's explanation: title, note (cost) and text.
type Tip = Option<(String, String, String)>;

/// Whose slots a slot row sets the wanted count for.
#[derive(Clone, Copy)]
enum WantAt {
    Building(u16),
    Igloo(u8),
}

/// A row of slot stamps on the card (see `GameScreen::slot_row`).
struct SlotRow {
    icon: Icon,
    /// What a filled slot says (its kind).
    filled_label: &'static str,
    slots: u8,
    filled: u8,
    want: u8,
    at: WantAt,
    /// The slip's text: what a slot does here.
    what: &'static str,
}

/// Pages of the command card.
pub const PAGE_MAIN: u8 = 0;
const PAGE_BUILD: u8 = 1;
const PAGE_CAMPS: u8 = 2;
const PAGE_RESEARCH: u8 = 3;

fn cost(fish: u16, wood: u16, crystal: u16) -> String {
    let mut parts = Vec::new();
    if fish > 0 {
        parts.push(format!("{fish}f"));
    }
    if wood > 0 {
        parts.push(format!("{wood}w"));
    }
    if crystal > 0 {
        parts.push(format!("{crystal}c"));
    }
    parts.join(" ")
}

/// Short names for the build stamps.
fn build_short(kind: BuildKind) -> &'static str {
    match kind {
        BuildKind::Sauna => "Sauna",
        BuildKind::Campfire => "Campfire",
        BuildKind::Tower => "Tower",
        BuildKind::Catapult => "Catapult",
        BuildKind::Store => "Sushi House",
        BuildKind::Workshop => "Shields",
        BuildKind::Track => "Sleds",
        BuildKind::Spa => "Spa",
        BuildKind::Palisade => "Palisade",
        BuildKind::SkiLodge => "Skiers",
        BuildKind::SlingRange => "Slingers",
        BuildKind::PackerYard => "Packers",
        BuildKind::Bunkhouse => "Bunks",
        BuildKind::Igloo => "Igloo",
    }
}

/// A colour made dark enough to read on paper; whites become plain ink.
fn ink_of(c: Color) -> Color {
    let (hi, lo) = (c[0].max(c[1]).max(c[2]).max(1) as f32, c[0].min(c[1]).min(c[2]) as f32);
    let (k, white) = (150.0 / hi, lo / 255.0);
    let mut out = INK;
    for i in 0..3 {
        out[i] = (c[i] as f32 * k * (1.0 - white) + INK[i] as f32 * white) as u8;
    }
    out
}

impl GameScreen {
    pub fn console(&self, ui: &Ui) -> Console {
        let map = &self.view.info().map;
        Console::new(ui.w, ui.h, map.w, map.h, ui.input.touch)
    }

    /// The plank of stores, the season sign and the tribes.
    pub(super) fn draw_top_bar(&mut self, ui: &mut Ui) {
        let info = self.view.info();
        let you = info.you;
        let touch = ui.input.touch;
        let wt = self.view.world.weather;
        let secs = self.view.world.tick / sim::TICK_RATE;

        // The season sign, top right (left of the phone's menu button).
        let menu_w = if touch { 52.0 } else { 0.0 };
        let left = (sim::game::SEASON_TICKS - wt.season_t) / sim::TICK_RATE;
        let day = sim::game::day_of(self.view.world.tick) + 1;
        let title = format!("Day {day} · {}", wt.season.name());
        let sub = if wt.blizzard > 0 { "Blizzard!".to_string() } else { format!("{}:{:02} left", left / 60, left % 60) };
        let sw = (ui.font.measure(&title, 22.0).max(ui.font.measure(&sub, 14.0)) + 30.0).max(200.0);
        let sign = rect(ui.w - sw - 10.0 - menu_w, 6.0, sw, 64.0);
        ui.paper(sign, 8.0);
        ui.text(&title, sign.x + sw / 2.0, sign.y + 6.0, 22.0, INK, Align::Center);
        ui.text(&sub, sign.x + sw / 2.0, sign.y + 31.0, 14.0, if wt.blizzard > 0 { INK_BAD } else { INK_DIM }, Align::Center);
        let col = match wt.season {
            sim::game::Season::DeepWinter => [90, 140, 200, 255],
            sim::game::Season::Thaw => [80, 160, 120, 255],
            sim::game::Season::Midsummer => [220, 160, 50, 255],
            sim::game::Season::PolarNight => [120, 100, 200, 255],
        };
        ui.meter(rect(sign.x + 12.0, sign.y + 50.0, sw - 24.0, 8.0), wt.season_t as f32 / sim::game::SEASON_TICKS as f32, col);
        if ui.hovered(sign) {
            let tip = format!("{} Next: {}.", wt.season.blurb(), wt.season.next().name());
            ui.slip(sign, 340.0, wt.season.name(), "", &tip);
        }
        if touch {
            // Phones have no Esc key: a menu button stands in for it.
            let b = rect(ui.w - 50.0, 8.0, 44.0, 40.0);
            if ui.stamp(b, true, self.menu_open) {
                self.menu_open = !self.menu_open;
            }
            for i in 0..3 {
                ui.fill(rect(b.x + 11.0, b.y + 12.0 + i as f32 * 7.0, 22.0, 3.0), 1.5, INK);
            }
        }

        // The tribes, on a note pinned under the sign.
        let counts: Vec<usize> = (0..info.players.len()).map(|p| self.view.world.igloos.iter().filter(|i| i.owner == p as u8).count()).collect();
        let rows: Vec<(String, String, Color, bool)> = info
            .players
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let out = self.view.world.out.get(i).copied().unwrap_or(false);
                // A tribe none of whose igloos you have found is a question mark.
                let known = self.view.fog.is_none()
                    || self.view.world.team(i as u8) == self.view.world.team(info.you)
                    || self.view.world.igloos.iter().enumerate().any(|(k, ig)| ig.owner == i as u8 && self.view.discovered(info.igloos[k].center));
                let right = if out {
                    "out".to_string()
                } else if known {
                    format!("{}", counts[i])
                } else {
                    "?".to_string()
                };
                (p.name.clone(), right, rgba3(player_color(info, i as u8)), out)
            })
            .collect();
        // Underneath: the map, the game clock and the ping.
        let foot = if touch { String::new() } else { format!("{} · {}:{:02} · {:.0} ms", info.map.name, secs / 60, secs % 60, self.ping) };
        let nw = (rows.iter().map(|r| ui.font.measure(&r.0, 16.0)).fold(0.0, f32::max) + 76.0).max(ui.font.measure(&foot, 13.0) + 24.0);
        let foot_h = if foot.is_empty() { 0.0 } else { 20.0 };
        let note = rect(ui.w - nw - 16.0, sign.y + sign.h + 14.0, nw, 12.0 + rows.len() as f32 * 21.0 + foot_h);
        ui.paper(note, 4.0);
        ui.batch.ellipse(ui.ctx, note.x + nw / 2.0, note.y + 2.0, 5.5, 5.5, [165, 42, 30, 255]);
        ui.batch.ellipse(ui.ctx, note.x + nw / 2.0 - 1.5, note.y + 0.5, 2.0, 2.0, [255, 150, 130, 255]);
        let mut y = note.y + 7.0;
        for (k, (name, right, c, out)) in rows.iter().enumerate() {
            let a = if *out { 0.45 } else { 1.0 };
            ui.batch.ellipse(ui.ctx, note.x + 16.0, y + 10.0, 6.0, 6.0, with_alpha(WOOD_EDGE, a * 0.6));
            ui.batch.ellipse(ui.ctx, note.x + 16.0, y + 10.0, 4.8, 4.8, with_alpha(*c, a));
            ui.text(name, note.x + 28.0, y, 16.0, with_alpha(INK, a), Align::Left);
            ui.text(right, note.x + nw - 12.0, y, 16.0, with_alpha(INK_DIM, a), Align::Right);
            if k + 1 < rows.len() {
                ui.fill(rect(note.x + 10.0, y + 20.0, nw - 20.0, 1.0), 0.0, [205, 178, 130, 255]);
            }
            y += 21.0;
        }
        if !foot.is_empty() {
            ui.text(&foot, note.x + nw / 2.0, y + 2.0, 13.0, INK_DIM, Align::Center);
        }

        // The plank of stores along the top left.
        let room = sign.x - 20.0;
        if you == NEUTRAL {
            let w = ui.font.measure("Spectating", 22.0) + 40.0;
            let p = rect(10.0, 8.0, w, 46.0);
            ui.wood(p, 10.0);
            ui.text_shadow("Spectating", p.x + 20.0, p.y + 11.0, 22.0, GOLD, Align::Left);
            return;
        }
        let w = &self.view.world;
        let (mut fish, mut crystal, mut igloos) = (0u32, 0u32, 0);
        for ig in &w.igloos {
            if ig.owner == you {
                fish += ig.fish as u32;
                crystal += ig.crystal as u32;
                igloos += 1;
            }
        }
        let (mut sb, mut wk, mut cold, mut exposed) = (0, 0, 0, 0);
        for p in &w.penguins {
            if p.owner == you {
                if p.kind.soldier() {
                    sb += 1;
                } else {
                    wk += 1;
                }
                cold += p.cold as usize;
                exposed += p.exposed as usize;
            }
        }
        // (icon, value, label, colour, what it means)
        let mut items: Vec<(Icon, String, &str, Color, String)> = vec![
            (Icon::Igloo, format!("{igloos}"), "igloos", CREAM, "Igloos".into()),
            (Icon::Fish, format!("{fish}"), "fish", [180, 225, 255, 255], "Fish: trains soldiers and pays for newcomers".into()),
        ];
        if crystal > 0 {
            items.push((Icon::Crystal, format!("{crystal}"), "crystal", [230, 180, 255, 255], "Crystal".into()));
        }
        items.push((Icon::Penguin, format!("{sb}"), "soldiers", CREAM, "Soldiers".into()));
        items.push((Icon::Worker, format!("{wk}"), "workers", CREAM, "Workers".into()));
        // The Long Night: a countdown, then how far the ring has to go.
        let start = w.info.long_night_at;
        if w.tick < start {
            let s = (start - w.tick) / sim::TICK_RATE;
            if s <= 180 {
                items.push((Icon::Frost, format!("{}:{:02}", s / 60, s % 60), "long night", [200, 185, 255, 255], "The Long Night begins: cold closes in from the map's edges".into()));
            }
        } else if let Some(r) = self.view.night_ring().map(|(_, r)| r) {
            if !self.night_noticed {
                self.night_noticed = true;
                self.notices.push(("The Long Night has begun! The cold closes in from the edges: get to the middle.".into(), theme::BAD, 0.0));
            }
            items.push((Icon::Frost, format!("{}", r as i32), "safe", [255, 140, 120, 255], format!("LONG NIGHT: only ground within {} tiles of the middle is safe", r as i32)));
        }
        let w = &self.view.world;
        let frozen = w.penguins.iter().filter(|p| p.act == Act::Frozen && (p.owner == you || p.owner == NEUTRAL)).count();
        if frozen > 0 {
            items.push((Icon::Frozen, format!("{frozen}"), "frozen", [200, 225, 245, 255], "Frozen penguins: Warmers thaw them into your tribe; left alone they wake up wild".into()));
        }
        if cold > 0 {
            items.push((Icon::Frost, format!("{cold}"), "cold", [160, 210, 255, 255], "Cold penguins: warm them at a sauna".into()));
        }
        if exposed > 0 {
            items.push((Icon::Frost, format!("{exposed}"), "exposed", [140, 190, 235, 255], "Exposed to the cold".into()));
        }
        // Fit the plank beside the sign: drop the words first, then shrink.
        let help_w = if touch { 0.0 } else { 40.0 };
        let need = |ui: &Ui, px: f32, labels: bool| -> f32 {
            items
                .iter()
                .map(|(_, v, l, _, _)| px + 22.0 + ui.font.measure(v, px) + if labels { ui.font.measure(l, px * 0.6) + 4.0 } else { 0.0 } + 4.0)
                .sum::<f32>()
                + 28.0
                + help_w
        };
        let mut px = 21.0;
        let labels = need(ui, px, true) <= room;
        while need(ui, px, labels) > room && px > 13.0 {
            px -= 1.0;
        }
        let plank = rect(10.0, 8.0, need(ui, px, labels).min(room.max(120.0)), 46.0);
        ui.wood(plank, 10.0);
        ui.nails(plank);
        let mut x = plank.x + 14.0;
        let mut hover: Option<(Rect, String)> = None;
        for (icon, value, label, c, tip) in &items {
            let r = ui.chip(x, plank.y + 23.0, *icon, value, if labels { label } else { "" }, *c, px);
            if ui.hovered(r) {
                hover = Some((r, tip.clone()));
            }
            x += r.w + 4.0;
        }
        if !touch {
            let b = rect(plank.x + plank.w - 44.0, plank.y + 9.0, 30.0, 28.0);
            if ui.stamp(b, true, self.show_help) {
                self.show_help = !self.show_help;
            }
            ui.text_in("?", b, 18.0, INK, Align::Center);
            if ui.hovered(b) {
                hover = Some((b, "Help and keys (F1)".into()));
            }
        }
        if let Some((r, tip)) = hover {
            ui.slip(r, 360.0_f32.min(ui.font.measure(&tip, 15.0) + 30.0), "", "", &tip);
        }
    }

    /// The bottom console. Returns where the minimap's map is drawn.
    pub(super) fn draw_console(&mut self, ui: &mut Ui, r: &mut dyn WorldRenderer, scale: f32) -> Rect {
        let c = self.console(ui);
        ui.wood(rect(-4.0, c.bar.y, ui.w + 8.0, c.bar.h + 6.0), 0.0);
        ui.wood(c.frame, 12.0);
        ui.nails(c.frame);
        ui.fill(c.map.shrink(-3.0), 4.0, WOOD_EDGE);
        self.draw_minimap(ui, r, scale, c.map);
        // The card sits in a dark well in the wood.
        ui.fill(c.card.shrink(-2.0), 11.0, WOOD_EDGE);
        ui.fill(c.card, 10.0, [44, 24, 12, 255]);
        ui.paper(c.sel, 6.0);

        // Start a fresh card when the selection changes.
        let key = (self.sel_igloo, self.sel_building, self.selected.first().copied(), self.selected.len());
        if key != self.card_for {
            self.card_for = key;
            self.card_page = PAGE_MAIN;
        }
        // B opens the build card; leaving it (Esc) goes back.
        if self.build_menu && self.card_page == PAGE_MAIN {
            self.card_page = PAGE_BUILD;
        } else if !self.build_menu && self.workers_selected() && matches!(self.card_page, PAGE_BUILD | PAGE_CAMPS) {
            self.card_page = PAGE_MAIN;
        }

        let mut tip: Tip = None;
        if let Some(igi) = self.sel_igloo {
            self.igloo_card(ui, &c, igi, &mut tip);
        } else if !self.selected.is_empty() {
            self.units_card(ui, &c, &mut tip);
        } else if self.sel_building.is_some() {
            self.building_card(ui, &c, &mut tip);
        } else {
            self.tribe_card(ui, &c, &mut tip);
        }
        if let Some((title, note, body)) = tip {
            ui.slip(c.card, c.card.w.max(320.0), &title, &note, &body);
        }

        // Idle workers waiting for a job: a tag on top of the console.
        let idle = self.idle_workers().len();
        if idle > 0 && !self.spectating() {
            let label = format!("Idle workers: {idle} (I)");
            let tw = ui.font.measure(&label, 16.0) + 24.0;
            if ui.stamp_text(rect(c.sel.x, c.bar.y - 36.0, tw, 30.0), &label, true) {
                self.next_idle_worker(r);
            }
        }
        c.map
    }

    /// A title on the selection paper, with a ribbon for the owner.
    fn sel_title(&self, ui: &mut Ui, c: &Console, title: &str, owner: Option<u8>) -> f32 {
        let s = c.sel;
        let px = (s.h * 0.15).clamp(18.0, 26.0);
        let tw = ui.text(title, s.x + 14.0, s.y + 8.0, px, INK, Align::Left);
        let info = self.view.info();
        if let Some(o) = owner {
            let (name, col) = if o == NEUTRAL {
                ("Unclaimed".to_string(), [130, 120, 110, 255])
            } else {
                (info.players.get(o as usize).map_or("?".to_string(), |p| p.name.clone()), ink_of(rgba3(player_color(info, o))))
            };
            let nw = ui.font.measure(&name, 15.0) + 26.0;
            let rx = s.x + 24.0 + tw;
            let ry = s.y + 10.0 + (px - 20.0) / 2.0;
            ui.fill(rect(rx, ry, nw, 21.0), 2.0, col);
            // The swallowtail end of the ribbon, cut back to the paper.
            let (a, b, m) = ([rx + nw + 0.5, ry - 0.5], [rx + nw + 0.5, ry + 21.5], [rx + nw - 7.0, ry + 10.5]);
            let (white, uv) = (ui.batch.white, ui.batch.white_uv);
            ui.batch.quad_pts(ui.ctx, white, [a, a, m, b], uv, [240, 226, 194, 255], [0; 4]);
            ui.text(&name, rx + 9.0, ry + 2.0, 15.0, CREAM, Align::Left);
        }
        s.y + 14.0 + px
    }

    fn igloo_card(&mut self, ui: &mut Ui, c: &Console, igi: u8, tip: &mut Tip) {
        let you = self.you();
        let v = self.view.world.igloos[igi as usize].clone();
        let s = c.sel;
        // A neutral igloo pelted down to nothing is just a ruin.
        let tier_name = if v.owner == NEUTRAL && v.hp == 0 { "Ruin" } else { sim::game::tier(v.tier).name };
        let mut y = self.sel_title(ui, c, tier_name, Some(v.owner)) + 4.0;
        let pct = v.hp as f32 / sim::game::igloo_max_hp(v.tier) as f32;
        ui.text("Walls", s.x + 14.0, y - 3.0, 15.0, INK_DIM, Align::Left);
        ui.meter(rect(s.x + 62.0, y, (s.w - 130.0).min(260.0), 11.0), pct, if pct < 0.35 { [200, 80, 60, 255] } else { [122, 166, 207, 255] });
        ui.text(&format!("{:.0}%", pct * 100.0), s.x + 70.0 + (s.w - 130.0).min(260.0), y - 3.0, 15.0, if pct < 0.35 { INK_BAD } else { INK_DIM }, Align::Left);
        y += 20.0;
        // Only the igloo's own: a wild or frozen penguin's home means nothing.
        let home = |k: fn(Kind) -> bool| self.view.world.penguins.iter().filter(|p| p.owner == v.owner && p.home == igi && p.act != Act::Frozen && k(p.kind)).count();
        let (soldiers, workers) = (home(|k| k.soldier()), home(|k| !k.soldier()));
        let mut stats: Vec<(Icon, String)> = vec![(Icon::Fish, format!("{}", v.fish))];
        if v.crystal > 0 {
            stats.push((Icon::Crystal, format!("{}", v.crystal)));
        }
        stats.push((Icon::Igloo, format!("{} in", v.occupants)));
        stats.push((Icon::Penguin, format!("{soldiers}")));
        stats.push((Icon::Worker, format!("{workers}")));
        if v.prisoners > 0 {
            stats.push((Icon::Frozen, format!("{} held", v.prisoners)));
        }
        let mut x = s.x + 14.0;
        for (icon, t) in &stats {
            icons::draw(ui, *icon, x + 9.0, y + 10.0, 18.0);
            x += 22.0 + ui.text(t, x + 22.0, y, 17.0, INK, Align::Left) + 14.0;
        }
        y += 26.0;
        if v.owner != you {
            // Igloos can't be captured; an empty one is founded by a worker.
            let founding = v.owner == NEUTRAL;
            if let Some(tk) = v.takeover {
                let f = 1.0 - tk.ticks_left as f32 / tk.total.max(1) as f32;
                let (what, col) = if !founding {
                    ("Being taken over!", INK_BAD)
                } else if tk.by == you {
                    ("Your worker is founding it", INK)
                } else {
                    ("Being founded by another tribe", INK_BAD)
                };
                ui.text(what, s.x + 14.0, y, 18.0, col, Align::Left);
                ui.meter(rect(s.x + 14.0, y + 24.0, s.w - 28.0, 11.0), f, rgba3(player_color(self.view.info(), tk.by)));
            } else if founding {
                let (fish, _) = sim::game::price(sim::game::GREAT_IGLOO_FISH, sim::game::GREAT_IGLOO_WOOD);
                ui.text(&format!("Send a worker inside to found it: it costs {fish} fish."), s.x + 14.0, y, 15.0, INK_DIM, Align::Left);
            } else {
                ui.text("Igloos can't be taken: pelt its walls down.", s.x + 14.0, y, 15.0, INK_DIM, Align::Left);
            }
            return;
        }

        let tech = self.view.info().tech;
        // The room says what the igloo is up to.
        self.igloo_doings(ui, s, y, igi, &v);
        let qx = s.x + 80.0;
        // Building up to the next tier, or learning something.
        let mut status: Option<(String, f32)> = v.upgrade.map(|(left, total)| (format!("Building the {}…", sim::game::tier(v.tier + 1).name), 1.0 - left as f32 / total.max(1) as f32));
        if status.is_none() {
            status = v.research.map(|(branch, left, total)| {
                let b = &sim::game::BRANCH_TABLE[branch as usize];
                let lvl = self.view.world.research.get(v.owner as usize).map_or(0, |l| l[branch as usize]);
                let learning = b.levels.get(lvl as usize).map_or("", |l| l.name);
                (format!("Learning {learning}…"), 1.0 - left as f32 / total.max(1) as f32)
            });
        }
        if let Some((label, f)) = status {
            let sx = qx + 10.0;
            let sw = s.x + s.w - 14.0 - sx;
            if sw > 80.0 {
                ui.text(&label, sx, y, 15.0, [150, 100, 20, 255], Align::Left);
                ui.meter(rect(sx, y + 22.0, sw, 10.0), f, GOLD);
            }
        }

        // The card: what this igloo trains, and what else it can do.
        let mut slot = 0;
        if self.card_page == PAGE_RESEARCH && tech {
            self.research_card(ui, c, igi, &v, tip);
            return;
        }
        // The igloo's two fishing slots, as many filled as you
        // want. (No guards, no building up: put a tower by it, build Sushi Houses.)
        let n = self.slot_row(
            ui,
            c,
            slot,
            SlotRow {
                icon: Icon::Fish,
                filled_label: "Fisher",
                slots: sim::game::IGLOO_FISH_SLOTS,
                filled: v.fishers,
                want: v.fish_want,
                at: WantAt::Igloo(igi),
                what: "An igloo has two fishing slots. Each one you want filled calls in an idle worker to fish the nearest banks and bring the catch home; with nobody idle, a newcomer swims in from the sea (4 fish). For more, build Sushi Houses by the water.",
            },
            tip,
        );
        slot += n;
    
        if tech {
            if let (None, Some(next)) = (v.upgrade, sim::game::next_tier(v.tier)) {
                let (fish, wood) = sim::game::price(next.fish, next.wood);
                let afford = v.fish >= fish && self.view.world.wood_of(v.owner) >= wood as u32 && self.view.world.crystal_of(v.owner) >= next.crystal as u32;
                let price = cost(fish, wood, next.crystal);
                let r = c.cell(slot);
                if ui.card_stamp(r, Icon::Up, next.name, "U", &price, afford, false) {
                    self.send(Command::Upgrade { igloo: igi });
                }
                if ui.hovered(r) {
                    let why = format!(
                        "Holds more penguins, warms them faster and takes longer to take over. Takes {}s.",
                        next.ticks as f32 / sim::TICK_RATE as f32
                    );
                    *tip = Some((format!("Upgrade to {}", next.name), price, why));
                }
                slot += 1;
            }
            let r = c.cell(slot);
            if ui.card_stamp(r, Icon::Scroll, "Research", "", "", true, false) {
                self.card_page = PAGE_RESEARCH;
            }
            if ui.hovered(r) {
                *tip = Some(("Research".into(), String::new(), "What your tribe has learnt, and what it can learn next.".into()));
            }
            slot += 1;
        }
        if v.prisoners > 0 && slot < CARD_SLOTS {
            let r = c.cell(slot);
            if ui.card_stamp(r, Icon::Door, "Release", "", "", true, false) {
                self.send(Command::Release { igloo: igi });
            }
            if ui.hovered(r) {
                *tip = Some(("Release".into(), String::new(), format!("Let the {} prisoners go home.", v.prisoners)));
            }
        }
    }

    fn research_card(&mut self, ui: &mut Ui, c: &Console, igi: u8, v: &sim::proto::IglooView, tip: &mut Tip) {
        let levels = self.view.world.research.get(v.owner as usize).copied().unwrap_or([0; sim::game::BRANCHES]);
        let running: Vec<(u8, u16, u16)> = self.view.world.igloos.iter().filter(|ig| ig.owner == v.owner).filter_map(|ig| ig.research).collect();
        for b in 0..sim::game::BRANCHES {
            let branch = &sim::game::BRANCH_TABLE[b];
            let icon = [Icon::Snowball, Icon::Frost, Icon::Fish].get(b).copied().unwrap_or(Icon::Scroll);
            let r = c.cell(b);
            let Some(level) = branch.levels.get(levels[b] as usize) else {
                ui.card_stamp(r, icon, branch.name, "", "done", false, false);
                continue;
            };
            let (fish, wood) = sim::game::price(level.fish, level.wood);
            let price = cost(fish, wood, level.crystal);
            // The igloo does one thing at a time, and a branch is learnt once.
            let busy = v.research.is_some() || running.iter().any(|&(br, _, _)| br as usize == b);
            let afford = v.tier >= level.tier
                && v.fish >= fish
                && self.view.world.wood_of(v.owner) >= wood as u32
                && self.view.world.crystal_of(v.owner) >= level.crystal as u32;
            if ui.card_stamp(r, icon, branch.name, "", &price, !busy && afford, false) {
                self.send(Command::StartResearch { igloo: igi, branch: b as u8 });
            }
            if ui.hovered(r) {
                let mut why = format!("{}. {}", level.name, level.blurb);
                if level.tier > v.tier {
                    why += &format!(" Needs a {}.", sim::game::tier(level.tier).name);
                }
                why += &format!(" Takes {}s. Learnt so far: level {}.", level.ticks as f32 / sim::TICK_RATE as f32, levels[b]);
                *tip = Some((branch.name.to_string(), price, why));
            }
        }
        if ui.card_stamp(c.cell(CARD_SLOTS - 1), Icon::Back, "Back", "", "", true, false) {
            self.card_page = PAGE_MAIN;
        }
    }

    fn units_card(&mut self, ui: &mut Ui, c: &Console, tip: &mut Tip) {
        let s = c.sel;
        let w = &self.view.world;
        let (mut sb, mut wk, mut balls, mut cold, mut exposed) = (0, 0, 0u32, 0, 0);
        let (mut warmth, mut courage) = (0.0, 0.0);
        let mut kinds = [0usize; Kind::ALL.len()];
        for &id in &self.selected {
            let p = &w.penguins[id as usize];
            kinds[p.kind as usize] += 1;
            if p.kind.soldier() {
                sb += 1;
                balls += p.snowballs as u32;
            } else {
                wk += 1;
            }
            warmth += p.warmth as f32 / p.max_warmth.max(1) as f32;
            courage += p.courage as f32 / sim::game::MAX_COURAGE as f32;
            cold += p.cold as usize;
            exposed += p.exposed as usize;
        }
        let n = self.selected.len() as f32;
        let parts: Vec<String> = Kind::ALL
            .iter()
            .filter(|k| kinds[**k as usize] > 0)
            .map(|k| {
                let n = kinds[*k as usize];
                let st = k.stats();
                format!("{n} {}", if n == 1 { st.name.to_lowercase() } else { st.plural.to_string() })
            })
            .collect();
        let mut title = parts.join(", ");
        let tpx = (s.h * 0.13).clamp(16.0, 22.0);
        while ui.font.measure(&title, tpx) > s.w - 28.0 && title.chars().count() > 4 {
            title.pop();
            title = format!("{}…", title.trim_end_matches('…'));
            title.pop();
            title.push('…');
        }
        ui.text(&title, s.x + 14.0, s.y + 8.0, tpx, INK, Align::Left);

        // A face per penguin with its warmth underneath; click one to pick it alone.
        let foot = 26.0;
        let top = s.y + 14.0 + tpx;
        let fs = ((s.h - (top - s.y) - foot - 8.0) / 2.0 - 4.0).clamp(20.0, 40.0);
        let cols = ((s.w - 28.0 + 4.0) / (fs + 4.0)).floor().max(1.0) as usize;
        let rows = ((s.h - (top - s.y) - foot - 6.0 + 4.0) / (fs + 4.0)).floor().max(1.0) as usize;
        let room = cols * rows;
        let shown = if self.selected.len() > room { room - 1 } else { self.selected.len() };
        let mut pick = None;
        for (k, &id) in self.selected.iter().take(shown).enumerate() {
            let p = &self.view.world.penguins[id as usize];
            let r = rect(s.x + 14.0 + (k % cols) as f32 * (fs + 4.0), top + (k / cols) as f32 * (fs + 4.0), fs, fs);
            let hover = ui.hovered(r);
            ui.fill(r, 6.0, if p.cold { [122, 166, 207, 255] } else { [185, 154, 102, 255] });
            ui.fill(r.shrink(1.5), 5.0, if p.cold { [220, 236, 248, 255] } else if hover { [255, 252, 240, 255] } else { [255, 246, 223, 255] });
            icons::draw(ui, icons::for_kind(p.kind), r.x + fs / 2.0, r.y + fs / 2.0 - 2.0, fs * 0.66);
            let f = p.warmth as f32 / p.max_warmth.max(1) as f32;
            ui.fill(rect(r.x + 3.0, r.y + fs - 5.0, fs - 6.0, 3.0), 1.5, [200, 100, 90, 255]);
            ui.fill(rect(r.x + 3.0, r.y + fs - 5.0, (fs - 6.0) * f.clamp(0.0, 1.0), 3.0), 1.5, if p.cold { [90, 150, 220, 255] } else { [95, 143, 62, 255] });
            if hover && ui.input.pressed[MOUSE_L] {
                ui.input.mouse_used = true;
                pick = Some(id);
            }
        }
        if shown < self.selected.len() {
            let k = shown;
            let r = rect(s.x + 14.0 + (k % cols) as f32 * (fs + 4.0), top + (k / cols) as f32 * (fs + 4.0), fs, fs);
            ui.text_in(&format!("+{}", self.selected.len() - shown), r, (fs * 0.4).max(12.0), INK_DIM, Align::Center);
        }
        if let Some(id) = pick {
            self.selected = vec![id];
        }
        // Warmth, courage, snowballs and the cold.
        let fy = s.y + s.h - foot;
        let mut x = s.x + 14.0;
        let mw = ((s.w - 28.0) / 4.0).clamp(50.0, 90.0);
        x += ui.text("Warmth", x, fy, 15.0, INK_DIM, Align::Left) + 6.0;
        ui.meter(rect(x, fy + 4.0, mw, 10.0), warmth / n, [217, 120, 58, 255]);
        x += mw + 14.0;
        if sb > 0 {
            x += ui.text("Courage", x, fy, 15.0, INK_DIM, Align::Left) + 6.0;
            ui.meter(rect(x, fy + 4.0, mw, 10.0), courage / n, GOLD);
            x += mw + 14.0;
            icons::draw(ui, Icon::Snowball, x + 7.0, fy + 9.0, 14.0);
            x += 18.0 + ui.text(&format!("{balls}"), x + 18.0, fy, 15.0, INK, Align::Left) + 14.0;
        }
        if cold > 0 {
            x += ui.text(&format!("{cold} too cold to obey"), x, fy, 15.0, INK_COLD, Align::Left) + 14.0;
        }
        if exposed > 0 && x < s.x + s.w - 80.0 {
            // The cold bites out in the open: they need a hut or home.
            ui.text(&format!("{exposed} out in the cold"), x, fy, 15.0, INK_COLD, Align::Left);
        }

        if matches!(self.card_page, PAGE_BUILD | PAGE_CAMPS) && wk > 0 {
            self.build_card(ui, c, tip, true);
            return;
        }
        // Orders.
        let mut slot = 0;
        let units = self.selected.clone();
        let r = c.cell(slot);
        if ui.card_stamp(r, Icon::Hand, "Hold", "T", "", true, false) {
            self.send(Command::Hold { units: units.clone() });
        }
        if ui.hovered(r) {
            *tip = Some(("Hold".into(), String::new(), "Stand here and throw at whatever comes near, without chasing.".into()));
        }
        slot += 1;
        let r = c.cell(slot);
        if ui.card_stamp(r, Icon::Stop, "Stop", "O", "", true, false) {
            self.send(Command::Stop { units: units.clone() });
        }
        if ui.hovered(r) {
            *tip = Some(("Stop".into(), String::new(), "Drop what they're doing.".into()));
        }
        slot += 1;
        let r = c.cell(slot);
        if ui.card_stamp(r, Icon::Igloo, "Go home", "", "", true, false) {
            // Each penguin heads to its own home igloo.
            let mut by_home: std::collections::BTreeMap<u8, Vec<u16>> = Default::default();
            for id in units {
                by_home.entry(self.view.world.penguins[id as usize].home).or_default().push(id);
            }
            for (igloo, units) in by_home {
                self.send(Command::Enter { units, igloo });
            }
        }
        if ui.hovered(r) {
            *tip = Some(("Go home".into(), String::new(), "Each heads into its own igloo to warm up.".into()));
        }
        slot += 1;
        // A Chieftain can call its tribe together.
        if let Some(unit) = self.selected_chief() {
            let cd = self.view.world.penguins[unit as usize].cry_cd;
            let r = c.cell(slot);
            if ui.card_stamp(r, Icon::Horn, "War Cry", "Y", &if cd > 0 { format!("{cd}s") } else { String::new() }, cd == 0, false) {
                self.send(Command::WarCry { unit });
            }
            if ui.hovered(r) {
                let why = if cd > 0 { "The Chieftain is gathering its breath." } else { "Friends nearby run and throw faster, and stop fleeing." };
                *tip = Some(("War Cry".into(), String::new(), why.into()));
            }
            slot += 1;
        }
        if wk > 0 {
            let r = c.cell(slot);
            if ui.card_stamp(r, Icon::Hammer, "Build", "B", "", true, false) {
                self.build_menu = true;
                self.card_page = PAGE_BUILD;
            }
            if ui.hovered(r) {
                *tip = Some(("Build".into(), String::new(), "Pick something for these workers to build.".into()));
            }
        }
    }

    /// The build card: buildings on one page, training camps on the next.
    /// `back` adds a way back to the selected workers' orders.
    fn build_card(&mut self, ui: &mut Ui, c: &Console, tip: &mut Tip, back: bool) {
        let camps = self.card_page == PAGE_CAMPS;
        let keys = self.build_menu;
        let kinds: Vec<(BuildKind, &str)> = BUILD_KEYS.iter().filter(|(k, _, _)| k.trains().is_some() == camps && k.offered()).map(|&(k, _, key)| (k, key)).collect();
        for (slot, &(kind, key)) in kinds.iter().take(CARD_SLOTS - 2).enumerate() {
            let r = c.cell(slot);
            let active = self.placing.is_some_and(|pl| pl.kind == kind);
            let price = if kind.wood() > 0 { format!("{}w", kind.wood()) } else { String::new() };
            if ui.card_stamp(r, icons::for_build(kind), build_short(kind), if keys { key } else { "" }, &price, true, active) {
                if !self.can_build() {
                    self.select_builders();
                }
                if self.can_build() {
                    self.build_menu = true;
                    self.start_placing(kind);
                }
            }
            if ui.hovered(r) {
                let how = if keys { String::new() } else { format!(" Keys: B, then {key}.") };
                *tip = Some((kind.name().to_string(), format!("{} wood, {} snow", kind.wood(), kind.snow()), format!("{}{how}", kind.describe())));
            }
        }
        let r = c.cell(CARD_SLOTS - 1);
        let (icon, label) = if camps { (Icon::Back, "Buildings") } else { (Icon::More, "Camps") };
        if ui.card_stamp(r, icon, label, "", "", true, false) {
            self.card_page = if camps { PAGE_BUILD } else { PAGE_CAMPS };
        }
        if ui.hovered(r) {
            let why = if camps {
                "Walls, fires, stores and towers."
            } else {
                "Camps house your elite: each slot calls in a worker, who mines a crystal and trains quickly."
            };
            *tip = Some((label.to_string(), String::new(), why.into()));
        }
        if back && ui.card_stamp(c.cell(CARD_SLOTS - 2), Icon::Back, "Orders", "", "", true, false) {
            self.build_menu = false;
            self.placing = None;
            self.card_page = PAGE_MAIN;
        }
    }

    fn building_card(&mut self, ui: &mut Ui, c: &Console, tip: &mut Tip) {
        let Some(bi) = self.sel_building else { return };
        let Some(b) = self.view.world.buildings.get(bi as usize).copied() else { return };
        if !b.alive {
            self.sel_building = None;
            return;
        }
        let s = c.sel;
        let mut y = self.sel_title(ui, c, b.kind.name(), Some(b.owner)) + 4.0;
        let (f, label, col) = if b.done() {
            let f = b.hp as f32 / b.kind.max_hp() as f32;
            (f, format!("Strength {}/{}", b.hp, b.kind.max_hp()), if f < 0.35 { [200, 80, 60, 255] } else { [95, 143, 62, 255] })
        } else {
            (b.built as f32 / b.kind.snow() as f32, format!("Building {}/{}   Wood {}/{}", b.built, b.kind.snow(), b.wood_in, b.kind.wood()), GOLD)
        };
        ui.meter(rect(s.x + 14.0, y + 2.0, 160.0_f32.min(s.w * 0.4), 11.0), f, col);
        ui.text(&label, s.x + 24.0 + 160.0_f32.min(s.w * 0.4), y - 2.0, 15.0, INK_DIM, Align::Left);
        y += 22.0;
        let mut notes: Vec<(String, Color)> = Vec::new();
        if b.kind == BuildKind::Campfire && b.done() {
            notes.push((if b.lit { "Burning: warms penguins right beside it" } else { "Burnt out" }.into(), INK));
        }
        if b.kind == BuildKind::Tower && b.done() {
            let manned = self.view.world.penguins.iter().filter(|p| p.posted && p.pos.dist(b.center()) < 1.0).count();
            notes.push((format!("On the roof {manned}/{}: right-click with soldiers for more", sim::game::TOWER_SLOTS), INK));
        }
        if b.kind == BuildKind::Catapult && b.done() && b.owner == self.you() {
            let line = match b.rally {
                Some(_) => "Aimed where you told it: right-click elsewhere to let it pick",
                None => "Lobs at the nearest enemy building: right-click one to aim",
            };
            notes.push((line.into(), INK_DIM));
        }
        if b.kind == BuildKind::Sauna && b.done() {
            notes.push((if b.lit { "Fire lit" } else { "Fire out: its stoker is bringing a log" }.into(), if b.lit { INK } else { INK_BAD }));
        }
        let camp = b.done() && b.owner == self.you() && b.kind.trains().is_some();
        let slotted = b.done() && b.owner == self.you() && b.kind.slot_sizes().is_some();
        if slotted {
            let (who, why_empty) = match b.kind {
                BuildKind::Store => ("fishers", "It needs water close by"),
                BuildKind::Tower => ("sentries", "Sentries come from your armies nearby"),
                BuildKind::Catapult => ("crew", "Crew are idle workers"),
                BuildKind::Bunkhouse => ("snowballers", "Waiting for fish to draft one"),
                _ => ("specialists", "Waiting for a crystal to mine and the fish"),
            };
            notes.push((format!("Room for {} {who}, {} wanted, {} here", b.slots, b.want.min(b.slots), b.filled), INK));
            if camp && b.training > 0 {
                notes.push((format!("Training: {}%", (b.training as u32 - 1) * 100 / 254), [150, 100, 20, 255]));
            } else if b.filled < b.want.min(b.slots) {
                notes.push((format!("{why_empty}; with nobody idle, newcomers swim in from the sea"), INK_BAD));
            }
        }
        if b.built == 0 {
            notes.push(("Staked out: until work starts it blocks nobody and can't be hit".into(), INK_DIM));
        }
        if b.upgrading() {
            notes.push((format!("Building up: {} snow and {} logs to go", b.up_steps, b.up_wood), [150, 100, 20, 255]));
        }
        if b.starved {
            notes.push(("No snow left nearby!".into(), INK_BAD));
        } else if b.waiting_for_wood() {
            notes.push(("Waiting for wood".into(), INK_BAD));
        }
        for (t, col) in notes {
            if y + 18.0 > s.y + s.h - 6.0 {
                break;
            }
            ui.text(&t, s.x + 14.0, y, 15.0, col, Align::Left);
            y += 19.0;
        }
        for line in ui.font.wrap(&b.kind.describe(), 14.0, s.w - 28.0) {
            if y + 17.0 > s.y + s.h - 4.0 {
                break;
            }
            ui.text(&line, s.x + 14.0, y, 14.0, INK_DIM, Align::Left);
            y += 17.0;
        }

        if slotted {
            let (icon, label, what): (Icon, &'static str, &'static str) = match b.kind {
                BuildKind::Store => (Icon::Fish, "Fisher", "Each fishing slot you want filled calls in an idle worker to fish the banks by this house; with nobody idle, a newcomer swims in from the sea (4 fish)."),
                BuildKind::Tower => (Icon::Shield, "Sentry", "Each place you want filled calls up a soldier from your armies nearby to throw from the roof. You can send more by hand."),
                BuildKind::Catapult => (Icon::Penguin, "Crew", "Each place you want filled calls in an idle worker (or a newcomer from the sea) to work the catapult from the foot of the tower. Fewer crew load slower; with none it stops."),
                BuildKind::Bunkhouse => (Icon::Penguin, "Snowballer", "Each bunk you want filled drafts an idle worker into a Snowballer (6 fish); with nobody idle, a newcomer swims in from the sea first (4 fish). When one freezes, the bunk drafts another."),
                _ => (icons::for_kind(b.kind.trains().unwrap_or(Kind::Snowballer)), "Trained", "Each slot you want filled calls in an idle worker (or a newcomer from the sea), who mines a crystal and trains here. When one freezes, the slot calls another."),
            };
            let label = match b.kind.trains() {
                Some(k) if b.kind != BuildKind::Bunkhouse => k.stats().name,
                _ => label,
            };
            self.slot_row(ui, c, 0, SlotRow { icon, filled_label: label, slots: b.slots, filled: b.filled, want: b.want, at: WantAt::Building(bi), what }, tip);
            if b.kind == BuildKind::Tower {
                let going = b.upgrading().then_some((b.up_steps, b.up_wood));
                let what = format!(
                    "Builders put a catapult on the roof: three workers crew it, and it lobs big snowballs at enemy buildings up to {} tiles off. The sentries climb down.",
                    sim::game::CATAPULT_RANGE as u32
                );
                self.build_up_stamp(ui, c.cell(CARD_SLOTS - 2), going, sim::game::CATAPULT_UPGRADE, "Catapult".into(), sim::game::CATAPULT_CREW, what, Command::UpgradeBuilding { building: bi }, tip);
            }
            if let Some((next, work)) = b.kind.next_slots(b.slots) {
                let going = b.upgrading().then_some((b.up_steps, b.up_wood));
                let what = format!("Room for {next} here. Builders come to do the work; the new room stays empty until you want it filled.");
                self.build_up_stamp(ui, c.cell(CARD_SLOTS - 2), going, work, format!("{next} slots"), next, what, Command::UpgradeBuilding { building: bi }, tip);
            }
        }
        // A Sauna is a campfire built up, where there's room for it.
        if b.kind == BuildKind::Campfire && b.done() && b.owner == self.you() {
            let (sauna, fire) = (BuildKind::Sauna, BuildKind::Campfire);
            let work = (sauna.snow() - fire.snow(), (sauna.wood() - fire.wood()) as u8);
            let r = c.cell(CARD_SLOTS - 2);
            if self.sauna_room(b.tile) {
                let what = format!("Builders turn the fire into a Sauna on the spot, where {} penguins bathe at once. The fire is out until it's built.", sim::game::SAUNA_CAPACITY);
                self.build_up_stamp(ui, r, None, work, "Sauna".into(), 0, what, Command::UpgradeBuilding { building: bi }, tip);
            } else {
                ui.card_stamp(r, Icon::More, "No room", "", "", false, false);
                if ui.hovered(r) {
                    *tip = Some(("Build up".into(), String::new(), "A Sauna needs a 2 by 2 spot over the fire, clear of other buildings, rocks and doors.".into()));
                }
            }
        }
        if b.owner == self.you() {
            let r = c.cell(CARD_SLOTS - 1);
            if ui.card_stamp(r, Icon::Cross, "Demolish", "", "", true, false) {
                self.send(Command::Demolish { building: bi });
                self.sel_building = None;
            }
            if ui.hovered(r) {
                *tip = Some(("Demolish".into(), String::new(), "Knock it down.".into()));
            }
        }
    }

    /// Nothing selected: the tribe at a glance, and the build card.
    fn tribe_card(&mut self, ui: &mut Ui, c: &Console, tip: &mut Tip) {
        let s = c.sel;
        let you = self.you();
        if self.spectating() {
            ui.text_in("Spectating", rect(s.x, s.y, s.w, s.h * 0.6), 24.0, INK, Align::Center);
            ui.text_in("Drag the minimap to look around", rect(s.x, s.y + s.h * 0.45, s.w, s.h * 0.3), 15.0, INK_DIM, Align::Center);
            return;
        }
        let w = &self.view.world;
        let igloos = w.igloos.iter().filter(|i| i.owner == you).count();
        let mine = || w.penguins.iter().filter(|p| p.owner == you);
        let soldiers = mine().filter(|p| p.kind.soldier()).count();
        let workers = mine().filter(|p| !p.kind.soldier()).count();
        let buildings = w.buildings.iter().filter(|b| b.alive && b.owner == you).count();
        let px = (s.h * 0.15).clamp(18.0, 26.0);
        ui.text("Your tribe", s.x + s.w / 2.0, s.y + s.h * 0.16, px, INK, Align::Center);
        let stats = [
            (Icon::Igloo, format!("{igloos}")),
            (Icon::Penguin, format!("{soldiers}")),
            (Icon::Worker, format!("{workers}")),
            (Icon::Camp, format!("{buildings}")),
        ];
        let tw: f32 = stats.iter().map(|(_, t)| 30.0 + ui.font.measure(t, 19.0) + 18.0).sum();
        let (mut x, y) = (s.x + s.w / 2.0 - tw / 2.0, s.y + s.h * 0.46);
        for (icon, t) in &stats {
            icons::draw(ui, *icon, x + 11.0, y + 11.0, 22.0);
            x += 30.0 + ui.text(t, x + 28.0, y, 19.0, INK, Align::Left) + 18.0;
        }
        let camp = |b: &&sim::proto::BuildingView| b.alive && b.owner == you && b.kind.trains().is_some();
        let standing = Standing {
            fish: w.fish_of(you),
            camps: w.buildings.iter().filter(camp).filter(|b| b.done()).count(),
            camps_going: w.buildings.iter().filter(camp).filter(|b| !b.done()).count(),
            houses: w.buildings.iter().filter(|b| b.alive && b.owner == you && b.kind == sim::game::BuildKind::Store).count(),
            bunkhouses: w.buildings.iter().filter(|b| b.alive && b.owner == you && b.kind == sim::game::BuildKind::Bunkhouse).count(),
            no_slot: {
                // Every fishing slot taken, and workers standing about.
                let houses = w.buildings.iter().filter(|b| b.alive && b.done() && b.owner == you && b.kind == sim::game::BuildKind::Store);
                let igs = w.igloos.iter().filter(|g| g.owner == you);
                let slots: usize = igs.clone().map(|g| g.fish_want as usize).sum::<usize>() + houses.clone().map(|b| b.want.min(b.slots) as usize).sum::<usize>();
                let fishers: usize = igs.map(|g| g.fishers as usize).sum::<usize>() + houses.map(|b| b.filled as usize).sum::<usize>();
                let standing = || w.penguins.iter().filter(|p| p.owner == you && p.kind == sim::game::Kind::Worker && !p.inside && p.act == sim::game::Act::Idle && p.carry == sim::game::Carry::None).count();
                if fishers >= slots { standing() } else { 0 }
            },
        };
        if let Some(advice) = advice::next_step(&standing) {
            let px = 15.0;
            for (i, line) in ui.font.wrap(advice, px, s.w - 40.0).iter().take(3).enumerate() {
                ui.text(line, s.x + s.w / 2.0, s.y + s.h * 0.66 + i as f32 * (px + 3.0), px, INK, Align::Center);
            }
        } else {
            let hint = if ui.input.touch { "Tap penguins to pick them · tap an igloo to train" } else { "Drag to pick penguins · click an igloo to train · B to build" };
            let mut hp = 15.0;
            while ui.font.measure(hint, hp) > s.w - 20.0 && hp > 10.0 {
                hp -= 0.5;
            }
            ui.text(hint, s.x + s.w / 2.0, s.y + s.h * 0.74, hp, INK_DIM, Align::Center);
        }
        if self.card_page == PAGE_MAIN {
            self.card_page = PAGE_BUILD;
        }
        self.build_card(ui, c, tip, false);
    }

    /// What an igloo of yours is up to: its
    /// Chieftain (with armies), and how to guard it.
    fn igloo_doings(&self, ui: &mut Ui, s: Rect, mut y: f32, igi: u8, v: &sim::proto::IglooView) {
        let chief = self.view.world.penguins.iter().find(|p| p.owner == v.owner && p.home == igi && p.kind == Kind::Chieftain && p.act != Act::Frozen);
        let (line, col) = match chief {
            Some(_) => ("Its Chieftain leads its army: move it and they follow.".to_string(), INK_DIM),
            None => ("No Chieftain: the igloo raises a new one from its penguins soon.".to_string(), INK_BAD),
        };
        ui.text(&line, s.x + 14.0, y, 15.0, col, Align::Left);
        y += 20.0;
        ui.text("Nobody guards an igloo by itself: build a Watchtower beside it.", s.x + 14.0, y, 15.0, INK_DIM, Align::Left);
    
    }

    /// A row of stamps, one per slot: filled, wanted and on its way, or off.
    /// Click a slot to want that many filled (click the last one wanted to
    /// want one fewer); roll the wheel over the row for one more or fewer.
    /// Building up adds slots but never fills them by itself. Returns how
    /// many card cells it used.
    fn slot_row(&mut self, ui: &mut Ui, c: &Console, first: usize, row: SlotRow, tip: &mut Tip) -> usize {
        let n = (row.slots as usize).min(CARD_SLOTS - 2 - first.min(CARD_SLOTS - 2));
        let mut want = row.want.min(row.slots);
        let mut over = false;
        for k in 0..n {
            let r = c.cell(first + k);
            let (label, on) = if k < row.filled as usize {
                (row.filled_label, true)
            } else if k < want as usize {
                ("Wanted", true)
            } else {
                ("Off", false)
            };
            if ui.card_stamp(r, row.icon, label, "", "", true, k < row.filled as usize) {
                want = if want as usize == k + 1 { k as u8 } else { k as u8 + 1 };
            }
            if !on {
                // Not wanted: washed out, but still there to click.
                ui.fill(r.shrink(3.0), 6.0, [190, 180, 166, 120]);
            }
            if ui.hovered(r) {
                over = true;
                *tip = Some((
                    format!("{} of {} wanted", want, row.slots),
                    format!("{} filled", row.filled),
                    format!("{} Click a slot to want that many filled, or roll the mouse wheel over them.", row.what),
                ));
            }
        }
        // The wheel anywhere on the card works the slots, not just on a stamp.
        if (over || ui.hovered(c.card)) && ui.input.wheel != 0.0 {
            want = if ui.input.wheel > 0.0 { (want + 1).min(row.slots) } else { want.saturating_sub(1) };
            ui.input.wheel = 0.0;
            ui.input.mouse_used = true;
        }
        if want != row.want.min(row.slots) {
            self.send(match row.at {
                WantAt::Building(building) => Command::SetWant { building, want },
                WantAt::Igloo(igloo) => Command::SetIglooFishers { igloo, want },
            });
        }
        n
    }

    /// A stamp to have builders build something up (`work`: snow steps and
    /// logs), or, with the work `going` (steps and logs still to do), how far
    /// along it is.
    #[allow(clippy::too_many_arguments)]
    fn build_up_stamp(&mut self, ui: &mut Ui, r: Rect, going: Option<(u8, u8)>, work: (u8, u8), label: String, next: u8, what: String, cmd: Command, tip: &mut Tip) {
        // The stamp's corner only fits the logs, as on the build card; the
        // slip says the lot.
        let price = format!("{} snow, {} logs", work.0, work.1);
        if let Some((steps, wood)) = going {
            let left = format!("{steps} snow, {wood} logs to go");
            ui.card_stamp(r, Icon::More, "Building up", "", "", true, true);
            if ui.hovered(r) {
                *tip = Some((format!("Building up to {next}"), left, what));
            }
            return;
        }
        if ui.card_stamp(r, Icon::More, &label, "", &format!("{}w", work.1), true, false) {
            self.send(cmd);
        }
        if ui.hovered(r) {
            *tip = Some(("Build up".into(), price, what));
        }
    }

    /// Short-lived messages, each on its own slip of paper.
    pub(super) fn draw_notices(&mut self, ui: &mut Ui) {
        let mut ny = 82.0;
        for (text, c, age) in &self.notices {
            let a = (1.0 - (age - 4.5).max(0.0) / 1.5).clamp(0.0, 1.0);
            let tw = ui.font.measure(text, 18.0) + 32.0;
            let r = rect(ui.w / 2.0 - tw / 2.0, ny, tw, 30.0);
            let paper = ui.batch.paper;
            ui.fill(rect(r.x + 1.0, r.y + 3.0, r.w, r.h), 4.0, [0, 0, 0, (60.0 * a) as u8]);
            ui.fill(r, 4.0, with_alpha([184, 151, 95, 255], a));
            ui.batch.round_rect_tex(ui.ctx, paper, r.shrink(1.5), 3.0, with_alpha([255, 255, 255, 255], a), skin::TILE);
            ui.text_in(text, r, 18.0, with_alpha(ink_of(*c), a), Align::Center);
            ny += 36.0;
        }
    }
}
