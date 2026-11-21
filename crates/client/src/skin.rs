//! The HUD's hand-made look: wooden planks and paper, generated in code
//! and tileable (so a panel of any size just repeats them), and the
//! widgets drawn with them.

use crate::font::Align;
use crate::gfx::{with_alpha, Color, Image};
use crate::icons::{self, Icon};
use crate::ui::{rect, Rect, Ui};

/// Side of the wood and paper tiles, in pixels.
pub const TILE: f32 = 256.0;

pub const INK: Color = [59, 42, 28, 255];
pub const INK_DIM: Color = [122, 100, 80, 255];
pub const INK_BAD: Color = [178, 52, 40, 255];
pub const INK_COLD: Color = [50, 100, 160, 255];
pub const CREAM: Color = [251, 238, 211, 255];
pub const CREAM_DIM: Color = [226, 200, 158, 255];
pub const WOOD_EDGE: Color = [61, 36, 20, 255];
const PAPER_EDGE: Color = [184, 151, 95, 255];
const STAMP_EDGE: Color = [138, 100, 56, 255];
const STAMP_LIP: Color = [107, 74, 40, 255];
pub const GOLD: Color = [217, 165, 58, 255];

fn hash(a: u32, b: u32, seed: u32) -> f32 {
    let mut v = a.wrapping_mul(374_761_393) ^ b.wrapping_mul(668_265_263) ^ seed.wrapping_mul(2_246_822_519);
    v = (v ^ (v >> 13)).wrapping_mul(1_274_126_177);
    ((v ^ (v >> 16)) & 0xffff) as f32 / 65535.0
}

/// Smooth value noise wrapping every `px` by `py` lattice cells.
fn noise(x: f32, y: f32, px: i32, py: i32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - xi as f32, y - yi as f32);
    let h = |i: i32, j: i32| hash(i.rem_euclid(px) as u32, j.rem_euclid(py) as u32, seed);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (s(fx), s(fy));
    let a = h(xi, yi) + (h(xi + 1, yi) - h(xi, yi)) * sx;
    let b = h(xi, yi + 1) + (h(xi + 1, yi + 1) - h(xi, yi + 1)) * sx;
    a + (b - a) * sy
}

fn image(f: impl Fn(f32, f32) -> [f32; 3]) -> Image {
    let n = TILE as u32;
    let mut rgba = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let c = f(x as f32, y as f32);
            rgba.extend_from_slice(&[c[0].clamp(0.0, 255.0) as u8, c[1].clamp(0.0, 255.0) as u8, c[2].clamp(0.0, 255.0) as u8, 255]);
        }
    }
    Image { w: n, h: n, rgba }
}

/// Horizontal planks, each its own shade, with grain and a butt joint.
pub fn wood() -> Image {
    const PLANK: f32 = 32.0;
    image(|x, y| {
        let row = (y / PLANK) as u32;
        let iy = y % PLANK;
        if iy < 1.0 {
            return [58.0, 34.0, 18.0];
        }
        let joint = (hash(row, 7, 3) * TILE).floor();
        if (x - joint).abs() < 1.0 {
            return [70.0, 42.0, 24.0];
        }
        let shade = 0.86 + 0.2 * hash(row, 1, 9);
        // Rings: stretched noise along the plank, cut into thin dark lines.
        let warp = noise(x / 64.0, y / 8.0, 4, 32, 1 + row) * 2.6 + noise(x / 16.0, y / 4.0, 16, 64, 5) * 0.5;
        let ring = (y / 3.5 + warp * 2.0).fract();
        let dark = (1.0 - (ring - 0.5).abs() * 6.0).clamp(0.0, 1.0) * 0.55;
        let fleck = noise(x / 2.0, y / 1.0, 128, 256, 11) * 0.12;
        let k = shade * (1.0 - dark * 0.35 - fleck);
        // A lighter top edge and a darker bottom one round each plank.
        let bevel = if iy < 2.5 { 1.15 } else if iy > PLANK - 2.5 { 0.82 } else { 1.0 };
        [150.0 * k * bevel, 98.0 * k * bevel, 58.0 * k * bevel]
    })
}

/// Cream paper with soft blotches and fibres.
pub fn paper() -> Image {
    image(|x, y| {
        let blotch = noise(x / 32.0, y / 32.0, 8, 8, 21) - 0.5;
        let fibre = noise(x / 2.0, y / 6.0, 128, 43, 23) - 0.5;
        let speck = noise(x, y, 256, 256, 29);
        let k = 1.0 + blotch * 0.07 + fibre * 0.05 - if speck > 0.93 { 0.06 } else { 0.0 };
        [244.0 * k, 231.0 * k, 201.0 * k]
    })
}

