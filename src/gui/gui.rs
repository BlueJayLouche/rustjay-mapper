//! # Control GUI
//!
//! ImGui-based control interface for the application.
//! Supports multiple input types: Webcam, NDI, OBS (via NDI)

// Allow deprecated ComboBox API - imgui 0.12 uses the older API
#![allow(deprecated)]

use crate::config::AppConfig;
use crate::core::{SharedState, NdiOutputCommand, InputChangeRequest, InputMapping};
use crate::videowall::{CalibrationController, CalibrationPhase, CalibrationStatus, GridSize, PresetManager, ConfigPreset};
use std::sync::{Arc, Mutex};

/// Main GUI tabs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainTab {
    Inputs,
    Mapping,
    VideoWall,
    Output,
    Settings,
}

/// Control GUI state
pub struct ControlGui {
    shared_state: Arc<Mutex<SharedState>>,
    
    // Current tab
    current_tab: MainTab,
    
    // Device lists
    webcam_devices: Vec<String>,
    ndi_sources: Vec<String>,
    
    // Selection state
    selected_webcam1: i32,
    selected_webcam2: i32,
    selected_ndi1: i32,
    selected_ndi2: i32,
    
    // Device selector popup
    show_device_selector: bool,
    selector_for_input: i32, // 1 or 2
    
    // Mapping tab input selection (0 = Input 1, 1 = Input 2)
    mapping_tab_input: i32,
    
    // Output
    ndi_output_name: String,
    syphon_server_name: String,
    
    // Mapping edit state (local copy to reduce lock contention)
    mapping_edit_input1: InputMapping,
    mapping_edit_input2: InputMapping,
    mapping_needs_update: bool,
    
    // Video Wall state
    videowall_grid_columns: i32,
    videowall_grid_rows: i32,
    videowall_show_preview: bool,
    videowall_camera_resolution: (u32, u32),
    videowall_output_resolution: (u32, u32),
    videowall_marker_size: f32,
    videowall_auto_detect: bool,
    
    // Per-display adjustment state
    videowall_selected_display: i32,
    videowall_preset_name: String,
    videowall_presets: Vec<String>,
}

impl ControlGui {
    pub fn new(_config: &AppConfig, shared_state: Arc<Mutex<SharedState>>) -> anyhow::Result<Self> {
        let (ndi_output_name, syphon_server_name, mapping1, mapping2) = {
            let state = shared_state.lock().unwrap();
            (
                state.ndi_output.stream_name.clone(),
                state.syphon_output.server_name.clone(),
                state.input1_mapping,
                state.input2_mapping,
            )
        };
        
        Ok(Self {
            shared_state,
            current_tab: MainTab::Inputs,
            webcam_devices: Vec::new(),
            ndi_sources: Vec::new(),
            selected_webcam1: 0,
            selected_webcam2: 0,
            selected_ndi1: 0,
            selected_ndi2: 0,
            show_device_selector: false,
            selector_for_input: 1,
            mapping_tab_input: 0,
            ndi_output_name,
            syphon_server_name,
            mapping_edit_input1: mapping1,
            mapping_edit_input2: mapping2,
            mapping_needs_update: false,
            videowall_grid_columns: 3,
            videowall_grid_rows: 3,
            videowall_show_preview: true,
            videowall_camera_resolution: (1920, 1080),
            videowall_output_resolution: (1920, 1080),
            videowall_marker_size: 0.75,
            videowall_auto_detect: true,
            videowall_selected_display: 0,
            videowall_preset_name: String::new(),
            videowall_presets: Vec::new(),
        })
    }
    
    /// Refresh all device lists
    pub fn refresh_devices(&mut self) {
        #[cfg(feature = "webcam")]
        {
            self.webcam_devices = crate::input::list_cameras();
            log::info!("Found {} webcam devices", self.webcam_devices.len());
        }
        
        self.ndi_sources = crate::input::list_ndi_sources(2000);
        log::info!("Found {} NDI sources", self.ndi_sources.len());
    }
    
    /// Sync mapping edits back to shared state
    fn sync_mapping_to_state(&mut self) {
        if self.mapping_needs_update {
            let mut state = self.shared_state.lock().unwrap();
            state.input1_mapping = self.mapping_edit_input1;
            state.input2_mapping = self.mapping_edit_input2;
            self.mapping_needs_update = false;
        }
    }
    
    /// Build the ImGui UI - builds directly to root (no wrapper window)
    pub fn build_ui(&mut self, ui: &mut imgui::Ui) {
        // Sync mapping changes to shared state
        self.sync_mapping_to_state();
        
        // Menu bar at the top
        self.build_menu_bar(ui);
        
        // Main tab bar with content
        self.build_main_tabs(ui);
        
        // Device selector window (separate popup window)
        if self.show_device_selector {
            self.build_device_selector(ui);
        }
    }
    
    /// Build the menu bar
    fn build_menu_bar(&mut self, ui: &imgui::Ui) {
        ui.menu_bar(|| {
            ui.menu("File", || {
                if ui.menu_item("Exit") {
                    // Exit handled by app
                }
            });
            
            ui.menu("Devices", || {
                if ui.menu_item("Refresh All") {
                    self.refresh_devices();
                }
            });
        });
    }
    
