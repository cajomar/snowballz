//! Post-processing: what turns the lit world into the finished picture.
//!
//! The world is drawn (4x multisampled where the GPU allows) into a
//! high-range picture. Then:
//!
//! - **Ambient occlusion**: from the depth buffer, at half size, darkens
//!   creases and the ground under and between things, so penguins stand on
//!   the snow instead of floating over it.
//! - **Bloom**: whatever is brighter than white (sun glints on ice, fire,
//!   sparkle) glows, through a chain of ever smaller blurred copies.
//! - **Finish**: tone mapping that leaves ordinary colours alone and rolls
//!   highlights off softly, a light colour grade (cool shadows, warm
//!   highlights), a vignette, and dithering against banding.

use crate::gpu::{bytes, Gpu};
use crate::mathx::Mat4;

const BLOOM_LEVELS: usize = 5;
const AO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
pub const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PostUniforms {
    /// View-space from clip space, and back (for ambient occlusion).
    pub inv_proj: Mat4,
    pub proj: Mat4,
    /// AO radius (world units), AO strength, bloom strength, bloom threshold.
    pub params: [f32; 4],
    /// Exposure, vignette, polar night (0..1), time.
    pub look: [f32; 4],
}

const SHADER: &str = r#"
struct Post {
    inv_proj: mat4x4<f32>,
    proj: mat4x4<f32>,
    params: vec4<f32>,
    look: vec4<f32>,
}
@group(0) @binding(0) var<uniform> u: Post;
@group(0) @binding(1) var tex_a: texture_2d<f32>;
@group(0) @binding(2) var tex_b: texture_2d<f32>;
@group(0) @binding(3) var tex_c: texture_2d<f32>;
@group(0) @binding(4) var lin: sampler;

struct POut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs(@builtin(vertex_index) v: u32) -> POut {
    let p = vec2<f32>(f32(v & 1u) * 4.0 - 1.0, f32(v >> 1u) * 4.0 - 1.0);
    var o: POut;
    o.clip = vec4<f32>(p, 0.0, 1.0);
    o.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return o;
}

fn texel(t: texture_2d<f32>) -> vec2<f32> {
    return 1.0 / vec2<f32>(textureDimensions(t));
}
fn lum(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// --- Bloom: a bright pass, then down and back up a chain of half sizes.

@fragment
fn fs_bright(i: POut) -> @location(0) vec4<f32> {
    let t = texel(tex_a);
    var c = vec3<f32>(0.0);
    for (var y = -1; y <= 1; y += 2) {
        for (var x = -1; x <= 1; x += 2) {
            let s = textureSampleLevel(tex_a, lin, i.uv + vec2<f32>(f32(x), f32(y)) * t, 0.0).rgb;
            // Each tap weighted down by its own brightness, so one hot pixel
            // doesn't flicker into a big blob.
            c += s / (1.0 + lum(s) * 0.25);
        }
    }
    c *= 0.25;
    let l = lum(c);
    let knee = 0.25;
    let soft = clamp(l - u.params.w + knee, 0.0, 2.0 * knee);
    let w = max(soft * soft / (4.0 * knee + 1e-4), l - u.params.w) / max(l, 1e-4);
    return vec4<f32>(c * max(w, 0.0), 1.0);
}

@fragment
fn fs_down(i: POut) -> @location(0) vec4<f32> {
    let t = texel(tex_a);
    var c = textureSampleLevel(tex_a, lin, i.uv, 0.0).rgb * 0.5;
    c += textureSampleLevel(tex_a, lin, i.uv + vec2<f32>(-t.x, -t.y), 0.0).rgb * 0.125;
    c += textureSampleLevel(tex_a, lin, i.uv + vec2<f32>(t.x, -t.y), 0.0).rgb * 0.125;
    c += textureSampleLevel(tex_a, lin, i.uv + vec2<f32>(-t.x, t.y), 0.0).rgb * 0.125;
    c += textureSampleLevel(tex_a, lin, i.uv + vec2<f32>(t.x, t.y), 0.0).rgb * 0.125;
    return vec4<f32>(c, 1.0);
}

@fragment
fn fs_up(i: POut) -> @location(0) vec4<f32> {
    let t = texel(tex_a);
    var c = vec3<f32>(0.0);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let w = (2.0 - abs(f32(x))) * (2.0 - abs(f32(y))) / 16.0;
            c += textureSampleLevel(tex_a, lin, i.uv + vec2<f32>(f32(x), f32(y)) * t, 0.0).rgb * w;
        }
    }
    return vec4<f32>(c, 1.0);
}

