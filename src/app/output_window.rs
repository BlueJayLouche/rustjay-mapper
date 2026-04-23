//! # Output Window
//!
//! A dedicated output window that blits a source texture to its surface
//! and manages independent NDI/Syphon outputs.

use crate::config::WindowConfig;
use crate::core::Vertex;
#[cfg(feature = "ndi")]
use crate::ndi::NdiOutputSender;
#[cfg(feature = "ndi")]
use crate::output::readback::ReadbackPool;
#[cfg(feature = "ndi")]
use crate::output::strip_row_padding;
#[cfg(target_os = "macos")]
use crate::output::syphon::SyphonOutput;

use anyhow::Result;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

/// Identifies which output type this window displays
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputType {
    Mapping,
    Matrix,
}

/// A dedicated output window with its own surface, blit pipeline,
/// and optional NDI / Syphon outputs.
pub struct OutputWindow {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,

    // Blit resources
    blit_pipeline: wgpu::RenderPipeline,
    blit_bind_group_layout: wgpu::BindGroupLayout,
    blit_sampler: wgpu::Sampler,
    vertex_buffer: wgpu::Buffer,

    // Per-output NDI
    #[cfg(feature = "ndi")]
    ndi_sender: Option<NdiOutputSender>,
    #[cfg(feature = "ndi")]
    readback_pool: ReadbackPool,

    // Per-output Syphon
    #[cfg(target_os = "macos")]
    syphon_output: Option<SyphonOutput>,

    frame_count: u64,
    output_type: OutputType,
}