    /// Build main tab bar - uses imgui 0.12 tab API
    fn build_main_tabs(&mut self, ui: &imgui::Ui) {
        let tab_labels = [("Inputs", MainTab::Inputs), 
                          ("Mapping", MainTab::Mapping), 
                          ("Video Wall", MainTab::VideoWall),
                          ("Output", MainTab::Output),
                          ("Settings", MainTab::Settings)];
        
        // Use tab_bar/tab_item for proper tab behavior in imgui 0.12
        if let Some(_tab_bar) = ui.tab_bar("##main_tabs") {
            for (label, tab) in tab_labels.iter() {
                let is_selected = self.current_tab == *tab;
                
                if let Some(_tab) = ui.tab_item(label) {
                    if !is_selected {
                        self.current_tab = *tab;
                    }
                }
            }
        }
        
        ui.separator();
        
        // Build content for current tab
        match self.current_tab {
            MainTab::Inputs => self.build_inputs_tab(ui),
            MainTab::Mapping => self.build_mapping_tab(ui),
            MainTab::VideoWall => self.build_videowall_tab(ui),
            MainTab::Output => self.build_output_tab(ui),
            MainTab::Settings => self.build_settings_tab(ui),
        }
    }
    
    /// Build the Inputs tab
    fn build_inputs_tab(&mut self, ui: &imgui::Ui) {
        ui.text("Input Sources");
        ui.separator();
        
        // Input 1 section
        ui.text_colored([0.0, 1.0, 1.0, 1.0], "Input 1 (Primary)");
        self.build_input_section(ui, 1);
        
        ui.spacing();
        ui.separator();
        ui.spacing();
        
        // Input 2 section
        ui.text_colored([1.0, 0.5, 0.0, 1.0], "Input 2 (Secondary)");
        self.build_input_section(ui, 2);
        
        ui.spacing();
        ui.separator();
        
        // Mix controls
        ui.text("Mix Controls");
        let mut mix_amount = {
            let state = self.shared_state.lock().unwrap();
            state.mix_amount
        };
        let old_mix = mix_amount;
        if ui.slider("Mix Amount", 0.0, 1.0, &mut mix_amount) {
            let mut state = self.shared_state.lock().unwrap();
            state.mix_amount = mix_amount;
            log::debug!("Mix slider changed: {:.2} -> {:.2}", old_mix, mix_amount);
        }
        ui.same_line();
        ui.text(format!("{:.0}% Input 2 (current: {:.2})", mix_amount * 100.0, mix_amount));
    }
    
    /// Build a single input section
    fn build_input_section(&mut self, ui: &imgui::Ui, input_num: i32) {
        let (is_active, source_name, input_type_str) = {
            let state = self.shared_state.lock().unwrap();
            let input_state = if input_num == 1 { &state.ndi_input1 } else { &state.ndi_input2 };
            (
                input_state.is_active,
                input_state.source_name.clone(),
                if input_state.is_active { "Active" } else { "None" },
            )
        };
        
        // Status display
        ui.text(format!("Status: {}", input_type_str));
        if is_active {
            ui.text(format!("Source: {}", source_name));
        }
        
        // Action buttons
        if ui.button(format!("Select Source##{}", input_num)) {
            self.selector_for_input = input_num;
            self.show_device_selector = true;
            self.refresh_devices();
        }
        
        if is_active {
            ui.same_line();
            if ui.button(format!("Stop##{}", input_num)) {
                let mut state = self.shared_state.lock().unwrap();
                if input_num == 1 {
                    state.input1_request = InputChangeRequest::StopInput;
                } else {
                    state.input2_request = InputChangeRequest::StopInput;
                }
            }
            
            ui.same_line();
            if ui.button(format!("Edit Mapping##{}", input_num)) {
                self.current_tab = MainTab::Mapping;
            }
        }
    }
    
