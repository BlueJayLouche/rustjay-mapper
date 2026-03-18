use super::App;
use crate::engine::WgpuEngine;
use crate::gui::{ControlGui, ImGuiRenderer};
use crate::input::InputManager;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowAttributes;

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Create shared wgpu instance
        if self.wgpu_instance.is_none() {
            self.wgpu_instance = Some(wgpu::Instance::new(&wgpu::InstanceDescriptor {
                backends: wgpu::Backends::all(),
                ..Default::default()
            }));
        }
        let instance = self.wgpu_instance.as_ref().unwrap();

        // Create output window
        if self.output_window.is_none() {
            let window_attrs = WindowAttributes::default()
                .with_title(&self.config.output_window.title)
                .with_inner_size(winit::dpi::LogicalSize::new(
                    self.config.output_window.width,
                    self.config.output_window.height,
                ))
                .with_resizable(self.config.output_window.resizable)
                .with_decorations(self.config.output_window.decorated);

            let window = Arc::new(event_loop.create_window(window_attrs).unwrap());
            window.set_cursor_visible(false);
            self.output_window = Some(Arc::clone(&window));

            let shared_state = Arc::clone(&self.shared_state);
            let config = self.config.clone();

            match pollster::block_on(WgpuEngine::new(instance, window, &config, shared_state)) {
                Ok(engine) => {
                    log::info!("Output engine initialized");
                    self.wgpu_adapter = Some(engine.adapter.clone());
                    self.wgpu_device = Some(Arc::clone(&engine.device));
                    self.wgpu_queue = Some(Arc::clone(&engine.queue));
                    self.output_engine = Some(engine);
                }
                Err(err) => {
                    log::error!("Failed to create output engine: {}", err);
                    event_loop.exit();
                    return;
                }
            }
        }

        // Create control window
        if self.control_window.is_none() {
            if let Some(ref engine) = self.output_engine {
                let device = Arc::clone(&engine.device);
                let queue = Arc::clone(&engine.queue);

                let window_attrs = WindowAttributes::default()
                    .with_title(&self.config.control_window.title)
                    .with_inner_size(winit::dpi::LogicalSize::new(
                        self.config.control_window.width,
                        self.config.control_window.height,
                    ))
                    .with_resizable(true)
                    .with_decorations(true);

                let window = Arc::new(event_loop.create_window(window_attrs).unwrap());
                self.control_window = Some(Arc::clone(&window));

                let adapter = self.wgpu_adapter.as_ref().unwrap();

                match pollster::block_on(ImGuiRenderer::new(instance, adapter, device, queue, window, 1.0)) {
                    Ok(mut renderer) => {
                        match ControlGui::new(&self.config, Arc::clone(&self.shared_state)) {
                            Ok(mut gui) => {
                                let iw = self.config.resolution.internal_width;
                                let ih = self.config.resolution.internal_height;
                                let input_preview_id = renderer.create_preview_texture(iw, ih);
                                let output_preview_id = renderer.create_preview_texture(iw, ih);

                                gui.set_input_preview_texture(input_preview_id);
                                gui.set_output_preview_texture(output_preview_id);

                                log::info!(
                                    "Created preview textures: input={:?}, output={:?} ({}x{})",
                                    input_preview_id, output_preview_id, iw, ih
                                );

                                self.control_gui = Some(gui);
                                self.imgui_renderer = Some(renderer);
                            }
                            Err(err) => log::error!("Failed to create control GUI: {}", err),
                        }
                    }
                    Err(err) => log::error!("Failed to create ImGui renderer: {}", err),
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        // --- Output window ---
        if let Some(ref output_window) = self.output_window {
            if window_id == output_window.id() {
                match event {
                    WindowEvent::CloseRequested => event_loop.exit(),
                    WindowEvent::CursorEntered { .. } => output_window.set_cursor_visible(false),
                    WindowEvent::CursorLeft { .. } => output_window.set_cursor_visible(true),
                    WindowEvent::KeyboardInput { event, .. } => {
                        if let winit::keyboard::Key::Named(winit::keyboard::NamedKey::Shift) = &event.logical_key {
                            self.shift_pressed = event.state == winit::event::ElementState::Pressed;
                        }
                        if event.state == winit::event::ElementState::Pressed {
                            match &event.logical_key {
                                winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) => {
                                    event_loop.exit();
                                }
                                winit::keyboard::Key::Character(ch) => {
                                    if self.shift_pressed && ch.to_lowercase() == "f" {
                                        self.toggle_fullscreen();
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    WindowEvent::Resized(size) => {
                        if let Some(ref mut engine) = self.output_engine {
                            engine.resize(size.width, size.height);
                        }
                    }
                    WindowEvent::RedrawRequested => {
                        if let Some(ref mut engine) = self.output_engine {
                            engine.render();
                            self.update_preview_textures();
                        }
                    }
                    WindowEvent::MouseInput { state: button_state, button, .. } => {
                        if button == winit::event::MouseButton::Left {
                            let mut shared_state = self.shared_state.lock().unwrap();
                            if shared_state.videowall_edit_mode
                                && button_state == winit::event::ElementState::Released
                            {
                                shared_state.videowall_edit_corner = None;
                                shared_state.videowall_edit_display = None;
                            }
                        }
                    }
                    WindowEvent::CursorMoved { position, .. } => {
                        let mut shared_state = self.shared_state.lock().unwrap();
                        if shared_state.videowall_edit_mode {
                            if let (Some(display_id), Some(corner_idx)) =
                                (shared_state.videowall_edit_display, shared_state.videowall_edit_corner)
                            {
                                if let Some(ref output_window) = self.output_window {
                                    let size = output_window.inner_size();
                                    let x = position.x as f32 / size.width as f32;
                                    let y = position.y as f32 / size.height as f32;
                                    if let Some(ref mut config) = shared_state.videowall_config {
                                        if let Some(display) = config.displays.iter_mut().find(|d| d.id == display_id) {
                                            if corner_idx < 4 {
                                                display.dest_quad[corner_idx] = [x, y];
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
                return;
            }
        }

        // --- Control window ---
        if let Some(ref control_window) = self.control_window {
            if window_id == control_window.id() {
                if let Some(ref mut renderer) = self.imgui_renderer {
                    renderer.handle_event(&event);
                }

                match event {
                    WindowEvent::CloseRequested => {
                        // Close control window only; keep output running
                        self.control_window = None;
                        self.control_gui = None;
                        self.imgui_renderer = None;
                    }
                    WindowEvent::Resized(size) => {
                        if let Some(ref mut renderer) = self.imgui_renderer {
                            renderer.resize(size.width, size.height);
                        }
                    }
                    WindowEvent::RedrawRequested => {
                        if let (Some(ref mut renderer), Some(ref mut gui)) =
                            (self.imgui_renderer.as_mut(), self.control_gui.as_mut())
                        {
                            let size = control_window.inner_size();
                            renderer.set_display_size(size.width as f32, size.height as f32);
                            if let Err(err) = renderer.render_frame(|ui| gui.build_ui(ui)) {
                                log::error!("ImGui render error: {}", err);
                            }
                        }
                    }
                    _ => {}
                }
                return;
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // Lazily initialize InputManager after GPU is available
        if self.input_manager.is_none() {
            let mut manager = InputManager::new();
            if let (Some(ref device), Some(ref queue)) = (&self.wgpu_device, &self.wgpu_queue) {
                manager.initialize(device, queue);
                log::info!("InputManager initialized with wgpu resources");
            } else {
                log::warn!("InputManager initialized without wgpu resources - Syphon unavailable");
            }
            self.input_manager = Some(manager);
        }

        self.dispatch_commands();
        self.process_videowall_calibration();
        self.sync_video_wall_state();
        self.sync_video_matrix_state();
        self.update_inputs();

        if let Some(ref window) = self.output_window {
            window.request_redraw();
        }
        if let Some(ref window) = self.control_window {
            window.request_redraw();
        }
    }
}
