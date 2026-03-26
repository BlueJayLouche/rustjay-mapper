//! # wgpu Renderer
//!
//! Main rendering engine using wgpu for GPU acceleration.

use crate::config::AppConfig;
use crate::core::{SharedState, Vertex, InputMapping};
use crate::engine::texture::{Texture, InputTextureManager};
use crate::output::OutputManager;
use crate::videowall::{VideoWallRenderer, VideoWallConfig, VideoMatrixRenderer, VideoMatrixConfig};

use anyhow::Result;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::window::Window;

/// GPU representation of InputMapping
/// Must match the shader's MappingParams struct
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct MappingUniforms {
    corners: [f32; 4],      // vec4: tl_x, tl_y, tr_x, tr_y
    corners2: [f32; 4],     // vec4: br_x, br_y, bl_x, bl_y
    transform: [f32; 4],    // vec4: scale_x, scale_y, offset_x, offset_y
    settings: [f32; 4],     // vec4: rotation, opacity, blend_mode, _padding
}

impl From<&InputMapping> for MappingUniforms {
    fn from(mapping: &InputMapping) -> Self {
        Self {
            corners: [mapping.corner0[0], mapping.corner0[1], 
                      mapping.corner1[0], mapping.corner1[1]],
            corners2: [mapping.corner2[0], mapping.corner2[1],
                       mapping.corner3[0], mapping.corner3[1]],
            transform: [mapping.scale[0], mapping.scale[1],
                        mapping.offset[0], mapping.offset[1]],
            settings: [mapping.rotation.to_radians(), mapping.opacity, 
                       mapping.blend_mode as f32, 0.0],
        }
    }
}

/// Main wgpu-based rendering engine
pub struct WgpuEngine {
    #[allow(dead_code)]
    instance: wgpu::Instance,
    /// GPU adapter
    pub adapter: wgpu::Adapter,
    /// GPU device (shared with control window)
    pub device: Arc<wgpu::Device>,
    /// GPU queue (shared with control window)
    pub queue: Arc<wgpu::Queue>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    
    // Window size
    window_width: u32,
    window_height: u32,
    
    // VSync and frame rate settings
    vsync: bool,
    target_fps: u32,
    
    // Shared state
    shared_state: Arc<std::sync::Mutex<SharedState>>,
    
    // Render pipeline
    render_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    
    // Render target (internal resolution)
    render_target: Texture,
    
    // Input texture manager
    pub input_texture_manager: InputTextureManager,
    
    // Vertex buffer
    vertex_buffer: wgpu::Buffer,
    
    // Frame counter
    frame_count: u64,
    
    // Output manager (NDI, Syphon, etc.)
    output_manager: OutputManager,

    // Uniform buffers for mapping parameters
    uniform_bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer_input1: wgpu::Buffer,
    uniform_buffer_input2: wgpu::Buffer,
    uniform_buffer_mix: wgpu::Buffer,
    /// Cached uniform bind group (recreated only when uniform buffers change identity)
    uniform_bind_group: wgpu::BindGroup,

    // Cached blit pipeline (created once, reused every frame)
    blit_pipeline: wgpu::RenderPipeline,
    blit_bind_group_layout: wgpu::BindGroupLayout,
    blit_sampler: wgpu::Sampler,
    
    // Video wall renderer
    video_wall_renderer: Option<VideoWallRenderer>,
    video_wall_enabled: bool,
    video_wall_output_texture: Option<Texture>,
    
    // Video matrix renderer (grid-based mapping)
    video_matrix_renderer: Option<VideoMatrixRenderer>,
    video_matrix_enabled: bool,
    video_matrix_output_texture: Option<Texture>,
}

