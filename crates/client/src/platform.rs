//! The window, keyboard and clock, the same on desktops and in browsers
//! (winit underneath). The rest of the client sees only these plain types.

use std::cell::RefCell;
use std::sync::Arc;

use winit::window::{Fullscreen, Window};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyCode {
    A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    Key0, Key1, Key2, Key3, Key4, Key5, Key6, Key7, Key8, Key9,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    Up, Down, Left, Right, PageUp, PageDown, Home, End,
    Enter, KpEnter, Escape, Backspace, Delete, Tab, Space,
    Comma, Period, Slash, Minus, Equal, KpAdd, KpSubtract,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyMods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchPhase {
    Started,
    Moved,
    Ended,
    Cancelled,
}

/// Our key for a physical key, by where it sits (so WASD works on any layout).
pub fn key(k: winit::keyboard::KeyCode) -> Option<KeyCode> {
    use winit::keyboard::KeyCode as W;
    use KeyCode::*;
    Some(match k {
        W::KeyA => A, W::KeyB => B, W::KeyC => C, W::KeyD => D, W::KeyE => E, W::KeyF => F, W::KeyG => G,
        W::KeyH => H, W::KeyI => I, W::KeyJ => J, W::KeyK => K, W::KeyL => L, W::KeyM => M, W::KeyN => N,
        W::KeyO => O, W::KeyP => P, W::KeyQ => Q, W::KeyR => R, W::KeyS => S, W::KeyT => T, W::KeyU => U,
        W::KeyV => V, W::KeyW => W, W::KeyX => X, W::KeyY => Y, W::KeyZ => Z,
        W::Digit0 | W::Numpad0 => Key0, W::Digit1 | W::Numpad1 => Key1, W::Digit2 | W::Numpad2 => Key2,
        W::Digit3 | W::Numpad3 => Key3, W::Digit4 | W::Numpad4 => Key4, W::Digit5 | W::Numpad5 => Key5,
        W::Digit6 | W::Numpad6 => Key6, W::Digit7 | W::Numpad7 => Key7, W::Digit8 | W::Numpad8 => Key8,
        W::Digit9 | W::Numpad9 => Key9,
        W::F1 => F1, W::F2 => F2, W::F3 => F3, W::F4 => F4, W::F5 => F5, W::F6 => F6, W::F7 => F7,
        W::F8 => F8, W::F9 => F9, W::F10 => F10, W::F11 => F11, W::F12 => F12,
        W::ArrowUp => Up, W::ArrowDown => Down, W::ArrowLeft => Left, W::ArrowRight => Right,
        W::PageUp => PageUp, W::PageDown => PageDown, W::Home => Home, W::End => End,
        W::Enter => Enter, W::NumpadEnter => KpEnter, W::Escape => Escape, W::Backspace => Backspace,
        W::Delete => Delete, W::Tab => Tab, W::Space => Space,
        W::Comma => Comma, W::Period => Period, W::Slash => Slash, W::Minus => Minus, W::Equal => Equal,
        W::NumpadAdd => KpAdd, W::NumpadSubtract => KpSubtract,
        _ => return None,
    })
}

/// Seconds since the epoch, with sub-millisecond precision.
pub fn now() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() / 1000.0
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
    }
}

/// The clock, by the name the game has always called it.
pub mod date {
    pub fn now() -> f64 {
        super::now()
    }
}

/// The open window, for the few things the game asks of it directly.
pub mod window {
    use super::*;

    #[derive(Default)]
    pub(crate) struct State {
        pub window: Option<Arc<Window>>,
        /// Drawable size in physical pixels, and pixels per point.
        pub size: (f32, f32),
        pub dpi: f32,
        pub quit: bool,
    }

    thread_local! {
        pub(crate) static STATE: RefCell<State> = RefCell::new(State { dpi: 1.0, size: (1280.0, 800.0), ..Default::default() });
    }

    pub fn screen_size() -> (f32, f32) {
        STATE.with(|s| s.borrow().size)
    }

    pub fn dpi_scale() -> f32 {
        STATE.with(|s| s.borrow().dpi)
    }

    pub fn set_fullscreen(on: bool) {
        STATE.with(|s| {
            if let Some(w) = &s.borrow().window {
                w.set_fullscreen(on.then_some(Fullscreen::Borderless(None)));
            }
        });
    }

    pub fn order_quit() {
        STATE.with(|s| s.borrow_mut().quit = true);
    }
}
