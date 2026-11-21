//! Signed-distance-field text, baked by tools/bake_font.py.

use std::collections::HashMap;

use crate::gfx::TextureWrap;
use crate::gpu::{Gpu, TextureId};

use crate::gfx::{self, Batch, Color, Vertex, MODE_SDF};

#[derive(Clone, Copy)]
struct Glyph {
    uv: [f32; 4],
    w: f32,
    h: f32,
    xoff: f32,
    yoff: f32,
    advance: f32,
}

pub struct Font {
    pub tex: TextureId,
    glyphs: HashMap<char, Glyph>,
    size: f32,
    ascent: f32,
    spread: f32,
    pub white_uv: [f32; 4],
}

#[derive(Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

impl Font {
    pub fn load(ctx: &mut Gpu, base: &std::path::Path) -> Result<Font, String> {
        let mut img = gfx::load_png(&base.with_extension("png"))?;
        let txt = crate::assets::read_string(&base.with_extension("txt"))?;
        let mut lines = txt.lines();
        let header: Vec<&str> = lines.next().ok_or("empty font")?.split_whitespace().collect();
        let num = |i: usize| header.get(i).and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
        let (size, ascent, spread) = (num(1), num(5), num(7));
        let (aw, ah) = (img.w as f32, img.h as f32);
        let mut glyphs = HashMap::new();
        for l in lines {
            let v: Vec<f32> = l.split_whitespace().filter_map(|s| s.parse().ok()).collect();
            if v.len() < 8 {
                continue;
            }
            let Some(ch) = char::from_u32(v[0] as u32) else { continue };
            glyphs.insert(
                ch,
                Glyph {
                    uv: [v[1] / aw, v[2] / ah, (v[1] + v[3]) / aw, (v[2] + v[4]) / ah],
                    w: v[3],
                    h: v[4],
                    xoff: v[5],
                    yoff: v[6],
                    advance: v[7],
                },
            );
        }
        // Stamp a solid white block in the bottom-right corner for shapes.
        for y in img.h - 4..img.h {
            for x in img.w - 4..img.w {
                let i = ((y * img.w + x) * 4) as usize;
                img.rgba[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        let white_uv = [(aw - 3.0) / aw, (ah - 3.0) / ah, (aw - 1.0) / aw, (ah - 1.0) / ah];
        let tex = gfx::texture(ctx, &img, false, TextureWrap::Clamp);
        Ok(Font { tex, glyphs, size, ascent, spread, white_uv })
    }

    pub fn measure(&self, text: &str, px: f32) -> f32 {
        let s = px / self.size;
        text.chars().map(|c| self.glyph(c).map_or(0.0, |g| g.advance * s)).sum()
    }

    fn glyph(&self, c: char) -> Option<&Glyph> {
        self.glyphs.get(&c).or_else(|| self.glyphs.get(&'?'))
    }

    /// Draw text with its top-left at (x, y). Returns the width drawn.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        ctx: &mut Gpu,
        batch: &mut Batch,
        text: &str,
        x: f32,
        y: f32,
        px: f32,
        color: Color,
        align: Align,
    ) -> f32 {
        let s = px / self.size;
        let width = self.measure(text, px);
        let mut pen = match align {
            Align::Left => x,
            Align::Center => x - width / 2.0,
            Align::Right => x - width,
        };
        // Edge softness: about one screen pixel in distance-field units.
        let soft = (0.75 / (s * self.spread * 2.0)).clamp(0.01, 0.5);
        let extra = [MODE_SDF, (soft * 255.0) as u8, 0, 0];
        let base_y = y + (self.ascent - self.size) * s * 0.0;
        let mut verts: Vec<Vertex> = Vec::with_capacity(text.len() * 4);
        let mut idx: Vec<u16> = Vec::with_capacity(text.len() * 6);
        for c in text.chars() {
            let Some(g) = self.glyph(c) else { continue };
            if g.w > 0.0 {
                let gx = (pen + g.xoff * s).round();
                let gy = base_y + g.yoff * s;
                let (w, h) = (g.w * s, g.h * s);
                let b = verts.len() as u16;
                let [u0, v0, u1, v1] = g.uv;
                verts.push(Vertex { pos: [gx, gy], uv: [u0, v0], color, extra });
                verts.push(Vertex { pos: [gx + w, gy], uv: [u1, v0], color, extra });
                verts.push(Vertex { pos: [gx + w, gy + h], uv: [u1, v1], color, extra });
                verts.push(Vertex { pos: [gx, gy + h], uv: [u0, v1], color, extra });
                idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
                if verts.len() > 4000 {
                    batch.push(ctx, self.tex, &verts, &idx);
                    verts.clear();
                    idx.clear();
                }
            }
            pen += g.advance * s;
        }
        batch.push(ctx, self.tex, &verts, &idx);
        width
    }

    /// Text with a soft dark drop shadow, for legibility over the world.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_shadowed(
        &self,
        ctx: &mut Gpu,
        batch: &mut Batch,
        text: &str,
        x: f32,
        y: f32,
        px: f32,
        color: Color,
        align: Align,
    ) -> f32 {
        let off = (px / 16.0).max(1.0);
        let shadow = [10, 20, 40, (color[3] as f32 * 0.6) as u8];
        self.draw(ctx, batch, text, x + off, y + off, px, shadow, align);
        self.draw(ctx, batch, text, x, y, px, color, align)
    }

    /// Word-wrap `text` to `max_w` pixels.
    pub fn wrap(&self, text: &str, px: f32, max_w: f32) -> Vec<String> {
        let mut out = Vec::new();
        for para in text.split('\n') {
            let mut line = String::new();
            for word in para.split(' ') {
                let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
                if self.measure(&candidate, px) > max_w && !line.is_empty() {
                    out.push(std::mem::take(&mut line));
                    line = word.to_string();
                } else {
                    line = candidate;
                }
            }
            out.push(line);
        }
        out
    }
}
