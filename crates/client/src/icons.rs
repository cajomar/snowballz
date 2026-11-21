//! Small vector icons drawn from the UI batch's shapes, so the HUD can
//! show a picture where it used to show a word. Each icon fits a square
//! `s` pixels wide centred on `(cx, cy)`.

use crate::gfx::Color;
use crate::ui::Ui;
use sim::game::BuildKind;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Icon {
    Fish,
    Crystal,
    Igloo,
    Penguin,
    Worker,
    Frost,
    Flame,
    Wall,
    Tower,
    Snowball,
    Store,
    Frozen,
    Shield,
    Sled,
    Skis,
    Sling,
    Pack,
    Heart,
    Camp,
    Crown,
    Hand,
    Stop,
    Horn,
    Up,
    Scroll,
    Door,
    Cross,
    Back,
    More,
    Hammer,
}

const OUTLINE: Color = [30, 40, 60, 255];
/// A cool grey edge so white snow shapes still show on paper.
const SNOW_EDGE: Color = [130, 150, 180, 255];

fn px(c: [u8; 3]) -> Color {
    [c[0], c[1], c[2], 255]
}

/// A triangle (a quad with one corner repeated).
fn tri(ui: &mut Ui, a: [f32; 2], b: [f32; 2], c: [f32; 2], col: Color) {
    let (white, uv) = (ui.batch.white, ui.batch.white_uv);
    ui.batch.quad_pts(ui.ctx, white, [a, b, c, c], uv, col, [0; 4]);
}