    /// Build the Mapping tab
    fn build_mapping_tab(&mut self, ui: &imgui::Ui) {
        ui.text("Projection Mapping");
        ui.separator();
        
        // Select which input to map
        ui.text("Select Input to Map:");
        ui.radio_button("Input 1", &mut self.mapping_tab_input, 0);
        ui.same_line();
        ui.radio_button("Input 2", &mut self.mapping_tab_input, 1);
        
        ui.separator();
        
        // Get the mapping to edit
        let mapping = if self.mapping_tab_input == 0 {
            &mut self.mapping_edit_input1
        } else {
            &mut self.mapping_edit_input2
        };
        
        // Corner pinning section
        ui.text_colored([1.0, 1.0, 0.0, 1.0], "Corner Pinning (UV Coordinates)");
        ui.text("Drag corners to warp the input");
        
        // Top row
        ui.columns(2, "corners_top", false);
        ui.text("Top-Left");
        if ui.slider("TL X", 0.0, 1.0, &mut mapping.corner0[0]) { self.mapping_needs_update = true; }
        if ui.slider("TL Y", 0.0, 1.0, &mut mapping.corner0[1]) { self.mapping_needs_update = true; }
        ui.next_column();
        ui.text("Top-Right");
        if ui.slider("TR X", 0.0, 1.0, &mut mapping.corner1[0]) { self.mapping_needs_update = true; }
        if ui.slider("TR Y", 0.0, 1.0, &mut mapping.corner1[1]) { self.mapping_needs_update = true; }
        ui.columns(1, "", false);
        
        // Bottom row
        ui.columns(2, "corners_bottom", false);
        ui.text("Bottom-Left");
        if ui.slider("BL X", 0.0, 1.0, &mut mapping.corner3[0]) { self.mapping_needs_update = true; }
        if ui.slider("BL Y", 0.0, 1.0, &mut mapping.corner3[1]) { self.mapping_needs_update = true; }
        ui.next_column();
        ui.text("Bottom-Right");
        if ui.slider("BR X", 0.0, 1.0, &mut mapping.corner2[0]) { self.mapping_needs_update = true; }
        if ui.slider("BR Y", 0.0, 1.0, &mut mapping.corner2[1]) { self.mapping_needs_update = true; }
        ui.columns(1, "", false);
        
        ui.separator();
        
        // Global transforms
        ui.text_colored([1.0, 1.0, 0.0, 1.0], "Global Transform");
        if ui.slider("Scale X", 0.1, 3.0, &mut mapping.scale[0]) { self.mapping_needs_update = true; }
        if ui.slider("Scale Y", 0.1, 3.0, &mut mapping.scale[1]) { self.mapping_needs_update = true; }
        if ui.slider("Offset X", -1.0, 1.0, &mut mapping.offset[0]) { self.mapping_needs_update = true; }
        if ui.slider("Offset Y", -1.0, 1.0, &mut mapping.offset[1]) { self.mapping_needs_update = true; }
        if ui.slider("Rotation", -180.0, 180.0, &mut mapping.rotation) { self.mapping_needs_update = true; }
        
        ui.separator();
        
        // Opacity and blend
        ui.text_colored([1.0, 1.0, 0.0, 1.0], "Blend Settings");
        if ui.slider("Opacity", 0.0, 1.0, &mut mapping.opacity) { self.mapping_needs_update = true; }
        
        let blend_modes = ["Normal", "Add", "Multiply", "Screen"];
        ui.text("Blend Mode:");
        for (i, mode) in blend_modes.iter().enumerate() {
            if ui.radio_button(mode, &mut mapping.blend_mode, i as i32) {
                self.mapping_needs_update = true;
            }
            if i < blend_modes.len() - 1 {
                ui.same_line();
            }
        }
        
        ui.separator();
        
        // Reset button
        if ui.button("Reset to Default") {
            mapping.reset();
            self.mapping_needs_update = true;
        }
        ui.same_line();
        if ui.button("Reset Corners Only") {
            mapping.corner0 = [0.0, 0.0];
            mapping.corner1 = [1.0, 0.0];
            mapping.corner2 = [1.0, 1.0];
            mapping.corner3 = [0.0, 1.0];
            self.mapping_needs_update = true;
        }
    }
    
    /// Build the Output tab
    fn build_output_tab(&mut self, ui: &imgui::Ui) {
        ui.text("Output Settings");
        ui.separator();
        
        // Fullscreen toggle
        let mut fullscreen = {
            let state = self.shared_state.lock().unwrap();
            state.output_fullscreen
        };
        
        if ui.checkbox("Fullscreen Output", &mut fullscreen) {
            let mut state = self.shared_state.lock().unwrap();
            state.output_fullscreen = fullscreen;
        }
        
        ui.separator();
        
        // NDI Output section
        ui.text_colored([0.0, 1.0, 0.5, 1.0], "NDI Output");
        
        ui.input_text("Stream Name", &mut self.ndi_output_name)
            .build();
        
        let ndi_active = {
            let state = self.shared_state.lock().unwrap();
            state.ndi_output.is_active
        };
        
        if !ndi_active {
            if ui.button("Start NDI Output") {
                let mut state = self.shared_state.lock().unwrap();
                state.ndi_output.stream_name = self.ndi_output_name.clone();
                state.ndi_output_command = NdiOutputCommand::Start;
            }
        } else {
            if ui.button("Stop NDI Output") {
                let mut state = self.shared_state.lock().unwrap();
                state.ndi_output_command = NdiOutputCommand::Stop;
            }
        }
        
        // Syphon Output section (macOS only)
        #[cfg(target_os = "macos")]
        {
            ui.separator();
            ui.text_colored([1.0, 0.5, 0.0, 1.0], "Syphon Output (macOS)");
            ui.text_disabled("Share GPU texture with Resolume, MadMapper, etc.");
            
            // Syphon server name input
            ui.input_text("Server Name", &mut self.syphon_server_name)
                .build();
            
            // Check if syphon should be active from shared state
            let syphon_requested = {
                let state = self.shared_state.lock().unwrap();
                state.syphon_output.enabled
            };
            
            if !syphon_requested {
                if ui.button("Start Syphon Output") {
                    let mut state = self.shared_state.lock().unwrap();
                    state.syphon_output.server_name = self.syphon_server_name.clone();
                    state.syphon_output.enabled = true;
                }
            } else {
                if ui.button("Stop Syphon Output") {
                    let mut state = self.shared_state.lock().unwrap();
                    state.syphon_output.enabled = false;
                }
            }
            
            ui.text(format!("Status: {}", 
                if syphon_requested { "Active" } else { "Inactive" }));
        }
        
        // Status
        ui.separator();
        ui.text("Status:");
        let state = self.shared_state.lock().unwrap();
        ui.text(format!("NDI Output: {}", 
            if state.ndi_output.is_active { "Active" } else { "Inactive" }));
        ui.text(format!("Input 1: {} ({}x{})", 
            if state.ndi_input1.is_active { "Active" } else { "Inactive" },
            state.ndi_input1.width,
            state.ndi_input1.height));
        ui.text(format!("Input 2: {} ({}x{})", 
            if state.ndi_input2.is_active { "Active" } else { "Inactive" },
            state.ndi_input2.width,
            state.ndi_input2.height));
    }
    