// --- The finished picture.

// Khronos PBR Neutral: ordinary colours pass through untouched, only the
// highlights are rolled off (and gently desaturated) toward white.
fn neutral(c_in: vec3<f32>) -> vec3<f32> {
    let start = 0.76;
    let desat = 0.15;
    var c = c_in;
    let x = min(c.r, min(c.g, c.b));
    let offset = select(0.04, x - 6.25 * x * x, x < 0.08);
    c -= offset;
    let peak = max(c.r, max(c.g, c.b));
    if (peak < start) { return c; }
    let d = 1.0 - start;
    let new_peak = 1.0 - d * d / (peak + d - start);
    c *= new_peak / peak;
    let g = 1.0 - 1.0 / (desat * (peak - new_peak) + 1.0);
    return mix(c, vec3<f32>(new_peak), g);
}

// The AO's 4x4 noise, averaged away: a 4x4 box holds every turn once.
@fragment
fn fs_ao_blur(i: POut) -> @location(0) vec4<f32> {
    let dim = vec2<i32>(textureDimensions(tex_a));
    let px = vec2<i32>(i.clip.xy);
    var a = 0.0;
    for (var y = -2; y < 2; y++) {
        for (var x = -2; x < 2; x++) {
            a += textureLoad(tex_a, clamp(px + vec2<i32>(x, y), vec2<i32>(0), dim - 1), 0).r;
        }
    }
    return vec4<f32>(a / 16.0, 0.0, 0.0, 1.0);
}

fn ao_at(uv: vec2<f32>) -> f32 {
    return textureSampleLevel(tex_c, lin, uv, 0.0).r;
}

