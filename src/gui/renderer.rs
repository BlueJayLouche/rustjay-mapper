//! # ImGui Renderer
//!
//! wgpu-based ImGui renderer for the control window.
//! Simplified approach following rustjay_waaaves pattern.

use imgui::Context;
use imgui_wgpu::{Renderer, RendererConfig, Texture, TextureConfig};
use std::sync::Arc;
use winit::window::Window;

/// ImGui renderer with wgpu integration
pub struct ImGuiRenderer {
    context: Context,
    renderer: Renderer,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    window: Arc<Window>,
}

impl ImGuiRenderer {
    pub async fn new(
        instance: &wgpu::Instance,
        adapter: &wgpu::Adapter,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        window: Arc<Window>,
        ui_scale: f32,
    ) -> anyhow::Result<Self> {
        // Create surface for control window
        let surface = instance.create_surface(Arc::clone(&window))?;
        
        // Configure surface using LOGICAL size (like rustjay_waaaves)
        let surface_caps = surface.get_capabilities(adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
        
        let size = window.inner_size();
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);
        
        // Create ImGui context (simpler approach - no winit-support)
        let mut context = Context::create();
        context.set_ini_filename(None);
        
        // Apply UI scaling via font scale only
        let ui_scale = ui_scale.clamp(0.5, 2.0);
        context.io_mut().font_global_scale = ui_scale;
        
        // Configure ImGui style - dark theme
        let style = context.style_mut();
        style.window_rounding = 4.0;
        style.frame_rounding = 2.0;
        style.scrollbar_rounding = 2.0;
        style.grab_rounding = 2.0;
        style.window_border_size = 1.0;
        style.frame_border_size = 0.0;
        
        // Dark theme colors
        use imgui::StyleColor;
        style.colors[StyleColor::WindowBg as usize] = [0.10, 0.10, 0.10, 1.0];
        style.colors[StyleColor::TitleBg as usize] = [0.15, 0.15, 0.15, 1.0];
        style.colors[StyleColor::TitleBgActive as usize] = [0.20, 0.20, 0.25, 1.0];
        style.colors[StyleColor::FrameBg as usize] = [0.20, 0.20, 0.20, 1.0];
        style.colors[StyleColor::FrameBgHovered as usize] = [0.25, 0.25, 0.25, 1.0];
        style.colors[StyleColor::FrameBgActive as usize] = [0.30, 0.30, 0.35, 1.0];
        style.colors[StyleColor::Button as usize] = [0.25, 0.25, 0.30, 1.0];
        style.colors[StyleColor::ButtonHovered as usize] = [0.35, 0.35, 0.40, 1.0];
        style.colors[StyleColor::ButtonActive as usize] = [0.40, 0.40, 0.50, 1.0];
        style.colors[StyleColor::Header as usize] = [0.30, 0.30, 0.35, 1.0];
        style.colors[StyleColor::HeaderHovered as usize] = [0.40, 0.40, 0.50, 1.0];
        style.colors[StyleColor::HeaderActive as usize] = [0.45, 0.45, 0.55, 1.0];
        style.colors[StyleColor::SliderGrab as usize] = [0.50, 0.50, 0.60, 1.0];
        style.colors[StyleColor::SliderGrabActive as usize] = [0.60, 0.60, 0.70, 1.0];
        
        // Add default font
        let font_size = 13.0 * ui_scale;
        context.fonts().add_font(&[imgui::FontSource::DefaultFontData {
            config: Some(imgui::FontConfig {
                size_pixels: font_size,
                ..imgui::FontConfig::default()
            }),
        }]);
        
        // Build font atlas
        context.fonts().build_rgba32_texture();
        
        // Create imgui-wgpu renderer
        let renderer_config = RendererConfig {
            texture_format: surface_format,
            ..Default::default()
        };
        let renderer = Renderer::new(&mut context, &device, &queue, renderer_config);
        
        Ok(Self {
            context,
            renderer,
            surface,
            surface_config,
            device,
            queue,
            window,
        })
    }
    
    /// Create a preview texture that can be displayed in ImGui
    /// Returns the TextureId to use with imgui::Image
    pub fn create_preview_texture(&mut self, width: u32, height: u32) -> imgui::TextureId {
        // Use Bgra8Unorm for consistency with macOS native format
        let texture_config = TextureConfig {
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            label: Some("Preview Texture"),
            format: Some(wgpu::TextureFormat::Bgra8Unorm),
            usage: wgpu::TextureUsages::TEXTURE_BINDING 
                | wgpu::TextureUsages::COPY_DST 
                | wgpu::TextureUsages::COPY_SRC,
            ..Default::default()
        };
        
        let texture = Texture::new(&self.device, &self.renderer, texture_config);
        
        // Insert into renderer's texture map and get the ID
        let texture_id = self.renderer.textures.insert(texture);
        
        texture_id
    }
    