/// Draw `icon` centred at `(cx, cy)` in a box `s` wide.
pub fn draw(ui: &mut Ui, icon: Icon, cx: f32, cy: f32, s: f32) {
    let u = s / 16.0;
    let (x, y) = (cx - s / 2.0, cy - s / 2.0);
    let at = |dx: f32, dy: f32| [x + dx * u, y + dy * u];
    match icon {
        Icon::Fish => {
            let body = px([120, 190, 240]);
            tri(ui, at(11.0, 8.0), at(16.0, 3.0), at(16.0, 13.0), body);
            ui.batch.ellipse(ui.ctx, x + 7.0 * u, y + 8.0 * u, 6.5 * u, 4.2 * u, body);
            ui.batch.ellipse(ui.ctx, x + 4.5 * u, y + 7.0 * u, 0.9 * u, 0.9 * u, OUTLINE);
            ui.batch.line(ui.ctx, at(8.0, 5.0), at(8.0, 11.0), 0.8 * u, px([70, 130, 190]));
        }
        Icon::Crystal => {
            tri(ui, at(8.0, 0.5), at(3.0, 6.0), at(13.0, 6.0), px([210, 150, 255]));
            tri(ui, at(3.0, 6.0), at(13.0, 6.0), at(8.0, 15.5), px([160, 100, 230]));
            ui.batch.line(ui.ctx, at(8.0, 0.5), at(8.0, 15.5), 0.6 * u, px([240, 215, 255]));
        }
        Icon::Igloo => {
            ui.batch.ellipse(ui.ctx, x + 8.0 * u, y + 11.0 * u, 7.7 * u, 7.7 * u, SNOW_EDGE);
            ui.batch.ellipse(ui.ctx, x + 8.0 * u, y + 11.0 * u, 7.0 * u, 7.0 * u, px([226, 236, 248]));
            ui.batch.rect(ui.ctx, x + 6.0 * u, y + 10.5 * u, 4.0 * u, 5.0 * u, px([60, 76, 110]));
            ui.batch.line(ui.ctx, at(1.0, 12.0), at(15.0, 12.0), 0.7 * u, px([170, 190, 215]));
        }
        Icon::Penguin | Icon::Worker => {
            ui.batch.ellipse(ui.ctx, x + 8.0 * u, y + 9.0 * u, 5.0 * u, 6.5 * u, px([40, 48, 66]));
            ui.batch.ellipse(ui.ctx, x + 8.0 * u, y + 10.0 * u, 3.0 * u, 4.5 * u, px([240, 244, 250]));
            ui.batch.ellipse(ui.ctx, x + 8.0 * u, y + 4.0 * u, 3.2 * u, 3.0 * u, px([40, 48, 66]));
            tri(ui, at(6.5, 4.5), at(9.5, 4.5), at(8.0, 6.5), px([240, 170, 50]));
            if icon == Icon::Worker {
                // A work hat.
                ui.batch.rect(ui.ctx, x + 4.8 * u, y + 0.8 * u, 6.4 * u, 1.6 * u, px([240, 190, 60]));
            } else {
                // A scarf.
                ui.batch.rect(ui.ctx, x + 5.0 * u, y + 6.6 * u, 6.0 * u, 1.3 * u, px([220, 70, 60]));
            }
        }
        Icon::Frost | Icon::Frozen => {
            let c = if icon == Icon::Frozen { px([170, 220, 255]) } else { px([200, 230, 255]) };
            for k in 0..3 {
                let a = k as f32 * std::f32::consts::PI / 3.0;
                let (dx, dy) = (a.cos() * 7.0 * u, a.sin() * 7.0 * u);
                ui.batch.line(ui.ctx, [cx - dx, cy - dy], [cx + dx, cy + dy], 1.3 * u, c);
            }
        }
        Icon::Flame => {
            ui.batch.ellipse(ui.ctx, x + 8.0 * u, y + 10.5 * u, 4.8 * u, 5.0 * u, px([240, 120, 40]));
            tri(ui, at(8.0, 0.5), at(4.0, 9.0), at(12.0, 9.0), px([245, 150, 50]));
            ui.batch.ellipse(ui.ctx, x + 8.0 * u, y + 11.5 * u, 2.5 * u, 3.0 * u, px([255, 220, 100]));
        }
        Icon::Wall => {
            let b = px([225, 233, 245]);
            let h = 4.2 * u;
            // Three courses of bricks, the middle one offset.
            for (row, bricks) in [(0.0, [(1.0, 6.4), (8.6, 6.4)]), (5.4, [(0.5, 3.0), (4.4, 7.0)]), (10.8, [(1.0, 6.4), (8.6, 6.4)])] {
                for (bx, w) in bricks {
                    ui.batch.rect(ui.ctx, x + (bx - 0.4) * u, y + (row + 0.4) * u, (w + 0.8) * u, h + 0.8 * u, SNOW_EDGE);
                    ui.batch.rect(ui.ctx, x + bx * u, y + (row + 0.8) * u, w * u, h, b);
                }
            }
            ui.batch.rect(ui.ctx, x + 11.6 * u, y + 5.8 * u, 4.3 * u, h + 0.8 * u, SNOW_EDGE);
            ui.batch.rect(ui.ctx, x + 12.0 * u, y + 6.2 * u, 3.5 * u, h, b);
        }
        Icon::Tower => {
            let b = px([225, 233, 245]);
            ui.batch.rect(ui.ctx, x + 2.4 * u, y + 4.4 * u, 11.2 * u, 11.6 * u, SNOW_EDGE);
            for k in 0..3 {
                ui.batch.rect(ui.ctx, x + (1.4 + k as f32 * 4.5) * u, y + 0.9 * u, 4.2 * u, 5.2 * u, SNOW_EDGE);
            }
            ui.batch.rect(ui.ctx, x + 3.0 * u, y + 5.0 * u, 10.0 * u, 11.0 * u, b);
            for k in 0..3 {
                ui.batch.rect(ui.ctx, x + (2.0 + k as f32 * 4.5) * u, y + 1.5 * u, 3.0 * u, 4.0 * u, b);
            }
            ui.batch.rect(ui.ctx, x + 6.5 * u, y + 11.0 * u, 3.0 * u, 5.0 * u, px([60, 76, 110]));
        }
        Icon::Snowball => {
            ui.batch.ellipse(ui.ctx, cx, cy, 6.0 * u, 6.0 * u, px([245, 248, 255]));
            ui.batch.ring(ui.ctx, cx, cy, 6.0 * u, 6.0 * u, 0.8 * u, px([150, 175, 210]));
        }
        Icon::Store => {
            ui.batch.rect(ui.ctx, x + 1.0 * u, y + 6.0 * u, 14.0 * u, 9.0 * u, px([150, 106, 70]));
            ui.batch.line(ui.ctx, at(1.0, 9.5), at(15.0, 9.5), 0.8 * u, px([100, 66, 40]));
            ui.batch.line(ui.ctx, at(1.0, 12.5), at(15.0, 12.5), 0.8 * u, px([100, 66, 40]));
            tri(ui, at(4.0, 6.0), at(9.0, 6.0), at(6.5, 2.0), px([120, 190, 240]));
        }
        Icon::Shield => {
            ui.batch.ellipse(ui.ctx, x + 8.0 * u, y + 7.0 * u, 6.5 * u, 6.5 * u, px([120, 170, 225]));
            tri(ui, at(1.5, 8.0), at(14.5, 8.0), at(8.0, 15.5), px([120, 170, 225]));
            ui.batch.line(ui.ctx, at(8.0, 2.0), at(8.0, 13.0), 0.9 * u, px([225, 236, 250]));
            ui.batch.line(ui.ctx, at(3.0, 7.0), at(13.0, 7.0), 0.9 * u, px([225, 236, 250]));
        }
        Icon::Sled => {
            ui.batch.rect(ui.ctx, x + 2.0 * u, y + 5.0 * u, 11.0 * u, 5.0 * u, px([190, 90, 70]));
            ui.batch.line(ui.ctx, at(0.5, 13.0), at(14.0, 13.0), 1.1 * u, px([150, 170, 200]));
            ui.batch.line(ui.ctx, at(14.0, 13.0), at(15.5, 10.5), 1.1 * u, px([150, 170, 200]));
            ui.batch.line(ui.ctx, at(4.0, 10.0), at(4.0, 13.0), 0.8 * u, px([150, 170, 200]));
            ui.batch.line(ui.ctx, at(11.0, 10.0), at(11.0, 13.0), 0.8 * u, px([150, 170, 200]));
        }
        Icon::Skis => {
            ui.batch.line(ui.ctx, at(1.0, 14.0), at(13.0, 3.0), 1.4 * u, px([220, 140, 60]));
            ui.batch.line(ui.ctx, at(4.5, 15.0), at(15.0, 5.5), 1.4 * u, px([220, 140, 60]));
            ui.batch.line(ui.ctx, at(10.0, 4.0), at(6.0, 12.0), 0.7 * u, px([225, 236, 250]));
        }
        Icon::Sling => {
            ui.batch.ring(ui.ctx, x + 6.0 * u, y + 9.0 * u, 3.0 * u, 5.5 * u, 1.0 * u, px([170, 120, 80]));
            ui.batch.line(ui.ctx, at(6.0, 3.5), at(13.5, 1.5), 0.8 * u, px([170, 120, 80]));
            ui.batch.ellipse(ui.ctx, x + 14.0 * u, y + 2.0 * u, 2.0 * u, 2.0 * u, px([245, 248, 255]));
        }
        Icon::Pack => {
            ui.batch.ellipse(ui.ctx, x + 5.5 * u, y + 11.0 * u, 4.0 * u, 4.0 * u, px([245, 248, 255]));
            ui.batch.ellipse(ui.ctx, x + 11.0 * u, y + 11.0 * u, 4.0 * u, 4.0 * u, px([235, 242, 252]));
            ui.batch.ellipse(ui.ctx, x + 8.0 * u, y + 5.5 * u, 4.0 * u, 4.0 * u, px([245, 248, 255]));
            ui.batch.ring(ui.ctx, x + 8.0 * u, y + 5.5 * u, 4.0 * u, 4.0 * u, 0.6 * u, px([150, 175, 210]));
        }
        Icon::Heart => {
            ui.batch.ellipse(ui.ctx, x + 5.0 * u, y + 5.5 * u, 3.8 * u, 3.8 * u, px([230, 90, 110]));
            ui.batch.ellipse(ui.ctx, x + 11.0 * u, y + 5.5 * u, 3.8 * u, 3.8 * u, px([230, 90, 110]));
            tri(ui, at(1.4, 7.5), at(14.6, 7.5), at(8.0, 15.0), px([230, 90, 110]));
        }
        Icon::Crown => {
            ui.batch.ellipse(ui.ctx, x + 8.0 * u, y + 11.0 * u, 5.0 * u, 4.5 * u, px([40, 48, 66]));
            tri(ui, at(2.0, 8.0), at(2.0, 2.0), at(6.0, 6.0), px([245, 200, 60]));
            tri(ui, at(6.0, 6.0), at(8.0, 1.0), at(10.0, 6.0), px([245, 200, 60]));
            tri(ui, at(10.0, 6.0), at(14.0, 2.0), at(14.0, 8.0), px([245, 200, 60]));
            ui.batch.rect(ui.ctx, x + 2.0 * u, y + 6.5 * u, 12.0 * u, 2.0 * u, px([245, 200, 60]));
        }
        Icon::Camp => {
            tri(ui, at(8.0, 1.5), at(0.5, 14.5), at(15.5, 14.5), px([210, 120, 80]));
            tri(ui, at(8.0, 7.0), at(5.5, 14.5), at(10.5, 14.5), px([70, 50, 40]));
        }
        Icon::Hand => {
            // An open flipper held up: stay put.
            let c = px([40, 48, 66]);
            ui.batch.ellipse(ui.ctx, x + 8.0 * u, y + 10.5 * u, 5.0 * u, 5.0 * u, c);
            for (k, h) in [(4.0, 6.0), (6.7, 3.5), (9.3, 3.0), (12.0, 5.0)] {
                ui.batch.line(ui.ctx, at(k, 10.0), at(k, h), 2.2 * u, c);
            }
            ui.batch.line(ui.ctx, at(3.5, 12.0), at(1.0, 8.5), 2.0 * u, c);
        }
        Icon::Stop => {
            let c = px([200, 64, 52]);
            ui.batch.round_rect(ui.ctx, crate::ui::rect(x + 2.0 * u, y + 2.0 * u, 12.0 * u, 12.0 * u), 3.0 * u, c);
            ui.batch.rect(ui.ctx, x + 4.5 * u, y + 7.0 * u, 7.0 * u, 2.0 * u, px([250, 240, 230]));
        }
        Icon::Horn => {
            let c = px([226, 196, 120]);
            tri(ui, at(1.0, 6.5), at(1.0, 9.5), at(14.0, 1.5), c);
            tri(ui, at(1.0, 9.5), at(14.0, 14.5), at(14.0, 1.5), c);
            ui.batch.ellipse(ui.ctx, x + 14.0 * u, y + 8.0 * u, 1.8 * u, 6.5 * u, px([150, 110, 60]));
            ui.batch.line(ui.ctx, at(5.0, 5.5), at(5.0, 10.5), 0.8 * u, px([150, 110, 60]));
        }
        Icon::Up => {
            let c = px([80, 150, 70]);
            tri(ui, at(8.0, 1.0), at(1.5, 8.5), at(14.5, 8.5), c);
            ui.batch.rect(ui.ctx, x + 5.0 * u, y + 8.0 * u, 6.0 * u, 7.0 * u, c);
        }
        Icon::Scroll => {
            ui.batch.rect(ui.ctx, x + 3.0 * u, y + 2.5 * u, 10.0 * u, 11.0 * u, px([240, 222, 180]));
            ui.batch.ellipse(ui.ctx, x + 3.0 * u, y + 8.0 * u, 1.6 * u, 6.0 * u, px([190, 160, 110]));
            ui.batch.ellipse(ui.ctx, x + 13.0 * u, y + 8.0 * u, 1.6 * u, 6.0 * u, px([190, 160, 110]));
            for k in 0..3 {
                let ly = 5.5 + k as f32 * 2.5;
                ui.batch.line(ui.ctx, at(5.5, ly), at(10.5, ly), 0.7 * u, px([120, 96, 70]));
            }
        }
        Icon::Door => {
            ui.batch.ellipse(ui.ctx, x + 8.0 * u, y + 11.0 * u, 7.0 * u, 7.0 * u, px([226, 236, 248]));
            ui.batch.rect(ui.ctx, x + 5.5 * u, y + 8.0 * u, 5.0 * u, 8.0 * u, px([60, 76, 110]));
            tri(ui, at(9.0, 11.0), at(15.5, 11.0), at(12.5, 8.0), px([80, 150, 70]));
            tri(ui, at(9.0, 11.0), at(15.5, 11.0), at(12.5, 14.0), px([80, 150, 70]));
        }
        Icon::Cross => {
            let c = px([200, 64, 52]);
            ui.batch.line(ui.ctx, at(3.0, 3.0), at(13.0, 13.0), 2.6 * u, c);
            ui.batch.line(ui.ctx, at(13.0, 3.0), at(3.0, 13.0), 2.6 * u, c);
        }
        Icon::Back | Icon::More => {
            let c = px([110, 76, 44]);
            let (a, b) = if icon == Icon::Back { (2.0, 10.0) } else { (14.0, 6.0) };
            tri(ui, at(a, 8.0), at(b, 2.0), at(b, 14.0), c);
            let bx = if icon == Icon::Back { 9.0 } else { 2.0 };
            ui.batch.rect(ui.ctx, x + bx * u, y + 6.0 * u, 5.0 * u, 4.0 * u, c);
        }
        Icon::Hammer => {
            ui.batch.line(ui.ctx, at(3.0, 14.0), at(10.0, 6.0), 1.8 * u, px([150, 106, 70]));
            let head = px([110, 118, 132]);
            tri(ui, at(7.0, 3.0), at(11.0, 0.5), at(15.5, 6.0), head);
            tri(ui, at(7.0, 3.0), at(15.5, 6.0), at(12.0, 8.5), head);
        }
    }
}

