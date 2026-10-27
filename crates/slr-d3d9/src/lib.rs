//! Tier-D textured triangle presentation on wgpu. The full D3D9 fixed-function
//! state emulator remains unfinished; this backend presents script-selected world
//! geometry with depth and perspective textures, then the script OSD.
pub mod display;
pub mod stream;
use serde_json::json;
use slr_telemetry::journal;
use std::{
    collections::HashMap,
    path::Path,
    sync::{mpsc, Arc},
};
use wgpu::util::DeviceExt;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, ModifiersState, PhysicalKey},
    window::{Fullscreen, Window, WindowId},
};

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}
/// Homogeneous clip position retains perspective-correct UV interpolation.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WorldVertex {
    pub position: [f32; 4],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}
#[derive(Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
}
/// One decoded picture of the in-game video (`GfxEngine.openVideo`'s texture
/// path). `serial` changes whenever the picture does.
#[derive(Clone)]
pub struct VideoImage {
    pub serial: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
}
#[derive(Default)]
pub struct Frame {
    /// Logical back buffer; physical window scaling is handled at presentation.
    pub logical_size: Option<(u32, u32)>,
    pub video: Option<Box<VideoImage>>,
    pub id: u64,
    pub input_id: u64,
    pub images: HashMap<String, Image>,
    pub draws: Vec<(String, Vec<Vertex>)>,
    pub world_draws: Vec<(String, Vec<WorldVertex>)>,
    pub labels: Vec<String>,
    pub text_bounds: HashMap<String, [f32; 4]>,
    /// The exact D3D9 call sequence of this frame, in submission order, when the
    /// producer models the device itself (see `stream::emit`). When empty the
    /// emitter states what the translator's passes imply instead.
    pub calls: Vec<stream::emit::Call>,
}
impl Frame {
    pub fn back_buffer_size(&self) -> (u32, u32) {
        self.logical_size.unwrap_or(display::DEFAULT_LOGICAL)
    }
    /// Stable image recipe hash, excluding host timing, frame IDs and GPU state.
    pub fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        let size = self.back_buffer_size();
        if size != display::DEFAULT_LOGICAL {
            h.update(b"logical");
            h.update(size.0.to_le_bytes());
            h.update(size.1.to_le_bytes());
        }
        if let Some(video) = &self.video {
            h.update(b"video");
            h.update(video.serial.to_le_bytes());
            h.update([video.width.to_le_bytes(), video.height.to_le_bytes()].concat());
            h.update(video.rgba.as_slice());
        }
        for (name, vertices) in &self.world_draws {
            h.update(b"world");
            h.update((name.len() as u64).to_le_bytes());
            h.update(name.as_bytes());
            h.update((vertices.len() as u64).to_le_bytes());
            h.update(bytemuck::cast_slice::<WorldVertex, u8>(vertices));
        }
        for (name, vertices) in &self.draws {
            h.update((name.len() as u64).to_le_bytes());
            h.update(name.as_bytes());
            h.update((vertices.len() as u64).to_le_bytes());
            h.update(bytemuck::cast_slice::<Vertex, u8>(vertices));
        }
        format!("{:x}", h.finalize())
    }
}
#[derive(Clone, Debug)]
pub struct PlatformInput {
    pub source_id: u64,
    pub event: InputEvent,
}
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputEvent {
    Key {
        code: u32,
        pressed: bool,
        #[serde(default)]
        repeat: bool,
    },
    Pointer {
        x: f32,
        y: f32,
    },
    Button {
        pressed: bool,
    },
    Text {
        text: String,
    },
    FocusLost,
    Close,
}
pub enum Message {
    Frame(Box<Frame>),
    Finished(Result<(), String>),
}
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    world_pipeline: wgpu::RenderPipeline,
    world_sampler: wgpu::Sampler,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    textures: HashMap<String, (wgpu::Texture, wgpu::BindGroup, wgpu::BindGroup)>,
    blit_pipeline: wgpu::RenderPipeline,
    blit_quad: wgpu::Buffer,
    video: Option<VideoTexture>,
    logical: Option<LogicalTarget>,
}
struct VideoTexture {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    serial: Option<u64>,
    rgba_sha256: Option<[u8; 32]>,
    size: (u32, u32),
}
/// The fixed-size back buffer the game renders into; the window shows it
/// scaled and letterboxed (`display::letterbox`).
struct LogicalTarget {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    size: (u32, u32),
}
const SHADER: &str = r#"
struct V { @builtin(position) p: vec4f, @location(0) uv: vec2f, @location(1) color: vec4f }
@vertex fn vs(@location(0) p:vec2f, @location(1) uv:vec2f, @location(2) color:vec4f) -> V {
 return V(vec4f(p,0.,1.),uv,color);
}
@vertex fn vs_world(@location(0) p:vec4f, @location(1) uv:vec2f, @location(2) color:vec4f) -> V {
 return V(p,uv,color);
}
@group(0) @binding(0) var image:texture_2d<f32>;
@group(0) @binding(1) var samp:sampler;
@fragment fn fs(v:V)->@location(0) vec4f { return textureSample(image,samp,v.uv)*v.color; }
@fragment fn fs_blit(v:V)->@location(0) vec4f { return vec4f(textureSample(image,samp,v.uv).rgb,1.); }
@fragment fn fs_world(v:V)->@location(0) vec4f {
 let color = textureSample(image,samp,v.uv)*v.color;
 if color.a < 0.01 { discard; }
 return color;
}
"#;
/// Two triangles covering clip rect `[x0, y0, x1, y1]` with the full texture.
fn quad_vertices(c: [f32; 4]) -> [Vertex; 6] {
    let v = |x: f32, y: f32, u: f32, w: f32| Vertex {
        position: [x, y],
        uv: [u, w],
        color: [1.; 4],
    };
    [
        v(c[0], c[1], 0., 0.),
        v(c[2], c[1], 1., 0.),
        v(c[0], c[3], 0., 1.),
        v(c[2], c[1], 1., 0.),
        v(c[2], c[3], 1., 1.),
        v(c[0], c[3], 0., 1.),
    ]
}
impl Gpu {
    async fn new(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        format: wgpu::TextureFormat,
    ) -> Result<Self, String> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: surface,
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?;
        journal::emit(
            "gpu-adapter",
            json!({"info":format!("{:?}",adapter.get_info())}),
        )?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("stock OSD"),
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?;
        device.on_uncaptured_error(Box::new(|error| {
            let _ = journal::emit("gpu-error", json!({"error":error.to_string()}));
            panic!("wgpu: {error}");
        }));
        device.set_device_lost_callback(|reason, message| {
            let _ = journal::emit(
                if matches!(reason, wgpu::DeviceLostReason::Destroyed) {
                    "gpu-device-destroyed"
                } else {
                    "gpu-device-lost"
                },
                json!({"reason":format!("{reason:?}"),"message":message}),
            );
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
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
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stock OSD"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stock OSD alpha"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x2,1=>Float32x2,2=>Float32x4],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let world_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stock world depth"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_world"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<WorldVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x2,2=>Float32x4],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_world"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("opaque blit"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x2,1=>Float32x2,2=>Float32x4],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_blit"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let blit_quad = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("blit quad"),
            contents: bytemuck::cast_slice(&quad_vertices([-1., 1., 1., -1.])),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let world_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Ok(Self {
            world_pipeline,
            world_sampler,
            device,
            queue,
            pipeline,
            layout,
            sampler,
            textures: HashMap::new(),
            blit_pipeline,
            blit_quad,
            video: None,
            logical: None,
        })
    }
    fn texture_binding(&self, view: &wgpu::TextureView) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }
    fn ensure_logical(&mut self, size: (u32, u32), format: wgpu::TextureFormat) {
        if self.logical.as_ref().is_some_and(|l| l.size == size) {
            return;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("back buffer"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let bind_group = self.texture_binding(&view);
        self.logical = Some(LogicalTarget {
            _texture: texture,
            view,
            bind_group,
            size,
        });
    }
    /// Scale the back buffer onto the window surface inside `rect`; the rest of
    /// the surface is cleared to black.
    fn present(
        &self,
        surface: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
        rect: display::Rect,
    ) {
        let Some(logical) = &self.logical else { return };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("present"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if rect.w > 0 && rect.h > 0 {
            pass.set_viewport(
                rect.x as f32,
                rect.y as f32,
                rect.w as f32,
                rect.h as f32,
                0.,
                1.,
            );
            pass.set_scissor_rect(rect.x, rect.y, rect.w, rect.h);
            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, &logical.bind_group, &[]);
            pass.set_vertex_buffer(0, self.blit_quad.slice(..));
            pass.draw(0..6, 0..1);
        }
    }
    /// Draw the video picture first, so the 3D scene and OSD land on top.
    fn draw_video(
        &self,
        view: &wgpu::TextureView,
        frame: &Frame,
        encoder: &mut wgpu::CommandEncoder,
        size: wgpu::Extent3d,
    ) -> bool {
        let (Some(image), Some(texture)) = (&frame.video, &self.video) else {
            return false;
        };
        let back = (size.width, size.height);
        let quad = display::to_clip(display::video_quad((image.width, image.height), back), back);
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("video quad"),
                contents: bytemuck::cast_slice(&quad_vertices(quad)),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("stock video"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.blit_pipeline);
        pass.set_bind_group(0, &texture.bind_group, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..6, 0..1);
        true
    }
    fn upload_video(&mut self, image: &VideoImage) {
        let size = (image.width, image.height);
        if self.video.as_ref().is_none_or(|v| v.size != size) {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("video"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let bind_group = self.texture_binding(&texture.create_view(&Default::default()));
            self.video = Some(VideoTexture {
                texture,
                bind_group,
                serial: None,
                rgba_sha256: None,
                size,
            });
        }
        let video = self.video.as_mut().expect("created above");
        use sha2::{Digest, Sha256};
        let rgba_sha256: [u8; 32] = Sha256::digest(image.rgba.as_slice()).into();
        if video.serial != Some(image.serial) || video.rgba_sha256 != Some(rgba_sha256) {
            self.queue.write_texture(
                video.texture.as_image_copy(),
                &image.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(image.width * 4),
                    rows_per_image: Some(image.height),
                },
                wgpu::Extent3d {
                    width: image.width,
                    height: image.height,
                    depth_or_array_layers: 1,
                },
            );
            video.serial = Some(image.serial);
            video.rgba_sha256 = Some(rgba_sha256);
        }
    }
    /// RGBA pixels of a surface/target texture (BGRA formats are swizzled).
    fn read_pixels(&self, texture: &wgpu::Texture) -> Result<(u32, u32, Vec<u8>), String> {
        let (width, height) = (texture.width(), texture.height());
        let stride = (width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(stride * height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::Wait)
            .map_err(|e| e.to_string())?;
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let bgra = matches!(
            texture.format(),
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let data = buffer.slice(..).get_mapped_range();
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for row in data.chunks(stride as usize) {
            for px in row[..width as usize * 4].as_chunks::<4>().0 {
                if bgra {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                } else {
                    rgba.extend_from_slice(px);
                }
            }
        }
        Ok((width, height, rgba))
    }
    fn upload(&mut self, frame: &Frame) {
        if let Some(video) = &frame.video {
            self.upload_video(video);
        }
        for (name, image) in &frame.images {
            if self.textures.contains_key(name) {
                continue;
            }
            let size = wgpu::Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            };
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(name),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.queue.write_texture(
                texture.as_image_copy(),
                &image.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(image.width * 4),
                    rows_per_image: Some(image.height),
                },
                size,
            );
            let view = texture.create_view(&Default::default());
            let binding = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(name),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            let world_binding = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(name),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.world_sampler),
                    },
                ],
            });
            self.textures
                .insert(name.clone(), (texture, binding, world_binding));
        }
    }
    fn render(
        &self,
        view: &wgpu::TextureView,
        frame: &Frame,
        encoder: &mut wgpu::CommandEncoder,
        size: wgpu::Extent3d,
    ) {
        let video_drawn = self.draw_video(view, frame, encoder, size);
        if !frame.world_draws.is_empty() {
            let depth = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("world depth"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let depth_view = depth.create_view(&Default::default());
            let buffers: Vec<_> = frame
                .world_draws
                .iter()
                .map(|(_, v)| {
                    self.device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("world vertices"),
                            contents: bytemuck::cast_slice(v),
                            usage: wgpu::BufferUsages::VERTEX,
                        })
                })
                .collect();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("stock world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if video_drawn {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.world_pipeline);
            for ((name, vertices), buffer) in frame.world_draws.iter().zip(&buffers) {
                if let Some((_, _, binding)) = self.textures.get(name) {
                    pass.set_bind_group(0, binding, &[]);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..vertices.len() as u32, 0..1);
                }
            }
        }
        let buffers: Vec<_> = frame
            .draws
            .iter()
            .map(|(_, v)| {
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: None,
                        contents: bytemuck::cast_slice(v),
                        usage: wgpu::BufferUsages::VERTEX,
                    })
            })
            .collect();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("stock OSD"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if frame.world_draws.is_empty() && !video_drawn {
                        wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        for ((name, vertices), buffer) in frame.draws.iter().zip(&buffers) {
            if let Some((_, binding, _)) = self.textures.get(name) {
                pass.set_bind_group(0, binding, &[]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..vertices.len() as u32, 0..1);
            }
        }
    }
    fn screenshot(&mut self, frame: &Frame, path: &Path) -> Result<(), String> {
        self.upload(frame);
        let (width, height) = frame.back_buffer_size();
        let limit = self.device.limits().max_texture_dimension_2d;
        if width == 0 || height == 0 || width > limit || height > limit {
            return Err("invalid logical capture dimensions".into());
        }
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("menu capture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let stride = (width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (stride * height) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.render(
            &target.create_view(&Default::default()),
            frame,
            &mut encoder,
            target.size(),
        );
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::Wait)
            .map_err(|e| e.to_string())?;
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let data = buffer.slice(..).get_mapped_range();
        let rgba: Vec<_> = data
            .chunks(stride as usize)
            .flat_map(|row| row[..width as usize * 4].iter().copied())
            .collect();
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut png = png::Encoder::new(file, width, height);
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        png.write_header()
            .map_err(|e| e.to_string())?
            .write_image_data(&rgba)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
pub fn capture(frame: &Frame, path: &Path) -> Result<(), String> {
    stream::emit::record_frame(frame)?;
    pollster::block_on(async {
        let instance = wgpu::Instance::new(&Default::default());
        let mut gpu = Gpu::new(&instance, None, wgpu::TextureFormat::Rgba8Unorm).await?;
        gpu.screenshot(frame, path)
    })
}
/// One scripted display action, run when `at_present` frames have reached the
/// real surface. This drives the same code paths as the keyboard shortcut so a
/// harness can exercise windowed/fullscreen transitions unattended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayStep {
    pub at_present: u64,
    pub action: DisplayAction,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DisplayAction {
    Fullscreen(bool),
    Toggle,
    /// Read back the presented drawable and write `drawable-<name>.png`.
    Capture(String),
    Exit,
}
#[derive(Clone, Debug)]
pub struct WindowOptions {
    pub fullscreen: bool,
    /// Back-buffer size the game renders at.
    pub logical: (u32, u32),
    pub script: Vec<DisplayStep>,
}
impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            fullscreen: false,
            logical: display::DEFAULT_LOGICAL,
            script: vec![],
        }
    }
}
impl WindowOptions {
    /// `SLR_FULLSCREEN=1`, `SLR_LOGICAL_SIZE=WxH` and
    /// `SLR_DISPLAY_SCRIPT=<n>:<action>,...` where action is `fullscreen`,
    /// `windowed`, `toggle`, `capture=<name>` or `exit`.
    pub fn from_env() -> Result<Self, String> {
        let mut options = Self::default();
        if std::env::var("SLR_FULLSCREEN").is_ok_and(|v| v == "1") {
            options.fullscreen = true;
        }
        if let Ok(size) = std::env::var("SLR_LOGICAL_SIZE") {
            let (w, h) = size.split_once('x').ok_or("SLR_LOGICAL_SIZE must be WxH")?;
            options.logical = (
                w.parse().map_err(|_| "SLR_LOGICAL_SIZE width")?,
                h.parse().map_err(|_| "SLR_LOGICAL_SIZE height")?,
            );
            if options.logical.0 == 0 || options.logical.1 == 0 {
                return Err("SLR_LOGICAL_SIZE must be non-zero".into());
            }
        }
        if let Ok(script) = std::env::var("SLR_DISPLAY_SCRIPT") {
            options.script = parse_script(&script)?;
        }
        Ok(options)
    }
}
pub fn parse_script(text: &str) -> Result<Vec<DisplayStep>, String> {
    let mut steps = vec![];
    for item in text.split(',').filter(|i| !i.is_empty()) {
        let (at, action) = item
            .split_once(':')
            .ok_or_else(|| format!("display step {item:?} needs <n>:<action>"))?;
        let at_present = at
            .parse()
            .map_err(|_| format!("display step {item:?}: bad frame count"))?;
        let action = match action {
            "fullscreen" => DisplayAction::Fullscreen(true),
            "windowed" => DisplayAction::Fullscreen(false),
            "toggle" => DisplayAction::Toggle,
            "exit" => DisplayAction::Exit,
            other => match other.strip_prefix("capture=") {
                Some(name)
                    if !name.is_empty()
                        && name
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') =>
                {
                    DisplayAction::Capture(name.into())
                }
                _ => return Err(format!("display step {item:?}: unknown action")),
            },
        };
        steps.push(DisplayStep { at_present, action });
    }
    if steps.windows(2).any(|w| w[0].at_present > w[1].at_present) {
        return Err("display steps must be in ascending frame order".into());
    }
    Ok(steps)
}
fn host_fullscreen(window: &Window) -> bool {
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::WindowExtMacOS;
        window.simple_fullscreen() || window.fullscreen().is_some()
    }
    #[cfg(not(target_os = "macos"))]
    {
        window.fullscreen().is_some()
    }
}
struct App {
    options: WindowOptions,
    modifiers: ModifiersState,
    presents: u64,
    next_step: usize,
    pending_capture: Option<String>,
    shortcuts: display::ShortcutKeys,
    layout: Option<(u32, u32, bool)>,
    rx: mpsc::Receiver<Message>,
    window: Option<Arc<Window>>,
    surface: Option<wgpu::Surface<'static>>,
    gpu: Option<Gpu>,
    config: Option<wgpu::SurfaceConfiguration>,
    frame: Frame,
    error: Option<String>,
    presented: bool,
    input: mpsc::Sender<PlatformInput>,
    last_presented: u64,
    last_heartbeat: std::time::Instant,
    last_received: std::time::Instant,
    stalled: bool,
}
impl App {
    fn send_input(&mut self, event: InputEvent) {
        let result = journal::emit(
            "input-received",
            json!({"event":event,"window_id":self.window.as_ref().map(|w|format!("{:?}",w.id())),"drawable":self.config.as_ref().map(|c|[c.width,c.height]),"scale_factor":self.window.as_ref().map(|w|w.scale_factor())}),
        );
        match result {
            Ok(source_id) => {
                if let Err(e) = self.input.send(PlatformInput { source_id, event }) {
                    self.error = Some(e.to_string());
                }
            }
            Err(e) => self.error = Some(e),
        }
    }
    fn note(&mut self, kind: &str, data: serde_json::Value) {
        if let Err(e) = journal::emit(kind, data) {
            self.error = Some(e);
        }
    }
    fn is_fullscreen(&self) -> bool {
        self.window.as_ref().is_some_and(|w| host_fullscreen(w))
    }
    fn set_fullscreen(&mut self, on: bool) {
        let Some(window) = &self.window else { return };
        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::WindowExtMacOS;
            window.set_simple_fullscreen(on);
        }
        #[cfg(not(target_os = "macos"))]
        window.set_fullscreen(on.then_some(Fullscreen::Borderless(None)));
        let applied = host_fullscreen(window) == on;
        self.note(
            "display-request",
            json!({"fullscreen":on,"applied":applied,"mode":if cfg!(target_os="macos"){"simple-fullscreen"}else{"borderless"},"presents":self.presents}),
        );
    }
    fn letterbox(&self) -> display::Rect {
        let drawable = self.config.as_ref().map_or((0, 0), |c| (c.width, c.height));
        display::letterbox(self.options.logical, drawable)
    }
    /// Journal the layout whenever the surface size or fullscreen state changes.
    fn note_layout(&mut self) {
        let Some(config) = &self.config else { return };
        let state = (config.width, config.height, self.is_fullscreen());
        if self.layout == Some(state) {
            return;
        }
        self.layout = Some(state);
        let monitor = self.window.as_ref().and_then(|w| w.current_monitor()).map(|m| {
            json!({"name":m.name(),"size":[m.size().width,m.size().height],"scale_factor":m.scale_factor()})
        });
        let data = json!({
            "drawable":[state.0,state.1],
            "fullscreen":state.2,
            "logical":[self.options.logical.0,self.options.logical.1],
            "letterbox":self.letterbox(),
            "scale_factor":self.window.as_ref().map(|w| w.scale_factor()),
            "monitor":monitor,
            "presents":self.presents,
        });
        self.note("display-layout", data);
    }
    fn run_script(&mut self, el: &ActiveEventLoop) {
        while let Some(step) = self.options.script.get(self.next_step).cloned() {
            if step.at_present > self.presents {
                break;
            }
            self.next_step += 1;
            match step.action {
                DisplayAction::Fullscreen(on) => self.set_fullscreen(on),
                DisplayAction::Toggle => {
                    let on = !self.is_fullscreen();
                    self.set_fullscreen(on);
                }
                DisplayAction::Capture(name) => self.pending_capture = Some(name),
                DisplayAction::Exit => {
                    self.note("display-script-exit", json!({"presents":self.presents}));
                    el.exit();
                }
            }
        }
    }
    fn capture_drawable(&mut self, gpu_texture: &wgpu::Texture, name: &str) {
        use sha2::{Digest, Sha256};
        let Some(gpu) = &self.gpu else { return };
        let result = gpu.read_pixels(gpu_texture).and_then(|(w, h, rgba)| {
            let dir = journal::root().unwrap_or_else(|| std::path::PathBuf::from("."));
            let path = dir.join(format!("drawable-{name}.png"));
            let file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
            let mut png = png::Encoder::new(file, w, h);
            png.set_color(png::ColorType::Rgba);
            png.set_depth(png::BitDepth::Eight);
            png.write_header()
                .map_err(|e| e.to_string())?
                .write_image_data(&rgba)
                .map_err(|e| e.to_string())?;
            Ok((w, h, format!("{:x}", Sha256::digest(&rgba)), path))
        });
        let rect = self.letterbox();
        let fullscreen = self.is_fullscreen();
        let frame_id = self.frame.id;
        match result {
            Ok((w, h, sha, path)) => self.note(
                "drawable-captured",
                json!({"name":name,"file":path.file_name().map(|f|f.to_string_lossy().into_owned()),"width":w,"height":h,"rgba_sha256":sha,"letterbox":rect,"fullscreen":fullscreen,"frame_id":frame_id,"presents":self.presents}),
            ),
            Err(e) => self.error = Some(format!("drawable capture {name}: {e}")),
        }
    }
}
impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let result = (|| -> Result<(), String> {
            let window = Arc::new(
                el.create_window(
                    Window::default_attributes()
                        .with_title("Street Legal Racing: Redline — exact port")
                        .with_inner_size(winit::dpi::LogicalSize::new(
                            f64::from(self.options.logical.0),
                            f64::from(self.options.logical.1),
                        ))
                        .with_fullscreen(if cfg!(target_os = "macos") {
                            None
                        } else {
                            self.options
                                .fullscreen
                                .then_some(Fullscreen::Borderless(None))
                        }),
                )
                .map_err(|e| e.to_string())?,
            );
            window.focus_window();
            #[cfg(target_os = "macos")]
            if self.options.fullscreen {
                use winit::platform::macos::WindowExtMacOS;
                window.set_simple_fullscreen(true);
            }
            let instance = wgpu::Instance::new(&Default::default());
            let surface = instance
                .create_surface(window.clone())
                .map_err(|e| e.to_string())?;
            let mut gpu = pollster::block_on(Gpu::new(
                &instance,
                Some(&surface),
                wgpu::TextureFormat::Bgra8Unorm,
            ))?;
            gpu.ensure_logical(self.options.logical, wgpu::TextureFormat::Bgra8Unorm);
            let size = window.inner_size();
            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                format: wgpu::TextureFormat::Bgra8Unorm,
                width: size.width.max(1),
                height: size.height.max(1),
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            };
            surface.configure(&gpu.device, &config);
            self.note("window-created",json!({"width":size.width,"height":size.height,"scale_factor":window.scale_factor(),"present_mode":"fifo","format":"bgra8unorm"}));
            self.surface = Some(surface);
            self.gpu = Some(gpu);
            self.config = Some(config);
            self.window = Some(window);
            self.note_layout();
            Ok(())
        })();
        if let Err(e) = result {
            self.error = Some(e);
            el.exit();
        }
    }
    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.send_input(InputEvent::Close);
            }
            WindowEvent::Focused(true) => self.note("window-focus", json!({"focused":true})),
            WindowEvent::CursorLeft { .. } => {
                self.send_input(InputEvent::Pointer { x: -1., y: -1. })
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.note("window-scale", json!({"scale_factor":scale_factor}))
            }
            WindowEvent::Focused(false) => {
                self.shortcuts = display::ShortcutKeys::default();
                self.modifiers = ModifiersState::empty();
                self.send_input(InputEvent::FocusLost);
            }
            WindowEvent::CursorMoved { position, .. } => {
                if self.config.is_some() {
                    let (x, y) = display::pointer_to_logical(position.x, position.y, self.letterbox());
                    self.send_input(InputEvent::Pointer { x, y });
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.send_input(InputEvent::Button {
                    pressed: state == ElementState::Pressed,
                });
            }
            WindowEvent::MouseInput {state,button,..}=>self.note("input-ignored-native",json!({"reason":"unsupported-mouse-button","button":format!("{button:?}"),"state":format!("{state:?}")})),
            WindowEvent::MouseWheel {delta,..}=>self.note("input-ignored-native",json!({"reason":"unsupported-wheel","delta":format!("{delta:?}")})),
            WindowEvent::Touch(touch)=>self.note("input-ignored-native",json!({"reason":"unsupported-touch","detail":format!("{touch:?}")})),
            WindowEvent::ModifiersChanged(modifiers)=>{
                self.modifiers = modifiers.state();
                self.note("keyboard-modifiers",json!({"state":format!("{modifiers:?}")}))
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let shortcut = match event.physical_key {
                    PhysicalKey::Code(KeyCode::Enter)=>Some(display::ShortcutKey::Enter),
                    PhysicalKey::Code(KeyCode::NumpadEnter)=>Some(display::ShortcutKey::NumpadEnter),
                    PhysicalKey::Code(KeyCode::F11)=>Some(display::ShortcutKey::F11),
                    _=>None,
                };
                if let Some(key)=shortcut {
                    let (consumed,toggle)=self.shortcuts.event(key,event.state==ElementState::Pressed,event.repeat,self.modifiers.super_key() || self.modifiers.alt_key());
                    if toggle {self.set_fullscreen(!self.is_fullscreen());}
                    if consumed {return;}
                }

                self.note("keyboard-native",json!({"physical":format!("{:?}",event.physical_key),"logical":format!("{:?}",event.logical_key),"state":format!("{:?}",event.state),"text":event.text.as_ref().map(|text|text.as_str()),"repeat":event.repeat}));
                if event.state == ElementState::Pressed {
                    if let Some(text) = &event.text {
                        if text.chars().any(|c| !c.is_control()) {
                            self.send_input(InputEvent::Text {
                                text: text.to_string(),
                            });
                        }
                    }
                }
                if let PhysicalKey::Code(key) = event.physical_key {
                    if let Some(code) = key_code(key) {
                        self.send_input(InputEvent::Key {
                            code,
                            pressed: event.state == ElementState::Pressed,
                            repeat: event.repeat,
                        });
                    }
                }
            }
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                self.note(
                    "window-resize",
                    json!({"width":size.width,"height":size.height}),
                );
                if let (Some(config), Some(surface), Some(gpu)) =
                    (&mut self.config, &self.surface, &self.gpu)
                {
                    config.width = size.width;
                    config.height = size.height;
                    surface.configure(&gpu.device, config);
                }
                self.note_layout();
            }
            WindowEvent::Resized(size)=>self.note("window-zero-size",json!({"width":size.width,"height":size.height})),
            WindowEvent::RedrawRequested => {
                while let Ok(message) = self.rx.try_recv() {
                    match message {
                        Message::Frame(mut frame) => {
                            frame.logical_size=Some(self.options.logical);
                            self.last_received = std::time::Instant::now();
                            self.stalled = false;
                            if let Err(e) = stream::emit::record_frame(&frame) {
                                self.error = Some(e);
                            }
                            if let Some(gpu) = &mut self.gpu {
                                gpu.upload(&frame);
                            }
                            self.frame = *frame;
                        }
                        Message::Finished(Err(e)) => {
                            self.error = Some(e);
                            el.exit();
                        }
                        Message::Finished(Ok(())) => {
                            el.exit();
                        }
                    }
                }
                let rect = self.letterbox();
                let logical = self.options.logical;
                if let (Some(surface), Some(gpu)) = (&self.surface, &self.gpu) {
                    match surface.get_current_texture() {
                        Ok(output) => {
                            let mut encoder =
                                gpu.device.create_command_encoder(&Default::default());
                            if let Some(target) = &gpu.logical {
                                gpu.render(
                                    &target.view,
                                    &self.frame,
                                    &mut encoder,
                                    wgpu::Extent3d {
                                        width: logical.0,
                                        height: logical.1,
                                        depth_or_array_layers: 1,
                                    },
                                );
                            }
                            gpu.present(
                                &output.texture.create_view(&Default::default()),
                                &mut encoder,
                                rect,
                            );
                            gpu.queue.submit([encoder.finish()]);
                            if let Some(name) = self.pending_capture.take() {
                                self.capture_drawable(&output.texture, &name);
                            }
                            output.present();
                            self.presents += 1;
                            if self.last_presented != self.frame.id {
                                let result = journal::emit(
                                    "frame-presented",
                                    json!({"frame_id":self.frame.id,"input_id":self.frame.input_id,"recipe_sha256":self.frame.fingerprint(),"drawable":self.config.as_ref().map(|c|[c.width,c.height])}),
                                );
                                if let Err(e) = result {
                                    self.error = Some(e);
                                    el.exit();
                                }
                                self.last_presented = self.frame.id;
                            }
                            if !self.presented && !self.frame.draws.is_empty() {
                                println!("WINDOW presented frame to visible surface");
                                self.presented = true;
                            }
                            self.run_script(el);
                        }
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            let _ = journal::emit(
                                "surface-reconfigure",
                                json!({"frame_id":self.frame.id}),
                            );
                            if let Some(config) = &self.config {
                                surface.configure(&gpu.device, config);
                            }
                        }
                        Err(e) => {
                            self.note(
                                "surface-error",
                                json!({"error":e.to_string(),"frame_id":self.frame.id}),
                            );
                            self.error = Some(e.to_string());
                            el.exit();
                        }
                    }
                }
            }
            _ => {}
        }
    }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.error.is_some() {
            el.exit();
        }
        if self.last_heartbeat.elapsed().as_secs() >= 1 {
            self.note("ui-heartbeat",json!({"last_frame_id":self.last_presented,"worker_silence_ms":self.last_received.elapsed().as_millis()}));
            self.last_heartbeat = std::time::Instant::now();
            if !self.stalled
                && self.last_presented != 0
                && self.last_received.elapsed().as_secs() >= 5
            {
                self.stalled = true;
                self.note("worker-stall", json!({"last_frame_id":self.last_presented}));
                if let Some(root) = journal::root() {
                    let result = capture(&self.frame, &root.join("stall.png"));
                    self.note(
                        "incident-image",
                        json!({"path":"stall.png","error":result.err(),"frame_id":self.frame.id}),
                    );
                }
            }
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}
pub fn window(
    worker: impl FnOnce(mpsc::SyncSender<Message>, mpsc::Receiver<PlatformInput>) + Send + 'static,
) -> Result<(), String> {
    window_with(WindowOptions::from_env()?, worker)
}
pub fn window_with(
    options: WindowOptions,
    worker: impl FnOnce(mpsc::SyncSender<Message>, mpsc::Receiver<PlatformInput>) + Send + 'static,
) -> Result<(), String> {
    if options.logical.0 == 0 || options.logical.1 == 0 {
        return Err("logical size must be non-zero".into());
    }
    let initial_frame = Frame {
        logical_size: Some(options.logical),
        ..Default::default()
    };
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let (tx, rx) = mpsc::sync_channel(1);
    let (input, input_rx) = mpsc::channel();
    std::thread::spawn(move || worker(tx, input_rx));
    let mut app = App {
        options,
        modifiers: ModifiersState::empty(),
        presents: 0,
        next_step: 0,
        pending_capture: None,
        shortcuts: display::ShortcutKeys::default(),
        layout: None,
        rx,
        window: None,
        surface: None,
        gpu: None,
        config: None,
        frame: initial_frame,
        error: None,
        presented: false,
        input,
        last_presented: 0,
        last_heartbeat: std::time::Instant::now(),
        last_received: std::time::Instant::now(),
        stalled: false,
    };
    event_loop.run_app(&mut app).map_err(|e| e.to_string())?;
    journal::emit(
        "ui-exit",
        json!({"error":app.error,"last_frame_id":app.last_presented}),
    )?;
    app.error.map_or(Ok(()), Err)
}