    /// Build the Settings tab
    fn build_settings_tab(&mut self, ui: &imgui::Ui) {
        ui.text("Application Settings");
        ui.separator();
        
        ui.text("UI Scale:");
        let mut ui_scale = {
            let state = self.shared_state.lock().unwrap();
            state.ui_scale
        };
        if ui.slider("Scale", 0.5, 2.0, &mut ui_scale) {
            let mut state = self.shared_state.lock().unwrap();
            state.ui_scale = ui_scale;
        }
        
        ui.separator();
        
        ui.text("Keyboard Shortcuts:");
        ui.bullet_text("Shift+F - Toggle Fullscreen");
        ui.bullet_text("Escape - Exit Application");
        
        ui.separator();
        
        if ui.button("Refresh All Devices") {
            self.refresh_devices();
        }
    }
    
    /// Build the Video Wall tab
    fn build_videowall_tab(&mut self, ui: &imgui::Ui) {
        ui.text("Video Wall Auto-Calibration");
        ui.separator();
        
        // Grid size selection
        ui.text("Grid Size:");
        ui.input_int("Columns", &mut self.videowall_grid_columns).build();
        ui.input_int("Rows", &mut self.videowall_grid_rows).build();
        
        // Clamp to valid range
        self.videowall_grid_columns = self.videowall_grid_columns.clamp(1, 4);
        self.videowall_grid_rows = self.videowall_grid_rows.clamp(1, 4);
        
        // Marker size for calibration
        ui.slider_config("Marker Size", 0.3, 0.95)
            .display_format("%.0f%%")
            .build(&mut self.videowall_marker_size);
        ui.text_disabled(format!("Marker fills {:.0}% of each display", self.videowall_marker_size * 100.0));
        
        // Auto-detect mode checkbox
        if ui.checkbox("Auto-detect displays", &mut self.videowall_auto_detect) {
            log::info!("Auto-detect mode: {}", self.videowall_auto_detect);
        }
        if self.videowall_auto_detect {
            ui.text_disabled("Will detect any number of displays (0-9)");
        } else {
            ui.text_disabled("Will expect all displays in grid to be present");
        }
        
        ui.separator();
        
        // Check if calibration is active from shared state
        let is_calibrating = {
            let state = self.shared_state.lock().unwrap();
            state.videowall_calibration.as_ref()
                .map(|c| c.is_active())
                .unwrap_or(false)
        };
        
        if !is_calibrating {
            // Not calibrating - show calibration start and config management
            self.build_videowall_calibration_section(ui);
            ui.separator();
            self.build_videowall_preset_section(ui);
            ui.separator();
            self.build_videowall_display_adjustments(ui);
        } else {
            // Calibration in progress
            self.build_calibration_progress(ui);
        }
    }
    
    /// Build the calibration start section
    fn build_videowall_calibration_section(&mut self, ui: &imgui::Ui) {
        ui.text("Calibration");
        ui.text_disabled("Configure displays and capture markers");
        
        // Camera selection
        ui.text("Camera Source:");
        
        // Show available cameras (simplified - in practice, enumerate cameras)
        let cameras = vec!["Webcam (Default)", "iPhone via Continuity", "External USB"];
        let mut selected_camera = 0usize;
        ui.combo_simple_string("##camera", &mut selected_camera, &cameras);
        
        ui.spacing();
        
        ui.set_next_item_width(150.0);
        if ui.button("Start Calibration") {
            let grid_size = GridSize::new(
                self.videowall_grid_columns as u32,
                self.videowall_grid_rows as u32,
            );
            
            let mut calibration = CalibrationController::new()
                .with_marker_config(crate::videowall::MarkerDisplayConfig {
                    marker_size_percent: self.videowall_marker_size,
                    margin_percent: (1.0 - self.videowall_marker_size) / 2.0,
                })
                .with_auto_detect(self.videowall_auto_detect);
            
            match calibration.start_realtime(
                grid_size,
                self.videowall_camera_resolution,
                self.videowall_output_resolution,
            ) {
                Ok(_) => {
                    log::info!("Started video wall calibration for {:?} grid", grid_size);
                    let mut state = self.shared_state.lock().unwrap();
                    state.videowall_calibration = Some(calibration);
                }
                Err(e) => {
                    log::error!("Failed to start calibration: {}", e);
                }
            }
        }
        
        ui.same_line();
        
        ui.same_line();
        if ui.button("Load from Photo") {
            // In a real implementation, open file picker dialog
            log::info!("Photo upload requested - would open file picker");
        }
        
        // Configuration status
        let has_config = {
            let state = self.shared_state.lock().unwrap();
            state.videowall_config.is_some()
        };
        
        if has_config {
            ui.text_colored([0.0, 1.0, 0.0, 1.0], "Configuration loaded");
            
            ui.same_line();
            
            if ui.button("Clear") {
                let mut state = self.shared_state.lock().unwrap();
                state.videowall_config = None;
                state.videowall_enabled = false;
            }
        } else {
            ui.text_disabled("No configuration loaded");
        }
    }
    
