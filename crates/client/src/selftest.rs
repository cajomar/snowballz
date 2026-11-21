//! `--selftest`: plays a quickstart game through the real window, injecting
//! mouse and keyboard input the way a player would, and checks that each
//! action did what it should. Prints a report and exits non-zero on failure.

use crate::platform::{window, KeyCode};
use sim::game::{Act, Kind};
use sim::map::Obstacle;
use sim::math::{v2, Vec2};

use crate::mathx::v3;
use crate::App;

#[derive(Default)]
pub struct SelfTest {
    step: usize,
    /// Seconds spent on the current step.
    t: f32,
    sub: usize,
    results: Vec<(&'static str, Result<(), String>)>,
    // Values carried between frames of a step.
    target: Vec2,
    before: f32,
    saved: Vec<u16>,
    home: u8,
    /// Seconds a step may take before it fails (set by steps whose time depends on the map).
    budget: f32,
    /// When a camp's builders were last seen at work.
    nudge: f32,
    /// How many times a worker was sent to the camp to train.
    attempts: u8,
    /// How many spots for the camp the server has turned down.
    site_try: usize,
}

impl App {
    /// Logical (UI) screen position of a map point at ground height + `dz`.
    fn st_screen(&self, p: Vec2, dz: f32) -> Option<[f32; 2]> {
        let g = self.game.as_ref()?;
        let z = g.view.info().map.height_at(p) + dz;
        let s = self.world_renderer()?.world_to_screen(v3(p.x, p.y, z))?;
        let k = self.scale();
        Some([s[0] / k, s[1] / k])
    }

    fn st_mouse(&mut self, at: [f32; 2]) {
        let d = [at[0] - self.input.mouse[0], at[1] - self.input.mouse[1]];
        self.input.mouse_delta[0] += d[0];
        self.input.mouse_delta[1] += d[1];
        self.input.mouse = at;
        self.input.inside = true;
    }

    fn st_press(&mut self, b: usize, at: [f32; 2]) {
        self.st_mouse(at);
        self.input.down[b] = true;
        self.input.pressed[b] = true;
    }

    fn st_release(&mut self, b: usize, at: [f32; 2]) {
        self.st_mouse(at);
        self.input.down[b] = false;
        self.input.released[b] = true;
    }

    fn st_key(&mut self, k: KeyCode, ctrl: bool) {
        self.input.mods.ctrl = ctrl;
        self.input.keys_pressed.push((k, false));
    }

    fn st_mine_on_screen(&self, kind: Kind) -> Vec<(u16, [f32; 2])> {
        let Some(g) = &self.game else { return Vec::new() };
        let (lw, lh) = self.logical_size();
        let w = &g.view.world;
        w.penguins
            .iter()
            .filter(|p| p.owner == w.info.you && !p.inside && p.kind == kind)
            .filter_map(|p| {
                let (pos, _) = w.penguin_at(p.id, g.view.clock as f32);
                let s = self.st_screen(pos, 0.45)?;
                // Keep clear of the HUD bars and minimap.
                (s[0] > 280.0 && s[0] < lw - 240.0 && s[1] > 80.0 && s[1] < lh - 230.0).then_some((p.id, s))
            })
            .collect()
    }

    fn logical_size(&self) -> (f32, f32) {
        let (pw, ph) = window::screen_size();
        (pw / self.scale(), ph / self.scale())
    }

    fn st_centroid(&self, ids: &[u16]) -> Option<Vec2> {
        let w = &self.game.as_ref()?.view.world;
        let ps: Vec<Vec2> = ids.iter().filter_map(|&i| w.penguins.get(i as usize)).map(|p| p.pos).collect();
        (!ps.is_empty()).then(|| ps.iter().fold(Vec2::ZERO, |a, &p| a + p) * (1.0 / ps.len() as f32))
    }

    /// Called once a frame before the UI runs. Injects input for the current
    /// step and checks the results of the previous one.
    pub(crate) fn selftest_frame(&mut self, dt: f32) {
        let Some(mut st) = self.selftest.take() else { return };
        st.t += dt;
        let ready = self.screen == crate::Screen::Game
            && self.game.as_ref().is_some_and(|g| g.view.world.tick > 60 && !g.view.world.penguins.is_empty());
        if ready || st.step > 0 {
            // The computers' panel would cover part of the map the test clicks on.
            if let Some(g) = self.game.as_mut() {
                g.ai_panel = false;
            }
            self.selftest_step(&mut st);
        } else if st.t > 30.0 {
            st.results.push(("game starts", Err("no game after 30 s".into())));
            st.step = usize::MAX;
        }
        if st.step == usize::MAX {
            let mut failed = 0;
            println!("\nselftest:");
            for (name, r) in &st.results {
                match r {
                    Ok(()) => println!("  ok    {name}"),
                    Err(e) => {
                        failed += 1;
                        println!("  FAIL  {name}: {e}");
                    }
                }
            }
            println!("{} passed, {failed} failed", st.results.len() - failed);
            std::process::exit(if failed > 0 { 1 } else { 0 });
        }
        self.selftest = Some(st);
    }

    fn st_next(&self, st: &mut SelfTest, name: &'static str, r: Result<(), String>) {
        st.results.push((name, r));
        st.step += 1;
        st.sub = 0;
        st.t = 0.0;
    }