/// The icon that stands for a building.
pub fn for_build(kind: BuildKind) -> Icon {
    match kind {
        BuildKind::Sauna | BuildKind::Campfire => Icon::Flame,
        BuildKind::Palisade => Icon::Wall,
        BuildKind::Tower | BuildKind::Catapult => Icon::Tower,
        BuildKind::Store => Icon::Store,
        BuildKind::Bunkhouse | BuildKind::Igloo => Icon::Igloo,
        BuildKind::Workshop => Icon::Shield,
        BuildKind::Track => Icon::Sled,
        BuildKind::SkiLodge => Icon::Skis,
        BuildKind::SlingRange => Icon::Sling,
        BuildKind::PackerYard => Icon::Pack,
        BuildKind::Spa => Icon::Heart,
    }
}

/// The icon that stands for a kind of penguin.
pub fn for_kind(kind: sim::game::Kind) -> Icon {
    use sim::game::Kind;
    match kind {
        Kind::Worker => Icon::Worker,
        Kind::Snowballer => Icon::Penguin,
        Kind::Slinger => Icon::Sling,
        Kind::Packer => Icon::Pack,
        Kind::Sledder => Icon::Sled,
        Kind::Shield => Icon::Shield,
        Kind::Chieftain => Icon::Crown,
        Kind::Warmer => Icon::Flame,
        Kind::Skier => Icon::Skis,
    }
}