    /// Build preset save/load section
    fn build_videowall_preset_section(&mut self, ui: &imgui::Ui) {
        ui.text("Presets");
        ui.text_disabled("Save and load display configurations");
        
        // Quick save
        if ui.button("Quick Save") {
            let manager = PresetManager::new();
            let state = self.shared_state.lock().unwrap();
            if let Some(ref config) = state.videowall_config {
                match manager.quick_save(config) {
                    Ok(path) => log::info!("Saved preset to {:?}", path),
                    Err(e) => log::error!("Failed to save preset: {}", e),
                }
            }
        }
        
        ui.same_line();
        
        // Named save
        ui.input_text("##preset_name", &mut self.videowall_preset_name)
            .hint("Preset name...")
            .build();
        ui.same_line();
        if ui.button("Save") {
            if !self.videowall_preset_name.is_empty() {
                let manager = PresetManager::new();
                let state = self.shared_state.lock().unwrap();
                if let Some(ref config) = state.videowall_config {
                    let preset = ConfigPreset::new(&self.videowall_preset_name, config.clone());
                    match manager.save_preset(&preset) {
                        Ok(_) => {
                            log::info!("Saved preset '{}'", self.videowall_preset_name);
                            self.videowall_preset_name.clear();
                        }
                        Err(e) => log::error!("Failed to save preset: {}", e),
                    }
                }
            }
        }
        
        // Load preset
        ui.spacing();
        
        // Refresh preset list
        if ui.button("Refresh") {
            let manager = PresetManager::new();
            match manager.list_presets() {
                Ok(presets) => {
                    self.videowall_presets = presets.into_iter()
                        .map(|p| p.name)
                        .collect();
                }
                Err(e) => log::error!("Failed to list presets: {}", e),
            }
        }
        
        if !self.videowall_presets.is_empty() {
            ui.same_line();
            
            let presets: Vec<&str> = self.videowall_presets.iter()
                .map(|s| s.as_str())
                .collect();
            let mut selected = 0usize;
            
            if ui.combo_simple_string("##presets", &mut selected, &presets) {
                // Load selected preset
                let manager = PresetManager::new();
                match manager.load_preset(&self.videowall_presets[selected as usize]) {
                    Ok(preset) => {
                        let mut state = self.shared_state.lock().unwrap();
                        state.videowall_config = Some(preset.config);
                        state.videowall_enabled = true;
                        log::info!("Loaded preset '{}'", self.videowall_presets[selected as usize]);
                    }
                    Err(e) => log::error!("Failed to load preset: {}", e),
                }
            }
        }
    }
    
