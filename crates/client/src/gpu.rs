//! The GPU: device, window surface, textures and the frame being drawn.
//!
//! Each frame records every pass (shadows, world, post-processing, UI) into
//! one command encoder, submitted at the end. Textures are handed out as
//! small ids so UI code can pass them around freely.

use std::sync::Arc;

use winit::window::Window;

use crate::gfx::Image;

/// A texture in [`Gpu`]'s table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureId(pub u32);

pub struct Tex {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub w: u32,
    pub h: u32,
    /// Tiles (wood and paper backgrounds) rather than clamping at the edge.
    pub repeat: bool,
}

pub struct Frame {
    pub encoder: wgpu::CommandEncoder,
    /// Where the finished picture goes: the window, or a screenshot.
    pub view: wgpu::TextureView,
    pub w: u32,
    pub h: u32,
    surface: Option<wgpu::SurfaceTexture>,
    shot: Option<wgpu::Texture>,
    /// Something has drawn to `view` this frame (else the UI clears it).
    pub drawn: bool,
}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// The window's format (never sRGB: colours are written already encoded).
    pub surface_format: wgpu::TextureFormat,
    /// What the lit world is drawn into before post-processing.
    pub hdr_format: wgpu::TextureFormat,
    /// WebGL2: fewer features, so lighter effects.
    pub webgl: bool,
    /// Samples per pixel for the world (4 where the GPU allows, for smooth edges).
    pub msaa: u32,
    textures: Vec<Option<Tex>>,
    pub linear_clamp: wgpu::Sampler,
    pub linear_repeat: wgpu::Sampler,
    pub frame: Option<Frame>,
}

/// Raw bytes of plain-old-data values, for uploading.
pub fn bytes<T: Copy>(v: &[T]) -> &[u8] {
    // SAFETY: only used with #[repr(C)] structs of floats and bytes, no padding.
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}

/// Run a future that is ready at once (wgpu's on desktops).
#[cfg(not(target_arch = "wasm32"))]
pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

