//! # Control GUI
//!
//! ImGui-based control interface for the application.
//! Supports multiple input types: Webcam, NDI, OBS (via NDI)

// Allow deprecated ComboBox API - imgui 0.12 uses the older API
#![allow(deprecated)]

use crate::config::AppConfig;
use crate::core::{SharedState, NdiOutputCommand, InputChangeRequest, InputMapping};
use crate::videowall::{CalibrationController, CalibrationPhase, CalibrationStatus, GridSize, PresetManager, ConfigPreset,
    VideoMatrixConfig, InputGridConfig, GridCellMapping, GridPosition, AspectRatio, Orientation,
    AprilTagAutoDetector, AprilTagGenerator, AprilTagFamily, AutoDetectConfig, TagPlacement,
    DetectedScreenRegion};
use std::sync::{Arc, Mutex};

/// Main GUI tabs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainTab {
    Inputs,
    Mapping,
    Matrix,  // Grid-based video matrix
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
    syphon_servers: Vec<String>,
    
    // Selection state
    selected_webcam1: i32,
    selected_webcam2: i32,
    selected_ndi1: i32,
    selected_ndi2: i32,
    selected_syphon1: i32,
    selected_syphon2: i32,
    
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
    
    // Video Matrix state (grid-based mapping)
    matrix_input_grid_cols: i32,
    matrix_input_grid_rows: i32,
    matrix_output_grid_cols: i32,
    matrix_output_grid_rows: i32,
    matrix_selected_input_cell: i32,
    matrix_selected_output_col: i32,
    matrix_selected_output_row: i32,
    matrix_aspect_ratio: usize,  // 0=4:3, 1=16:9, 2=16:10, 3=1:1, 4=21:9
    matrix_orientation: usize,   // 0=0°, 1=90°, 2=180°, 3=270°
    matrix_input_source: i32,  // 0=Input 1, 1=Input 2
    // AprilTag auto-detection state
    matrix_apriltag_expected_screens: i32,
    matrix_apriltag_marker_size: f32,
    matrix_apriltag_showing_pattern: bool,
    matrix_apriltag_output_col: i32,  // Starting column for detected screens
    matrix_apriltag_output_row: i32,  // Starting row for detected screens
    
    // Matrix preset state
    matrix_preset_name: String,
    matrix_presets: Vec<String>,
    
    // Preview textures for GUI display (public so app can update them)
    pub input_preview_texture_id: Option<imgui::TextureId>,
    pub output_preview_texture_id: Option<imgui::TextureId>,
    // Preview aspect ratio (updated when photo is loaded)
    preview_aspect_ratio: f32,
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
            syphon_servers: Vec::new(),
            selected_webcam1: 0,
            selected_webcam2: 0,
            selected_ndi1: 0,
            selected_ndi2: 0,
            selected_syphon1: 0,
            selected_syphon2: 0,
            show_device_selector: false,
            selector_for_input: 1,
            mapping_tab_input: 0,
            ndi_output_name,
            syphon_server_name,
            mapping_edit_input1: mapping1,
            mapping_edit_input2: mapping2,
            mapping_needs_update: false,
            // Video Matrix defaults
            matrix_input_grid_cols: 3,
            matrix_input_grid_rows: 3,
            matrix_output_grid_cols: 3,
            matrix_output_grid_rows: 3,
            matrix_selected_input_cell: 0,
            matrix_selected_output_col: 0,
            matrix_selected_output_row: 0,
            matrix_aspect_ratio: 1usize,  // 16:9 default
            matrix_orientation: 0usize,   // Normal
            matrix_input_source: 0,  // Input 1
            // AprilTag auto-detection defaults
            matrix_apriltag_expected_screens: 2,
            matrix_apriltag_marker_size: 0.60, // 60% for good detection resolution
            matrix_apriltag_output_col: 0,
            matrix_apriltag_output_row: 0,
            matrix_apriltag_showing_pattern: false,
            // Matrix preset defaults
            matrix_preset_name: String::new(),
            matrix_presets: Vec::new(),
            // Preview defaults
            input_preview_texture_id: None,
            output_preview_texture_id: None,
            preview_aspect_ratio: 16.0 / 9.0, // Default 16:9
        })
    }
    
    /// Set the input preview texture ID (from ImGui renderer)
    pub fn set_input_preview_texture(&mut self, texture_id: imgui::TextureId) {
        self.input_preview_texture_id = Some(texture_id);
    }
    
    /// Set the output preview texture ID (from ImGui renderer)
    pub fn set_output_preview_texture(&mut self, texture_id: imgui::TextureId) {
        self.output_preview_texture_id = Some(texture_id);
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
        
        #[cfg(target_os = "macos")]
        {
            // Try discovery multiple times (servers may take time to appear)
            let discovery = crate::input::syphon_input::SyphonDiscovery::new();
            let mut servers = discovery.discover_servers();
            
            // If no servers found, retry once after a short delay
            if servers.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(200));
                servers = discovery.discover_servers();
            }
            
            // Use display_name() which handles empty names by falling back to app_name
            self.syphon_servers = servers
                .into_iter()
                .map(|s| s.display_name().to_string())
                .collect();
            log::info!("Found {} Syphon servers", self.syphon_servers.len());
            for (i, name) in self.syphon_servers.iter().enumerate() {
                log::info!("  [{}] {}", i, name);
            }
        }
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
    
    /// Build the ImGui UI with 3-panel layout:
    /// - Left: Main controls (50% width)
    /// - Top-right: Input preview with draggable sampling boxes (25%)
    /// - Bottom-right: Output preview with grid divisions (25%)
    pub fn build_ui(&mut self, ui: &mut imgui::Ui) {
        // Sync mapping changes to shared state
        self.sync_mapping_to_state();
        
        // Get window size for layout calculations
        let window_size = ui.io().display_size;
        let window_width = window_size[0];
        let window_height = window_size[1];
        
        // Calculate panel dimensions
        let left_panel_width = window_width * 0.5;
        let right_panel_width = window_width * 0.48; // Slightly less to leave gap
        let right_panel_height = window_height * 0.48;
        let padding = 8.0;
        
        // === LEFT PANEL: Main Controls ===
        ui.window("Controls")
            .position([padding, padding], imgui::Condition::FirstUseEver)
            .size([left_panel_width - padding * 2.0, window_height - padding * 2.0], imgui::Condition::FirstUseEver)
            .movable(false)
            .collapsible(false)
            .resizable(false)
            .bring_to_front_on_focus(false)
            .build(|| {
                // Menu bar at the top
                self.build_menu_bar(ui);
                
                // Main tab bar with content
                self.build_main_tabs(ui);
                
                // Device selector window (separate popup window)
                if self.show_device_selector {
                    self.build_device_selector(ui);
                }
            });
        
        // === TOP-RIGHT: Input Preview ===
        let input_preview_pos = [left_panel_width + padding, padding];
        let input_preview_size = [right_panel_width - padding, right_panel_height - padding];
        
        ui.window("Input Preview")
            .position(input_preview_pos, imgui::Condition::FirstUseEver)
            .size(input_preview_size, imgui::Condition::FirstUseEver)
            .movable(true)
            .collapsible(true)
            .resizable(true)
            .build(|| {
                self.build_input_preview_window(ui, input_preview_size);
            });
        
        // === BOTTOM-RIGHT: Output Preview ===
        let output_preview_pos = [left_panel_width + padding, right_panel_height + padding * 2.0];
        let output_preview_size = [right_panel_width - padding, right_panel_height - padding];
        
        ui.window("Output Preview")
            .position(output_preview_pos, imgui::Condition::FirstUseEver)
            .size(output_preview_size, imgui::Condition::FirstUseEver)
            .movable(true)
            .collapsible(true)
            .resizable(true)
            .build(|| {
                self.build_output_preview_window(ui, output_preview_size);
            });
    }
    
    /// Build the input preview window with draggable sampling boxes
    fn build_input_preview_window(&mut self, ui: &imgui::Ui, available_size: [f32; 2]) {
        let window_width = available_size[0];
        let window_height = available_size[1];
        
        // Show input texture
        if let Some(texture_id) = self.input_preview_texture_id {
            // Get actual input resolution from shared state and update aspect ratio
            let (input_width, input_height) = {
                let state = self.shared_state.lock().unwrap();
                (state.ndi_input1.width, state.ndi_input1.height)
            };
            let actual_aspect = input_width as f32 / input_height as f32;
            
            // Use actual input aspect ratio if input is active
            let aspect = if actual_aspect > 0.1 && actual_aspect < 10.0 {
                actual_aspect
            } else {
                self.preview_aspect_ratio
            };
            let max_width = window_width - 16.0;
            let max_height = window_height - 40.0; // Leave room for title/padding
            
            // Calculate size that fits within bounds
            let mut tex_width = max_width;
            let mut tex_height = tex_width / aspect;
            
            if tex_height > max_height {
                tex_height = max_height;
                tex_width = tex_height * aspect;
            }
            
            // Center the texture in the window
            let x_offset = (window_width - tex_width) / 2.0;
            let y_offset = 30.0; // Below window title bar
            
            // Position cursor and draw image with full UV coordinates
            ui.set_cursor_pos([x_offset, y_offset]);
            
            // Get the actual screen position where the image will be drawn
            let image_pos = ui.cursor_screen_pos();
            
            // Draw the texture with explicit UVs to show full texture
            imgui::Image::new(texture_id, [tex_width, tex_height])
                .uv0([0.0, 0.0])  // Top-left of texture
                .uv1([1.0, 1.0])  // Bottom-right of texture
                .build(ui);
            
            // Draw draggable sampling boxes overlay using screen coordinates
            self.draw_sampling_boxes(ui, image_pos, [tex_width, tex_height]);
        } else {
            ui.text_disabled("No input preview available");
        }
    }
    
    /// Build the output preview window with grid divisions
    fn build_output_preview_window(&mut self, ui: &imgui::Ui, available_size: [f32; 2]) {
        let window_width = available_size[0];
        let window_height = available_size[1];
        
        // Show output texture
        if let Some(texture_id) = self.output_preview_texture_id {
            // Use available space, maintaining aspect ratio
            let aspect = 16.0 / 9.0;
            let max_width = window_width - 16.0;
            let max_height = window_height - 40.0;
            
            // Calculate size that fits within bounds
            let mut tex_width = max_width;
            let mut tex_height = tex_width / aspect;
            
            if tex_height > max_height {
                tex_height = max_height;
                tex_width = tex_height * aspect;
            }
            
            // Center the texture
            let x_offset = (window_width - tex_width) / 2.0;
            let y_offset = 30.0;
            
            // Position cursor and draw image
            ui.set_cursor_pos([x_offset, y_offset]);
            
            // Get actual screen position
            let image_pos = ui.cursor_screen_pos();
            
            // Draw texture with full UVs
            imgui::Image::new(texture_id, [tex_width, tex_height])
                .uv0([0.0, 0.0])
                .uv1([1.0, 1.0])
                .build(ui);
            
            // Draw grid divisions overlay
            self.draw_grid_divisions(ui, image_pos, [tex_width, tex_height]);
        } else {
            ui.text_disabled("No output preview available");
        }
    }
    
    /// Draw draggable sampling boxes on input preview
    fn draw_sampling_boxes(&mut self, ui: &imgui::Ui, pos: [f32; 2], size: [f32; 2]) {
        // Get current matrix config
        let (grid_cols, grid_rows, mappings, detected_screens) = {
            let state = self.shared_state.lock().unwrap();
            let config = &state.video_matrix_config;
            (
                config.input_grid.grid_size.columns,
                config.input_grid.grid_size.rows,
                config.input_grid.mappings.clone(),
                config.detected_screens.clone(),
            )
        };
        
        // Use foreground draw list for overlay
        let draw_list = ui.get_foreground_draw_list();
        let tex_width = size[0];
        let tex_height = size[1];
        
        // Draw detected screen regions from auto-detection (if any)
        if !detected_screens.is_empty() {
            for screen in &detected_screens {
                // Convert normalized coordinates to screen coordinates
                let x = pos[0] + screen.corners[0].0 * tex_width;
                let y = pos[1] + screen.corners[0].1 * tex_height;
                let w = screen.width * tex_width;
                let h = screen.height * tex_height;
                
                // Draw detected region with different color for each screen
                let color = match screen.screen_id % 3 {
                    0 => [0.0, 1.0, 0.0, 0.3], // Green
                    1 => [0.0, 0.5, 1.0, 0.3], // Blue
                    _ => [1.0, 0.5, 0.0, 0.3], // Orange
                };
                let border_color = match screen.screen_id % 3 {
                    0 => [0.0, 1.0, 0.0, 0.9],
                    1 => [0.0, 0.5, 1.0, 0.9],
                    _ => [1.0, 0.5, 0.0, 0.9],
                };
                
                // Fill
                draw_list
                    .add_rect([x, y], [x + w, y + h], color)
                    .filled(true)
                    .build();
                
                // Border
                draw_list
                    .add_rect([x, y], [x + w, y + h], border_color)
                    .thickness(3.0)
                    .build();
                
                // Label
                let label = format!("Screen {}\n{}", screen.screen_id, screen.aspect_ratio.name());
                let text_size = ui.calc_text_size(&label);
                let text_x = x + (w - text_size[0]) / 2.0;
                let text_y = y + (h - text_size[1]) / 2.0;
                draw_list
                    .add_text([text_x, text_y], [1.0, 1.0, 1.0, 1.0], label);
            }
        }
        
        // Draw grid lines
        let cell_width = tex_width / grid_cols as f32;
        let cell_height = tex_height / grid_rows as f32;
        
        // Draw vertical grid lines
        for col in 1..grid_cols {
            let x = pos[0] + col as f32 * cell_width;
            draw_list
                .add_line([x, pos[1]], [x, pos[1] + tex_height], [0.5, 0.5, 0.5, 0.5])
                .thickness(1.0)
                .build();
        }
        
        // Draw horizontal grid lines
        for row in 1..grid_rows {
            let y = pos[1] + row as f32 * cell_height;
            draw_list
                .add_line([pos[0], y], [pos[0] + tex_width, y], [0.5, 0.5, 0.5, 0.5])
                .thickness(1.0)
                .build();
        }
        
        // Draw mapping boxes (highlighted cells) - only if no detected screens
        if detected_screens.is_empty() {
            for mapping in &mappings {
                if !mapping.enabled {
                    continue;
                }
                
                let cell_idx = mapping.input_cell;
                let cell_col = (cell_idx % grid_cols as usize) as f32;
                let cell_row = (cell_idx / grid_cols as usize) as f32;
                
                let x = pos[0] + cell_col * cell_width;
                let y = pos[1] + cell_row * cell_height;
                
                // Draw highlighted box
                draw_list
                    .add_rect([x, y], [x + cell_width, y + cell_height], [0.0, 1.0, 0.0, 0.3])
                    .filled(true)
                    .build();
                
                draw_list
                    .add_rect([x, y], [x + cell_width, y + cell_height], [0.0, 1.0, 0.0, 0.8])
                    .thickness(2.0)
                    .build();
                
                // Draw cell index
                let text = format!("{}", cell_idx);
                let text_size = ui.calc_text_size(&text);
                let text_x = x + (cell_width - text_size[0]) / 2.0;
                let text_y = y + (cell_height - text_size[1]) / 2.0;
                draw_list
                    .add_text([text_x, text_y], [1.0, 1.0, 1.0, 1.0], text);
            }
        }
    }
    
    /// Draw grid divisions on output preview
    fn draw_grid_divisions(&mut self, ui: &imgui::Ui, pos: [f32; 2], size: [f32; 2]) {
        // Get output grid size
        let (grid_cols, grid_rows, mappings) = {
            let state = self.shared_state.lock().unwrap();
            let config = &state.video_matrix_config;
            (
                config.output_grid.columns,
                config.output_grid.rows,
                config.input_grid.mappings.clone(),
            )
        };
        
        // Use foreground draw list for overlay
        let draw_list = ui.get_foreground_draw_list();
        let tex_width = size[0];
        let tex_height = size[1];
        
        // Calculate cell size
        let cell_width = tex_width / grid_cols as f32;
        let cell_height = tex_height / grid_rows as f32;
        
        // Draw all grid cells with borders
        for row in 0..grid_rows {
            for col in 0..grid_cols {
                let x = pos[0] + col as f32 * cell_width;
                let y = pos[1] + row as f32 * cell_height;
                
                // Check if this cell has a mapping
                let has_mapping = mappings.iter().any(|m| {
                    m.enabled &&
                    m.output_position.col as u32 == col &&
                    m.output_position.row as u32 == row
                });
                
                // Color based on mapping status
                let color = if has_mapping {
                    [0.0, 1.0, 0.0, 0.2] // Green for mapped
                } else {
                    [0.5, 0.5, 0.5, 0.1] // Gray for unmapped
                };
                
                // Fill cell
                draw_list
                    .add_rect([x, y], [x + cell_width, y + cell_height], color)
                    .filled(true)
                    .build();
                
                // Draw border
                draw_list
                    .add_rect([x, y], [x + cell_width, y + cell_height], [0.8, 0.8, 0.8, 0.5])
                    .thickness(1.0)
                    .build();
                
                // Draw cell coordinates
                let text = format!("{},{}\n{}", col, row, 
                    if has_mapping { "M" } else { "-" });
                let text_size = ui.calc_text_size(&text);
                let text_x = x + (cell_width - text_size[0]) / 2.0;
                let text_y = y + (cell_height - text_size[1]) / 2.0;
                draw_list
                    .add_text([text_x, text_y], [1.0, 1.0, 1.0, 0.7], text);
            }
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
                          ("Matrix", MainTab::Matrix),
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
            MainTab::Matrix => self.build_matrix_tab(ui),
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
    
    
    /// Build the Matrix tab (grid-based video matrix)
    fn build_matrix_tab(&mut self, ui: &imgui::Ui) {
        ui.text("Video Matrix (Grid-Based Mapping)");
        ui.separator();
        
        // Grid configuration section
        ui.text_colored([0.0, 1.0, 1.0, 1.0], "Grid Configuration");
        
        // Input source selection
        ui.text("Input Source:");
        ui.radio_button("Input 1", &mut self.matrix_input_source, 0);
        ui.same_line();
        ui.radio_button("Input 2", &mut self.matrix_input_source, 1);
        
        ui.separator();
        
        // Input grid size
        ui.text("Input Grid (subdivides input texture):");
        ui.input_int("Input Columns", &mut self.matrix_input_grid_cols).build();
        ui.input_int("Input Rows", &mut self.matrix_input_grid_rows).build();
        self.matrix_input_grid_cols = self.matrix_input_grid_cols.clamp(1, 9);
        self.matrix_input_grid_rows = self.matrix_input_grid_rows.clamp(1, 9);
        
        // Output grid size
        ui.text("Output Grid (maps to physical displays):");
        ui.input_int("Output Columns", &mut self.matrix_output_grid_cols).build();
        ui.input_int("Output Rows", &mut self.matrix_output_grid_rows).build();
        self.matrix_output_grid_cols = self.matrix_output_grid_cols.clamp(1, 9);
        self.matrix_output_grid_rows = self.matrix_output_grid_rows.clamp(1, 9);
        
        // Apply grid configuration
        if ui.button("Apply Grid Configuration") {
            self.apply_matrix_grid_config();
        }
        
        ui.separator();
        
        // Cell mapping section
        ui.text_colored([0.0, 1.0, 1.0, 1.0], "Cell Mapping");
        ui.text_disabled("Map input cells to output positions");
        
        // Input cell selector
        let total_input_cells = (self.matrix_input_grid_cols * self.matrix_input_grid_rows) as usize;
        ui.text(format!("Select Input Cell (0-{}):", total_input_cells.saturating_sub(1)));
        ui.input_int("Input Cell", &mut self.matrix_selected_input_cell).build();
        self.matrix_selected_input_cell = self.matrix_selected_input_cell
            .clamp(0, total_input_cells.saturating_sub(1) as i32);
        
        // Show input grid visualization
        self.build_input_grid_visualization(ui);
        
        ui.separator();
        
        // Output position
        ui.text("Output Position:");
        ui.input_int("Output Col", &mut self.matrix_selected_output_col).build();
        ui.input_int("Output Row", &mut self.matrix_selected_output_row).build();
        self.matrix_selected_output_col = self.matrix_selected_output_col
            .clamp(0, self.matrix_output_grid_cols - 1);
        self.matrix_selected_output_row = self.matrix_selected_output_row
            .clamp(0, self.matrix_output_grid_rows - 1);
        
        // Show output grid visualization
        self.build_output_grid_visualization(ui);
        
        ui.separator();
        
        // Aspect ratio and orientation
        ui.text("Display Properties:");
        
        let aspect_ratios = ["4:3", "16:9", "16:10", "1:1", "21:9"];
        ui.combo_simple_string("Aspect Ratio", &mut self.matrix_aspect_ratio, &aspect_ratios);
        
        let orientations = ["0° Normal", "90° CW", "180°", "270° CW"];
        ui.combo_simple_string("Orientation", &mut self.matrix_orientation, &orientations);
        
        ui.separator();
        
        // Action buttons
        if ui.button("Add/Update Mapping") {
            self.add_matrix_mapping();
        }
        ui.same_line();
        if ui.button("Remove Mapping") {
            self.remove_matrix_mapping();
        }
        ui.same_line();
        if ui.button("Clear All") {
            self.clear_matrix_mappings();
        }
        
        ui.separator();
        
        // Enable/disable matrix
        let (enabled, mapping_count) = {
            let state = self.shared_state.lock().unwrap();
            (state.video_matrix_enabled, state.video_matrix_config.input_grid.mappings.len())
        };
        let mut enabled_mut = enabled;
        if ui.checkbox("Enable Video Matrix", &mut enabled_mut) {
            let mut state = self.shared_state.lock().unwrap();
            state.video_matrix_enabled = enabled_mut;
            log::info!("Video Matrix {} ({} mappings)", 
                if enabled_mut { "ENABLED" } else { "DISABLED" },
                mapping_count);
        }
        
        // Show mapping status
        if mapping_count == 0 {
            ui.text_colored([1.0, 0.5, 0.0, 1.0], "⚠️ No cell mappings configured. Add mappings above.");
        } else {
            ui.text_disabled(format!("{} cell mapping(s) configured", mapping_count));
        }
        
        // Preview section - now in separate windows
        ui.text_disabled("Previews shown in separate windows on the right");
        
        // Preset Section
        ui.separator();
        self.build_matrix_preset_section(ui);
        
        // AprilTag Auto-Detection Section
        ui.separator();
        ui.text_colored([0.0, 1.0, 1.0, 1.0], "AprilTag Auto-Detection");
        ui.text_disabled("Detect screen positions, aspect ratios, and orientations");
        
        // Show current grid configuration
        ui.text_disabled(format!(
            "Pattern Grid: {}×{} ({} cells)",
            self.matrix_input_grid_cols,
            self.matrix_input_grid_rows,
            self.matrix_input_grid_cols * self.matrix_input_grid_rows
        ));
        
        // Marker size (as percentage of screen) - can go up to 100% for maximum detection resolution
        let mut marker_percent = self.matrix_apriltag_marker_size * 100.0;
        ui.slider_config("Marker Size %", 10.0, 100.0)
            .display_format("%.0f%%")
            .build(&mut marker_percent);
        self.matrix_apriltag_marker_size = marker_percent / 100.0;
        ui.text_disabled(format!("Tag fills {:.0}% of screen (better detection with larger tags)", 
            self.matrix_apriltag_marker_size * 100.0));
        
        // Output position for detected screens (when not all detected)
        ui.text("Output Position for Detected Screens:");
        ui.input_int("Start Column", &mut self.matrix_apriltag_output_col).build();
        ui.input_int("Start Row", &mut self.matrix_apriltag_output_row).build();
        self.matrix_apriltag_output_col = self.matrix_apriltag_output_col.clamp(0, self.matrix_output_grid_cols - 1);
        self.matrix_apriltag_output_row = self.matrix_apriltag_output_row.clamp(0, self.matrix_output_grid_rows - 1);
        ui.text_disabled(format!("Detected screens will map to ({},{}) and right", 
            self.matrix_apriltag_output_col, self.matrix_apriltag_output_row));
        
        ui.spacing();
        
        // Pattern display button
        let pattern_button_text = if self.matrix_apriltag_showing_pattern {
            "Hide AprilTag Pattern"
        } else {
            "Show AprilTag Pattern"
        };
        
        if ui.button(&pattern_button_text) {
            self.matrix_apriltag_showing_pattern = !self.matrix_apriltag_showing_pattern;
            
            // Update shared state flag
            {
                let mut state = self.shared_state.lock().unwrap();
                state.matrix_showing_test_pattern = self.matrix_apriltag_showing_pattern;
            }
            
            if self.matrix_apriltag_showing_pattern {
                // Generate and display AprilTag pattern
                self.generate_and_show_apriltag_pattern();
            } else {
                // Clear pattern
                let mut state = self.shared_state.lock().unwrap();
                state.matrix_test_pattern = None;
            }
        }
        ui.same_line();
        
        // Load from Photo button
        if ui.button("Load from Photo") {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "tiff", "webp"])
                .add_filter("All files", &["*"])
                .set_title("Select calibration photo with AprilTags")
                .pick_file() 
            {
                log::info!("Selected calibration photo: {:?}", path);
                self.run_apriltag_detection_from_photo(&path);
            }
        }
        
        ui.spacing();
        
        // Auto-detect from current input button
        if ui.button("Auto-Detect from Current Input") {
            log::info!("Starting AprilTag auto-detection from current input");
            self.run_apriltag_detection_from_input();
        }
        ui.text_disabled("Requires AprilTags to be visible in current input");
        
        // Quick preset buttons for common configurations
        ui.separator();
        ui.text("Quick Presets:");
        if ui.button("2× 16:9 (Side-by-Side)") {
            self.apply_matrix_preset(AspectRatio::Ratio16_9, AspectRatio::Ratio16_9);
        }
        ui.same_line();
        if ui.button("4:3 + 16:9 (CRT+TV)") {
            self.apply_matrix_preset(AspectRatio::Ratio4_3, AspectRatio::Ratio16_9);
        }
        ui.same_line();
        if ui.button("2× 4:3 (Side-by-Side)") {
            self.apply_matrix_preset(AspectRatio::Ratio4_3, AspectRatio::Ratio4_3);
        }
    }
    
    /// Build the matrix preset save/load section
    fn build_matrix_preset_section(&mut self, ui: &imgui::Ui) {
        ui.text_colored([0.0, 1.0, 1.0, 1.0], "Presets");
        ui.text_disabled("Save and load matrix configurations");
        
        // Quick save
        if ui.button("Quick Save") {
            self.quick_save_matrix_preset();
        }
        
        ui.same_line();
        
        // Named save
        ui.input_text("##matrix_preset_name", &mut self.matrix_preset_name)
            .hint("Preset name...")
            .build();
        ui.same_line();
        if ui.button("Save") {
            if !self.matrix_preset_name.is_empty() {
                self.save_matrix_preset(&self.matrix_preset_name.clone());
                self.matrix_preset_name.clear();
            }
        }
        
        // Load preset
        ui.spacing();
        
        // Refresh preset list
        if ui.button("Refresh") {
            self.refresh_matrix_presets();
        }
        
        if !self.matrix_presets.is_empty() {
            ui.same_line();
            
            let presets: Vec<&str> = self.matrix_presets.iter()
                .map(|s| s.as_str())
                .collect();
            let mut selected = 0usize;
            
            if ui.combo_simple_string("##matrix_presets", &mut selected, &presets) {
                // Load selected preset
                self.load_matrix_preset(&self.matrix_presets[selected].clone());
            }
        }
    }
    
    /// Quick save matrix preset to default location
    fn quick_save_matrix_preset(&mut self) {
        let state = self.shared_state.lock().unwrap();
        let config = state.video_matrix_config.clone();
        drop(state);
        
        match self.save_matrix_config_to_file("matrix_quick", &config) {
            Ok(path) => log::info!("Saved matrix preset to {:?}", path),
            Err(e) => log::error!("Failed to save matrix preset: {}", e),
        }
    }
    
    /// Save matrix preset with given name
    fn save_matrix_preset(&mut self, name: &str) {
        let state = self.shared_state.lock().unwrap();
        let config = state.video_matrix_config.clone();
        drop(state);
        
        match self.save_matrix_config_to_file(name, &config) {
            Ok(_) => {
                log::info!("Saved matrix preset '{}'", name);
                self.refresh_matrix_presets();
            }
            Err(e) => log::error!("Failed to save matrix preset: {}", e),
        }
    }
    
    /// Save VideoMatrixConfig to a JSON file
    fn save_matrix_config_to_file(&self, name: &str, config: &crate::videowall::VideoMatrixConfig) -> anyhow::Result<std::path::PathBuf> {
        use std::fs;
        use std::path::PathBuf;
        
        let presets_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rusty_mapper")
            .join("matrix_presets");
        
        fs::create_dir_all(&presets_dir)?;
        
        let file_path = presets_dir.join(format!("{}.json", name));
        let json = serde_json::to_string_pretty(config)?;
        fs::write(&file_path, json)?;
        
        Ok(file_path)
    }
    
    /// Refresh list of available matrix presets
    fn refresh_matrix_presets(&mut self) {
        use std::fs;
        
        let presets_dir = dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("rusty_mapper")
            .join("matrix_presets");
        
        match fs::read_dir(&presets_dir) {
            Ok(entries) => {
                self.matrix_presets = entries
                    .filter_map(|entry| entry.ok())
                    .filter_map(|entry| {
                        let path = entry.path();
                        if path.extension()?.to_str()? == "json" {
                            path.file_stem()?.to_str().map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                    .collect();
            }
            Err(e) => log::debug!("No matrix presets directory yet: {}", e),
        }
    }
    
    /// Load a matrix preset by name
    fn load_matrix_preset(&mut self, name: &str) {
        use std::fs;
        use std::path::PathBuf;
        
        let presets_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rusty_mapper")
            .join("matrix_presets");
        
        let file_path = presets_dir.join(format!("{}.json", name));
        
        match fs::read_to_string(&file_path) {
            Ok(json) => {
                match serde_json::from_str::<crate::videowall::VideoMatrixConfig>(&json) {
                    Ok(config) => {
                        let mut state = self.shared_state.lock().unwrap();
                        state.video_matrix_config = config;
                        state.video_matrix_enabled = true;
                        log::info!("Loaded matrix preset '{}'", name);
                    }
                    Err(e) => log::error!("Failed to parse matrix preset: {}", e),
                }
            }
            Err(e) => log::error!("Failed to load matrix preset: {}", e),
        }
    }
    
    /// Build the device selector popup window
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
                    }
                    
                    // NDI tab
                    if let Some(tab) = ui.tab_item("NDI") {
                        let sources: Vec<(usize, String)> = self.ndi_sources.iter()
                            .enumerate()
                            .filter(|(_, s)| !s.to_lowercase().contains("obs"))
                            .map(|(i, s)| (i, s.clone()))
                            .collect();
                        
                        if sources.is_empty() {
                            ui.text("No NDI sources found");
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
                    }
                    
                    // OBS tab
                    if let Some(tab) = ui.tab_item("OBS") {
                        let obs_sources: Vec<(usize, String)> = self.ndi_sources.iter()
                            .enumerate()
                            .filter(|(_, s)| s.to_lowercase().contains("obs"))
                            .map(|(i, s)| (i, s.clone()))
                            .collect();
                        
                        if obs_sources.is_empty() {
                            ui.text("No OBS NDI sources found");
                        } else {
                            for (i, source) in obs_sources {
                                if ui.button(format!("{}##obs{}", source, i)) {
                                    self.select_obs(input_num, source);
                                    self.show_device_selector = false;
                                }
                            }
                        }
                    }
                    
                    // Syphon tab (macOS only)
                    #[cfg(target_os = "macos")]
                    if let Some(tab) = ui.tab_item("Syphon") {
                        let servers: Vec<(usize, String)> = self.syphon_servers.iter()
                            .enumerate()
                            .map(|(i, s)| (i, s.clone()))
                            .collect();
                        
                        if servers.is_empty() {
                            ui.text("No Syphon servers found");
                        } else {
                            for (i, server) in servers {
                                if ui.button(format!("{}##syphon{}", server, i)) {
                                    if input_num == 1 {
                                        self.selected_syphon1 = i as i32;
                                    } else {
                                        self.selected_syphon2 = i as i32;
                                    }
                                    self.select_syphon(input_num, server);
                                    self.show_device_selector = false;
                                }
                            }
                        }
                    }
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
    
    /// Select Syphon source for input (macOS only)
    #[cfg(target_os = "macos")]
    fn select_syphon(&mut self, input_num: i32, server_name: String) {
        let mut state = self.shared_state.lock().unwrap();
        let request = InputChangeRequest::StartSyphon { server_name: server_name.clone() };
        
        if input_num == 1 {
            state.input1_request = request;
        } else {
            state.input2_request = request;
        }
        
        log::info!("Selected Syphon server '{}' for input {}", server_name, input_num);
    }
    
    /// Build the preview section for input and output
    /// Build visualization of input grid
    fn build_input_grid_visualization(&self, ui: &imgui::Ui) {
        let cols = self.matrix_input_grid_cols as u32;
        let rows = self.matrix_input_grid_rows as u32;
        let selected = self.matrix_selected_input_cell as usize;
        
        ui.text("Input Grid:");
        
        // Simple text-based visualization
        for row in 0..rows {
            let mut row_text = String::new();
            for col in 0..cols {
                let cell_idx = (row * cols + col) as usize;
                if cell_idx == selected {
                    row_text.push_str("[X] ");
                } else {
                    row_text.push_str(&format!("[{}] ", cell_idx));
                }
            }
            ui.text(row_text);
        }
    }
    
    /// Build visualization of output grid
    fn build_output_grid_visualization(&self, ui: &imgui::Ui) {
        let cols = self.matrix_output_grid_cols as u32;
        let rows = self.matrix_output_grid_rows as u32;
        let sel_col = self.matrix_selected_output_col as u32;
        let sel_row = self.matrix_selected_output_row as u32;
        
        ui.text("Output Grid:");
        
        // Get current mappings to show which cells are mapped
        let mappings = {
            let state = self.shared_state.lock().unwrap();
            state.video_matrix_config.input_grid.mappings.clone()
        };
        
        for row in 0..rows {
            let mut row_text = String::new();
            for col in 0..cols {
                // Check if this output position has a mapping
                let has_mapping = mappings.iter().any(|m| {
                    m.enabled &&
                    m.output_position.col as u32 == col &&
                    m.output_position.row as u32 == row
                });
                
                if col == sel_col && row == sel_row {
                    if has_mapping {
                        row_text.push_str("[#] "); // Selected and mapped
                    } else {
                        row_text.push_str("[.] "); // Selected but not mapped
                    }
                } else if has_mapping {
                    row_text.push_str("[M] "); // Mapped
                } else {
                    row_text.push_str("[ ] "); // Empty
                }
            }
            ui.text(row_text);
        }
        
        ui.text_disabled("[#]=Selected [M]=Mapped [ ]=Empty");
    }
    
    /// Apply grid configuration to the video matrix
    fn apply_matrix_grid_config(&mut self) {
        let mut state = self.shared_state.lock().unwrap();
        
        // Create new input grid config
        let input_grid_size = GridSize::new(
            self.matrix_input_grid_cols as u32,
            self.matrix_input_grid_rows as u32,
        );
        let mut input_grid = InputGridConfig::new(input_grid_size)
            .with_input_source((self.matrix_input_source + 1) as u8);
        
        // Preserve existing mappings that fit in new grid
        let existing_mappings: Vec<GridCellMapping> = state.video_matrix_config
            .input_grid
            .mappings
            .iter()
            .filter(|m| m.input_cell < input_grid.total_cells())
            .cloned()
            .collect();
        
        input_grid.mappings = existing_mappings;
        
        // Create new video matrix config
        let mut config = VideoMatrixConfig::new(input_grid_size)
            .with_output_grid(GridSize::new(
                self.matrix_output_grid_cols as u32,
                self.matrix_output_grid_rows as u32,
            ));
        config.input_grid = input_grid;
        
        state.video_matrix_config = config;
        
        log::info!("Applied matrix grid config: {}x{} input, {}x{} output",
            self.matrix_input_grid_cols, self.matrix_input_grid_rows,
            self.matrix_output_grid_cols, self.matrix_output_grid_rows);
    }
    
    /// Add or update a matrix mapping
    fn add_matrix_mapping(&mut self) {
        let input_cell = self.matrix_selected_input_cell as usize;
        let output_col = self.matrix_selected_output_col as f32;
        let output_row = self.matrix_selected_output_row as f32;
        
        let aspect_ratio = match self.matrix_aspect_ratio {
            0usize => AspectRatio::Ratio4_3,
            1usize => AspectRatio::Ratio16_9,
            2usize => AspectRatio::Ratio16_10,
            3usize => AspectRatio::Ratio1_1,
            4usize => AspectRatio::Ratio21_9,
            _ => AspectRatio::Ratio16_9,
        };
        
        let orientation = match self.matrix_orientation {
            0usize => Orientation::Normal,
            1usize => Orientation::Rotated90,
            2usize => Orientation::Rotated180,
            3usize => Orientation::Rotated270,
            _ => Orientation::Normal,
        };
        
        let mut state = self.shared_state.lock().unwrap();
        
        // Remove existing mapping for this input cell if any
        state.video_matrix_config.input_grid.remove_mapping(input_cell);
        
        // Create new mapping
        let mapping = GridCellMapping::new(
            input_cell,
            GridPosition::new(output_col, output_row, 1.0, 1.0),
        )
        .with_aspect_ratio(aspect_ratio)
        .with_orientation(orientation);
        
        state.video_matrix_config.input_grid.add_mapping(mapping);
        
        // Note: We do NOT update output grid here - it stays at the user's configured size
        // The user controls output grid via "Apply Grid Configuration" button
        
        log::info!("Added mapping: input cell {} -> output ({}, {})",
            input_cell, output_col, output_row);
    }
    
    /// Remove a matrix mapping
    fn remove_matrix_mapping(&mut self) {
        let input_cell = self.matrix_selected_input_cell as usize;
        
        let mut state = self.shared_state.lock().unwrap();
        
        if let Some(removed) = state.video_matrix_config.input_grid.remove_mapping(input_cell) {
            // Output grid stays at user's configured size
            log::info!("Removed mapping for input cell {}", removed.input_cell);
        }
    }
    
    /// Clear all matrix mappings
    fn clear_matrix_mappings(&mut self) {
        let mut state = self.shared_state.lock().unwrap();
        state.video_matrix_config.input_grid.clear_mappings();
        // Output grid stays at user's configured size
        log::info!("Cleared all matrix mappings");
    }
    
    /// Generate and display AprilTag pattern for calibration
    fn generate_and_show_apriltag_pattern(&mut self) {
        let marker_size = self.matrix_apriltag_marker_size;
        
        // Use configured input grid size (not expected_screens)
        let grid_cols = self.matrix_input_grid_cols as u32;
        let grid_rows = self.matrix_input_grid_rows as u32;
        let total_cells = grid_cols * grid_rows;
        
        // Calculate actual marker dimensions for logging
        let output_width = 1920u32;
        let output_height = 1080u32;
        let display_width = output_width / grid_cols;
        let display_height = output_height / grid_rows;
        let marker_pixels = (display_width.min(display_height) as f32 * marker_size) as u32;
        
        log::info!(
            "Generating AprilTag pattern: {}x{} output, {}x{} grid, display_region={}x{}, marker_size={}px ({:.0}%)",
            output_width, output_height, grid_cols, grid_rows, 
            display_width, display_height, marker_pixels, marker_size * 100.0
        );
        
        let generator = AprilTagGenerator::new(AprilTagFamily::Tag36h11);
        
        // Generate pattern with all markers for the configured grid
        match generator.generate_all_markers_frame(
            (grid_cols, grid_rows),
            (output_width, output_height),
            marker_size,
        ) {
            Ok(frame) => {
                let mut state = self.shared_state.lock().unwrap();
                // Store as test pattern for display on output
                let rgba_data: Vec<u8> = frame.pixels()
                    .flat_map(|p| [p[0], p[1], p[2], p[3]])
                    .collect();
                state.matrix_test_pattern = Some((rgba_data, frame.width(), frame.height()));
                log::info!(
                    "Generated AprilTag pattern for {} cells ({}x{} grid), {}x{} frame",
                    total_cells, grid_cols, grid_rows, frame.width(), frame.height()
                );
            }
            Err(e) => {
                log::error!("Failed to generate AprilTag pattern: {}", e);
            }
        }
    }
    
    /// Apply a preset configuration for two screens
    fn apply_matrix_preset(&mut self, screen0_aspect: AspectRatio, screen1_aspect: AspectRatio) {
        let detector = AprilTagAutoDetector::new();
        let config = detector.create_two_screen_config(screen0_aspect, screen1_aspect);
        
        let mut state = self.shared_state.lock().unwrap();
        state.video_matrix_config = config;
        
        // Update UI to match config
        self.matrix_input_grid_cols = 2;
        self.matrix_input_grid_rows = 1;
        self.matrix_output_grid_cols = 2;
        self.matrix_output_grid_rows = 1;
        
        log::info!(
            "Applied preset: Screen 0 = {:?}, Screen 1 = {:?}",
            screen0_aspect,
            screen1_aspect
        );
    }
    
    /// Run AprilTag detection from a photo file
    fn run_apriltag_detection_from_photo(&mut self, path: &std::path::Path) {
        // Load image
        let image = match image::open(path) {
            Ok(img) => img.to_luma8(),
            Err(e) => {
                log::error!("Failed to load photo: {}", e);
                return;
            }
        };
        
        let (width, height) = (image.width(), image.height());
        
        // Update preview aspect ratio to match photo
        self.preview_aspect_ratio = width as f32 / height as f32;
        log::info!("Photo loaded: {}x{}, aspect ratio: {:.3}", width, height, self.preview_aspect_ratio);
        
        // Create detector with current settings
        let detector = AprilTagAutoDetector::with_config(AutoDetectConfig {
            expected_screens: self.matrix_apriltag_expected_screens as usize,
            tag_size_ratio: self.matrix_apriltag_marker_size,
            tag_placement: TagPlacement::Centered,
            ..Default::default()
        });
        
        // Run detection
        match detector.detect_screens(&image, (width, height)) {
            Ok(screens) => {
                log::info!("Detected {} screens from photo", screens.len());
                
                // Display detection results
                for screen in &screens {
                    log::info!(
                        "  Screen {}: {:?} at ({:.0}, {:.0}), size {:.0}x{:.0}",
                        screen.screen_id,
                        screen.aspect_ratio.name(),
                        screen.center.x * width as f32,
                        screen.center.y * height as f32,
                        screen.width * width as f32,
                        screen.height * height as f32
                    );
                }
                
                // Create configuration with user-specified output position
                let start_col = self.matrix_apriltag_output_col as u32;
                let start_row = self.matrix_apriltag_output_row as u32;
                match detector.create_matrix_config_with_position(&screens, (width, height), start_col, start_row) {
                    Ok(mut config) => {
                        // Convert detected screens to regions for visualization
                        let detected_regions: Vec<DetectedScreenRegion> = screens.iter().map(|s| {
                            DetectedScreenRegion {
                                screen_id: s.screen_id,
                                corners: [
                                    (s.corners[0].x, s.corners[0].y),
                                    (s.corners[1].x, s.corners[1].y),
                                    (s.corners[2].x, s.corners[2].y),
                                    (s.corners[3].x, s.corners[3].y),
                                ],
                                center: (s.center.x, s.center.y),
                                width: s.width,
                                height: s.height,
                                aspect_ratio: s.aspect_ratio,
                                orientation: s.orientation,
                            }
                        }).collect();
                        config.detected_screens = detected_regions;
                        
                        // Preserve user's configured output grid size (from GUI settings)
                        config.output_grid = GridSize::new(
                            self.matrix_output_grid_cols as u32,
                            self.matrix_output_grid_rows as u32,
                        );
                        
                        let mut state = self.shared_state.lock().unwrap();
                        state.video_matrix_config = config;
                        log::info!("Applied auto-detected matrix configuration with {} screens at output ({},{}), grid: {}x{}", 
                            screens.len(), start_col, start_row, self.matrix_output_grid_cols, self.matrix_output_grid_rows);
                    }
                    Err(e) => {
                        log::error!("Failed to create matrix config: {}", e);
                    }
                }
            }
            Err(e) => {
                log::error!("AprilTag detection failed: {}", e);
            }
        }
    }
    
    /// Run AprilTag detection from current input
    fn run_apriltag_detection_from_input(&mut self) {
        // Get input resolution from state
        let (input_width, input_height) = {
            let state = self.shared_state.lock().unwrap();
            (state.ndi_input1.width, state.ndi_input1.height)
        };
        
        if input_width == 0 || input_height == 0 {
            log::warn!("No input available for AprilTag detection");
            return;
        }
        
        // TODO: Get actual input texture data and convert to grayscale
        // This requires access to the input texture, which is in the renderer
        // For now, log that this would run detection
        log::info!(
            "Would run AprilTag detection on {}x{} input (requires texture access)",
            input_width, input_height
        );
        
        // In the full implementation, we would:
        // 1. Get the input texture from the renderer
        // 2. Convert to grayscale using texture_to_gray_image()
        // 3. Run AprilTagAutoDetector::detect_screens()
        // 4. Apply the resulting configuration
    }
}