/// macOS physical keys to the keyboard's Windows virtual-key namespace.
/// Virtual controller-axis bindings are resolved by the script input adapter.
fn key_code(key: KeyCode) -> Option<u32> {
    Some(match key {
        KeyCode::ArrowLeft => 0x25,
        KeyCode::ArrowUp => 0x26,
        KeyCode::ArrowRight => 0x27,
        KeyCode::ArrowDown => 0x28,
        KeyCode::Enter | KeyCode::NumpadEnter => 0x0d,
        KeyCode::Escape => 0x1b,
        KeyCode::Space => 0x20,
        KeyCode::Tab => 9,
        KeyCode::Backspace => 8,
        KeyCode::Delete => 0x2e,
        KeyCode::Home => 0x24,
        KeyCode::End => 0x23,
        _ => return None,
    })
}

#[cfg(all(test, target_os = "macos"))]
mod world_tests {
    use super::*;
    #[test]
    fn world_depth_and_osd_overlay_use_separate_passes() {
        let path = std::env::temp_dir().join(format!("slr-world-depth-{}.png", std::process::id()));
        let mut frame = Frame::default();
        frame.images.insert(
            "white".into(),
            Image {
                width: 1,
                height: 1,
                rgba: Arc::new(vec![255; 4]),
            },
        );
        let triangle = |z, color| {
            vec![
                WorldVertex {
                    position: [-0.9, -0.9, z, 1.],
                    uv: [0.; 2],
                    color,
                },
                WorldVertex {
                    position: [0.9, -0.9, z, 1.],
                    uv: [0.; 2],
                    color,
                },
                WorldVertex {
                    position: [0., 0.9, z, 1.],
                    uv: [0.; 2],
                    color,
                },
            ]
        };
        // Farther green is submitted last: painter ordering would overwrite red.
        frame
            .world_draws
            .push(("white".into(), triangle(0.2, [1., 0., 0., 1.])));
        frame
            .world_draws
            .push(("white".into(), triangle(0.8, [0., 1., 0., 1.])));
        frame.draws.push((
            "white".into(),
            vec![
                Vertex {
                    position: [-0.2, -0.2],
                    uv: [0.; 2],
                    color: [0., 0., 1., 1.],
                },
                Vertex {
                    position: [0.2, -0.2],
                    uv: [0.; 2],
                    color: [0., 0., 1., 1.],
                },
                Vertex {
                    position: [0., 0.2],
                    uv: [0.; 2],
                    color: [0., 0., 1., 1.],
                },
            ],
        ));
        capture(&frame, &path).unwrap();
        let mut reader =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()))
                .read_info()
                .unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        let pixel = |x, y| {
            let i = ((y * info.width + x) * 4) as usize;
            &pixels[i..i + 4]
        };
        assert_eq!(
            pixel(400, 300),
            [0, 0, 255, 255],
            "OSD must cover the world"
        );
        assert_eq!(
            pixel(400, 450),
            [255, 0, 0, 255],
            "depth must keep the nearer triangle"
        );
        assert_eq!(
            pixel(0, 0),
            [0, 0, 0, 255],
            "clear remains black outside geometry"
        );
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(all(test, target_os = "macos"))]
mod video_tests {
    use super::*;
    #[test]
    fn video_upload_handles_max_serial_and_reused_serial_with_new_pixels() {
        pollster::block_on(async {
            let instance = wgpu::Instance::new(&Default::default());
            let mut gpu = Gpu::new(&instance, None, wgpu::TextureFormat::Rgba8Unorm)
                .await
                .unwrap();
            let path =
                std::env::temp_dir().join(format!("slr-video-upload-{}.png", std::process::id()));
            let mut frame = Frame {
                logical_size: Some((32, 32)),
                video: Some(Box::new(VideoImage {
                    serial: u64::MAX,
                    width: 1,
                    height: 1,
                    rgba: Arc::new(vec![255, 0, 0, 255]),
                })),
                ..Default::default()
            };
            for color in [[255, 0, 0, 255], [0, 255, 0, 255]] {
                let old_fingerprint = frame.fingerprint();
                frame.video.as_mut().unwrap().rgba = Arc::new(color.to_vec());
                if color[1] == 255 {
                    assert_ne!(frame.fingerprint(), old_fingerprint);
                }
                gpu.screenshot(&frame, &path).unwrap();
                let mut reader =
                    png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()))
                        .read_info()
                        .unwrap();
                let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
                let info = reader.next_frame(&mut pixels).unwrap();
                assert_eq!((info.width, info.height), (32, 32));
                let center = (16 * 32 + 16) * 4;
                assert_eq!(&pixels[center..center + 4], &color);
            }
            std::fs::remove_file(path).unwrap();
        });
    }
}
