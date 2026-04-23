//! # wgpu Renderer
//!
//! Main rendering engine using wgpu for GPU acceleration.

use crate::config::AppConfig;
use crate::core::{SharedState, Vertex, InputMapping};
use crate::engine::texture::{Texture, InputTextureManager};
use crate::videowall::{VideoWallRenderer, VideoWallConfig, VideoMatrixRenderer, VideoMatrixConfig};

use anyhow::Result;
use std::sync::Arc;
use wgpu::util::DeviceExt;

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

/// GPU rendering engine — produces intermediate textures, does not own a surface.
pub struct RenderEngine {
    #[allow(dead_code)]
    instance: wgpu::Instance,
    /// GPU adapter
    pub adapter: wgpu::Adapter,
    /// GPU device (shared with control window)
    pub device: Arc<wgpu::Device>,
    /// GPU queue (shared with control window)
    pub queue: Arc<wgpu::Queue>,
    
    // Shared state
    shared_state: Arc<std::sync::Mutex<SharedState>>,
    
    // Render pipeline
    render_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    
    // Render targets (internal resolution) — one per output path
    render_target: Texture,
    matrix_render_target: Texture,
    
    // Input texture managers — one per subsystem
    pub mapping_input_texture_manager: InputTextureManager,
    pub matrix_input_texture_manager: InputTextureManager,
    
    // Vertex buffer
    vertex_buffer: wgpu::Buffer,
    
    // Frame counter
    frame_count: u64,

    // Uniform buffers for mapping parameters (Mapping output)
    uniform_bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer_input1: wgpu::Buffer,
    uniform_buffer_input2: wgpu::Buffer,
    uniform_buffer_mix: wgpu::Buffer,
    /// Cached uniform bind group (recreated only when uniform buffers change identity)
    uniform_bind_group: wgpu::BindGroup,
    
    // Uniform buffers for matrix output
    matrix_uniform_buffer_input1: wgpu::Buffer,
    matrix_uniform_buffer_input2: wgpu::Buffer,
    matrix_uniform_buffer_mix: wgpu::Buffer,
    matrix_uniform_bind_group: wgpu::BindGroup,
    
    // Video wall renderer
    video_wall_renderer: Option<VideoWallRenderer>,
    video_wall_enabled: bool,
    video_wall_output_texture: Option<Texture>,
    
    // Video matrix renderer (grid-based mapping)
    video_matrix_renderer: Option<VideoMatrixRenderer>,
    video_matrix_enabled: bool,
    video_matrix_output_texture: Option<Texture>,
}

impl RenderEngine {
    pub async fn new(
        instance: &wgpu::Instance,
        app_config: &AppConfig,
        shared_state: Arc<std::sync::Mutex<SharedState>>,
    ) -> Result<Self> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
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
        
        // Create render target at internal resolution
        let internal_width = app_config.resolution.internal_width;
        let internal_height = app_config.resolution.internal_height;
        
        let render_target = Texture::create_render_target(
            &device,
            internal_width,
            internal_height,
            "Mapping Render Target",
        );
        let matrix_render_target = Texture::create_render_target(
            &device,
            internal_width,
            internal_height,
            "Matrix Render Target",
        );
        