impl OutputWindow {
    pub fn new(
        instance: &wgpu::Instance,
        adapter: &wgpu::Adapter,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        event_loop: &ActiveEventLoop,
        config: &WindowConfig,
        output_type: OutputType,
    ) -> Result<Self> {
        let window_attrs = WindowAttributes::default()
            .with_title(&config.title)
            .with_inner_size(winit::dpi::LogicalSize::new(config.width, config.height))
            .with_resizable(config.resizable)
            .with_decorations(config.decorated);

        let window = Arc::new(event_loop.create_window(window_attrs)?);
        window.set_cursor_visible(false);

        let size = window.inner_size();
        let surface = instance.create_surface(Arc::clone(&window))?;

        let surface_caps = surface.get_capabilities(adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let present_mode = if config.vsync {
            wgpu::PresentMode::AutoVsync
        } else {
            wgpu::PresentMode::AutoNoVsync
        };

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(device, &surface_config);

        // Blit bind group layout
        let blit_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Blit Bind Group Layout"),
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

        let blit_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Blit Shader"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
                struct VertexOutput {
                    @builtin(position) position: vec4<f32>,
                    @location(0) texcoord: vec2<f32>,
                };

                @vertex
                fn vs_main(@location(0) position: vec2<f32>, @location(1) texcoord: vec2<f32>) -> VertexOutput {
                    var out: VertexOutput;
                    out.position = vec4<f32>(position, 0.0, 1.0);
                    out.texcoord = texcoord;
                    return out;
                }

                @group(0) @binding(0)
                var source_tex: texture_2d<f32>;
                @group(0) @binding(1)
                var source_sampler: sampler;

                @fragment
                fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
                    return textureSample(source_tex, source_sampler, in.texcoord);
                }
            "#
                .into(),
            ),
        });

        let blit_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Blit Pipeline Layout"),
                bind_group_layouts: &[&blit_bind_group_layout],
                push_constant_ranges: &[],
            });

        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Blit Pipeline"),
            layout: Some(&blit_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &blit_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let vertices = Vertex::quad_vertices();
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Blit Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        Ok(Self {
            window,
            surface,
            surface_config,
            blit_pipeline,
            blit_bind_group_layout,
            blit_sampler,
            vertex_buffer,
            #[cfg(feature = "ndi")]
            ndi_sender: None,
            #[cfg(feature = "ndi")]
            readback_pool: ReadbackPool::new(),
            #[cfg(target_os = "macos")]
            syphon_output: None,
            frame_count: 0,
            output_type,
        })
    }

    /// Resize the surface to match the new window size.
    pub fn resize(&mut self, width: u32, height: u32, device: &wgpu::Device) {
        if width == 0 || height == 0 {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(device, &self.surface_config);
    }

    /// Blit `source_texture` to this window's surface and submit to NDI/Syphon.
    pub fn present(
        &mut self,
        source_texture: &wgpu::Texture,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) {
        let surface_texture = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(e) => {
                log::warn!("Surface get_current_texture error: {:?}", e);
                return;
            }
        };
        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let source_view = source_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Blit pass
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("OutputWindow Blit Encoder"),
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Blit Bind Group"),
            layout: &self.blit_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&source_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.blit_sampler),
                },
            ],
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("OutputWindow Blit Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_view,
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

            render_pass.set_pipeline(&self.blit_pipeline);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_bind_group(0, &bind_group, &[]);
            render_pass.draw(0..6, 0..1);
        }

        queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();

        // ── GPU-path outputs (zero-copy) ──────────────────────────────────────
        #[cfg(target_os = "macos")]
        if let Some(syphon) = &mut self.syphon_output {
            if let Err(e) = syphon.submit_frame(source_texture, device, queue) {
                log::error!("Syphon output error: {}", e);
            }
        }

        // ── CPU-path outputs (via readback pool) ──────────────────────────────
        #[cfg(feature = "ndi")]
        {
            let needs_readback = self.ndi_sender.is_some();
            if needs_readback {
                // Harvest the previous frame's readback (non-blocking).
                if let Some((data, w, h)) = self.readback_pool.harvest_previous() {
                    if let Some(ndi) = &self.ndi_sender {
                        let tight = strip_row_padding(&data, w, h);
                        ndi.submit_frame(tight, w, h);
                    }
                }
                // Submit copy of the current frame into the pool.
                self.readback_pool.submit_copy(source_texture, device, queue);
            }
        }

        self.frame_count += 1;
    }

    /// Toggle fullscreen on this window.
    pub fn toggle_fullscreen(&self) {
        let is_fullscreen = self.window.fullscreen().is_some();
        let new_mode = if is_fullscreen {
            None
        } else {
            Some(winit::window::Fullscreen::Borderless(None))
        };
        self.window.set_fullscreen(new_mode);
    }

    /// Set the cursor visibility on this window.
    pub fn set_cursor_visible(&self, visible: bool) {
        self.window.set_cursor_visible(visible);
    }

    /// Start NDI output for this window.
    #[cfg(feature = "ndi")]
    pub fn start_ndi(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        include_alpha: bool,
    ) -> anyhow::Result<()> {
        let sender = NdiOutputSender::new(name, width, height, include_alpha)?;
        self.ndi_sender = Some(sender);
        log::info!(
            "{:?} NDI output started: {} ({}x{})",
            self.output_type,
            name,
            width,
            height
        );
        Ok(())
    }

    /// Stop NDI output for this window.
    #[cfg(feature = "ndi")]
    pub fn stop_ndi(&mut self) {
        if self.ndi_sender.take().is_some() {
            log::info!("{:?} NDI output stopped", self.output_type);
        }
    }

    /// Check if NDI is active.
    #[cfg(feature = "ndi")]
    pub fn is_ndi_active(&self) -> bool {
        self.ndi_sender.is_some()
    }

    /// Start Syphon output for this window (macOS).
    #[cfg(target_os = "macos")]
    pub fn start_syphon(
        &mut self,
        server_name: &str,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
    ) -> anyhow::Result<()> {
        let mut syphon = SyphonOutput::new(server_name, device, queue)?;
        syphon.initialize(1920, 1080)?;
        self.syphon_output = Some(syphon);
        log::info!("{:?} Syphon output started: {}", self.output_type, server_name);
        Ok(())
    }

    /// Stop Syphon output for this window (macOS).
    #[cfg(target_os = "macos")]
    pub fn stop_syphon(&mut self) {
        if let Some(mut syphon) = self.syphon_output.take() {
            syphon.shutdown();
            log::info!("{:?} Syphon output stopped", self.output_type);
        }
    }

    /// Check if Syphon is active.
    #[cfg(target_os = "macos")]
    pub fn is_syphon_active(&self) -> bool {
        self.syphon_output.is_some()
    }

    /// Get the window ID for event routing.
    pub fn window_id(&self) -> winit::window::WindowId {
        self.window.id()
    }

    /// Get the inner window size.
    pub fn inner_size(&self) -> winit::dpi::PhysicalSize<u32> {
        self.window.inner_size()
    }

    /// Get the underlying window reference.
    pub fn window(&self) -> &Arc<Window> {
        &self.window
    }

    /// Check if fullscreen is currently active.
    pub fn is_fullscreen(&self) -> bool {
        self.window.fullscreen().is_some()
    }
}