    /// Build per-display adjustment controls
    fn build_videowall_display_adjustments(&mut self, ui: &imgui::Ui) {
        let (has_config, display_count) = {
            let state = self.shared_state.lock().unwrap();
            let count = state.videowall_config.as_ref()
                .map(|c| c.displays.len())
                .unwrap_or(0);
            (state.videowall_config.is_some(), count)
        };
        
        if !has_config {
            ui.text_disabled("Calibrate or load a preset to adjust displays");
            return;
        }
        
        ui.text("Display Adjustments");
        ui.text_disabled("Per-display color and position adjustments");
        
        // Display selector
        let max_display = display_count.saturating_sub(1).max(0) as i32;
        self.videowall_selected_display = self.videowall_selected_display.clamp(0, max_display);
        
        ui.input_int("Display #", &mut self.videowall_selected_display).build();
        self.videowall_selected_display = self.videowall_selected_display.clamp(0, max_display);
        
        // Get current display info and create local copies for editing
        let display_id = self.videowall_selected_display as u32;
        let display_info = {
            let state = self.shared_state.lock().unwrap();
            state.videowall_config.as_ref()
                .and_then(|c| c.displays.iter().find(|d| d.id == display_id).cloned())
        };
        
        if let Some(display) = display_info {
            let mut enabled = display.enabled;
            let mut brightness = display.brightness;
            let mut contrast = display.contrast;
            let mut gamma = display.gamma;
            
            // Enabled toggle
            if ui.checkbox("Enabled", &mut enabled) {
                let mut state = self.shared_state.lock().unwrap();
                if let Some(ref mut config) = state.videowall_config {
                    config.set_display_enabled(display_id, enabled);
                }
            }
            
            ui.separator();
            
            // Color adjustments
            ui.text("Color Adjustments (applied after sampling):");
            
            let mut changed = false;
            
            if ui.slider_config("Brightness", 0.0, 2.0)
                .display_format("%.2f")
                .build(&mut brightness) 
            {
                changed = true;
            }
            
            if ui.slider_config("Contrast", 0.0, 2.0)
                .display_format("%.2f")
                .build(&mut contrast) 
            {
                changed = true;
            }
            
            if ui.slider_config("Gamma", 0.1, 3.0)
                .display_format("%.2f")
                .build(&mut gamma) 
            {
                changed = true;
            }
            
            if changed {
                let mut state = self.shared_state.lock().unwrap();
                if let Some(ref mut config) = state.videowall_config {
                    config.update_display_adjustments(
                        display_id,
                        Some(brightness),
                        Some(contrast),
                        Some(gamma),
                    );
                }
            }
            
            ui.separator();
            
            // Reset button
            if ui.button("Reset to Defaults") {
                let mut state = self.shared_state.lock().unwrap();
                if let Some(ref mut config) = state.videowall_config {
                    config.update_display_adjustments(
                        display_id,
                        Some(1.0),
                        Some(1.0),
                        Some(1.0),
                    );
                }
            }
            
            ui.same_line();
            
            // Reset ALL displays
            if ui.button("Reset All Displays") {
                let mut state = self.shared_state.lock().unwrap();
                if let Some(ref mut config) = state.videowall_config {
                    config.reset_adjustments();
                }
            }
        }
        
        ui.separator();
        
        // Manual Corner Adjustment
        self.build_corner_adjustment_controls(ui);
    }
    
    /// Build manual corner adjustment controls
    fn build_corner_adjustment_controls(&mut self, ui: &imgui::Ui) {
        ui.text("Manual Corner Adjustment");
        ui.text_disabled("Fine-tune display corners by dragging or using sliders");
        
        // Edit mode toggle
        let mut edit_mode = {
            let state = self.shared_state.lock().unwrap();
            state.videowall_edit_mode
        };
        
        if ui.checkbox("Enable Edit Mode", &mut edit_mode) {
            let mut state = self.shared_state.lock().unwrap();
            state.videowall_edit_mode = edit_mode;
            if !edit_mode {
                // Clear selection when disabling edit mode
                state.videowall_edit_display = None;
                state.videowall_edit_corner = None;
            }
        }
        
        if !edit_mode {
            return;
        }
        
        // Corner selection
        let display_id = self.videowall_selected_display as u32;
        let corner_names = ["Top-Left", "Top-Right", "Bottom-Right", "Bottom-Left"];
        
        // Get current corners
        let corners = {
            let state = self.shared_state.lock().unwrap();
            state.videowall_config.as_ref()
                .and_then(|c| c.displays.iter().find(|d| d.id == display_id))
                .map(|d| d.dest_quad)
        };
        
        if let Some(corners) = corners {
            ui.text("Adjust Corners:");
            
            for (i, name) in corner_names.iter().enumerate() {
                let mut corner = corners[i];
                
                ui.text(*name);
                
                // X coordinate
                let mut x = corner[0];
                ui.set_next_item_width(100.0);
                if ui.slider_config(&format!("##x{}", i), 0.0, 1.0)
                    .display_format("X: %.3f")
                    .build(&mut x)
                {
                    corner[0] = x;
                    self.update_display_corner(display_id, i, corner);
                }
                
                ui.same_line();
                
                // Y coordinate
                let mut y = corner[1];
                ui.set_next_item_width(100.0);
                if ui.slider_config(&format!("##y{}", i), 0.0, 1.0)
                    .display_format("Y: %.3f")
                    .build(&mut y)
                {
                    corner[1] = y;
                    self.update_display_corner(display_id, i, corner);
                }
                
                // Select button for drag editing
                ui.same_line();
                let is_selected = {
                    let state = self.shared_state.lock().unwrap();
                    state.videowall_edit_display == Some(display_id) &&
                    state.videowall_edit_corner == Some(i)
                };
                
                let label = if is_selected { "Selected" } else { "Select" };
                if ui.button(&format!("{}##select{}", label, i)) {
                    let mut state = self.shared_state.lock().unwrap();
                    if is_selected {
                        state.videowall_edit_display = None;
                        state.videowall_edit_corner = None;
                    } else {
                        state.videowall_edit_display = Some(display_id);
                        state.videowall_edit_corner = Some(i);
                    }
                }
            }
            
            ui.separator();
            
            // Reset corners button
            if ui.button("Reset Corners to Calibration") {
                // Reload from calibration info if available
                // For now, just reset to grid-aligned positions
                self.reset_corners_to_grid(display_id);
            }
        }
    }
    
    /// Update a specific display corner
    fn update_display_corner(&mut self, display_id: u32, corner_index: usize, corner: [f32; 2]) {
        let mut state = self.shared_state.lock().unwrap();
        if let Some(ref mut config) = state.videowall_config {
            if let Some(display) = config.displays.iter_mut().find(|d| d.id == display_id) {
                if corner_index < 4 {
                    display.dest_quad[corner_index] = corner;
                }
            }
        }
    }
    
