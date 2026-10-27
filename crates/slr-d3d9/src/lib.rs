//! Tier-D textured triangle presentation on wgpu. The full D3D9 fixed-function
//! state emulator remains unfinished; this backend presents script-selected world
//! geometry with depth and perspective textures, then the script OSD.
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
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
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
#[derive(Default)]
pub struct Frame {
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
    /// Stable image recipe hash, excluding host timing, frame IDs and GPU state.
    pub fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
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
    Frame(Frame),
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
@fragment fn fs_world(v:V)->@location(0) vec4f {
 let color = textureSample(image,samp,v.uv)*v.color;
 if color.a < 0.01 { discard; }
 return color;
}
"#;
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
        })
    }
    fn upload(&mut self, frame: &Frame) {
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
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
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
                    load: if frame.world_draws.is_empty() {
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
        let (width, height) = (800u32, 600u32);
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
struct App {
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
                        .with_inner_size(winit::dpi::LogicalSize::new(800., 600.)),
                )
                .map_err(|e| e.to_string())?,
            );
            window.focus_window();
            let instance = wgpu::Instance::new(&Default::default());
            let surface = instance
                .create_surface(window.clone())
                .map_err(|e| e.to_string())?;
            let gpu = pollster::block_on(Gpu::new(
                &instance,
                Some(&surface),
                wgpu::TextureFormat::Bgra8Unorm,
            ))?;
            let size = window.inner_size();
            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
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
                self.send_input(InputEvent::FocusLost);
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(config) = &self.config {
                    self.send_input(InputEvent::Pointer {
                        x: position.x as f32 / config.width as f32,
                        y: position.y as f32 / config.height as f32,
                    });
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
            WindowEvent::ModifiersChanged(modifiers)=>self.note("keyboard-modifiers",json!({"state":format!("{modifiers:?}")})),
            WindowEvent::KeyboardInput { event, .. } => {
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
            }
            WindowEvent::Resized(size)=>self.note("window-zero-size",json!({"width":size.width,"height":size.height})),
            WindowEvent::RedrawRequested => {
                while let Ok(message) = self.rx.try_recv() {
                    match message {
                        Message::Frame(frame) => {
                            self.last_received = std::time::Instant::now();
                            self.stalled = false;
                            if let Err(e) = stream::emit::record_frame(&frame) {
                                self.error = Some(e);
                            }
                            if let Some(gpu) = &mut self.gpu {
                                gpu.upload(&frame);
                            }
                            self.frame = frame;
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
                if let (Some(surface), Some(gpu)) = (&self.surface, &self.gpu) {
                    match surface.get_current_texture() {
                        Ok(output) => {
                            let mut encoder =
                                gpu.device.create_command_encoder(&Default::default());
                            gpu.render(
                                &output.texture.create_view(&Default::default()),
                                &self.frame,
                                &mut encoder, output.texture.size(),
                            );
                            gpu.queue.submit([encoder.finish()]);
                            output.present();
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
                                println!("WINDOW presented stock menu to visible surface");
                                self.presented = true;
                            }
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
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let (tx, rx) = mpsc::sync_channel(1);
    let (input, input_rx) = mpsc::channel();
    std::thread::spawn(move || worker(tx, input_rx));
    let mut app = App {
        rx,
        window: None,
        surface: None,
        gpu: None,
        config: None,
        frame: Frame::default(),
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
