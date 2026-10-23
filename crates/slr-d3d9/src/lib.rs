//! Tier-D textured triangle presentation on wgpu. The full D3D9 fixed-function
//! state emulator remains unfinished; this backend presents the script OSD.
use std::{
    collections::HashMap,
    path::Path,
    sync::{mpsc, Arc},
};
use wgpu::util::DeviceExt;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}
#[derive(Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
#[derive(Default)]
pub struct Frame {
    pub images: HashMap<String, Image>,
    pub draws: Vec<(String, Vec<Vertex>)>,
    pub labels: Vec<String>,
    pub text_bounds: HashMap<String, [f32; 4]>,
}
pub enum Message {
    Frame(Frame),
    Finished(Result<(), String>),
}
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    textures: HashMap<String, (wgpu::Texture, wgpu::BindGroup)>,
}
const SHADER: &str = r#"
struct V { @builtin(position) p: vec4f, @location(0) uv: vec2f, @location(1) color: vec4f }
@vertex fn vs(@location(0) p:vec2f, @location(1) uv:vec2f, @location(2) color:vec4f) -> V {
 return V(vec4f(p,0.,1.),uv,color);
}
@group(0) @binding(0) var image:texture_2d<f32>;
@group(0) @binding(1) var samp:sampler;
@fragment fn fs(v:V)->@location(0) vec4f { return textureSample(image,samp,v.uv)*v.color; }
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
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("stock OSD"),
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?;
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Ok(Self {
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
            self.textures.insert(name.clone(), (texture, binding));
        }
    }
    fn render(&self, view: &wgpu::TextureView, frame: &Frame, encoder: &mut wgpu::CommandEncoder) {
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
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        for ((name, vertices), buffer) in frame.draws.iter().zip(&buffers) {
            if let Some((_, binding)) = self.textures.get(name) {
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
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                if let (Some(config), Some(surface), Some(gpu)) =
                    (&mut self.config, &self.surface, &self.gpu)
                {
                    config.width = size.width;
                    config.height = size.height;
                    surface.configure(&gpu.device, config);
                }
            }
            WindowEvent::RedrawRequested => {
                while let Ok(message) = self.rx.try_recv() {
                    match message {
                        Message::Frame(frame) => {
                            if let Some(gpu) = &mut self.gpu {
                                gpu.upload(&frame);
                            }
                            self.frame = frame;
                        }
                        Message::Finished(Err(e)) => {
                            self.error = Some(e);
                            el.exit();
                        }
                        Message::Finished(Ok(())) => {}
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
                                &mut encoder,
                            );
                            gpu.queue.submit([encoder.finish()]);
                            output.present();
                            if !self.presented && !self.frame.draws.is_empty() {
                                println!("WINDOW presented stock menu to visible surface");
                                self.presented = true;
                            }
                        }
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            if let Some(config) = &self.config {
                                surface.configure(&gpu.device, config);
                            }
                        }
                        Err(e) => {
                            self.error = Some(e.to_string());
                            el.exit();
                        }
                    }
                }
            }
            _ => {}
        }
    }
    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}
pub fn window(worker: impl FnOnce(mpsc::Sender<Message>) + Send + 'static) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || worker(tx));
    let mut app = App {
        rx,
        window: None,
        surface: None,
        gpu: None,
        config: None,
        frame: Frame::default(),
        error: None,
        presented: false,
    };
    event_loop.run_app(&mut app).map_err(|e| e.to_string())?;
    app.error.map_or(Ok(()), Err)
}
