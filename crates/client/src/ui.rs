//! Input state and a tiny immediate-mode UI toolkit.

use std::collections::HashSet;

use crate::gpu::Gpu;
use crate::platform::{KeyCode, KeyMods};

use crate::font::{Align, Font};
use crate::gfx::{with_alpha, Batch, Color};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub const fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, w, h }
}

impl Rect {
    pub fn contains(&self, p: [f32; 2]) -> bool {
        p[0] >= self.x && p[1] >= self.y && p[0] < self.x + self.w && p[1] < self.y + self.h
    }
    pub fn shrink(&self, m: f32) -> Rect {
        rect(self.x + m, self.y + m, self.w - 2.0 * m, self.h - 2.0 * m)
    }
}

pub const MOUSE_L: usize = 0;
pub const MOUSE_R: usize = 1;
pub const MOUSE_M: usize = 2;

/// Raw input, accumulated from window events and reset every frame.
#[derive(Default)]
pub struct Input {
    pub mouse: [f32; 2],
    pub mouse_delta: [f32; 2],
    pub down: [bool; 3],
    pub pressed: [bool; 3],
    pub released: [bool; 3],
    pub double_click: bool,
    pub wheel: f32,
    pub keys_down: HashSet<KeyCode>,
    pub keys_pressed: Vec<(KeyCode, bool)>,
    pub chars: Vec<char>,
    pub mods: KeyMods,
    /// Set by UI widgets that used this frame's mouse press.
    pub mouse_used: bool,
    pub keyboard_used: bool,
    /// A button was clicked this frame (for the click sound).
    pub clicked: bool,
    pub last_click: f64,
    pub last_click_pos: [f32; 2],
    /// False once the pointer leaves the window (stops edge scrolling).
    pub inside: bool,
    /// The pointer is a finger: no hover, no edge scrolling.
    pub touch: bool,
    /// A second finger landed: drop whatever the first one was doing.
    pub cancel: bool,
    /// Two-finger gesture this frame: spread ratio (0 when none) and how far
    /// the midpoint moved, in window pixels.
    pub pinch: f32,
    pub pan2: [f32; 2],
}

impl Input {
    pub fn end_frame(&mut self) {
        self.mouse_delta = [0.0, 0.0];
        self.pressed = [false; 3];
        self.released = [false; 3];
        self.double_click = false;
        self.wheel = 0.0;
        self.keys_pressed.clear();
        self.chars.clear();
        self.mouse_used = false;
        self.keyboard_used = false;
        self.clicked = false;
        self.cancel = false;
        self.pinch = 0.0;
        self.pan2 = [0.0, 0.0];
    }
    pub fn key(&self, k: KeyCode) -> bool {
        self.keys_down.contains(&k)
    }
    /// Key went down this frame (not counting auto-repeat).
    pub fn hit(&self, k: KeyCode) -> bool {
        !self.keyboard_used && self.keys_pressed.iter().any(|&(kk, rep)| kk == k && !rep)
    }
    /// Key went down this frame, including auto-repeat.
    pub fn hit_repeat(&self, k: KeyCode) -> bool {
        self.keys_pressed.iter().any(|&(kk, _)| kk == k)
    }
    pub fn ctrl(&self) -> bool {
        self.mods.ctrl || self.mods.logo
    }
}

/// Colours for text on the wooden boards (see `skin` for the paper ones).
pub mod theme {
    use crate::gfx::Color;
    pub const ACCENT: Color = [248, 198, 88, 255];
    pub const TEXT: Color = crate::skin::CREAM;
    pub const TEXT_DIM: Color = crate::skin::CREAM_DIM;
    pub const GOOD: Color = [150, 222, 130, 255];
    pub const BAD: Color = [255, 128, 104, 255];
}

/// Per-frame UI context.
pub struct Ui<'a> {
    pub ctx: &'a mut Gpu,
    pub batch: &'a mut Batch,
    pub font: &'a Font,
    pub input: &'a mut Input,
    pub focus: &'a mut Option<u64>,
    pub time: f64,
    pub w: f32,
    pub h: f32,
}