    /// Update a preview texture with data from a source texture
    /// This copies the source texture to the preview texture
    pub fn update_preview_texture(
        &mut self,
        preview_id: imgui::TextureId,
        source_texture: &wgpu::Texture,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        if let Some(preview_texture) = self.renderer.textures.get(preview_id) {
            // Copy from source to preview
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: source_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: preview_texture.texture(),
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: preview_texture.width().min(source_texture.width()),
                    height: preview_texture.height().min(source_texture.height()),
                    depth_or_array_layers: 1,
                },
            );
        }
    }
    
    /// Get the wgpu texture for a preview ID (for copying data to it)
    pub fn get_preview_texture(&self, preview_id: imgui::TextureId) -> Option<&wgpu::Texture> {
        self.renderer.textures.get(preview_id).map(|t| t.texture())
    }
    
    /// Remove a registered texture
    pub fn remove_texture(&mut self, texture_id: imgui::TextureId) {
        self.renderer.textures.remove(texture_id);
    }
    
    /// Get a reference to the renderer (for texture access)
    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }
    
    /// Get a mutable reference to the renderer
    pub fn renderer_mut(&mut self) -> &mut Renderer {
        &mut self.renderer
    }
    
    /// Get reference to device
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    
    /// Get reference to queue
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
    
    /// Handle window events and update ImGui IO
    pub fn handle_event(&mut self, event: &winit::event::WindowEvent) {
        use winit::event::WindowEvent;
        
        let io = self.context.io_mut();
        
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                io.mouse_pos = [position.x as f32, position.y as f32];
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button_idx = match button {
                    winit::event::MouseButton::Left => 0,
                    winit::event::MouseButton::Right => 1,
                    winit::event::MouseButton::Middle => 2,
                    _ => return,
                };
                io.mouse_down[button_idx] = match state {
                    winit::event::ElementState::Pressed => true,
                    winit::event::ElementState::Released => false,
                };
            }
            WindowEvent::MouseWheel { delta, .. } => {
                match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => {
                        io.mouse_wheel_h = *x;
                        io.mouse_wheel = *y;
                    }
                    winit::event::MouseScrollDelta::PixelDelta(pos) => {
                        io.mouse_wheel_h = pos.x as f32 / 50.0;
                        io.mouse_wheel = pos.y as f32 / 50.0;
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                use winit::keyboard::{Key, NamedKey};
                
                // Handle special keys
                let pressed = event.state == winit::event::ElementState::Pressed;
                
                if let Key::Named(named) = &event.logical_key {
                    match named {
                        NamedKey::Tab => io.keys_down[imgui::Key::Tab as usize] = pressed,
                        NamedKey::ArrowLeft => io.keys_down[imgui::Key::LeftArrow as usize] = pressed,
                        NamedKey::ArrowRight => io.keys_down[imgui::Key::RightArrow as usize] = pressed,
                        NamedKey::ArrowUp => io.keys_down[imgui::Key::UpArrow as usize] = pressed,
                        NamedKey::ArrowDown => io.keys_down[imgui::Key::DownArrow as usize] = pressed,
                        NamedKey::PageUp => io.keys_down[imgui::Key::PageUp as usize] = pressed,
                        NamedKey::PageDown => io.keys_down[imgui::Key::PageDown as usize] = pressed,
                        NamedKey::Home => io.keys_down[imgui::Key::Home as usize] = pressed,
                        NamedKey::End => io.keys_down[imgui::Key::End as usize] = pressed,
                        NamedKey::Insert => io.keys_down[imgui::Key::Insert as usize] = pressed,
                        NamedKey::Delete => io.keys_down[imgui::Key::Delete as usize] = pressed,
                        NamedKey::Backspace => io.keys_down[imgui::Key::Backspace as usize] = pressed,
                        NamedKey::Space => io.keys_down[imgui::Key::Space as usize] = pressed,
                        NamedKey::Enter => io.keys_down[imgui::Key::Enter as usize] = pressed,
                        NamedKey::Escape => io.keys_down[imgui::Key::Escape as usize] = pressed,
                        NamedKey::Control => io.key_ctrl = pressed,
                        NamedKey::Shift => io.key_shift = pressed,
                        NamedKey::Alt => io.key_alt = pressed,
                        NamedKey::Super => io.key_super = pressed,
                        _ => {}
                    }
                }
                
                // Handle character input for text fields (only on key press, not release)
                if pressed {
                    if let Key::Character(ch) = &event.logical_key {
                        // Add character to ImGui input queue
                        for c in ch.chars() {
                            io.add_input_character(c);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    
    /// Resize the surface (logical size)
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.surface_config.width = width;
            self.surface_config.height = height;
            self.surface.configure(&self.device, &self.surface_config);
        }
    }
    
    /// Set display size for ImGui (call this before render_frame)
    pub fn set_display_size(&mut self, width: f32, height: f32) {
        self.context.io_mut().display_size = [width, height];
    }
    
    /// Get current UI scale
    pub fn ui_scale(&self) -> f32 {
        self.context.io().font_global_scale
    }
    
    /// Set UI scale
    pub fn set_ui_scale(&mut self, scale: f32) {
        self.context.io_mut().font_global_scale = scale.clamp(0.5, 2.0);
    }
    
    /// Render a frame to the control window surface
    pub fn render_frame<F: FnOnce(&mut imgui::Ui)>(
        &mut self,
        draw_fn: F,
    ) -> anyhow::Result<()> {
        // Get surface texture
        let surface_texture = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(_) => {
                self.surface.configure(&self.device, &self.surface_config);
                return Ok(());
            }
        };
        
        // Build UI frame (uses display_size set previously)
        let ui = self.context.new_frame();
        draw_fn(ui);
        let draw_data = self.context.render();
        
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ImGui Encoder"),
        });
        
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ImGui Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.1,
                            b: 0.1,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            
            let _ = self.renderer
                .render(draw_data, &self.queue, &self.device, &mut render_pass);
        }
        
        self.queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();
        
        Ok(())
    }
    
    /// Get the ImGui context
    pub fn context(&self) -> &Context {
        &self.context
    }
}
