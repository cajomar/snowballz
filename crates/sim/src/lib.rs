//! Snowballz game simulation: map, rules, AI and the network protocol.
//!
//! The server owns the authoritative [`game::Game`]; clients only ever see a
//! [`proto::WorldView`] rebuilt from snapshots. Nothing in here depends on
//! graphics, sockets or threads.

pub mod ai;
pub mod balance;
pub mod fog;
pub mod game;
pub mod json;
pub mod map;
pub mod mapgen;
pub mod math;
pub mod path;
pub mod proto;
pub mod rng;
pub mod wire;

/// Simulation ticks per second.
pub const TICK_RATE: u32 = 20;
/// Seconds per tick.
pub const DT: f32 = 1.0 / TICK_RATE as f32;

/// Converts seconds to ticks.
pub const fn secs(s: f32) -> u16 {
    (s * TICK_RATE as f32) as u16
}