impl Ui<'_> {
    pub fn hovered(&self, r: Rect) -> bool {
        r.contains(self.input.mouse)
    }

    /// A nailed wooden board, for menus and dialogs.
    pub fn panel(&mut self, r: Rect) {
        self.wood(r, 12.0);
        self.nails(r);
    }

    pub fn fill(&mut self, r: Rect, radius: f32, color: Color) {
        self.batch.round_rect(self.ctx, r, radius, color);
    }

    pub fn text(&mut self, text: &str, x: f32, y: f32, px: f32, color: Color, align: Align) -> f32 {
        self.font.draw(self.ctx, self.batch, text, x, y, px, color, align)
    }

    pub fn text_shadow(&mut self, text: &str, x: f32, y: f32, px: f32, color: Color, align: Align) -> f32 {
        self.font.draw_shadowed(self.ctx, self.batch, text, x, y, px, color, align)
    }

    /// Text vertically centered in a rect.
    pub fn text_in(&mut self, text: &str, r: Rect, px: f32, color: Color, align: Align) {
        let y = r.y + (r.h - px * 1.1) / 2.0;
        let x = match align {
            Align::Left => r.x + 8.0,
            Align::Center => r.x + r.w / 2.0,
            Align::Right => r.x + r.w - 8.0,
        };
        self.text(text, x, y, px, color, align);
    }

    pub fn button(&mut self, r: Rect, label: &str) -> bool {
        self.button_ex(r, label, true, false)
    }

    pub fn button_ex(&mut self, r: Rect, label: &str, enabled: bool, selected: bool) -> bool {
        let clicked = self.stamp(r, enabled, selected);
        let lift = if enabled && self.hovered(r) && self.input.down[0] { 2.0 } else { 0.0 };
        let px = (r.h * 0.5).min(22.0);
        let mut fit = px;
        while self.font.measure(label, fit) > r.w - 14.0 && fit > 10.0 {
            fit -= 0.5;
        }
        self.text_in(label, rect(r.x, r.y + lift, r.w, r.h), fit, if enabled { crate::skin::INK } else { crate::skin::INK_DIM }, Align::Center);
        clicked
    }

    /// Single-line text input. Returns true when Enter is pressed.
    pub fn text_field(&mut self, id: u64, r: Rect, value: &mut String, max: usize, placeholder: &str) -> bool {
        let hover = self.hovered(r);
        if self.input.pressed[0] {
            if hover {
                *self.focus = Some(id);
                self.input.mouse_used = true;
            } else if *self.focus == Some(id) {
                *self.focus = None;
            }
        }
        let focused = *self.focus == Some(id);
        let mut submitted = false;
        if focused {
            for &c in &self.input.chars {
                if !c.is_control() && value.chars().count() < max {
                    value.push(c);
                }
            }
            if self.input.hit_repeat(KeyCode::Backspace) {
                value.pop();
            }
            if self.input.hit_repeat(KeyCode::Enter) || self.input.hit_repeat(KeyCode::KpEnter) {
                submitted = true;
            }
            self.input.keyboard_used = true;
        }
        // A dark groove cut into the wood.
        if focused {
            self.fill(r.shrink(-2.0), 8.0, theme::ACCENT);
        }
        self.fill(r, 6.0, [34, 18, 8, 255]);
        self.fill(rect(r.x + 1.0, r.y + 2.0, r.w - 2.0, r.h - 3.0), 5.0, if focused { [70, 42, 22, 255] } else { [56, 33, 17, 255] });
        let px = (r.h * 0.55).min(22.0);
        let y = r.y + (r.h - px * 1.1) / 2.0;
        if value.is_empty() && !focused {
            self.text(placeholder, r.x + 8.0, y, px, with_alpha(theme::TEXT_DIM, 0.7), Align::Left);
        } else {
            // Show the tail of long text.
            let mut shown: &str = value;
            while self.font.measure(shown, px) > r.w - 20.0 && !shown.is_empty() {
                let mut it = shown.chars();
                it.next();
                shown = it.as_str();
            }
            let w = self.text(shown, r.x + 8.0, y, px, theme::TEXT, Align::Left);
            if focused && (self.time * 2.0).fract() < 0.6 {
                self.batch.rect(self.ctx, r.x + 9.0 + w, y + 2.0, 2.0, px * 1.05, theme::TEXT);
            }
        }
        submitted
    }

    pub fn swatch(&mut self, r: Rect, color: Color) -> bool {
        let hover = self.hovered(r);
        self.fill(r.shrink(-2.0), 6.0, if hover { theme::ACCENT } else { [255, 255, 255, 60] });
        self.fill(r, 5.0, color);
        if hover && self.input.pressed[0] {
            self.input.mouse_used = true;
            return true;
        }
        false
    }

    /// A small horizontal bar (progress / warmth).
    pub fn bar(&mut self, r: Rect, frac: f32, color: Color) {
        self.batch.rect(self.ctx, r.x, r.y, r.w, r.h, [0, 0, 0, 150]);
        self.batch.rect(self.ctx, r.x + 1.0, r.y + 1.0, (r.w - 2.0) * frac.clamp(0.0, 1.0), r.h - 2.0, color);
    }
}