impl Ui<'_> {
    /// A wooden board with a dark rim and a soft shadow.
    pub fn wood(&mut self, r: Rect, radius: f32) {
        self.fill(rect(r.x + 1.0, r.y + 4.0, r.w, r.h), radius, [0, 0, 0, 90]);
        self.fill(r, radius, WOOD_EDGE);
        let wood = self.batch.wood;
        self.batch.round_rect_tex(self.ctx, wood, r.shrink(3.0), (radius - 2.0).max(0.0), [255, 255, 255, 255], TILE);
        self.fill(rect(r.x + radius.max(4.0), r.y + 3.0, r.w - 2.0 * radius.max(4.0), 1.5), 0.0, [255, 220, 170, 50]);
        self.block_mouse(r);
    }

    /// Four nail heads in the corners of a board.
    pub fn nails(&mut self, r: Rect) {
        for (x, y) in [(r.x + 8.0, r.y + 8.0), (r.x + r.w - 8.0, r.y + 8.0), (r.x + 8.0, r.y + r.h - 8.0), (r.x + r.w - 8.0, r.y + r.h - 8.0)] {
            self.batch.ellipse(self.ctx, x + 0.5, y + 1.0, 3.2, 3.2, [0, 0, 0, 120]);
            self.batch.ellipse(self.ctx, x, y, 3.0, 3.0, [140, 132, 120, 255]);
            self.batch.ellipse(self.ctx, x - 0.8, y - 0.8, 1.3, 1.3, [220, 212, 196, 255]);
        }
    }

    /// A sheet of paper.
    pub fn paper(&mut self, r: Rect, radius: f32) {
        self.fill(rect(r.x + 1.0, r.y + 4.0, r.w, r.h), radius, [0, 0, 0, 70]);
        self.fill(r, radius, PAPER_EDGE);
        let paper = self.batch.paper;
        self.batch.round_rect_tex(self.ctx, paper, r.shrink(2.0), (radius - 1.5).max(0.0), [255, 255, 255, 255], TILE);
        self.block_mouse(r);
    }

    /// A pressable paper stamp. Returns true when clicked.
    pub fn stamp(&mut self, r: Rect, enabled: bool, selected: bool) -> bool {
        let hover = self.hovered(r);
        let down = enabled && hover && self.input.down[0];
        let lift = if down { 2.0 } else { 0.0 };
        if selected {
            self.fill(r.shrink(-3.0), 10.0, GOLD);
        }
        self.fill(rect(r.x, r.y + 3.0, r.w, r.h), 8.0, if enabled { STAMP_LIP } else { [86, 62, 40, 200] });
        let face = rect(r.x, r.y + lift, r.w, r.h);
        self.fill(face, 8.0, if enabled { STAMP_EDGE } else { [120, 98, 74, 230] });
        let tint = match (enabled, hover) {
            (false, _) => [190, 180, 166, 235],
            (true, true) => [255, 252, 236, 255],
            (true, false) => [236, 220, 188, 255],
        };
        let paper = self.batch.paper;
        self.batch.round_rect_tex(self.ctx, paper, face.shrink(2.0), 6.5, tint, TILE);
        let clicked = enabled && hover && self.input.pressed[0];
        self.input.clicked |= clicked;
        if hover && self.input.pressed.iter().any(|&p| p) {
            self.input.mouse_used = true;
        }
        clicked
    }

    /// A stamp with a label, for plain actions (text only).
    pub fn stamp_text(&mut self, r: Rect, label: &str, enabled: bool) -> bool {
        let clicked = self.stamp(r, enabled, false);
        let px = (r.h * 0.5).min(20.0);
        self.text_in(label, r, px, if enabled { INK } else { INK_DIM }, Align::Center);
        clicked
    }

    /// A command card stamp: icon, name, hotkey and cost.
    #[allow(clippy::too_many_arguments)]
    pub fn card_stamp(&mut self, r: Rect, icon: Icon, label: &str, key: &str, cost: &str, enabled: bool, selected: bool) -> bool {
        let clicked = self.stamp(r, enabled, selected);
        let lift = if enabled && self.hovered(r) && self.input.down[0] { 2.0 } else { 0.0 };
        let r = rect(r.x, r.y + lift, r.w, r.h);
        let lp = (r.h * 0.24).clamp(10.0, 15.0);
        let is = (r.h - lp - 10.0).min(r.w * 0.5).max(10.0);
        let cy = r.y + 4.0 + is / 2.0;
        icons::draw(self, icon, r.x + r.w / 2.0, cy, is);
        if !enabled {
            // Greyed out: a wash of the stamp's colour over the picture.
            self.fill(rect(r.x + r.w / 2.0 - is / 2.0, cy - is / 2.0, is, is), 3.0, [190, 180, 166, 140]);
        }
        let mut lp2 = lp;
        while self.font.measure(label, lp2) > r.w - 6.0 && lp2 > 8.0 {
            lp2 -= 0.5;
        }
        self.text(label, r.x + r.w / 2.0, r.y + r.h - lp - 5.0, lp2, if enabled { INK } else { [96, 80, 64, 255] }, Align::Center);
        if !key.is_empty() {
            let kw = self.font.measure(key, 11.0) + 7.0;
            self.fill(rect(r.x + 3.0, r.y + 3.0, kw, 14.0), 4.0, [94, 55, 32, 230]);
            self.text(key, r.x + 3.0 + kw / 2.0, r.y + 3.5, 11.0, CREAM, Align::Center);
        }
        if !cost.is_empty() {
            self.text(cost, r.x + r.w - 4.0, r.y + 3.0, 11.0, INK_DIM, Align::Right);
        }
        clicked
    }

    /// A paper slip with a title, a right-aligned note and wrapped text,
    /// beside `anchor` on the side toward the middle of the screen.
    pub fn slip(&mut self, anchor: Rect, w: f32, title: &str, note: &str, body: &str) {
        let w = w.min(self.w - 8.0);
        let lines = if body.is_empty() { Vec::new() } else { self.font.wrap(body, 15.0, w - 24.0) };
        let h = 12.0 + if title.is_empty() { 0.0 } else { 24.0 } + lines.len() as f32 * 19.0;
        let x = (anchor.x + anchor.w / 2.0 - w / 2.0).clamp(4.0, (self.w - w - 4.0).max(4.0));
        let y = if anchor.y + anchor.h / 2.0 > self.h / 2.0 { (anchor.y - h - 8.0).max(4.0) } else { anchor.y + anchor.h + 8.0 };
        let r = rect(x, y, w, h);
        self.paper(r, 6.0);
        let mut ty = r.y + 6.0;
        if !title.is_empty() {
            self.text(title, r.x + 12.0, ty, 19.0, INK, Align::Left);
            self.text(note, r.x + r.w - 12.0, ty + 3.0, 14.0, INK_DIM, Align::Right);
            ty += 24.0;
        }
        for l in lines {
            self.text(&l, r.x + 12.0, ty, 15.0, INK, Align::Left);
            ty += 19.0;
        }
    }

    /// A small slot meter on paper.
    pub fn meter(&mut self, r: Rect, frac: f32, color: Color) {
        self.fill(r, r.h / 2.0, [156, 124, 74, 255]);
        let inner = r.shrink(1.5);
        self.fill(inner, inner.h / 2.0, [211, 187, 140, 255]);
        let f = frac.clamp(0.0, 1.0);
        if f > 0.0 {
            self.fill(rect(inner.x, inner.y, (inner.w * f).max(inner.h), inner.h), inner.h / 2.0, color);
        }
    }

    /// A chip of cream text on a dark inset in the wood.
    pub fn chip(&mut self, x: f32, cy: f32, icon: Icon, value: &str, label: &str, color: Color, px: f32) -> Rect {
        let lw = if label.is_empty() { 0.0 } else { self.font.measure(label, px * 0.6) + 4.0 };
        let w = px + 6.0 + self.font.measure(value, px) + lw + 16.0;
        let r = rect(x, cy - px * 0.7, w, px * 1.4);
        self.fill(r, 7.0, [40, 20, 8, 95]);
        icons::draw(self, icon, x + 8.0 + px / 2.0, cy, px);
        let tx = x + 12.0 + px;
        let vw = self.font.draw_shadowed(self.ctx, self.batch, value, tx, cy - px * 0.58, px, color, Align::Left);
        if !label.is_empty() {
            self.text(label, tx + vw + 4.0, cy - px * 0.58 + px * 0.36, px * 0.6, with_alpha(CREAM_DIM, 1.0), Align::Left);
        }
        r
    }

    fn block_mouse(&mut self, r: Rect) {
        if self.hovered(r) && self.input.pressed.iter().any(|&p| p) {
            self.input.mouse_used = true;
        }
    }
}