impl WgpuEngine {
    pub async fn new(
        instance: &wgpu::Instance,
        window: Arc<Window>,
        app_config: &AppConfig,
        shared_state: Arc<std::sync::Mutex<SharedState>>,
    ) -> Result<Self> {
        let size = window.inner_size();
        
        let surface = instance.create_surface(window)?;
        
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await?;
        
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    label: Some("Device"),
                    memory_hints: wgpu::MemoryHints::default(),
                    trace: wgpu::Trace::Off,
                },
            )
            .await?;
        
        let device = Arc::new(device);
        let queue = Arc::new(queue);
        
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
        
        let vsync = app_config.output_window.vsync;
        let target_fps = app_config.output_window.fps;
        let present_mode = if vsync {
            wgpu::PresentMode::AutoVsync
        } else {
            wgpu::PresentMode::AutoNoVsync
        };
        
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        
        // Create render target at internal resolution
        let internal_width = app_config.resolution.internal_width;
        let internal_height = app_config.resolution.internal_height;
        
        let render_target = Texture::create_render_target(
            &device,
            internal_width,
            internal_height,
            "Render Target",
        );
        
        // Create input texture manager
        let input_texture_manager = InputTextureManager::new(
            Arc::clone(&device),
            Arc::clone(&queue),
        );
        
        // Create shader
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Main Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/main.wgsl").into()),
        });
        
        // Create texture bind group layout (group 0)
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Texture Bind Group Layout"),
            entries: &[
                // Input 1 texture
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
                // Input 1 sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // Input 2 texture
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Input 2 sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        
        // Create uniform bind group layout (group 1) for mapping parameters
        let uniform_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Uniform Bind Group Layout"),
            entries: &[
                // Input 1 mapping
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Input 2 mapping
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Mix settings
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        
        // Create pipeline layout with both bind groups
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout, &uniform_bind_group_layout],
            push_constant_ranges: &[],
        });
        
        // Create separate uniform buffers for each mapping
        let uniform_buffer_input1 = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Input 1 Mapping Uniform Buffer"),
            size: std::mem::size_of::<MappingUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        
        let uniform_buffer_input2 = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Input 2 Mapping Uniform Buffer"),
            size: std::mem::size_of::<MappingUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        
        let uniform_buffer_mix = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Mix Settings Uniform Buffer"),
            size: std::mem::size_of::<[f32; 4]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        
        // Create render pipeline
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Bgra8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        
        // Create vertex buffer
        let vertices = Vertex::quad_vertices();
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        
        // Create cached uniform bind group
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Uniform Bind Group"),
            layout: &uniform_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer_input1.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniform_buffer_input2.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_buffer_mix.as_entire_binding(),
                },
            ],
        });

        // Create cached blit pipeline (reused every frame)
        let blit_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
            source: wgpu::ShaderSource::Wgsl(r#"
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
            "#.into()),
        });

        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
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

        Ok(Self {
            instance: instance.clone(),
            adapter,
            device: Arc::clone(&device),
            queue: Arc::clone(&queue),
            surface,
            config,
            window_width: size.width,
            window_height: size.height,
            vsync,
            target_fps,
            shared_state,
            render_pipeline,
            bind_group_layout,
            render_target,
            input_texture_manager,
            vertex_buffer,
            frame_count: 0,
            output_manager: OutputManager::new(),
            uniform_bind_group_layout,
            uniform_buffer_input1,
            uniform_buffer_input2,
            uniform_buffer_mix,
            uniform_bind_group,
            blit_pipeline,
            blit_bind_group_layout,
            blit_sampler,
            video_wall_renderer: None,
            video_wall_enabled: false,
            video_wall_output_texture: None,
            video_matrix_renderer: None,
            video_matrix_enabled: false,
            video_matrix_output_texture: None,
        })
    }
    
    /// Resize the surface
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.window_width = width;
            self.window_height = height;
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            log::debug!("Resized to {}x{}", width, height);
        }
    }
    
    /// Set VSync
    pub fn set_vsync(&mut self, enabled: bool) {
        if self.vsync != enabled {
            self.vsync = enabled;
            self.config.present_mode = if enabled {
                wgpu::PresentMode::AutoVsync
            } else {
                wgpu::PresentMode::AutoNoVsync
            };
            self.surface.configure(&self.device, &self.config);
            log::info!("VSync {}", if enabled { "enabled" } else { "disabled" });
        }
    }
    
    /// Set target FPS
    pub fn set_target_fps(&mut self, fps: u32) {
        self.target_fps = fps.max(1).min(240);
        log::info!("Target FPS set to {}", self.target_fps);
    }
    
    /// Enable/disable video wall rendering
    pub fn set_video_wall_enabled(&mut self, enabled: bool) {
        if self.video_wall_enabled != enabled {
            self.video_wall_enabled = enabled;
            log::info!("Video wall {}", if enabled { "enabled" } else { "disabled" });
            
            // Initialize video wall renderer if enabling
            if enabled && self.video_wall_renderer.is_none() {
                self.video_wall_renderer = Some(VideoWallRenderer::new(
                    &self.device,
                    &self.queue,
                    self.config.format,
                ));
                
                // Create output texture for video wall with same format as surface
                let internal_width = self.render_target.width;
                let internal_height = self.render_target.height;
                self.video_wall_output_texture = Some(Texture::create_render_target_with_format(
                    &self.device,
                    internal_width,
                    internal_height,
                    "Video Wall Output",
                    self.config.format,
                ));
            }
        }
    }
    
    /// Update video wall configuration (only if changed)
    pub fn update_video_wall_config(&mut self, config: &VideoWallConfig) {
        if let Some(ref mut renderer) = self.video_wall_renderer {
            // Only update if config has actually changed
            let should_update = renderer.config().map(|existing| {
                existing.displays.len() != config.displays.len() ||
                existing.grid_size != config.grid_size
            }).unwrap_or(true);
            
            if should_update {
                renderer.update_config(config, &self.device, &self.queue);
                log::info!("Video wall config updated: {} displays", config.displays.len());
            }
        }
    }
    
    /// Check if video wall is enabled
    pub fn is_video_wall_enabled(&self) -> bool {
        self.video_wall_enabled
    }
    
    /// Enable/disable video matrix rendering
    pub fn set_video_matrix_enabled(&mut self, enabled: bool) {
        if self.video_matrix_enabled != enabled {
            self.video_matrix_enabled = enabled;
            log::info!("Video matrix {}", if enabled { "enabled" } else { "disabled" });
            
            // Initialize video matrix renderer if enabling
            if enabled && self.video_matrix_renderer.is_none() {
                let mut renderer = VideoMatrixRenderer::new(
                    &self.device,
                    &self.queue,
                    self.config.format,
                );
                
                // Set output resolution from render target
                renderer.set_output_resolution(self.render_target.width, self.render_target.height);
                
                self.video_matrix_renderer = Some(renderer);
                
                // Create output texture for video matrix
                let internal_width = self.render_target.width;
                let internal_height = self.render_target.height;
                self.video_matrix_output_texture = Some(Texture::create_render_target_with_format(
                    &self.device,
                    internal_width,
                    internal_height,
                    "Video Matrix Output",
                    self.config.format,
                ));
            }
        }
    }
    
    /// Update video matrix configuration
    pub fn update_video_matrix_config(&mut self, config: &VideoMatrixConfig) {
        if let Some(ref mut renderer) = self.video_matrix_renderer {
            // Ensure output resolution is set before updating config
            renderer.set_output_resolution(self.render_target.width, self.render_target.height);
            renderer.update_config(config, &self.device, &self.queue);
            log::debug!("Video matrix config updated");
        }
    }
    
    /// Check if video matrix is enabled
    pub fn is_video_matrix_enabled(&self) -> bool {
        self.video_matrix_enabled
    }
    
    /// Get reference to input texture manager
    pub fn input_texture_manager(&self) -> &InputTextureManager {
        &self.input_texture_manager
    }
    
    /// Get reference to render target texture
    pub fn render_target(&self) -> &Texture {
        &self.render_target
    }
    
    /// Get reference to video matrix output texture (if enabled)
    pub fn video_matrix_output_texture(&self) -> Option<&Texture> {
        self.video_matrix_output_texture.as_ref()
    }
    
    /// Start NDI output
    #[cfg(feature = "ndi")]
    pub fn start_ndi_output(&mut self, name: &str, include_alpha: bool, _frame_skip: u8) -> anyhow::Result<()> {
        self.output_manager.start_ndi(
            name,
            self.render_target.width,
            self.render_target.height,
            include_alpha
        )?;
        Ok(())
    }

    /// Stop NDI output
    #[cfg(feature = "ndi")]
    pub fn stop_ndi_output(&mut self) {
        self.output_manager.stop_ndi();
    }
    
    /// Start Syphon output (macOS only)
    #[cfg(target_os = "macos")]
    pub fn start_syphon_output(&mut self, server_name: &str) -> anyhow::Result<()> {
        self.output_manager.start_syphon(
            server_name,
            Arc::clone(&self.device),
            Arc::clone(&self.queue)
        )?;
        Ok(())
    }
    
    /// Stop Syphon output (macOS only)
    #[cfg(target_os = "macos")]
    pub fn stop_syphon_output(&mut self) {
        self.output_manager.stop_syphon();
    }
    
    /// Check if Syphon is active (macOS only)
    #[cfg(target_os = "macos")]
    pub fn is_syphon_active(&self) -> bool {
        self.output_manager.is_syphon_active()
    }
    
    /// Render a frame
    pub fn render(&mut self) {
        // Get current state including mapping parameters
        let (input1_mapping, input2_mapping, mix_amount) = {
            let state = self.shared_state.lock().unwrap();
            (state.input1_mapping, state.input2_mapping, state.mix_amount)
        };
        
        // Get surface texture
        let surface_texture = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(_) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        
        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        
        // Create command encoder
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });
        
        // Ensure we have placeholder input textures if needed
        if self.input_texture_manager.input1.is_none() {
            self.input_texture_manager.ensure_input1(1920, 1080);
            // Clear to black
            if let Some(ref tex) = self.input_texture_manager.input1 {
                tex.clear_to_black(&self.queue);
            }
        }
        
        // Create bind group for shader
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        self.input_texture_manager.get_input1_view()
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(
                        &self.input_texture_manager.input1.as_ref().unwrap().sampler
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        self.input_texture_manager.input2.as_ref()
                            .map(|t| &t.view)
                            .unwrap_or_else(|| &self.input_texture_manager.input1.as_ref().unwrap().view)
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(
                        &self.input_texture_manager.input2.as_ref()
                            .map(|t| &t.sampler)
                            .unwrap_or_else(|| &self.input_texture_manager.input1.as_ref().unwrap().sampler)
                    ),
                },
            ],
        });
        
        // Update uniform buffers with mapping parameters
        let mapping1: MappingUniforms = (&input1_mapping).into();
        let mapping2: MappingUniforms = (&input2_mapping).into();
        let mix_settings: [f32; 4] = [mix_amount, 0.0, 0.0, 0.0];
        
        // Debug log mix amount periodically
        if self.frame_count % 60 == 0 {
            log::debug!("Mix amount: {:.2}", mix_amount);
        }
        
        // Write uniforms to separate buffers
        self.queue.write_buffer(&self.uniform_buffer_input1, 0, bytemuck::bytes_of(&mapping1));
        self.queue.write_buffer(&self.uniform_buffer_input2, 0, bytemuck::bytes_of(&mapping2));
        self.queue.write_buffer(&self.uniform_buffer_mix, 0, bytemuck::bytes_of(&mix_settings));
        
        // Render to render target (uniform bind group is cached; buffers updated via write_buffer)
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Main Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.render_target.view,
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
            
            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_bind_group(0, &bind_group, &[]);
            render_pass.set_bind_group(1, &self.uniform_bind_group, &[]);
            render_pass.draw(0..6, 0..1);
        }
        
        // Apply video matrix rendering if enabled
        let final_output_view = if self.video_matrix_enabled {
            if let (Some(ref mut video_matrix), Some(ref output_tex)) = 
                (self.video_matrix_renderer.as_mut(), self.video_matrix_output_texture.as_ref()) 
            {
                // Check if video matrix has config
                let has_config = video_matrix.has_config();
                let has_bind_group = video_matrix.has_uniform_bind_group();
                
                if self.frame_count % 60 == 0 {
                    log::debug!("Video Matrix: enabled={}, has_config={}, has_bind_group={}", 
                        self.video_matrix_enabled, has_config, has_bind_group);
                }
                
                // Render video matrix to output texture
                video_matrix.render(
                    &mut encoder,
                    &self.render_target.view,  // Source: main render output
                    &output_tex.view,          // Destination: video matrix output
                    &self.device,
                    &self.queue,
                    output_tex.width,
                    output_tex.height,
                );
                &output_tex.view
            } else {
                if self.frame_count % 60 == 0 {
                    log::debug!("Video Matrix: renderer={:?}, output_tex={:?}", 
                        self.video_matrix_renderer.is_some(), 
                        self.video_matrix_output_texture.is_some());
                }
                &self.render_target.view
            }
        } else if self.video_wall_enabled {
            // Fall back to video wall if matrix is not enabled
            if let (Some(ref mut video_wall), Some(ref output_tex)) = 
                (self.video_wall_renderer.as_mut(), self.video_wall_output_texture.as_ref()) 
            {
                video_wall.render(
                    &mut encoder,
                    &self.render_target.view,
                    &output_tex.view,
                    &self.device,
                    &self.queue,
                    output_tex.width,
                    output_tex.height,
                );
                &output_tex.view
            } else {
                &self.render_target.view
            }
        } else {
            &self.render_target.view
        };

        // Blit final output to surface
        self.blit_to_surface(&mut encoder, &surface_view, final_output_view);

        // Submit commands to GPU
        self.queue.submit(std::iter::once(encoder.finish()));

        // Present surface
        surface_texture.present();

        // Submit frame to all active outputs (Syphon, NDI, etc.)
        let output_texture = if self.video_matrix_enabled {
            self.video_matrix_output_texture.as_ref()
                .map(|t| &t.texture)
                .unwrap_or(&self.render_target.texture)
        } else if self.video_wall_enabled {
            self.video_wall_output_texture.as_ref()
                .map(|t| &t.texture)
                .unwrap_or(&self.render_target.texture)
        } else {
            &self.render_target.texture
        };
        self.output_manager.submit_frame(output_texture, &self.device, &self.queue);
        
        self.frame_count += 1;
    }
    
    /// Blit texture to surface using cached pipeline
    fn blit_to_surface(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        source_view: &wgpu::TextureView,
    ) {
        // Only the bind group is created per-frame (binds the source texture view)
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Blit Bind Group"),
            layout: &self.blit_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.blit_sampler),
                },
            ],
        });

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Blit Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface_view,
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
    
    /// Upload calibration pattern for video wall calibration
    /// This displays the ArUco marker pattern on the output window
    pub fn upload_calibration_pattern(&mut self, rgba_data: &[u8], width: u32, height: u32) {
        // Ensure input1 texture exists at the right size
        self.input_texture_manager.ensure_input1(width, height);
        
        // Upload the pattern data
        self.input_texture_manager.update_input1(rgba_data, width, height);
        
        log::debug!("Uploaded calibration pattern: {}x{}", width, height);
    }
    
    /// Upload test pattern for matrix calibration
    /// Displays AprilTag marker pattern on the output window
    pub fn upload_test_pattern(&mut self, rgba_data: &[u8], width: u32, height: u32) -> anyhow::Result<()> {
        // Ensure input1 texture exists at the right size
        self.input_texture_manager.ensure_input1(width, height);
        
        // Upload the pattern data
        self.input_texture_manager.update_input1(rgba_data, width, height);
        
        log::debug!("Uploaded matrix test pattern: {}x{}", width, height);
        Ok(())
    }
}