    fn selftest_step(&mut self, st: &mut SelfTest) {
        let (lw, lh) = self.logical_size();
        match st.step {
            // Settle: the camera starts on our igloo.
            0 => {
                if st.t < 1.0 {
                    return;
                }
                let g = self.game.as_ref().unwrap();
                let mine = g.view.my_igloos();
                let r = match mine.first() {
                    None => Err("we own no igloo".to_string()),
                    Some(&ig) => {
                        st.home = ig;
                        let door = g.view.info().igloos[ig as usize].door;
                        let c = self.world_renderer().unwrap().center();
                        let s = self.st_screen(door, 0.0).unwrap_or([-1.0, -1.0]);
                        let off = [s[0] / lw - 0.5, s[1] / lh - 0.5];
                        if c.dist(door) > 3.0 {
                            Err(format!("camera at {:?}, igloo door at {:?}", c, door))
                        } else if off[0].abs() > 0.1 || off[1].abs() > 0.15 {
                            Err(format!("igloo door drawn at {:.0}%, {:.0}% of the screen", s[0] / lw * 100.0, s[1] / lh * 100.0))
                        } else {
                            Ok(())
                        }
                    }
                };
                self.st_next(st, "camera starts on your igloo", r);
            }
            // Drag a box around our snowballers.
            1 => {
                let found = self.st_mine_on_screen(Kind::Snowballer);
                if st.sub == 0 {
                    if found.is_empty() {
                        self.st_next(st, "box select", Err("no snowballers on screen".into()));
                        return;
                    }
                    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                    for (_, s) in &found {
                        x0 = x0.min(s[0]);
                        y0 = y0.min(s[1]);
                        x1 = x1.max(s[0]);
                        y1 = y1.max(s[1]);
                    }
                    st.saved = vec![(x0 - 25.0) as u16, (y0 - 25.0) as u16, (x1 + 25.0) as u16, (y1 + 25.0) as u16];
                    st.before = found.len() as f32;
                    self.st_press(0, [x0 - 25.0, y0 - 25.0]);
                } else if st.sub < 6 {
                    let b = &st.saved;
                    let f = st.sub as f32 / 5.0;
                    let at = [b[0] as f32 + (b[2] - b[0]) as f32 * f, b[1] as f32 + (b[3] - b[1]) as f32 * f];
                    self.st_mouse(at);
                } else if st.sub == 6 {
                    self.st_release(0, [st.saved[2] as f32, st.saved[3] as f32]);
                } else {
                    let g = self.game.as_ref().unwrap();
                    let n = g.selected.len();
                    let you = g.view.info().you;
                    let all_mine = g.selected.iter().all(|&i| g.view.world.penguins[i as usize].owner == you);
                    let r = if n == 0 {
                        Err(format!("nothing selected ({} snowballers were in the box)", st.before))
                    } else if !all_mine {
                        Err("selected someone else's penguins".into())
                    } else {
                        Ok(())
                    };
                    self.st_next(st, "box select", r);
                    return;
                }
                st.sub += 1;
            }
            // Right-click the ground: the selection walks there.
            2 => {
                if st.sub == 0 {
                    let g = self.game.as_ref().unwrap();
                    let info = g.view.info();
                    let door = info.igloos[st.home as usize].door;
                    let mid = v2(info.map.w as f32 / 2.0, info.map.h as f32 / 2.0);
                    // Toward the middle, turned as little as it takes to land
                    // on open snow that isn't hidden under the HUD.
                    let (lw, lh) = self.logical_size();
                    let map = &info.map;
                    let base = (mid - door).norm();
                    let open = |p: Vec2| {
                        let (x, y) = (p.x as i32, p.y as i32);
                        map.in_bounds(x, y) && map.ground[map.idx(x, y)] == sim::map::Ground::Snow && map.obstacle[map.idx(x, y)] == sim::map::Obstacle::None
                    };
                    let clear = |s: [f32; 2]| s[0] > 280.0 && s[0] < lw - 240.0 && s[1] > 80.0 && s[1] < lh - 230.0;
                    st.target = [0.0f32, 0.5, -0.5, 1.0, -1.0, 1.6, -1.6, 2.2, -2.2, 3.1]
                        .iter()
                        .map(|&a| door + v2(base.x * a.cos() - base.y * a.sin(), base.x * a.sin() + base.y * a.cos()) * 7.0)
                        .find(|&t| open(t) && self.st_screen(t, 0.0).is_some_and(clear))
                        .unwrap_or(door + base * 7.0);
                    let g = self.game.as_ref().unwrap();
                    let Some(at) = self.st_screen(st.target, 0.0) else {
                        self.st_next(st, "right-click move order", Err("target off screen".into()));
                        return;
                    };
                    st.saved = g.selected.clone();
                    st.before = self.st_centroid(&st.saved).map_or(0.0, |c| c.dist(st.target));
                    self.st_press(1, at);
                } else if st.sub == 1 {
                    let at = self.input.mouse;
                    self.st_release(1, at);
                } else if st.t > 4.0 {
                    let after = self.st_centroid(&st.saved).map_or(f32::MAX, |c| c.dist(st.target));
                    let r = if after < st.before - 2.0 {
                        Ok(())
                    } else {
                        Err(format!("group was {:.1} tiles away, now {:.1}", st.before, after))
                    };
                    self.st_next(st, "right-click move order", r);
                    return;
                }
                st.sub += 1;
            }
            // Ctrl+1, Escape, 1: groups.
            3 => {
                let g = self.game.as_ref().unwrap();
                match st.sub {
                    0 => {
                        st.saved = g.selected.clone();
                        self.st_key(KeyCode::Key1, true);
                    }
                    1 => self.st_key(KeyCode::Escape, false),
                    2 => {
                        if !g.selected.is_empty() {
                            self.st_next(st, "Escape clears selection", Err(format!("{} still selected", g.selected.len())));
                        } else {
                            self.st_key(KeyCode::Key1, false);
                        }
                    }
                    _ => {
                        let mut a = g.selected.clone();
                        let mut b = st.saved.clone();
                        a.sort();
                        b.sort();
                        let r = if a == b && !a.is_empty() {
                            Ok(())
                        } else {
                            Err(format!("recalled {} of {} penguins", a.len(), b.len()))
                        };
                        self.st_next(st, "Ctrl+1 / 1 control groups", r);
                        return;
                    }
                }
                st.sub += 1;
            }
            // Hold W then D: the camera moves north, then east.
            4 => {
                let c = self.world_renderer().unwrap().center();
                match st.sub {
                    0 => {
                        st.target = c;
                        self.input.keys_down.insert(KeyCode::W);
                        st.sub = 1;
                    }
                    1 if st.t > 0.4 => {
                        self.input.keys_down.remove(&KeyCode::W);
                        let d = c - st.target;
                        if !(d.y < -1.0 && d.x.abs() < d.y.abs() * 0.2) {
                            self.st_next(st, "W scrolls north", Err(format!("moved by {:?}", d)));
                            return;
                        }
                        st.target = c;
                        self.input.keys_down.insert(KeyCode::D);
                        st.sub = 2;
                        st.t = 0.0;
                    }
                    2 if st.t > 0.4 => {
                        self.input.keys_down.remove(&KeyCode::D);
                        let d = c - st.target;
                        let r = if d.x > 1.0 && d.y.abs() < d.x.abs() * 0.2 { Ok(()) } else { Err(format!("D moved by {:?}", d)) };
                        self.st_next(st, "WASD scroll directions", r);
                    }
                    _ => {}
                }
            }
            // Click the minimap: the camera jumps there.
            5 => {
                let g = self.game.as_ref().unwrap();
                let map = &g.view.info().map;
                let mm = crate::game::Console::new(lw, lh, map.w, map.h, false).map;
                let at = [mm.x + mm.w * 0.5, mm.y + mm.h * 0.5];
                match st.sub {
                    0 => self.st_press(0, at),
                    1 => self.st_release(0, at),
                    _ => {
                        let c = self.world_renderer().unwrap().center();
                        let want = v2(map.w as f32 / 2.0, map.h as f32 / 2.0);
                        let r = if c.dist(want) < 4.0 { Ok(()) } else { Err(format!("camera at {:?}, wanted {:?}", c, want)) };
                        self.st_next(st, "minimap click moves camera", r);
                        return;
                    }
                }
                st.sub += 1;
            }
            // H selects (and jumps to) our igloo.
            6 => {
                let g = self.game.as_ref().unwrap();
                match st.sub {
                    0 => self.st_key(KeyCode::H, false),
                    1 => {
                        if g.selected_igloo() != Some(st.home) {
                            self.st_next(st, "H selects your igloo", Err(format!("selected {:?}", g.selected_igloo())));
                            return;
                        }
                        let c = self.world_renderer().unwrap().center();
                        let door = g.view.info().igloos[st.home as usize].door;
                        if c.dist(door) > 3.0 {
                            self.st_next(st, "H selects your igloo", Err("camera didn't jump to it".into()));
                            return;
                        }
                        self.st_next(st, "H selects your igloo", Ok(()));
                        return;
                    }
                    _ => return,
                }
                st.sub += 1;
            }
            // Click on the igloo dome (where players actually click).
            7 => {
                let g = self.game.as_ref().unwrap();
                let ig = &g.view.info().igloos[st.home as usize];
                let center = ig.center;
                match st.sub {
                    0 => self.st_key(KeyCode::Escape, false),
                    1 => {
                        let Some(at) = self.st_screen(center, crate::meshgen::IGLOO_HEIGHT * 0.7) else {
                            self.st_next(st, "click igloo dome selects it", Err("off screen".into()));
                            return;
                        };
                        self.st_press(0, at);
                    }
                    2 => {
                        let at = self.input.mouse;
                        self.st_release(0, at);
                    }
                    _ => {
                        let r = if g.selected_igloo() == Some(st.home) {
                            Ok(())
                        } else {
                            Err(format!("selected igloo {:?}, {} penguins", g.selected_igloo(), g.selected.len()))
                        };
                        self.st_next(st, "click igloo dome selects it", r);
                        return;
                    }
                }
                st.sub += 1;
            }
            // Mouse wheel zooms out, then back in.
            8 => {
                let r = self.world_renderer().unwrap();
                let ppt = r.pixels_per_tile(r.center());
                match st.sub {
                    0 => {
                        st.before = ppt;
                        self.st_mouse([lw / 2.0, lh / 2.0]);
                        self.input.wheel = -1.0;
                    }
                    1 => {
                        if ppt >= st.before * 0.95 {
                            let e = format!("wheel down: {:.1} -> {ppt:.1} px/tile", st.before);
                            self.st_next(st, "wheel zoom", Err(e));
                            return;
                        }
                        st.before = ppt;
                        self.input.wheel = 1.0;
                    }
                    _ => {
                        let res = if ppt > st.before * 1.05 { Ok(()) } else { Err(format!("wheel up: {:.1} -> {ppt:.1} px/tile", st.before)) };
                        self.st_next(st, "wheel zoom", res);
                        return;
                    }
                }
                st.sub += 1;
            }
            // Enter, type, Enter: chat reaches the server and comes back.
            9 => {
                let g = self.game.as_ref().unwrap();
                match st.sub {
                    0 => self.st_key(KeyCode::Enter, false),
                    1 => self.input.chars.extend("hello bots".chars()),
                    2 => self.st_key(KeyCode::Enter, false),
                    _ => {
                        if g.chat.iter().any(|c| c.text == "hello bots") {
                            self.st_next(st, "chat", Ok(()));
                        } else if st.t > 2.0 {
                            self.st_next(st, "chat", Err("message never came back".into()));
                        }
                        return;
                    }
                }
                st.sub += 1;
            }
            // Typing in chat must not also trigger hotkeys or scroll.
            10 => {
                let g = self.game.as_ref().unwrap();
                match st.sub {
                    0 => {
                        st.target = self.world_renderer().unwrap().center();
                        st.saved = g.selected.clone();
                        self.st_key(KeyCode::Enter, false);
                    }
                    1 => {
                        self.input.keys_down.insert(KeyCode::W);
                        self.st_key(KeyCode::W, false);
                        self.input.chars.push('w');
                    }
                    2..=10 => {}
                    11 => {
                        self.input.keys_down.remove(&KeyCode::W);
                        self.st_key(KeyCode::Escape, false);
                    }
                    _ => {
                        let moved = self.world_renderer().unwrap().center().dist(st.target);
                        let r = if moved < 0.01 { Ok(()) } else { Err(format!("camera moved {moved:.1} tiles while typing")) };
                        self.st_next(st, "typing in chat doesn't scroll", r);
                        return;
                    }
                }
                st.sub += 1;
            }
            // Click a worker, right-click a tree: it gets chopped down.
            11 => {
                let g = self.game.as_ref().unwrap();
                let map = &g.view.info().map;
                match st.sub {
                    0 => {
                        let door = g.view.info().igloos[st.home as usize].door;
                        let tree = (0..map.h)
                            .flat_map(|y| (0..map.w).map(move |x| (x, y)))
                            .filter(|&(x, y)| map.obstacle_at(x, y) == Obstacle::Tree)
                            .map(|(x, y)| v2(x as f32 + 0.5, y as f32 + 0.5))
                            // A lone worker sent farther than this takes minutes and gets frozen.
                            .filter(|t| t.dist(door) <= 14.0)
                            .min_by(|a, b| a.dist(door).partial_cmp(&b.dist(door)).unwrap());
                        match tree {
                            None => {
                                self.st_next(st, "chop a tree (skipped: no trees near your igloo)", Ok(()));
                                return;
                            }
                            Some(t) => {
                                st.target = t;
                                // A worker carries one wood per trip, so a far tree takes
                                // a round trip for each of its logs.
                                let trip = 2.0 * t.dist(door) / sim::game::Kind::Worker.stats().speed;
                                st.budget = 30.0 + sim::game::TREE_WOOD as f32 * (trip + sim::game::CHOP_TICKS as f32 / sim::TICK_RATE as f32);
                            }
                        }
                        self.st_key(KeyCode::H, false);
                    }
                    1 => self.st_key(KeyCode::Escape, false),
                    2 => {
                        let Some(&(_, at)) = self.st_mine_on_screen(Kind::Worker).first() else {
                            self.st_next(st, "chop a tree", Err("no workers on screen".into()));
                            return;
                        };
                        self.st_press(0, at);
                    }
                    3 => {
                        let at = self.input.mouse;
                        self.st_release(0, at);
                    }
                    4 => {
                        if g.selected.is_empty() {
                            self.st_next(st, "chop a tree", Err("clicking a worker didn't select it".into()));
                            return;
                        }
                        st.saved = g.selected.clone();
                        // Aim at the trunk, below the branches of the trees in front.
                        let (lw, lh) = self.logical_size();
                        // Not under the build panel, which a selected worker brings up.
                        let on_screen = |s: [f32; 2]| s[0] > 280.0 && s[0] < lw - 240.0 && s[1] > 80.0 && s[1] < lh - 350.0;
                        let at = self.st_screen(st.target, 0.2).filter(|&s| on_screen(s));
                        let Some(at) = at else {
                            // Far from the igloo: bring the tree into view, then try again.
                            let tree = st.target;
                            if st.t > 10.0 {
                                self.st_next(st, "chop a tree", Err("tree off screen".into()));
                                return;
                            }
                            self.renderer().unwrap().focus(tree);
                            return;
                        };
                        self.st_press(1, at);
                    }
                    5 => {
                        let at = self.input.mouse;
                        self.st_release(1, at);
                        st.t = 0.0;
                        st.before = 0.0;
                    }
                    _ => {
                        let w = &g.view.world;
                        if st.saved.iter().any(|&i| w.penguins[i as usize].act == Act::Chop) {
                            st.before = 1.0;
                        }
                        // The clicked tree or one of its neighbours is now a stump.
                        let (x, y) = st.target.tile();
                        let r = sim::game::CHOP_RADIUS.ceil() as i32;
                        let felled = (y - r..=y + r)
                            .flat_map(|yy| (x - r..=x + r).map(move |xx| (xx, yy)))
                            .filter(|&(xx, yy)| map.obstacle_at(xx, yy) == Obstacle::Stump)
                            .count();
                        if felled > 0 && st.before > 0.0 {
                            self.st_next(st, "right-click a tree: workers chop it down", Ok(()));
                        } else if st.t > st.budget {
                            let who: Vec<String> = st
                                .saved
                                .iter()
                                .map(|&i| {
                                    let p = &w.penguins[i as usize];
                                    format!("#{i} {:?} {:.1} tiles away{}{}", p.act, p.pos.dist(st.target), if p.cold { " cold" } else { "" }, if p.inside { " inside" } else { "" })
                                })
                                .collect();
                            let e = format!("after {:.0} s: clicked {:?} of {:?}, chopping seen {}, stumps {felled}; target {:?}; {}", st.budget, self.input.mouse, (lw, lh), st.before > 0.0, st.target.tile(), who.join(", "));
                            self.st_next(st, "right-click a tree: workers chop it down", Err(e));
                        }
                        return;
                    }
                }
                st.sub += 1;
            }
            // Select a worker, B then J, drag out a wall: it gets built.
            12 => {
                // The only wall is the tall one, on J.
                let (wall_kind, wall_key) = (sim::game::BuildKind::Palisade, KeyCode::J);
                let g = self.game.as_ref().unwrap();
                let you = g.view.info().you;
                let mine = |g: &crate::game::GameScreen| {
                    g.view.world.buildings.iter().filter(|b| b.alive && b.owner == you && b.kind == wall_kind).count()
                };
                match st.sub {
                    0 => self.st_key(KeyCode::H, false),
                    1 => self.st_key(KeyCode::Escape, false),
                    2 => {
                        let Some(&(_, at)) = self.st_mine_on_screen(Kind::Worker).first() else {
                            self.st_next(st, "build a wall", Err("no workers on screen".into()));
                            return;
                        };
                        self.st_press(0, at);
                    }
                    3 => {
                        let at = self.input.mouse;
                        self.st_release(0, at);
                    }
                    4 => self.st_key(KeyCode::B, false),
                    5 => {
                        if !g.build_menu {
                            self.st_next(st, "build a wall", Err("B didn't open the build menu".into()));
                            return;
                        }
                        self.st_key(wall_key, false);
                    }
                    6 => {
                        if g.placing.is_none() {
                            self.st_next(st, "build a wall", Err("the wall key didn't start placing a wall".into()));
                            return;
                        }
                        st.before = mine(g) as f32;
                        // A short line a few tiles from the door, away from the map edge.
                        let info = g.view.info();
                        let door = info.igloos[st.home as usize].door;
                        let mid = v2(info.map.w as f32 / 2.0, info.map.h as f32 / 2.0);
                        let dir = (mid - door).norm();
                        let side = v2(-dir.y, dir.x);
                        // The first line near the door where every tile is buildable and
                        // the whole drag is clear of the build panel, which takes the bottom
                        // of the screen and grows with the number of buildings.
                        let wall = wall_kind;
                        let (lw, lh) = self.logical_size();
                        let clear = |p: Vec2| self.st_screen(p, 0.0).is_some_and(|s| s[0] > 280.0 && s[0] < lw - 240.0 && s[1] > 80.0 && s[1] < lh - 350.0);
                        let free = |start: Vec2, step: Vec2| {
                            clear(start)
                                && clear(start + step * 3.0)
                                && (0..=3).all(|k| g.site_ok(wall, crate::game::GameScreen::site_at(wall, start + step * k as f32)))
                        };
                        let mut line = None;
                        'search: for turn in [0.0f32, 45.0, -45.0, 90.0, -90.0, 135.0, -135.0, 180.0] {
                            let (sn, cs) = turn.to_radians().sin_cos();
                            let d = v2(dir.x * cs - dir.y * sn, dir.x * sn + dir.y * cs);
                            let step = v2(-d.y, d.x);
                            for dist in [4.5f32, 6.0, 7.5, 3.5] {
                                let start = door + d * dist - step * 1.5;
                                if free(start, step) {
                                    line = Some((start, step));
                                    break 'search;
                                }
                            }
                        }
                        let (start, step) = line.unwrap_or((door + dir * 4.5 - side * 1.5, side));
                        st.target = start;
                        st.saved = vec![];
                        let end = st.target + step * 3.0;
                        let (Some(a), Some(b)) = (self.st_screen(st.target, 0.0), self.st_screen(end, 0.0)) else {
                            self.st_next(st, "build a wall", Err("site off screen".into()));
                            return;
                        };
                        st.saved = vec![b[0] as u16, b[1] as u16];
                        // Let the cursor settle on the start tile for a frame before pressing,
                        // or the drag may start from wherever the cursor was before.
                        if st.nudge == 0.0 {
                            st.nudge = 1.0;
                            self.st_mouse(a);
                            return;
                        }
                        st.nudge = 0.0;
                        self.st_press(0, a);
                    }
                    7 => {
                        let at = [st.saved[0] as f32, st.saved[1] as f32];
                        self.st_mouse(at);
                    }
                    8 => {
                        let ghosts = g.ghosts();
                        if ghosts.len() < 2 {
                            self.st_next(st, "build a wall", Err(format!("dragging shows {} wall segments (from {:?} at {:?} to {:?}, window {:?}, mouse {:?}, drag {:?})", ghosts.len(), st.target, self.st_screen(st.target, 0.0), st.saved, self.logical_size(), self.input.mouse, g.placing.map(|p| p.drag))));
                            return;
                        }
                        let at = self.input.mouse;
                        self.st_release(0, at);
                        st.t = 0.0;
                    }
                    _ => {
                        let w = &g.view.world;
                        let walls: Vec<_> = w.buildings.iter().filter(|b| b.alive && b.owner == you && b.kind == wall_kind).collect();
                        let placed = walls.len() as f32 > st.before;
                        if placed && walls.iter().all(|b| b.done()) {
                            self.st_next(st, "B, V (J), drag: workers build a snow wall", Ok(()));
                        } else if st.t > 120.0 {
                            let e = format!("{} walls placed, {} finished after 120 s", walls.len(), walls.iter().filter(|b| b.done()).count());
                            self.st_next(st, "B, V (J), drag: workers build a snow wall", Err(e));
                        }
                        return;
                    }
                }
                st.sub += 1;
            }
            // Camps: B, R puts up a Bunkhouse, which drafts a snowballer by itself.
            13 => {
                let g = self.game.as_ref().unwrap();
                let you = g.view.info().you;
                let camp_kind = sim::game::BuildKind::Bunkhouse;
                let name = "camp: a Bunkhouse goes up and drafts a snowballer";
                let range = g.view.world.buildings.iter().position(|b| b.alive && b.owner == you && b.kind == camp_kind);
                let snowballers = g.view.world.penguins.iter().filter(|p| p.owner == you && p.kind == Kind::Snowballer).count() as f32;
                match st.sub {
                    0 => {
                        // Nobody picked: B should call the workers up itself.
                        self.game.as_mut().unwrap().selected.clear();
                        self.st_key(KeyCode::B, false);
                    }
                    1 => {
                        if !g.build_menu {
                            self.st_next(st, name, Err("B didn't open the build menu".into()));
                            return;
                        }
                        self.st_key(KeyCode::R, false);
                    }
                    2 => {
                        if g.placing.is_none() {
                            self.st_next(st, name, Err("R didn't start placing a range".into()));
                            return;
                        }
                        let info = g.view.info();
                        let door = info.igloos[st.home as usize].door;
                        let mid = v2(info.map.w as f32 / 2.0, info.map.h as f32 / 2.0);
                        let dir = (mid - door).norm();
                        // The first spot near the door the game says is free.
                        let spots = [0.0f32, 45.0, -45.0, 90.0, -90.0, 135.0, -135.0, 180.0].into_iter().flat_map(|turn| {
                            let (sn, cs) = turn.to_radians().sin_cos();
                            let d = v2(dir.x * cs - dir.y * sn, dir.x * sn + dir.y * cs);
                            let side = v2(-d.y, d.x);
                            [5.0f32, 6.5, 8.0, 3.5].into_iter().flat_map(move |dist| [-4.0f32, 4.0, -6.0, 6.0, 0.0].map(move |o| door + d * dist + side * o))
                        });
                        let spot = spots.into_iter().filter(|&p| {
                            let t = crate::game::GameScreen::site_at(camp_kind, p);
                            // Builders dig the snow around the site, so it needs plenty of it
                            // (the wall before this already used up some by the door).
                            let map = &info.map;
                            let r = sim::game::QUARRY_RADIUS;
                            let snowy = (t.1 - r..=t.1 + r + 1)
                                .flat_map(|y| (t.0 - r..=t.0 + r + 1).map(move |x| (x, y)))
                                .filter(|&(x, y)| map.ground_at(x, y) == sim::map::Ground::Snow)
                                .count();
                            let (lw, lh) = self.logical_size();
                            let clear = self.st_screen(p, 0.0).is_some_and(|s| s[0] > 280.0 && s[0] < lw - 240.0 && s[1] > 80.0 && s[1] < lh - 350.0);
                            clear && snowy >= 20 && g.site_ok(camp_kind, t)
                        }).nth(st.site_try);
                        let Some(at) = spot.and_then(|p| self.st_screen(p, 0.0)) else {
                            self.st_next(st, name, Err("no free site on screen near the igloo".into()));
                            return;
                        };
                        st.before = snowballers;
                        self.st_mouse(at);
                    }
                    3 => {
                        let at = self.input.mouse;
                        self.st_press(0, at);
                    }
                    4 => {
                        let at = self.input.mouse;
                        self.st_release(0, at);
                        st.t = 0.0;
                    }
                    5 => {
                        // Wait for the range to be finished, then right-click it.
                        let Some(i) = range else {
                            // The server knows more about the ground than the client's guess:
                            // if it turned the spot down, try the next one.
                            if st.t > 6.0 && st.site_try < 5 && g.placing.is_none() {
                                st.site_try += 1;
                                st.sub = 0;
                                return;
                            }
                            if st.t > 20.0 {
                                self.st_next(st, name, Err(format!("no range was placed (clicked {:?} of {:?}, placing {}, selected {}, wood {})", self.input.mouse, self.logical_size(), g.placing.is_some(), g.selected.len(), g.view.world.wood_of(you))));
                            }
                            return;
                        };
                        let b = &g.view.world.buildings[i];
                        if !b.done() {
                            // A builder that got cold goes home and doesn't come back, so
                            // a player would send someone to finish the job.
                            if g.view.world.penguins.iter().any(|p| p.owner == you && p.act == Act::Build) {
                                st.nudge = st.t;
                            } else if st.t - st.nudge > 8.0 {
                                st.nudge = st.t;
                                st.sub = 10;
                                return;
                            }
                            if st.t > 150.0 {
                                self.st_next(st, name, Err(format!(
                                    "range at {}/{} after 150 s{}; at {:?}; builders {}",
                                    b.built,
                                    b.kind.snow(),
                                    if b.starved { ", out of snow" } else { "" },
                                    b.tile,
                                    g.selected
                                        .iter()
                                        .map(|&i| {
                                            let p = &g.view.world.penguins[i as usize];
                                            format!("#{i} {:?}{}{}", p.act, if p.cold { " cold" } else { "" }, if p.inside { " inside" } else { "" })
                                        })
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )));
                            }
                            return;
                        }
                        // Built: its bunks draft by themselves.
                        st.t = 0.0;
                        st.sub = 22;
                        return;
                    }
                    // Pick a worker again and send it to the range (below).
                    10 => {
                        let warm = self.st_mine_on_screen(Kind::Worker).into_iter().find(|&(id, _)| !g.view.world.penguins[id as usize].cold);
                        let Some((_, at)) = warm else {
                            if st.t > 150.0 {
                                self.st_next(st, name, Err("no warm workers on screen to send".into()));
                            }
                            return;
                        };
                        self.st_press(0, at);
                    }
                    11 => {
                        let at = self.input.mouse;
                        self.st_release(0, at);
                    }
                    12 => {
                        let Some(at) = range.and_then(|i| self.st_screen(g.view.world.buildings[i].center(), 0.0)) else {
                            self.st_next(st, name, Err("range off screen".into()));
                            return;
                        };
                        self.st_mouse(at);
                    }
                    13 => {
                        let at = self.input.mouse;
                        self.st_press(1, at);
                    }
                    14 => {
                        let at = self.input.mouse;
                        self.st_release(1, at);
                        // Back to waiting for the range.
                        st.sub = 4;
                    }
                    // Then: roll the wheel over the Bunkhouse's slots to
                    // want one fewer and back. The card takes the wheel, not the camera.
                    30 => {
                        let Some(i) = range else {
                            self.st_next(st, "wheel over a Bunkhouse's slots", Err("the Bunkhouse is gone".into()));
                            return;
                        };
                        st.before = g.view.world.buildings[i].want as f32;
                        st.target = v2(self.world_renderer().map_or(0.0, |r| r.pixels_per_tile(r.center())), 0.0);
                        let at = g.st_card_cell(lw, lh, 0);
                        self.game.as_mut().unwrap().st_select_building(i as u16);
                        self.st_mouse(at);
                    }
                    31 => {
                        self.input.wheel = -1.0;
                        st.t = 0.0;
                    }
                    32 | 33 => {
                        let want = range.map_or(0.0, |i| g.view.world.buildings[i].want as f32);
                        let goal = if st.sub == 32 { st.before - 1.0 } else { st.before };
                        if want == goal {
                            if st.sub == 32 {
                                self.input.wheel = 1.0;
                                st.t = 0.0;
                                st.sub += 1;
                                return;
                            }
                            let ppt = self.world_renderer().map_or(0.0, |r| r.pixels_per_tile(r.center()));
                            let res = if (ppt - st.target.x).abs() > 0.01 { Err(format!("the camera zoomed too: {:.1} -> {ppt:.1} px/tile", st.target.x)) } else { Ok(()) };
                            self.st_next(st, "wheel over a Bunkhouse's slots", res);
                        } else if st.t > 3.0 {
                            let e = format!("wanted {want} after the wheel, expected {goal} (selected {:?})", g.selected_building());
                            self.st_next(st, "wheel over a Bunkhouse's slots", Err(e));
                        }
                        return;
                    }
                    _ => {
                        if snowballers > st.before {
                            st.results.push((name, Ok(())));
                            st.sub = 30;
                            return;
                        } else if st.t > 25.0 && st.attempts < 3 {
                            // The order may have been lost (the worker went cold): send another.
                            st.sub = 22;
                            return;
                        } else if st.t > 40.0 {
                            let w = &g.view.world;
                            let centre = range.map(|i| w.buildings[i].center());
                            let who: Vec<String> = w
                                .penguins
                                .iter()
                                .filter(|p| p.owner == you && p.kind == Kind::Worker && centre.is_some_and(|c| p.pos.dist(c) < 6.0))
                                .map(|p| format!("#{} {:?} {:.1} away{}{}", p.id, p.act, centre.map_or(0.0, |c| p.pos.dist(c)), if p.cold { " cold" } else { "" }, if p.inside { " inside" } else { "" }))
                                .collect();
                            let ig = &w.igloos[st.home as usize];
                            let e = format!("no snowballer came out after {} tries; igloo {}f {}w tier {}; selected {}; range {:?}; workers near it: {}", st.attempts, ig.fish, ig.wood, ig.tier, g.selected.len(), centre, who.join(", "));
                            self.st_next(st, name, Err(e));
                        }
                        return;
                    }
                }
                st.sub += 1;
            }
            // Press K with an igloo selected;
            // the game must survive it, i.e. ticks keep coming.
            14 => {
                let g = self.game.as_ref().unwrap();
                match st.sub {
                    0 => self.st_key(KeyCode::H, false),
                    1 => {
                        if g.selected_igloo() != Some(st.home) {
                            self.st_next(st, "new core: K with igloo selected", Err(format!("H left igloo {:?} selected", g.selected_igloo())));
                            return;
                        }
                        st.before = g.view.world.tick as f32;
                        self.st_key(KeyCode::K, false);
                    }
                    _ => {
                        let ticks = g.view.world.tick as f32 - st.before;
                        if ticks >= 10.0 {
                            self.st_next(st, "new core: K with igloo selected", Ok(()));
                        } else if st.t > 5.0 {
                            self.st_next(st, "new core: K with igloo selected", Err(format!("only {ticks:.0} ticks came after K")));
                        }
                        return;
                    }
                }
                st.sub += 1;
            }
            // A phone: a tap selects, a drag pans, a hold gives an order.
            15 => {
                self.input.touch = true;
                let (cx, cy) = (lw * 0.5, lh * 0.4);
                match st.sub {
                    0 => {
                        self.game.as_mut().unwrap().selected.clear();
                        let found = self.st_mine_on_screen(Kind::Snowballer);
                        let Some(&(id, at)) = found.first() else {
                            self.st_next(st, "touch: tap, drag, hold", Err("no snowballers on screen".into()));
                            return;
                        };
                        st.saved = vec![id];
                        self.st_press(0, at);
                    }
                    1 => {
                        let at = self.input.mouse;
                        self.st_release(0, at);
                    }
                    2 => {
                        let sel = self.game.as_ref().unwrap().selected.clone();
                        if sel != st.saved {
                            self.input.touch = false;
                            self.st_next(st, "touch: tap, drag, hold", Err(format!("tap selected {sel:?}, wanted {:?}", st.saved)));
                            return;
                        }
                        st.target = self.world_renderer().unwrap().center();
                        self.st_press(0, [cx, cy]);
                    }
                    3..=5 => self.st_mouse([cx - 60.0 * (st.sub - 2) as f32, cy]),
                    6 => self.st_release(0, [cx - 180.0, cy]),
                    7 => {
                        let moved = self.world_renderer().unwrap().center().dist(st.target);
                        let sel = self.game.as_ref().unwrap().selected.clone();
                        if moved < 0.3 || sel != st.saved {
                            self.input.touch = false;
                            self.st_next(st, "touch: tap, drag, hold", Err(format!("drag moved camera {moved:.2} tiles, selection {sel:?}")));
                            return;
                        }
                        let c = self.st_centroid(&st.saved).unwrap_or(st.target);
                        let info = self.game.as_ref().unwrap().view.info();
                        let mid = v2(info.map.w as f32 / 2.0, info.map.h as f32 / 2.0);
                        // Open snow about six tiles off, towards the middle of
                        // the map if it's clear there, else round about; and
                        // somewhere on screen a finger can reach (not under
                        // the HUD).
                        let (lw, lh) = self.logical_size();
                        let clear = |p: Vec2| {
                            let ok = |x: i32, y: i32| info.map.in_bounds(x, y) && info.map.ground_at(x, y) != sim::map::Ground::Water && !info.map.obstacle_at(x, y).blocks();
                            let (x, y) = p.tile();
                            (-1..=1).all(|dx| (-1..=1).all(|dy| ok(x + dx, y + dy)))
                        };
                        let reachable = |p: Vec2| self.st_screen(p, 0.0).is_some_and(|s| s[0] > 300.0 && s[0] < lw - 260.0 && s[1] > 100.0 && s[1] < lh - 250.0);
                        let toward = (mid - c).angle();
                        st.target = (0..16)
                            .map(|k| c + Vec2::from_angle(toward + (k as f32 / 16.0) * std::f32::consts::TAU) * 6.0)
                            .find(|&p| clear(p) && reachable(p))
                            .unwrap_or(c + (mid - c).norm() * 6.0);
                        st.before = c.dist(st.target);
                        let Some(at) = self.st_screen(st.target, 0.0) else {
                            self.input.touch = false;
                            self.st_next(st, "touch: tap, drag, hold", Err("target off screen".into()));
                            return;
                        };
                        st.t = 0.0;
                        self.st_press(0, at);
                    }
                    8 => {
                        if st.t < 0.7 {
                            return;
                        }
                        let at = self.input.mouse;
                        self.st_release(0, at);
                        st.t = 0.0;
                    }
                    _ => {
                        if st.t > 4.0 {
                            let after = self.st_centroid(&st.saved).map_or(f32::MAX, |c| c.dist(st.target));
                            let r = if after < st.before - 2.0 { Ok(()) } else { Err(format!("walker was {:.1} tiles from the spot, now {:.1}; selected {:?}; others near the spot {:?} ({:?})", st.before, after, self.game.as_ref().unwrap().selected, self.game.as_ref().unwrap().view.world.penguins.iter().filter(|p| p.pos.dist(st.target) < 1.5).map(|p| (p.id, p.kind, p.owner)).collect::<Vec<_>>(), st.saved.first().and_then(|&id| self.game.as_ref().unwrap().view.world.penguins.get(id as usize)).map(|p| format!("{p:?}")))) };
                            self.input.touch = false;
                            self.st_next(st, "touch: tap, drag, hold", r);
                        }
                        return;
                    }
                }
                st.sub += 1;
            }
            _ => st.step = usize::MAX,
        }
    }
}