impl Gpu {
    pub async fn new(window: Arc<Window>, w: u32, h: u32) -> Result<Gpu, String> {
        #[cfg(target_arch = "wasm32")]
        let backends = wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL;
        #[cfg(not(target_arch = "wasm32"))]
        let backends = wgpu::Backends::PRIMARY | wgpu::Backends::GL;
        let desc = wgpu::InstanceDescriptor { backends, ..wgpu::InstanceDescriptor::new_without_display_handle() };
        // A browser can offer WebGPU yet have no adapter for it: then WebGL2.
        #[cfg(target_arch = "wasm32")]
        let instance = wgpu::util::new_instance_with_webgpu_detection(desc).await;
        #[cfg(not(target_arch = "wasm32"))]
        let instance = wgpu::Instance::new(desc);
        let surface = instance.create_surface(window).map_err(|e| format!("can't draw to the window: {e}"))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|e| format!("no graphics adapter: {e}"))?;
        let info = adapter.get_info();
        let webgl = info.backend == wgpu::Backend::Gl && cfg!(target_arch = "wasm32");
        eprintln!("graphics: {} ({:?})", info.name, info.backend);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("snowballz"),
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .map_err(|e| format!("can't open the graphics device: {e}"))?;
        let caps = surface.get_capabilities(&adapter);
        let surface_format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        let mut config = surface.get_default_config(&adapter, w.max(1), h.max(1)).ok_or("the window can't be drawn to")?;
        config.format = surface_format;
        config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        // SNOWBALLZ_NOVSYNC=1 draws as fast as it can, to measure frame time.
        config.present_mode = if std::env::var_os("SNOWBALLZ_NOVSYNC").is_some() {
            wgpu::PresentMode::AutoNoVsync
        } else {
            wgpu::PresentMode::AutoVsync
        };
        surface.configure(&device, &config);
        // Half floats if they can be drawn to and blended, else plain bytes.
        let f16 = adapter.get_texture_format_features(wgpu::TextureFormat::Rgba16Float);
        let hdr_ok = f16.allowed_usages.contains(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING)
            && f16.flags.contains(wgpu::TextureFormatFeatureFlags::BLENDABLE | wgpu::TextureFormatFeatureFlags::FILTERABLE);
        let hdr_format = if hdr_ok { wgpu::TextureFormat::Rgba16Float } else { wgpu::TextureFormat::Rgba8Unorm };
        let msaa = if adapter.get_texture_format_features(hdr_format).flags.contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_X4) { 4 } else { 1 };
        let sampler = |mode: wgpu::AddressMode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: mode,
                address_mode_v: mode,
                address_mode_w: mode,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            })
        };
        let linear_clamp = sampler(wgpu::AddressMode::ClampToEdge);
        let linear_repeat = sampler(wgpu::AddressMode::Repeat);
        Ok(Gpu { device, queue, surface, config, surface_format, hdr_format, webgl, msaa, textures: Vec::new(), linear_clamp, linear_repeat, frame: None })
    }

    /// The window changed size (physical pixels).
    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 || (w, h) == (self.config.width, self.config.height) {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
    }

    // ---------------------------------------------------------------------
    // Textures

    /// An RGBA8 texture, with mipmaps if asked (power-of-two sizes only).
    pub fn new_texture(&mut self, img: &Image, mipmaps: bool, repeat: bool) -> TextureId {
        let levels = if mipmaps && img.w.is_power_of_two() && img.h.is_power_of_two() { img.w.max(img.h).ilog2() + 1 } else { 1 };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: img.w, height: img.h, depth_or_array_layers: 1 },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut level = img.rgba.clone();
        let (mut w, mut h) = (img.w, img.h);
        for mip in 0..levels {
            write_level(&self.queue, &texture, mip, w, h, &level);
            if mip + 1 < levels {
                level = half(&level, w, h);
                w = (w / 2).max(1);
                h = (h / 2).max(1);
            }
        }
        let view = texture.create_view(&Default::default());
        self.textures.push(Some(Tex { texture, view, w: img.w, h: img.h, repeat }));
        TextureId(self.textures.len() as u32 - 1)
    }

    /// A blank texture to fill later with [`Gpu::texture_update`].
    pub fn new_blank(&mut self, w: u32, h: u32) -> TextureId {
        self.new_texture(&Image { w, h, rgba: vec![0; (w * h * 4) as usize] }, false, false)
    }

    /// Replace a texture's pixels (same size; the top mip only).
    pub fn texture_update(&mut self, id: TextureId, rgba: &[u8]) {
        if let Some(t) = self.textures.get(id.0 as usize).and_then(|t| t.as_ref()) {
            write_level(&self.queue, &t.texture, 0, t.w, t.h, rgba);
        }
    }

    pub fn delete_texture(&mut self, id: TextureId) {
        if let Some(t) = self.textures.get_mut(id.0 as usize) {
            *t = None;
        }
    }

    pub fn tex(&self, id: TextureId) -> Option<&Tex> {
        self.textures.get(id.0 as usize).and_then(|t| t.as_ref())
    }

    pub fn buffer(&self, label: &str, usage: wgpu::BufferUsages, data: &[u8]) -> wgpu::Buffer {
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (data.len() as u64).max(16).next_multiple_of(4),
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if !data.is_empty() {
            self.write(&buf, 0, data);
        }
        buf
    }

    /// Write bytes into a buffer (padded to wgpu's four-byte steps).
    pub fn write(&self, buf: &wgpu::Buffer, offset: u64, data: &[u8]) {
        if data.len() % 4 == 0 {
            self.queue.write_buffer(buf, offset, data);
        } else {
            let mut v = data.to_vec();
            v.resize(data.len().next_multiple_of(4), 0);
            self.queue.write_buffer(buf, offset, &v);
        }
    }

    // ---------------------------------------------------------------------
    // Frames

    /// Start a frame. With `shot`, it is drawn offscreen to be read back.
    /// False if the window can't be drawn to right now (minimised).
    pub fn begin_frame(&mut self, shot: bool) -> bool {
        let (w, h) = (self.config.width, self.config.height);
        let encoder = self.device.create_command_encoder(&Default::default());
        if shot {
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("screenshot"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.surface_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = tex.create_view(&Default::default());
            self.frame = Some(Frame { encoder, view, w, h, surface: None, shot: Some(tex), drawn: false });
            return true;
        }
        let st = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return false;
            }
            _ => return false,
        };
        let view = st.texture.create_view(&Default::default());
        self.frame = Some(Frame { encoder, view, w, h, surface: Some(st), shot: None, drawn: false });
        true
    }

    /// Submit the frame and show it; a screenshot frame returns its pixels.
    pub fn end_frame(&mut self) -> Option<Image> {
        let f = self.frame.take()?;
        let mut readback = None;
        let mut encoder = f.encoder;
        if let Some(tex) = &f.shot {
            let row = (f.w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
            let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("screenshot"),
                size: (row * f.h) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                tex.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buf,
                    layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(f.h) },
                },
                wgpu::Extent3d { width: f.w, height: f.h, depth_or_array_layers: 1 },
            );
            readback = Some((buf, row));
        }
        self.queue.submit([encoder.finish()]);
        if let Some(st) = f.surface {
            self.queue.present(st);
        }
        let (buf, row) = readback?;
        self.read_pixels(&buf, row, f.w, f.h)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn read_pixels(&self, buf: &wgpu::Buffer, row: u32, w: u32, h: u32) -> Option<Image> {
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let data = slice.get_mapped_range().ok()?;
        let bgra = matches!(self.surface_format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let line = &data[(y * row) as usize..(y * row + w * 4) as usize];
            for p in line.chunks(4) {
                rgba.extend_from_slice(&if bgra { [p[2], p[1], p[0], 255] } else { [p[0], p[1], p[2], 255] });
            }
        }
        Some(Image { w, h, rgba })
    }

    #[cfg(target_arch = "wasm32")]
    fn read_pixels(&self, _buf: &wgpu::Buffer, _row: u32, _w: u32, _h: u32) -> Option<Image> {
        None
    }
}

fn write_level(queue: &wgpu::Queue, texture: &wgpu::Texture, mip: u32, w: u32, h: u32, rgba: &[u8]) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture, mip_level: mip, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        rgba,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
}

/// The next mip level down: each pixel the average of four.
fn half(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            for c in 0..4 {
                let at = |xx: u32, yy: u32| rgba[((yy.min(h - 1) * w + xx.min(w - 1)) * 4 + c) as usize] as u32;
                let s = at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1);
                out[((y * nw + x) * 4 + c) as usize] = (s / 4) as u8;
            }
        }
    }
    out
}