    /// Reset corners to grid-aligned positions (approximate)
    fn reset_corners_to_grid(&mut self, display_id: u32) {
        let mut state = self.shared_state.lock().unwrap();
        if let Some(ref mut config) = state.videowall_config {
            let grid_size = config.grid_size;
            if let Some(display) = config.displays.iter_mut().find(|d| d.id == display_id) {
                let (col, row) = display.grid_position;
                let cols = grid_size.columns as f32;
                let rows = grid_size.rows as f32;
                let c = col as f32;
                let r = row as f32;
                
                // Calculate normalized grid positions
                display.dest_quad = [
                    [c / cols, r / rows],         // Top-left
                    [(c + 1.0) / cols, r / rows], // Top-right
                    [(c + 1.0) / cols, (r + 1.0) / rows], // Bottom-right
                    [c / cols, (r + 1.0) / rows], // Bottom-left
                ];
            }
        }
    }
    
    /// Build calibration progress UI
    fn build_calibration_progress(&mut self, ui: &imgui::Ui) {
        // First, get all the info we need from shared state
        let (phase, progress) = {
            let state = self.shared_state.lock().unwrap();
            if let Some(ref calibration) = state.videowall_calibration {
                (calibration.phase(), calibration.progress())
            } else {
                return;
            }
        };
        
        // Display current phase
        let phase_text = match phase {
            CalibrationPhase::Idle => "Idle".to_string(),
            CalibrationPhase::Countdown { seconds_remaining } => format!("Countdown: {}s", seconds_remaining),
            CalibrationPhase::ShowingAllPatterns => "Patterns showing - Click Capture".to_string(),
            CalibrationPhase::Processing { current, total } => {
                format!("Processing {}/{}", current, total)
            }
            CalibrationPhase::BuildingMap => "Building quad map...".to_string(),
            CalibrationPhase::Complete => "Complete!".to_string(),
            CalibrationPhase::Error(ref e) => format!("Error: {}", e),
        };
        
        ui.text_colored([0.0, 1.0, 1.0, 1.0], &phase_text);
        
        // Progress display (imgui 0.12 doesn't have progress_bar)
        let progress_text = format!("Progress: {:.0}%", progress * 100.0);
        ui.text(&progress_text);
        
        // Visual progress bar using slider (disabled)
        let mut progress_val = progress;
        ui.slider_config("##progress", 0.0, 1.0)
            .display_format("")
            .build(&mut progress_val);
        
        // Control buttons
        ui.separator();
        
        if matches!(phase, CalibrationPhase::Complete) {
            if ui.button("Save Configuration") {
                log::info!("Saving video wall configuration");
            }
            ui.same_line();
            if ui.button("Dismiss") {
                let mut state = self.shared_state.lock().unwrap();
                state.videowall_calibration = None;
                return;
            }
        } else if matches!(phase, CalibrationPhase::Error(_)) {
            if ui.button("Retry") {
                let mut state = self.shared_state.lock().unwrap();
                state.videowall_calibration = None;
                return;
            }
            ui.same_line();
            if ui.button("Cancel") {
                let mut state = self.shared_state.lock().unwrap();
                state.videowall_calibration = None;
                return;
            }
        } else {
            // Calibration in progress
            let mut state = self.shared_state.lock().unwrap();
            if let Some(ref mut calibration) = state.videowall_calibration {
                // Show Capture button when patterns are showing
                if matches!(phase, CalibrationPhase::ShowingAllPatterns) {
                    if ui.button("Capture") {
                        calibration.trigger_capture();
                    }
                    ui.same_line();
                }
                
                if ui.button("Cancel") {
                    calibration.cancel();
                }
            }
        }
        
        // Update calibration state and handle camera capture
        let mut state = self.shared_state.lock().unwrap();
        if let Some(ref mut calibration) = state.videowall_calibration {
            match calibration.update() {
                CalibrationStatus::InProgress => {}
                CalibrationStatus::ReadyForCapture => {
                    // Waiting for user to click capture - auto-capture if camera frame available
                    // In real implementation, this would check for camera frame
                }
                CalibrationStatus::Processing => {
                    // Processing captured frame
                }
                CalibrationStatus::Complete(ref config) => {
                    log::info!("Calibration complete! {} displays configured", config.displays.len());
                    state.videowall_config = Some(config.clone());
                    state.videowall_enabled = true;
                }
                CalibrationStatus::Error(ref e) => {
                    log::error!("Calibration error: {}", e);
                }
            }
        }
    }
    
    /// Submit a camera frame for calibration capture
    /// Call this from the camera frame callback when calibration is active
    pub fn submit_camera_frame_for_calibration(&mut self, frame_data: Vec<u8>, width: u32, height: u32) {
        let mut state = self.shared_state.lock().unwrap();
        if let Some(ref mut calibration) = state.videowall_calibration {
            if calibration.is_ready_for_capture() {
                log::info!("Auto-submitting camera frame {}x{} for calibration", width, height);
                calibration.submit_frame(frame_data, width, height);
            }
        }
    }
    
