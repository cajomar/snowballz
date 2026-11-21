//! Shore fishing, the only way to fish. Any water (a bay, a lake, or one of
//! the little ponds that stand where older maps had fish holes) can be
//! fished from its bank, without end. A bank holds one worker at a time, so
//! a crowd spreads along the shore instead of piling onto one spot.

use super::*;

/// How many times slower than the base fishing time a bank lands a fish.
pub const SHORE_SLOWDOWN: u32 = 2;

/// A tile a worker can stand on to fish: walkable, with water beside it.
#[derive(Clone, Debug)]
pub struct Shore {
    pub tile: (i32, i32),
    pub pos: Vec2,
    /// Which way the water lies, so the worker faces it.
    pub face: f32,
}

/// Every bank on the map, in tile order (so it is the same on every machine).
pub(super) fn find_shores(map: &Map, grid: &Grid) -> Vec<Shore> {
    let mut out = Vec::new();
    for y in 0..map.h {
        for x in 0..map.w {
            if !grid.walkable(x, y) || map.ground_at(x, y) == Ground::Water {
                continue;
            }
            // Face the first water tile of the four around it.
            let Some(&(dx, dy)) = [(0, 1), (1, 0), (-1, 0), (0, -1)].iter().find(|&&(dx, dy)| map.ground_at(x + dx, y + dy) == Ground::Water) else {
                continue;
            };
            out.push(Shore { tile: (x, y), pos: v2(x as f32 + 0.5, y as f32 + 0.5), face: v2(dx as f32, dy as f32).angle() });
        }
    }
    out
}

impl Game {
    /// The free bank worker `i` should fish: the nearest one that no other
    /// worker has taken.
    fn choose_shore(&mut self, i: usize) -> Option<u16> {
        // A fisher in a slot fishes the banks by its igloo or Sushi House.
        let at = self.fishery_of(i).map_or(self.penguins[i].pos, |(_, at)| at);
        self.choose_shore_near(at)
    }

    /// The free bank nearest `near` that no other worker has taken.
    fn choose_shore_near(&mut self, near: Vec2) -> Option<u16> {
        let mut taken = vec![false; self.shores.len()];
        for q in &self.penguins {
            if let Some(s) = q.shore {
                taken[s as usize] = true;
            }
        }
        let mut best: Option<(f32, u16)> = None;
        for (k, s) in self.shores.iter().enumerate() {
            if taken[k] {
                continue;
            }
            let d = s.pos.dist(near) + self.rng.range(0.0, 3.0);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, k as u16));
            }
        }
        best.map(|(_, k)| k)
    }

    /// Worker job: walk to a bank and fish it. False if there is none to use.
    pub(super) fn fish_shore(&mut self, i: usize) -> bool {
        let Some(k) = self.penguins[i].shore.or_else(|| self.choose_shore(i)) else { return false };
        self.penguins[i].shore = Some(k);
        let (pos, face) = (self.shores[k as usize].pos, self.shores[k as usize].face);
        if self.travel(i, pos, 0.25) {
            let p = &mut self.penguins[i];
            p.act = Act::Fish;
            p.act_t = 0;
            p.anim_seq = p.anim_seq.wrapping_add(1);
            p.facing = face;
        }
        true
    }

    /// Send workers to fish the bank nearest `tile`, which should be water.
    /// Each takes a free bank of its own near there; the rest of the tribe's
    /// penguins just walk to the shore.
    pub(super) fn order_fish(&mut self, player: u8, units: Vec<u16>, tile: (i32, i32)) {
        if self.map.ground_at(tile.0, tile.1) != Ground::Water {
            return;
        }
        let at = v2(tile.0 as f32 + 0.5, tile.1 as f32 + 0.5);
        let mut others = Vec::new();
        for u in units {
            if !self.controllable(player, u) {
                continue;
            }
            if self.penguins[u as usize].kind != Kind::Worker {
                others.push(u);
                continue;
            }
            self.give_order(u, Order::None);
            let k = self.choose_shore_near(at);
            let p = &mut self.penguins[u as usize];
            p.prefer = ResourceKind::Fish;
            p.shore = k;
            p.shore_pin = k.is_some();
        }
        if !others.is_empty() {
            if let Some(s) = self.shores.iter().min_by(|a, b| a.pos.dist2(at).partial_cmp(&b.pos.dist2(at)).unwrap()) {
                let target = s.pos;
                self.command(player, Command::Move { units: others, target, run: false, queue: false });
            }
        }
    }

    /// A bank gives a fish after a long wait, and never runs dry.
    pub(super) fn shore_catch(&mut self, i: usize) {
        // Midsummer fish bite twice as fast.
        // With armies only the few in fishing slots fish, so each is quicker.
        let slow = 1;
        let fish_ticks = self.effects_of(self.penguins[i].owner).scale(Stat::FishTime, FISH_TICKS as u32) * slow / self.fish_rate() as u32;
        let p = &mut self.penguins[i];
        if p.act_t as u32 >= fish_ticks {
            p.carry = Carry::Fish;
            p.act = Act::Idle;
        }
    }
}