        // Create input texture managers — one per subsystem
        let mapping_input_texture_manager = InputTextureManager::new(
            Arc::clone(&device),
            Arc::clone(&queue),
        );
        let matrix_input_texture_manager = InputTextureManager::new(
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
        
        // Create cached uniform bind group (Mapping)
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Mapping Uniform Bind Group"),
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
        
        // Create matrix uniform buffers
        let matrix_uniform_buffer_input1 = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Matrix Input 1 Mapping Uniform Buffer"),
            size: std::mem::size_of::<MappingUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        
        let matrix_uniform_buffer_input2 = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Matrix Input 2 Mapping Uniform Buffer"),
            size: std::mem::size_of::<MappingUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        
        let matrix_uniform_buffer_mix = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Matrix Mix Settings Uniform Buffer"),
            size: std::mem::size_of::<[f32; 4]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        
        let matrix_uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Matrix Uniform Bind Group"),
            layout: &uniform_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: matrix_uniform_buffer_input1.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: matrix_uniform_buffer_input2.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: matrix_uniform_buffer_mix.as_entire_binding(),
                },
            ],
        });

        Ok(Self {
            instance: instance.clone(),
            adapter,
            device: Arc::clone(&device),
            queue: Arc::clone(&queue),
            shared_state,
            render_pipeline,
            bind_group_layout,
            render_target,
            matrix_render_target,
            mapping_input_texture_manager,
            matrix_input_texture_manager,
            vertex_buffer,
            frame_count: 0,
            uniform_bind_group_layout,
            uniform_buffer_input1,
            uniform_buffer_input2,
            uniform_buffer_mix,
            uniform_bind_group,
            matrix_uniform_buffer_input1,
            matrix_uniform_buffer_input2,
            matrix_uniform_buffer_mix,
            matrix_uniform_bind_group,
            video_wall_renderer: None,
            video_wall_enabled: false,
            video_wall_output_texture: None,
            video_matrix_renderer: None,
            video_matrix_enabled: false,
            video_matrix_output_texture: None,
        })
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
                    wgpu::TextureFormat::Bgra8Unorm,
                ));
                
                // Create output texture for video wall
                let internal_width = self.render_target.width;
                let internal_height = self.render_target.height;
                self.video_wall_output_texture = Some(Texture::create_render_target_with_format(
                    &self.device,
                    internal_width,
                    internal_height,
                    "Video Wall Output",
                    wgpu::TextureFormat::Bgra8Unorm,
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
                    wgpu::TextureFormat::Bgra8Unorm,
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
                    wgpu::TextureFormat::Bgra8Unorm,
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
    
    /// Get reference to mapping input texture manager
    pub fn mapping_input_texture_manager(&self) -> &InputTextureManager {
        &self.mapping_input_texture_manager
    }
    
    /// Get reference to matrix input texture manager
    pub fn matrix_input_texture_manager(&self) -> &InputTextureManager {
        &self.matrix_input_texture_manager
    }
    
    /// Get reference to render target texture
    pub fn render_target(&self) -> &Texture {
        &self.render_target
    }
    
    /// Get reference to video wall output texture (if enabled)
    pub fn video_wall_output_texture(&self) -> Option<&Texture> {
        self.video_wall_output_texture.as_ref()
    }
    
    /// Get reference to video matrix output texture (if enabled)
    pub fn video_matrix_output_texture(&self) -> Option<&Texture> {
        self.video_matrix_output_texture.as_ref()
    }
    
    /// Render a frame.
    /// 
    /// Renders the main pipeline twice — once for Mapping and once for Matrix —
    /// using independent input textures and uniforms. Then optionally renders
    /// video wall and video matrix to their respective output textures.
    pub fn render(&mut self) {
        // Get current state for both subsystems
        let (
            mapping_input1_mapping,
            mapping_input2_mapping,
            mapping_mix_amount,
            matrix_input1_mapping,
            matrix_input2_mapping,
            matrix_mix_amount,
        ) = {
            let state = self.shared_state.lock().unwrap();
            (
                state.input1_mapping,
                state.input2_mapping,
                state.mix_amount,
                state.matrix_input1_mapping,
                state.matrix_input2_mapping,
                state.matrix_mix_amount,
            )
        };
        
        // Create command encoder
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });
        
        // ── Mapping pass ──────────────────────────────────────────────────────
        
        // Ensure placeholder input textures if needed
        if self.mapping_input_texture_manager.input1.is_none() {
            self.mapping_input_texture_manager.ensure_input1(1920, 1080);
            if let Some(ref tex) = self.mapping_input_texture_manager.input1 {
                tex.clear_to_black(&self.queue);
            }
        }
        
        let mapping_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Mapping Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        self.mapping_input_texture_manager.get_input1_view()
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(
                        &self.mapping_input_texture_manager.input1.as_ref().unwrap().sampler
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        self.mapping_input_texture_manager.input2.as_ref()
                            .map(|t| &t.view)
                            .unwrap_or_else(|| &self.mapping_input_texture_manager.input1.as_ref().unwrap().view)
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(
                        &self.mapping_input_texture_manager.input2.as_ref()
                            .map(|t| &t.sampler)
                            .unwrap_or_else(|| &self.mapping_input_texture_manager.input1.as_ref().unwrap().sampler)
                    ),
                },
            ],
        });
        
        let mapping_uniforms1: MappingUniforms = (&mapping_input1_mapping).into();
        let mapping_uniforms2: MappingUniforms = (&mapping_input2_mapping).into();
        let mapping_mix: [f32; 4] = [mapping_mix_amount, 0.0, 0.0, 0.0];
        
        self.queue.write_buffer(&self.uniform_buffer_input1, 0, bytemuck::bytes_of(&mapping_uniforms1));
        self.queue.write_buffer(&self.uniform_buffer_input2, 0, bytemuck::bytes_of(&mapping_uniforms2));
        self.queue.write_buffer(&self.uniform_buffer_mix, 0, bytemuck::bytes_of(&mapping_mix));
        
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Mapping Main Render Pass"),
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
            render_pass.set_bind_group(0, &mapping_bind_group, &[]);
            render_pass.set_bind_group(1, &self.uniform_bind_group, &[]);
            render_pass.draw(0..6, 0..1);
        }
        
        // ── Matrix pass ───────────────────────────────────────────────────────
        
        if self.matrix_input_texture_manager.input1.is_none() {
            self.matrix_input_texture_manager.ensure_input1(1920, 1080);
            if let Some(ref tex) = self.matrix_input_texture_manager.input1 {
                tex.clear_to_black(&self.queue);
            }
        }
        
        let matrix_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Matrix Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        self.matrix_input_texture_manager.get_input1_view()
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(
                        &self.matrix_input_texture_manager.input1.as_ref().unwrap().sampler
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        self.matrix_input_texture_manager.input2.as_ref()
                            .map(|t| &t.view)
                            .unwrap_or_else(|| &self.matrix_input_texture_manager.input1.as_ref().unwrap().view)
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(
                        &self.matrix_input_texture_manager.input2.as_ref()
                            .map(|t| &t.sampler)
                            .unwrap_or_else(|| &self.matrix_input_texture_manager.input1.as_ref().unwrap().sampler)
                    ),
                },
            ],
        });
        
        let matrix_uniforms1: MappingUniforms = (&matrix_input1_mapping).into();
        let matrix_uniforms2: MappingUniforms = (&matrix_input2_mapping).into();
        let matrix_mix: [f32; 4] = [matrix_mix_amount, 0.0, 0.0, 0.0];
        
        self.queue.write_buffer(&self.matrix_uniform_buffer_input1, 0, bytemuck::bytes_of(&matrix_uniforms1));
        self.queue.write_buffer(&self.matrix_uniform_buffer_input2, 0, bytemuck::bytes_of(&matrix_uniforms2));
        self.queue.write_buffer(&self.matrix_uniform_buffer_mix, 0, bytemuck::bytes_of(&matrix_mix));
        
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Matrix Main Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.matrix_render_target.view,
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
            render_pass.set_bind_group(0, &matrix_bind_group, &[]);
            render_pass.set_bind_group(1, &self.matrix_uniform_bind_group, &[]);
            render_pass.draw(0..6, 0..1);
        }
        
        // ── Post-process passes ───────────────────────────────────────────────
        
        // Render video matrix if enabled
        if self.video_matrix_enabled {
            if let (Some(ref mut video_matrix), Some(ref output_tex)) = 
                (self.video_matrix_renderer.as_mut(), self.video_matrix_output_texture.as_ref()) 
            {
                video_matrix.render(
                    &mut encoder,
                    &self.matrix_render_target.view,
                    &output_tex.view,
                    &self.device,
                    &self.queue,
                    output_tex.width,
                    output_tex.height,
                );
            }
        }
        
        // Render video wall if enabled
        if self.video_wall_enabled {
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
            }
        }

        // Submit commands to GPU
        self.queue.submit(std::iter::once(encoder.finish()));
        
        self.frame_count += 1;
    }
    
    /// Get references to the output textures for presentation.
    /// 
    /// Returns `(mapping_render_target, matrix_render_target, video_wall_output, video_matrix_output)`.
    /// `video_wall_output` and `video_matrix_output` are `None` when disabled.
    pub fn output_textures(&self) -> (&Texture, &Texture, Option<&Texture>, Option<&Texture>) {
        (
            &self.render_target,
            &self.matrix_render_target,
            self.video_wall_output_texture.as_ref(),
            self.video_matrix_output_texture.as_ref(),
        )
    }
    
    /// Upload calibration pattern for video wall calibration
    /// This displays the ArUco marker pattern on the mapping output
    pub fn upload_calibration_pattern(&mut self, rgba_data: &[u8], width: u32, height: u32) {
        self.mapping_input_texture_manager.ensure_input1(width, height);
        self.mapping_input_texture_manager.update_input1(rgba_data, width, height);
        log::debug!("Uploaded calibration pattern to mapping input: {}x{}", width, height);
    }
    
    /// Upload test pattern for matrix calibration
    /// Displays AprilTag marker pattern on the matrix output
    pub fn upload_test_pattern(&mut self, rgba_data: &[u8], width: u32, height: u32) -> anyhow::Result<()> {
        self.matrix_input_texture_manager.ensure_input1(width, height);
        self.matrix_input_texture_manager.update_input1(rgba_data, width, height);
        log::debug!("Uploaded test pattern to matrix input: {}x{}", width, height);
        Ok(())
    }
}
