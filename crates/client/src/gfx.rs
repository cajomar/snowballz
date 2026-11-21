//! Images, and the immediate-mode 2D batcher used for UI and text.

use std::collections::HashMap;

use crate::gpu::{bytes, Gpu, TextureId};
use crate::mathx::Mat4;
use crate::ui::Rect;

pub type Color = [u8; 4];

pub const WHITE: Color = [255, 255, 255, 255];

pub fn with_alpha(c: Color, a: f32) -> Color {
    [c[0], c[1], c[2], (c[3] as f32 * a.clamp(0.0, 1.0)) as u8]
}

/// How a texture behaves past its edges.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TextureWrap {
    Clamp,
    Repeat,
}

pub struct Image {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

pub fn load_png(path: &std::path::Path) -> Result<Image, String> {
    let bytes = crate::assets::read(path)?;
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&c| [c, c, c, 255]).collect(),
        png::ColorType::Indexed => return Err("unexpanded palette".into()),
    };
    Ok(Image { w: info.width, h: info.height, rgba })
}

pub fn save_png(path: &std::path::Path, img: &Image) -> std::io::Result<()> {
    let f = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), img.w, img.h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Best);
    let mut w = enc.write_header().map_err(std::io::Error::other)?;
    w.write_image_data(&img.rgba).map_err(std::io::Error::other)?;
    Ok(())
}

pub fn texture(ctx: &mut Gpu, img: &Image, mipmaps: bool, wrap: TextureWrap) -> TextureId {
    ctx.new_texture(img, mipmaps, wrap == TextureWrap::Repeat)
}

// ---------------------------------------------------------------------------
// 2D batch

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: Color,
    /// x: mode (0 tex*color, 1 sdf text, 2 team sprite), y: sdf softness,
    /// z: cold tint, w: alpha (team sprites; y is a white hit flash there).
    pub extra: [u8; 4],
}

pub const MODE_SDF: u8 = 1;

const BATCH_WGSL: &str = r#"
struct U { proj: mat4x4<f32> }
@group(0) @binding(0) var<uniform> u: U;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct V {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) extra: vec4<f32>,
}

@vertex
fn vs(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>, @location(3) extra: vec4<f32>) -> V {
    var o: V;
    o.pos = u.proj * vec4<f32>(pos, 0.0, 1.0);
    o.uv = uv;
    o.color = color;
    o.extra = extra;
    return o;
}

@fragment
fn fs(i: V) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, i.uv);
    // Team-coloured sprites keep their mask in the right half of the atlas.
    let m = textureSample(tex, samp, i.uv + vec2<f32>(0.5, 0.0));
    let mode = i.extra.x * 255.0;
    if (mode < 0.5) {
        return c * i.color;
    }
    if (mode < 1.5) {
        let s = max(i.extra.y, 0.004);
        let a = smoothstep(0.5 - s, 0.5 + s, c.a);
        return vec4<f32>(i.color.rgb, i.color.a * a);
    }
    var rgb = c.rgb * mix(vec3<f32>(1.0), i.color.rgb, m.r);
    let lum = dot(rgb, vec3<f32>(0.3, 0.55, 0.15));
    rgb = mix(rgb, vec3<f32>(0.62, 0.82, 1.0) * (0.35 + lum), i.extra.z);
    // The softness slot carries a white hit flash for team sprites.
    rgb = mix(rgb, vec3<f32>(1.0), i.extra.y * 0.7);
    return vec4<f32>(rgb, c.a * i.extra.w);
}
"#;

pub struct Batch {
    pipeline: wgpu::RenderPipeline,
    tex_layout: wgpu::BindGroupLayout,
    ubuf: wgpu::Buffer,
    ubind: wgpu::BindGroup,
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    verts: Vec<Vertex>,
    indices: Vec<u32>,
    /// Draws so far this frame: texture, first index, index count.
    segs: Vec<(TextureId, u32, u32)>,
    seg_start: u32,
    tex: Option<TextureId>,
    binds: HashMap<TextureId, wgpu::BindGroup>,
    proj: Mat4,
    /// Texture + uv rect of a solid white area, used for untextured shapes.
    /// Pointed at the font atlas once it loads so UI is one draw call.
    pub white: TextureId,
    pub white_uv: [f32; 4],
    pub draw_calls: u32,
    /// Tileable wood and paper for the HUD (see skin.rs).
    pub wood: TextureId,
    pub paper: TextureId,
}