    /// Build device selector window
    fn build_device_selector(&mut self, ui: &imgui::Ui) {
        let input_num = self.selector_for_input;
        
        ui.window(format!("Select Source for Input {}", input_num))
            .size([350.0, 400.0], imgui::Condition::FirstUseEver)
            .build(|| {
                if ui.button("Refresh") {
                    self.refresh_devices();
                }
                
                ui.separator();
                
                // Source type tabs
                if let Some(tab_bar) = ui.tab_bar("SourceTypeTabs") {
                    // Webcam tab
                    #[cfg(feature = "webcam")]
                    if let Some(tab) = ui.tab_item("Webcam") {
                        // Collect devices first to avoid borrow issues
                        let devices: Vec<(usize, String)> = self.webcam_devices.iter()
                            .enumerate()
                            .map(|(i, d)| (i, d.clone()))
                            .collect();
                        
                        if devices.is_empty() {
                            ui.text("No webcam devices found");
                        } else {
                            for (i, device) in devices {
                                let is_selected = if input_num == 1 { 
                                    self.selected_webcam1 == i as i32 
                                } else { 
                                    self.selected_webcam2 == i as i32 
                                };
                                
                                if ui.selectable_config(&device)
                                    .selected(is_selected)
                                    .build() 
                                {
                                    if input_num == 1 {
                                        self.selected_webcam1 = i as i32;
                                    } else {
                                        self.selected_webcam2 = i as i32;
                                    }
                                    self.select_webcam(input_num, i);
                                    self.show_device_selector = false;
                                }
                            }
                        }
                        tab.end();
                    }
                    
                    // NDI tab
                    if let Some(tab) = ui.tab_item("NDI") {
                        // Collect sources first to avoid borrow issues
                        let sources: Vec<(usize, String)> = self.ndi_sources.iter()
                            .enumerate()
                            .filter(|(_, s)| !s.to_lowercase().contains("obs"))
                            .map(|(i, s)| (i, s.clone()))
                            .collect();
                        
                        if sources.is_empty() {
                            ui.text("No NDI sources found");
                            ui.text("Make sure NDI sources are active on the network.");
                        } else {
                            for (i, source) in sources {
                                let is_selected = if input_num == 1 { 
                                    self.selected_ndi1 == i as i32 
                                } else { 
                                    self.selected_ndi2 == i as i32 
                                };
                                
                                if ui.selectable_config(&source)
                                    .selected(is_selected)
                                    .build() 
                                {
                                    if input_num == 1 {
                                        self.selected_ndi1 = i as i32;
                                    } else {
                                        self.selected_ndi2 = i as i32;
                                    }
                                    self.select_ndi(input_num, source);
                                    self.show_device_selector = false;
                                }
                            }
                        }
                        tab.end();
                    }
                    
                    // OBS tab
                    if let Some(tab) = ui.tab_item("OBS") {
                        // Collect OBS sources first to avoid borrow issues
                        let obs_sources: Vec<(usize, String)> = self.ndi_sources.iter()
                            .enumerate()
                            .filter(|(_, s)| s.to_lowercase().contains("obs"))
                            .map(|(i, s)| (i, s.clone()))
                            .collect();
                        
                        if obs_sources.is_empty() {
                            ui.text("No OBS NDI sources found");
                            ui.text("Enable NDI output in OBS Tools menu.");
                        } else {
                            for (i, source) in obs_sources {
                                if ui.button(format!("{}##obs{}", source, i)) {
                                    self.select_obs(input_num, source);
                                    self.show_device_selector = false;
                                }
                            }
                        }
                        tab.end();
                    }
                    
                    tab_bar.end();
                }
                
                ui.separator();
                
                if ui.button("Cancel") {
                    self.show_device_selector = false;
                }
            });
    }
    
    /// Select webcam for input
    fn select_webcam(&mut self, input_num: i32, device_index: usize) {
        let mut state = self.shared_state.lock().unwrap();
        let request = InputChangeRequest::StartWebcam {
            device_index,
            width: 1920,
            height: 1080,
            fps: 30,
        };
        
        if input_num == 1 {
            state.input1_request = request;
        } else {
            state.input2_request = request;
        }
        
        log::info!("Selected webcam {} for input {}", device_index, input_num);
    }
    
    /// Select NDI source for input
    fn select_ndi(&mut self, input_num: i32, source_name: String) {
        let mut state = self.shared_state.lock().unwrap();
        let request = InputChangeRequest::StartNdi { source_name: source_name.clone() };
        
        if input_num == 1 {
            state.input1_request = request;
        } else {
            state.input2_request = request;
        }
        
        log::info!("Selected NDI source '{}' for input {}", source_name, input_num);
    }
    
    /// Select OBS source for input
    fn select_obs(&mut self, input_num: i32, source_name: String) {
        let mut state = self.shared_state.lock().unwrap();
        let request = InputChangeRequest::StartObs { source_name: source_name.clone() };
        
        if input_num == 1 {
            state.input1_request = request;
        } else {
            state.input2_request = request;
        }
        
        log::info!("Selected OBS source '{}' for input {}", source_name, input_num);
    }
}
