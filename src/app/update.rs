use super::App;
use crate::videowall::{CalibrationStatus};

impl App {
    /// Update all inputs and upload frames to GPU.
    pub(super) fn update_inputs(&mut self) {
        let Some(ref mut manager) = self.input_manager else { return };
        manager.update();

        let calibration_waiting = {
            let state = self.shared_state.lock().unwrap();
            state.videowall_calibration.as_ref()
                .map(|c| c.is_ready_for_capture())
                .unwrap_or(false)
        };
        let showing_matrix_pattern = self.shared_state.lock().unwrap().matrix_showing_test_pattern;

        // --- Input 1: collect frame data while manager is in scope ---
        struct Input1Frame {
            #[cfg(target_os = "macos")]
            syphon: Option<wgpu::Texture>,
            cpu: Option<(Vec<u8>, u32, u32)>,
        }

        let input1 = if manager.input1.has_frame() && !showing_matrix_pattern {
            #[cfg(target_os = "macos")]
            {
                if manager.input1.input_type() == crate::input::InputType::Syphon {
                    Input1Frame {
                        syphon: manager.input1.take_syphon_texture(),
                        cpu: None,
                    }
                } else {
                    let frame = manager.input1.take_frame();
                    let res = manager.input1.resolution();
                    Input1Frame {
                        syphon: None,
                        cpu: frame.map(|f| (f, res.0, res.1)),
                    }
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let frame = manager.input1.take_frame();
                let res = manager.input1.resolution();
                Input1Frame {
                    cpu: frame.map(|f| (f, res.0, res.1)),
                }
            }
        } else {
            if showing_matrix_pattern && manager.input1.has_frame() {
                let _ = manager.input1.take_frame();
                #[cfg(target_os = "macos")]
                let _ = manager.input1.take_syphon_texture();
            }
            Input1Frame {
                #[cfg(target_os = "macos")]
                syphon: None,
                cpu: None,
            }
        };

        // --- Input 2: collect frame data while manager is in scope ---
        struct Input2Frame {
            #[cfg(target_os = "macos")]
            syphon: Option<wgpu::Texture>,
            cpu: Option<(Vec<u8>, u32, u32)>,
        }

        let input2 = if manager.input2.has_frame() {
            #[cfg(target_os = "macos")]
            {
                if manager.input2.input_type() == crate::input::InputType::Syphon {
                    Input2Frame {
                        syphon: manager.input2.take_syphon_texture(),
                        cpu: None,
                    }
                } else {
                    let frame = manager.input2.take_frame();
                    let res = manager.input2.resolution();
                    Input2Frame {
                        syphon: None,
                        cpu: frame.map(|f| (f, res.0, res.1)),
                    }
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let frame = manager.input2.take_frame();
                let res = manager.input2.resolution();
                Input2Frame {
                    cpu: frame.map(|f| (f, res.0, res.1)),
                }
            }
        } else {
            Input2Frame {
                #[cfg(target_os = "macos")]
                syphon: None,
                cpu: None,
            }
        };

        // manager borrow ends here — now safe to borrow self.output_engine and self.shared_state

        // Upload input 1
        #[cfg(target_os = "macos")]
        if let Some(texture) = input1.syphon {
            let (w, h) = (texture.width(), texture.height());
            if let Some(ref mut engine) = self.output_engine {
                engine.input_texture_manager.update_input1_from_texture(&texture);
            }
            let mut state = self.shared_state.lock().unwrap();
            state.ndi_input1.width = w;
            state.ndi_input1.height = h;
        }
        if let Some((frame_data, width, height)) = input1.cpu {
            if calibration_waiting {
                let mut state = self.shared_state.lock().unwrap();
                if let Some(ref mut calibration) = state.videowall_calibration {
                    log::info!("Auto-submitting camera frame {}x{} for calibration", width, height);
                    calibration.submit_frame(frame_data.clone(), width, height);
                }
            }
            if let Some(ref mut engine) = self.output_engine {
                engine.input_texture_manager.update_input1(&frame_data, width, height);
            }
            let mut state = self.shared_state.lock().unwrap();
            state.ndi_input1.width = width;
            state.ndi_input1.height = height;
        }

        // Upload input 2
        #[cfg(target_os = "macos")]
        if let Some(texture) = input2.syphon {
            let (w, h) = (texture.width(), texture.height());
            if let Some(ref mut engine) = self.output_engine {
                engine.input_texture_manager.update_input2_from_texture(&texture);
            }
            let mut state = self.shared_state.lock().unwrap();
            state.ndi_input2.width = w;
            state.ndi_input2.height = h;
        }
        if let Some((frame_data, width, height)) = input2.cpu {
            if let Some(ref mut engine) = self.output_engine {
                engine.input_texture_manager.update_input2(&frame_data, width, height);
            }
            let mut state = self.shared_state.lock().unwrap();
            state.ndi_input2.width = width;
            state.ndi_input2.height = height;
        }
    }

    /// Process video wall calibration state machine.
    pub(super) fn process_videowall_calibration(&mut self) {
        let calibration_active = {
            let mut state = self.shared_state.lock().unwrap();
            if let Some(ref mut calibration) = state.videowall_calibration {
                if calibration.is_active() {
                    match calibration.update() {
                        CalibrationStatus::InProgress
                        | CalibrationStatus::ReadyForCapture
                        | CalibrationStatus::Processing => {
                            if let Some(pattern) = calibration.current_pattern() {
                                let (width, height) = (pattern.width(), pattern.height());
                                let rgba_data: Vec<u8> = pattern.pixels()
                                    .flat_map(|p| [p[0], p[1], p[2], p[3]])
                                    .collect();
                                drop(state);
                                if let Some(ref mut engine) = self.output_engine {
                                    engine.upload_calibration_pattern(&rgba_data, width, height);
                                }
                            }
                            true
                        }
                        CalibrationStatus::Complete(config) => {
                            log::info!("Calibration complete! {} displays configured", config.displays.len());
                            state.videowall_config = Some(config.clone());
                            state.videowall_enabled = true;
                            false
                        }
                        CalibrationStatus::Error(ref e) => {
                            log::error!("Calibration error: {}", e);
                            false
                        }
                    }
                } else {
                    false
                }
            } else {
                false
            }
        };

        if !calibration_active {
            let mut state = self.shared_state.lock().unwrap();
            if let Some(ref calibration) = state.videowall_calibration {
                if !calibration.is_active() {
                    state.videowall_calibration = None;
                }
            }
        }

        self.process_matrix_test_pattern();
    }

    fn process_matrix_test_pattern(&mut self) {
        let pattern_to_display = {
            let state = self.shared_state.lock().unwrap();
            state.matrix_test_pattern.clone()
        };

        if let Some((rgba_data, width, height)) = pattern_to_display {
            if self.last_matrix_pattern != Some((width, height)) {
                if let Some(ref mut engine) = self.output_engine {
                    if let Err(e) = engine.upload_test_pattern(&rgba_data, width, height) {
                        log::error!("Failed to upload matrix test pattern: {}", e);
                    } else {
                        self.last_matrix_pattern = Some((width, height));
                        log::info!("Uploaded matrix test pattern: {}x{}", width, height);
                    }
                }
            }
        } else {
            self.last_matrix_pattern = None;
        }
    }

    /// Sync video wall enabled/config from shared state to engine.
    pub(super) fn sync_video_wall_state(&mut self) {
        let (enabled, config) = {
            let state = self.shared_state.lock().unwrap();
            (state.videowall_enabled, state.videowall_config.clone())
        };

        if let Some(ref mut engine) = self.output_engine {
            engine.set_video_wall_enabled(enabled);
            if enabled {
                if let Some(ref cfg) = config {
                    engine.update_video_wall_config(cfg);
                }
            }
        }
    }

    /// Sync video matrix config from shared state to engine.
    pub(super) fn sync_video_matrix_state(&mut self) {
        let (enabled, config) = {
            let state = self.shared_state.lock().unwrap();
            (state.video_matrix_enabled, state.video_matrix_config.clone())
        };

        let mapping_count = config.input_grid.mappings.len();

        if let Some(ref mut engine) = self.output_engine {
            let was_enabled = engine.is_video_matrix_enabled();
            engine.set_video_matrix_enabled(enabled);

            let config_changed = self.last_video_matrix_config.as_ref() != Some(&config);
            if enabled && config_changed {
                log::info!("Video matrix config updated: {} mappings", mapping_count);
                engine.update_video_matrix_config(&config);
                self.last_video_matrix_config = Some(config);
            }

            if enabled != was_enabled {
                log::info!(
                    "Video matrix {} ({} mappings)",
                    if enabled { "ENABLED" } else { "DISABLED" },
                    mapping_count
                );
            }
        }
    }

    /// Copy input/output textures into ImGui preview textures.
    pub(super) fn update_preview_textures(&mut self) {
        let (input_tex, output_tex) = {
            if let Some(ref engine) = self.output_engine {
                let input = engine.input_texture_manager().input1.as_ref()
                    .map(|t| &t.texture);
                let output = if engine.is_video_matrix_enabled() {
                    engine.video_matrix_output_texture()
                        .map(|t| &t.texture)
                        .or_else(|| Some(&engine.render_target().texture))
                } else {
                    Some(&engine.render_target().texture)
                };
                (input, output)
            } else {
                (None, None)
            }
        };

        if let Some(ref mut renderer) = self.imgui_renderer {
            if let (Some(input), Some(gui)) = (input_tex, self.control_gui.as_ref()) {
                if let Some(preview_id) = gui.input_preview_texture_id {
                    let mut encoder = renderer.device().create_command_encoder(
                        &wgpu::CommandEncoderDescriptor { label: Some("Preview Update Encoder") },
                    );
                    renderer.update_preview_texture(preview_id, input, &mut encoder);
                    renderer.queue().submit(std::iter::once(encoder.finish()));
                }
            }

            if let (Some(output), Some(gui)) = (output_tex, self.control_gui.as_ref()) {
                if let Some(preview_id) = gui.output_preview_texture_id {
                    let mut encoder = renderer.device().create_command_encoder(
                        &wgpu::CommandEncoderDescriptor { label: Some("Preview Update Encoder") },
                    );
                    renderer.update_preview_texture(preview_id, output, &mut encoder);
                    renderer.queue().submit(std::iter::once(encoder.finish()));
                }
            }
        }
    }
}