/// A bind group layout for one filtered texture and its sampler.
pub fn texture_layout(gpu: &Gpu) -> wgpu::BindGroupLayout {
    gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("texture"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// A bind group layout for one uniform buffer.
pub fn uniform_layout(gpu: &Gpu, dynamic: bool) -> wgpu::BindGroupLayout {
    gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("uniforms"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: dynamic, min_binding_size: None },
            count: None,
        }],
    })
}

pub const ALPHA_BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::SrcAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

impl Batch {
    pub fn new(ctx: &mut Gpu) -> Batch {
        let device = &ctx.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("batch"), source: wgpu::ShaderSource::Wgsl(BATCH_WGSL.into()) });
        let ulayout = uniform_layout(ctx, false);
        let tex_layout = texture_layout(ctx);
        let layout = ctx.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("batch"),
            bind_group_layouts: &[Some(&ulayout), Some(&tex_layout)],
            immediate_size: 0,
        });
        let pipeline = ctx.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("batch"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4, 3 => Unorm8x4],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: ctx.surface_format, blend: Some(ALPHA_BLEND), write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let ubuf = ctx.buffer("batch uniforms", wgpu::BufferUsages::UNIFORM, bytes(&[Mat4::IDENTITY]));
        let ubind = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("batch uniforms"),
            layout: &ulayout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }],
        });
        let vbuf = ctx.buffer("batch vertices", wgpu::BufferUsages::VERTEX, &[]);
        let ibuf = ctx.buffer("batch indices", wgpu::BufferUsages::INDEX, &[]);
        let white = ctx.new_texture(&Image { w: 1, h: 1, rgba: vec![255; 4] }, false, false);
        let wood = texture(ctx, &crate::skin::wood(), true, TextureWrap::Repeat);
        let paper = texture(ctx, &crate::skin::paper(), true, TextureWrap::Repeat);
        Batch {
            pipeline,
            tex_layout,
            ubuf,
            ubind,
            vbuf,
            ibuf,
            verts: Vec::new(),
            indices: Vec::new(),
            segs: Vec::new(),
            seg_start: 0,
            tex: None,
            binds: HashMap::new(),
            proj: Mat4::IDENTITY,
            white,
            white_uv: [0.0, 0.0, 1.0, 1.0],
            draw_calls: 0,
            wood,
            paper,
        }
    }

    /// Start a frame's drawing with a projection.
    pub fn begin(&mut self, proj: Mat4) {
        self.proj = proj;
        self.verts.clear();
        self.indices.clear();
        self.segs.clear();
        self.seg_start = 0;
        self.tex = None;
    }

    /// Close the current draw (the next shape starts a new one).
    pub fn flush(&mut self, _ctx: &mut Gpu) {
        let n = self.indices.len() as u32;
        if let Some(tex) = self.tex {
            if n > self.seg_start {
                self.segs.push((tex, self.seg_start, n - self.seg_start));
                self.draw_calls += 1;
            }
        }
        self.seg_start = n;
        self.tex = None;
    }

    /// Draw everything batched this frame over the frame's picture.
    pub fn render(&mut self, ctx: &mut Gpu) {
        self.flush(ctx);
        if self.segs.is_empty() && ctx.frame.as_ref().is_some_and(|f| f.drawn) {
            return;
        }
        let vb = bytes(&self.verts);
        if vb.len() as u64 > self.vbuf.size() {
            self.vbuf = ctx.buffer("batch vertices", wgpu::BufferUsages::VERTEX, &vec![0u8; vb.len().next_power_of_two()]);
        }
        let ib = bytes(&self.indices);
        if ib.len() as u64 > self.ibuf.size() {
            self.ibuf = ctx.buffer("batch indices", wgpu::BufferUsages::INDEX, &vec![0u8; ib.len().next_power_of_two()]);
        }
        ctx.write(&self.vbuf, 0, vb);
        ctx.write(&self.ibuf, 0, ib);
        ctx.write(&self.ubuf, 0, bytes(&[self.proj]));
        // Bind groups for textures new this frame; forget deleted ones.
        self.binds.retain(|id, _| ctx.tex(*id).is_some());
        for &(id, _, _) in &self.segs {
            if self.binds.contains_key(&id) {
                continue;
            }
            let Some(t) = ctx.tex(id) else { continue };
            let bind = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.tex_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&t.view) },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(if t.repeat { &ctx.linear_repeat } else { &ctx.linear_clamp }),
                    },
                ],
            });
            self.binds.insert(id, bind);
        }
        let Some(f) = &mut ctx.frame else { return };
        let load = if f.drawn { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color { r: 0.35, g: 0.5, b: 0.7, a: 1.0 }) };
        f.drawn = true;
        let mut pass = f.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &f.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
        if self.segs.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.ubind, &[]);
        pass.set_vertex_buffer(0, self.vbuf.slice(..));
        pass.set_index_buffer(self.ibuf.slice(..), wgpu::IndexFormat::Uint32);
        for &(id, start, count) in &self.segs {
            let Some(b) = self.binds.get(&id) else { continue };
            pass.set_bind_group(1, b, &[]);
            pass.draw_indexed(start..start + count, 0, 0..1);
        }
    }

    /// Append raw geometry.
    pub fn push(&mut self, ctx: &mut Gpu, tex: TextureId, verts: &[Vertex], indices: &[u16]) {
        if self.tex != Some(tex) {
            self.flush(ctx);
            self.tex = Some(tex);
        }
        let base = self.verts.len() as u32;
        self.verts.extend_from_slice(verts);
        self.indices.extend(indices.iter().map(|&i| i as u32 + base));
    }

    /// Axis-aligned textured quad.
    #[allow(clippy::too_many_arguments)]
    pub fn quad(
        &mut self,
        ctx: &mut Gpu,
        tex: TextureId,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        uv: [f32; 4],
        color: Color,
        extra: [u8; 4],
    ) {
        let [u0, v0, u1, v1] = uv;
        let v = |px, py, u, vv| Vertex { pos: [px, py], uv: [u, vv], color, extra };
        self.push(
            ctx,
            tex,
            &[v(x, y, u0, v0), v(x + w, y, u1, v0), v(x + w, y + h, u1, v1), v(x, y + h, u0, v1)],
            &[0, 1, 2, 0, 2, 3],
        );
    }

    /// Quad from four arbitrary corners (clockwise from top-left).
    pub fn quad_pts(
        &mut self,
        ctx: &mut Gpu,
        tex: TextureId,
        p: [[f32; 2]; 4],
        uv: [f32; 4],
        color: Color,
        extra: [u8; 4],
    ) {
        let [u0, v0, u1, v1] = uv;
        let uvs = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
        let verts: Vec<Vertex> = (0..4).map(|i| Vertex { pos: p[i], uv: uvs[i], color, extra }).collect();
        self.push(ctx, tex, &verts, &[0, 1, 2, 0, 2, 3]);
    }

    pub fn rect(&mut self, ctx: &mut Gpu, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let (white, uv) = (self.white, self.white_uv);
        self.quad(ctx, white, x, y, w, h, uv, color, [0; 4]);
    }

    pub fn line(&mut self, ctx: &mut Gpu, a: [f32; 2], b: [f32; 2], width: f32, color: Color) {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let l = (dx * dx + dy * dy).sqrt().max(1e-4);
        let (nx, ny) = (-dy / l * width * 0.5, dx / l * width * 0.5);
        let (white, uv) = (self.white, self.white_uv);
        self.quad_pts(
            ctx,
            white,
            [[a[0] + nx, a[1] + ny], [b[0] + nx, b[1] + ny], [b[0] - nx, b[1] - ny], [a[0] - nx, a[1] - ny]],
            uv,
            color,
            [0; 4],
        );
    }

    pub fn rect_outline(&mut self, ctx: &mut Gpu, x: f32, y: f32, w: f32, h: f32, t: f32, color: Color) {
        self.rect(ctx, x, y, w, t, color);
        self.rect(ctx, x, y + h - t, w, t, color);
        self.rect(ctx, x, y + t, t, h - 2.0 * t, color);
        self.rect(ctx, x + w - t, y + t, t, h - 2.0 * t, color);
    }

    fn wc(&self) -> [f32; 2] {
        [(self.white_uv[0] + self.white_uv[2]) * 0.5, (self.white_uv[1] + self.white_uv[3]) * 0.5]
    }

    /// Filled rounded rectangle (relies on MSAA for smooth edges).
    pub fn round_rect(&mut self, ctx: &mut Gpu, r: Rect, radius: f32, color: Color) {
        let (white, c) = (self.white, self.wc());
        self.round_rect_uv(ctx, white, r, radius, color, |_| c);
    }

    /// Rounded rectangle filled with a repeating texture, `tile` pixels a repeat.
    pub fn round_rect_tex(&mut self, ctx: &mut Gpu, tex: TextureId, r: Rect, radius: f32, color: Color, tile: f32) {
        self.round_rect_uv(ctx, tex, r, radius, color, |p| [(p[0] - r.x) / tile, (p[1] - r.y) / tile]);
    }

    #[allow(clippy::too_many_arguments)]
    fn round_rect_uv(
        &mut self,
        ctx: &mut Gpu,
        tex: TextureId,
        r: Rect,
        radius: f32,
        color: Color,
        uv: impl Fn([f32; 2]) -> [f32; 2],
    ) {
        let (x, y, w, h) = (r.x, r.y, r.w, r.h);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
        let mut verts = Vec::with_capacity(32);
        let seg = 6;
        let corners = [
            (x + w - r, y + r, -std::f32::consts::FRAC_PI_2),
            (x + w - r, y + h - r, 0.0),
            (x + r, y + h - r, std::f32::consts::FRAC_PI_2),
            (x + r, y + r, std::f32::consts::PI),
        ];
        let c = [x + w / 2.0, y + h / 2.0];
        verts.push(Vertex { pos: c, uv: uv(c), color, extra: [0; 4] });
        for (cx, cy, a0) in corners {
            for s in 0..=seg {
                let a = a0 + s as f32 / seg as f32 * std::f32::consts::FRAC_PI_2;
                let p = [cx + a.cos() * r, cy + a.sin() * r];
                verts.push(Vertex { pos: p, uv: uv(p), color, extra: [0; 4] });
            }
        }
        let n = verts.len() as u16 - 1;
        let mut idx = Vec::with_capacity(n as usize * 3);
        for i in 0..n {
            idx.extend_from_slice(&[0, 1 + i, 1 + (i + 1) % n]);
        }
        self.push(ctx, tex, &verts, &idx);
    }

    /// Filled circle / ellipse.
    pub fn ellipse(&mut self, ctx: &mut Gpu, cx: f32, cy: f32, rx: f32, ry: f32, color: Color) {
        let seg = 20u16;
        let mut verts = vec![Vertex { pos: [cx, cy], uv: self.wc(), color, extra: [0; 4] }];
        for i in 0..seg {
            let a = i as f32 / seg as f32 * std::f32::consts::TAU;
            verts.push(Vertex { pos: [cx + a.cos() * rx, cy + a.sin() * ry], uv: self.wc(), color, extra: [0; 4] });
        }
        let mut idx = Vec::new();
        for i in 0..seg {
            idx.extend_from_slice(&[0, 1 + i, 1 + (i + 1) % seg]);
        }
        let white = self.white;
        self.push(ctx, white, &verts, &idx);
    }

    /// Ellipse outline ring.
    #[allow(clippy::too_many_arguments)]
    pub fn ring(&mut self, ctx: &mut Gpu, cx: f32, cy: f32, rx: f32, ry: f32, t: f32, color: Color) {
        let seg = 24u16;
        let mut verts = Vec::new();
        for i in 0..seg {
            let a = i as f32 / seg as f32 * std::f32::consts::TAU;
            let (c, s) = (a.cos(), a.sin());
            verts.push(Vertex { pos: [cx + c * rx, cy + s * ry], uv: self.wc(), color, extra: [0; 4] });
            verts.push(Vertex {
                pos: [cx + c * (rx - t), cy + s * (ry - t * ry / rx.max(0.01))],
                uv: self.wc(),
                color,
                extra: [0; 4],
            });
        }
        let mut idx = Vec::new();
        for i in 0..seg {
            let a = i * 2;
            let b = ((i + 1) % seg) * 2;
            idx.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
        let white = self.white;
        self.push(ctx, white, &verts, &idx);
    }
}

/// Screen-space orthographic projection (y down).
pub fn screen_proj(w: f32, h: f32) -> Mat4 {
    Mat4::ortho(0.0, w, h, 0.0, -1.0, 1.0)
}
