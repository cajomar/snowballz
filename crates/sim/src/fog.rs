//! Fog of war: which tiles a team can see right now. Deterministic and
//! cheap. The server uses it to send each player only what their team can
//! see; clients use it to dim the rest.

use crate::map::Map;
use crate::math::Vec2;
use crate::game::{Act, BuildKind, Game, NEUTRAL};

/// How far a penguin sees.
pub const PENGUIN_SIGHT: f32 = 9.0;
/// How far an igloo sees.
pub const IGLOO_SIGHT: f32 = 12.0;
/// How far a finished watchtower sees.
pub const TOWER_SIGHT: f32 = 16.0;

/// How far a finished building of this kind sees, if it sees at all.
pub fn building_sight(kind: BuildKind) -> Option<f32> {
    matches!(kind, BuildKind::Tower | BuildKind::Catapult).then_some(TOWER_SIGHT)
}

/// One flag per tile: true where any of `eyes` (position, sight radius)
/// can see. No line of sight yet: hills and trees don't hide anything.
pub fn visible(map: &Map, eyes: &[(Vec2, f32)]) -> Vec<bool> {
    let mut seen = vec![false; (map.w * map.h) as usize];
    for &(at, r) in eyes {
        let (cx, cy) = at.tile();
        let ri = r.ceil() as i32;
        for dy in -ri..=ri {
            for dx in -ri..=ri {
                let (x, y) = (cx + dx, cy + dy);
                if !map.in_bounds(x, y) {
                    continue;
                }
                let (fx, fy) = (x as f32 + 0.5 - at.x, y as f32 + 0.5 - at.y);
                if fx * fx + fy * fy <= r * r {
                    seen[map.idx(x, y)] = true;
                }
            }
        }
    }
    seen
}

/// What fraction of their usual sight penguins keep in a blizzard.
pub const BLIZZARD_SIGHT: f32 = 0.6;

/// Everything `team` sees with: its penguins, igloos, and finished watchtowers.
pub fn team_eyes(g: &Game, team: u8) -> Vec<(Vec2, f32)> {
    let blizzard = if g.weather.blizzard > 0 { BLIZZARD_SIGHT } else { 1.0 };
    let mut eyes: Vec<(Vec2, f32)> = g
        .penguins
        .iter()
        .filter(|p| p.inside.is_none() && p.owner != NEUTRAL && g.team(p.owner) == team && p.act != Act::Frozen)
        .map(|p| (p.pos, PENGUIN_SIGHT * blizzard))
        .collect();
    for ig in &g.igloos {
        if ig.owner != NEUTRAL && g.team(ig.owner) == team {
            eyes.push((ig.center, IGLOO_SIGHT));
        }
    }
    for b in &g.buildings {
        if let Some(r) = building_sight(b.kind).filter(|_| b.alive && b.done() && b.owner != NEUTRAL && g.team(b.owner) == team) {
            eyes.push((b.center(), r));
        }
    }
    eyes
}

/// What one team can see right now, kept by the server for each client.
pub struct Fov {
    pub team: u8,
    w: i32,
    seen: Vec<bool>,
    pub at_tick: u32,
}

impl Fov {
    pub fn compute(g: &Game, team: u8) -> Fov {
        Fov { team, w: g.map.w, seen: visible(&g.map, &team_eyes(g, team)), at_tick: g.tick }
    }

    pub fn sees_tile(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && (y * self.w + x) < self.seen.len() as i32 && self.seen[(y * self.w + x) as usize]
    }

    pub fn sees(&self, at: Vec2) -> bool {
        let (x, y) = at.tile();
        self.sees_tile(x, y)
    }
}