// Tilt-shift: the top and bottom of the screen go softly out of focus,
// so the battlefield looks like a model under glass.
fn tilt_shift(uv: vec2<f32>) -> vec3<f32> {
    let centre = textureSampleLevel(tex_a, lin, uv, 0.0).rgb;
    let r = ss_(0.2, 0.5, abs(uv.y - 0.52)) * 4.5;
    if (r < 0.3) { return centre; }
    let t = texel(tex_a);
    var c = centre;
    var w = 1.0;
    // A golden-angle spiral of taps over a disc `r` pixels wide.
    for (var k = 0; k < 12; k++) {
        let f = f32(k) + 0.5;
        let a = f * 2.39996;
        let d = sqrt(f / 12.0) * r;
        c += textureSampleLevel(tex_a, lin, uv + vec2<f32>(cos(a), sin(a)) * d * t, 0.0).rgb;
        w += 1.0;
    }
    return c / w;
}
fn ss_(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

@fragment
fn fs_finish(i: POut) -> @location(0) vec4<f32> {
    var col = tilt_shift(i.uv);
    let ao = ao_at(i.uv);
    col *= mix(1.0, ao, u.params.y);
    col += textureSampleLevel(tex_b, lin, i.uv, 0.0).rgb * u.params.z;
    col *= u.look.x;
    col = neutral(max(col, vec3<f32>(0.0)));
    // Grade: a touch more colour, shadows toward blue, highlights warm.
    let l = lum(col);
    col = mix(vec3<f32>(l), col, 1.08);
    col += (vec3<f32>(-0.012, 0.0, 0.025) * (1.0 - l) + vec3<f32>(0.012, 0.006, -0.012) * l) * (1.0 - u.look.z * 0.5);
    // Vignette.
    let q = i.uv - 0.5;
    col *= 1.0 - u.look.y * dot(q, q) * 1.6;
    // Dither: no banding in the sky and the snow.
    let n = fract(52.9829189 * fract(dot(i.clip.xy, vec2<f32>(0.06711056, 0.00583715))));
    col += (n - 0.5) / 255.0;
    return vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
"#;

/// Ambient occlusion, from the depth buffer. DEPTH_T and the loads are
/// swapped for the multisampled kind when the world is drawn that way.
const AO_SHADER: &str = r#"
struct Post {
    inv_proj: mat4x4<f32>,
    proj: mat4x4<f32>,
    params: vec4<f32>,
    look: vec4<f32>,
}
@group(0) @binding(0) var<uniform> u: Post;
@group(0) @binding(1) var depth: DEPTH_T;

struct POut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs(@builtin(vertex_index) v: u32) -> POut {
    let p = vec2<f32>(f32(v & 1u) * 4.0 - 1.0, f32(v >> 1u) * 4.0 - 1.0);
    var o: POut;
    o.clip = vec4<f32>(p, 0.0, 1.0);
    o.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return o;
}

fn depth_at(px: vec2<i32>) -> f32 {
    let dim = vec2<i32>(textureDimensions(depth));
    return textureLoad(depth, clamp(px, vec2<i32>(0), dim - 1), 0);
}
fn view_pos(uv: vec2<f32>, d: f32) -> vec3<f32> {
    let v = u.inv_proj * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, d, 1.0);
    return v.xyz / v.w;
}
fn hash(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 = p3 + dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

@fragment
fn fs(i: POut) -> @location(0) vec4<f32> {
    let dim = vec2<f32>(textureDimensions(depth));
    let px = vec2<i32>(i.uv * dim);
    let d = depth_at(px);
    if (d >= 1.0) { return vec4<f32>(1.0); }
    let p = view_pos(i.uv, d);
    // The surface's normal, from its neighbours (the nearer pair each way,
    // so edges don't smear across a silhouette).
    let s = 2;
    let pr = view_pos(i.uv + vec2<f32>(f32(s), 0.0) / dim, depth_at(px + vec2<i32>(s, 0)));
    let pl = view_pos(i.uv - vec2<f32>(f32(s), 0.0) / dim, depth_at(px - vec2<i32>(s, 0)));
    let pd = view_pos(i.uv + vec2<f32>(0.0, f32(s)) / dim, depth_at(px + vec2<i32>(0, s)));
    let pu = view_pos(i.uv - vec2<f32>(0.0, f32(s)) / dim, depth_at(px - vec2<i32>(0, s)));
    let dx = select(p - pl, pr - p, abs(pr.z - p.z) < abs(p.z - pl.z));
    let dy = select(p - pu, pd - p, abs(pd.z - p.z) < abs(p.z - pu.z));
    var n = normalize(cross(dx, dy));
    if (dot(n, -p) < 0.0) { n = -n; }
    // A turned frame per pixel, repeating every 4x4 pixels so the 4x4 blur
    // afterwards evens it out exactly.
    let cell = vec2<u32>(vec2<i32>(i.clip.xy)) % vec2<u32>(4u);
    let bayer = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    let a = (bayer[cell.y * 4u + cell.x] + 0.5) / 16.0 * 6.2831853;
    let r = vec3<f32>(cos(a), sin(a), 0.0);
    let t = normalize(r - n * dot(r, n));
    let b = cross(n, t);
    let radius = u.params.x;
    let count = 12;
    var occ = 0.0;
    for (var k = 0; k < count; k++) {
        let fk = f32(k);
        var h = vec3<f32>(hash(vec2<f32>(fk, 1.7)) * 2.0 - 1.0, hash(vec2<f32>(fk, 9.1)) * 2.0 - 1.0, hash(vec2<f32>(fk, 4.3)) * 0.7 + 0.3);
        h = normalize(h) * mix(0.15, 1.0, pow((fk + 1.0) / f32(count), 2.0));
        let sp = p + (t * h.x + b * h.y + n * h.z) * radius;
        let c = u.proj * vec4<f32>(sp, 1.0);
        let suv = vec2<f32>(c.x / c.w * 0.5 + 0.5, 0.5 - c.y / c.w * 0.5);
        if (suv.x < 0.0 || suv.x > 1.0 || suv.y < 0.0 || suv.y > 1.0) { continue; }
        let sd = depth_at(vec2<i32>(suv * dim));
        let sz = view_pos(suv, sd).z;
        // Something in front of the sample point hides it; far-off things
        // (a penguin against the distant snow) don't count.
        let range = smoothstep(0.0, 1.0, radius / max(abs(p.z - sz), 1e-3));
        // The bias grows with distance, as depth gets coarser.
        occ += select(0.0, 1.0, sz >= sp.z + 0.04 + 0.004 * abs(p.z)) * range;
    }
    let ao = 1.0 - occ / f32(count);
    return vec4<f32>(ao * ao, 0.0, 0.0, 1.0);
}
"#;

struct Targets {
    w: u32,
    h: u32,
    /// The multisampled picture the world is drawn into (resolved into `hdr`).
    msaa: Option<wgpu::TextureView>,
    hdr: wgpu::TextureView,
    depth: wgpu::TextureView,
    ao: wgpu::TextureView,
    ao_blurred: wgpu::TextureView,
    bloom: Vec<wgpu::TextureView>,
    ao_bind: Option<wgpu::BindGroup>,
    ao_blur_bind: wgpu::BindGroup,
    bright_bind: wgpu::BindGroup,
    /// Down: level i from level i-1. Up: level i from level i+1.
    down_binds: Vec<wgpu::BindGroup>,
    up_binds: Vec<wgpu::BindGroup>,
    finish_bind: wgpu::BindGroup,
}

pub struct Post {
    pub samples: u32,
    ao_on: bool,
    layout: wgpu::BindGroupLayout,
    ao_layout: wgpu::BindGroupLayout,
    ubuf: wgpu::Buffer,
    ao_pipe: Option<wgpu::RenderPipeline>,
    ao_blur: wgpu::RenderPipeline,
    bright: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    finish: wgpu::RenderPipeline,
    white: wgpu::TextureView,
    targets: Option<Targets>,
}

fn texture(gpu: &Gpu, label: &str, w: u32, h: u32, format: wgpu::TextureFormat, samples: u32, sampled: bool) -> wgpu::TextureView {
    let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
    if sampled {
        usage |= wgpu::TextureUsages::TEXTURE_BINDING;
    }
    gpu.device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

fn fullscreen(gpu: &Gpu, label: &str, module: &wgpu::ShaderModule, entry: &str, layout: &wgpu::BindGroupLayout, format: wgpu::TextureFormat, blend: Option<wgpu::BlendState>) -> wgpu::RenderPipeline {
    let pl = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: &[Some(layout)], immediate_size: 0 });
    gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pl),
        vertex: wgpu::VertexState { module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

impl Post {
    pub fn new(gpu: &Gpu) -> Post {
        let samples = gpu.msaa;
        // Browsers' WebGL can't read a depth buffer back in a shader the way
        // this needs; there the picture goes without ambient occlusion.
        let ao_on = !gpu.webgl;
        let ftex = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let uniform = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post"),
            entries: &[
                uniform,
                ftex(1),
                ftex(2),
                ftex(3),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let ao_layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ao"),
            entries: &[
                uniform,
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: samples > 1,
                    },
                    count: None,
                },
            ],
        });
        let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("post"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let add = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let hdr = gpu.hdr_format;
        let ubuf = gpu.buffer("post uniforms", wgpu::BufferUsages::UNIFORM, &[0u8; std::mem::size_of::<PostUniforms>()]);
        let white = gpu
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("white"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
        gpu.queue.write_texture(white.as_image_copy(), &[255; 4], wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: None }, wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 });
        Post {
            samples,
            ao_on,
            ao_pipe: ao_on.then(|| {
                let src = AO_SHADER.replace("DEPTH_T", if samples > 1 { "texture_depth_multisampled_2d" } else { "texture_depth_2d" });
                let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("ao"), source: wgpu::ShaderSource::Wgsl(src.into()) });
                fullscreen(gpu, "ao", &module, "fs", &ao_layout, AO_FORMAT, None)
            }),
            ao_blur: fullscreen(gpu, "ao blur", &module, "fs_ao_blur", &layout, AO_FORMAT, None),
            bright: fullscreen(gpu, "bloom bright", &module, "fs_bright", &layout, hdr, None),
            down: fullscreen(gpu, "bloom down", &module, "fs_down", &layout, hdr, None),
            up: fullscreen(gpu, "bloom up", &module, "fs_up", &layout, hdr, Some(add)),
            finish: fullscreen(gpu, "finish", &module, "fs_finish", &layout, gpu.surface_format, None),
            layout,
            ao_layout,
            ubuf,
            white: white.create_view(&Default::default()),
            targets: None,
        }
    }

    fn bind(&self, gpu: &Gpu, label: &str, a: &wgpu::TextureView, b: &wgpu::TextureView, c: &wgpu::TextureView) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(a) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(b) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(c) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&gpu.linear_clamp) },
            ],
        })
    }

    /// Make the screen-sized pictures for a `w` x `h` frame.
    pub fn ensure(&mut self, gpu: &Gpu, w: u32, h: u32) {
        if self.targets.as_ref().is_some_and(|t| (t.w, t.h) == (w, h)) {
            return;
        }
        let hf = gpu.hdr_format;
        let msaa = (self.samples > 1).then(|| texture(gpu, "world msaa", w, h, hf, self.samples, false));
        let hdr = texture(gpu, "world", w, h, hf, 1, true);
        let depth = texture(gpu, "depth", w, h, DEPTH, self.samples, self.ao_on);
        let ao = texture(gpu, "ao", w / 2, h / 2, AO_FORMAT, 1, true);
        let ao_blurred = texture(gpu, "ao blurred", w / 2, h / 2, AO_FORMAT, 1, true);
        let bloom: Vec<wgpu::TextureView> = (0..BLOOM_LEVELS).map(|i| texture(gpu, "bloom", w >> (i + 1), h >> (i + 1), hf, 1, true)).collect();
        let ao_bind = self.ao_on.then(|| {
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ao"),
                layout: &self.ao_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.ubuf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&depth) },
                ],
            })
        });
        let w1 = &self.white;
        let bright_bind = self.bind(gpu, "bright", &hdr, w1, w1);
        let ao_blur_bind = self.bind(gpu, "ao blur", &ao, w1, w1);
        let down_binds = (1..BLOOM_LEVELS).map(|i| self.bind(gpu, "down", &bloom[i - 1], w1, w1)).collect();
        let up_binds = (0..BLOOM_LEVELS - 1).map(|i| self.bind(gpu, "up", &bloom[i + 1], w1, w1)).collect();
        let ao_view = if self.ao_on { &ao_blurred } else { w1 };
        let finish_bind = self.bind(gpu, "finish", &hdr, &bloom[0], ao_view);
        self.targets = Some(Targets { w, h, msaa, hdr, depth, ao, ao_blurred, bloom, ao_bind, ao_blur_bind, bright_bind, down_binds, up_binds, finish_bind });
    }

    /// Where the world pass draws: colour (with its resolve target) and depth.
    pub fn world_attachments(&self) -> (&wgpu::TextureView, Option<&wgpu::TextureView>, &wgpu::TextureView) {
        let t = self.targets.as_ref().expect("post targets");
        match &t.msaa {
            Some(m) => (m, Some(&t.hdr), &t.depth),
            None => (&t.hdr, None, &t.depth),
        }
    }

    /// Finish the frame's world picture onto `out`.
    pub fn run(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, out: &wgpu::TextureView, u: &PostUniforms) {
        let Some(t) = &self.targets else { return };
        gpu.write(&self.ubuf, 0, bytes(&[*u]));
        let pass = |encoder: &mut wgpu::CommandEncoder, label: &str, view: &wgpu::TextureView, load: bool, pipe: &wgpu::RenderPipeline, bind: &wgpu::BindGroup| {
            let mut p = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if load { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color::WHITE) },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            p.set_pipeline(pipe);
            p.set_bind_group(0, bind, &[]);
            p.draw(0..3, 0..1);
        };
        if let (Some(pipe), Some(bind)) = (&self.ao_pipe, &t.ao_bind) {
            pass(encoder, "ao", &t.ao, false, pipe, bind);
            pass(encoder, "ao blur", &t.ao_blurred, false, &self.ao_blur, &t.ao_blur_bind);
        }
        pass(encoder, "bloom bright", &t.bloom[0], false, &self.bright, &t.bright_bind);
        for i in 1..BLOOM_LEVELS {
            pass(encoder, "bloom down", &t.bloom[i], false, &self.down, &t.down_binds[i - 1]);
        }
        for i in (0..BLOOM_LEVELS - 1).rev() {
            pass(encoder, "bloom up", &t.bloom[i], true, &self.up, &t.up_binds[i]);
        }
        pass(encoder, "finish", out, false, &self.finish, &t.finish_bind);
    }
}
