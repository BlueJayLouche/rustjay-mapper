//! # Shared State
//!
//! Thread-safe state shared between windows, threads, and the render loop.

use crate::videowall::{VideoWallConfig, CalibrationController, VideoMatrixConfig, InputGridConfig, GridSize};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Output mode for the renderer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputMode {
    /// Show processed output
    Processed,
    /// Show raw input 1
    Input1,
    /// Show raw input 2
    Input2,
}

impl Default for OutputMode {
    fn default() -> Self {
        OutputMode::Processed
    }
}

/// NDI input source state
#[derive(Debug, Clone, Default)]
pub struct NdiInputState {
    /// Selected source name
    pub source_name: String,
    /// Whether input is active
    pub is_active: bool,
    /// Current resolution
    pub width: u32,
    pub height: u32,
    /// Frame rate
    pub fps: f32,
}

/// NDI output state
#[derive(Debug, Clone, Default)]
pub struct NdiOutputState {
    /// Output stream name
    pub stream_name: String,
    /// Whether output is active
    pub is_active: bool,
    /// Include alpha channel
    pub include_alpha: bool,
    /// Frame skip (0 = every frame, 1 = every 2nd, etc.)
    pub frame_skip: u8,
}

/// Syphon output state (macOS only)
#[derive(Debug, Clone, Default)]
pub struct SyphonOutputState {
    /// Server name displayed to clients
    pub server_name: String,
    /// Whether output is enabled
    pub enabled: bool,
}

/// Audio analysis state
#[derive(Debug, Clone, Default)]
pub struct AudioState {
    /// 8-band FFT values (normalized 0-1)
    pub fft: [f32; 8],
    /// Overall volume/energy
    pub volume: f32,
    /// Beat detected this frame
    pub beat: bool,
    /// Estimated BPM
    pub bpm: f32,
    /// Beat phase (0-1)
    pub beat_phase: f32,
    /// Audio processing enabled
    pub enabled: bool,
    /// Amplitude multiplier
    pub amplitude: f32,
    /// Smoothing factor
    pub smoothing: f32,
}

/// Commands for NDI output control
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NdiOutputCommand {
    None,
    Start,
    Stop,
}

/// Texture mapping parameters for projection mapping
/// Supports corner pinning (quad warping) and UV transformation
#[derive(Debug, Clone, Copy)]
pub struct InputMapping {
    /// Corner 0 (top-left) - UV coordinates 0-1
    pub corner0: [f32; 2],
    /// Corner 1 (top-right) - UV coordinates 0-1
    pub corner1: [f32; 2],
    /// Corner 2 (bottom-right) - UV coordinates 0-1
    pub corner2: [f32; 2],
    /// Corner 3 (bottom-left) - UV coordinates 0-1
    pub corner3: [f32; 2],
    
    /// Global transform: scale X, scale Y
    pub scale: [f32; 2],
    /// Global transform: offset X, offset Y
    pub offset: [f32; 2],
    /// Global transform: rotation in degrees
    pub rotation: f32,
    
    /// Opacity (0-1)
    pub opacity: f32,
    /// Blend mode: 0=Normal, 1=Add, 2=Multiply, 3=Screen
    pub blend_mode: i32,
}

impl Default for InputMapping {
    fn default() -> Self {
        Self {
            corner0: [0.0, 0.0],  // Top-left
            corner1: [1.0, 0.0],  // Top-right
            corner2: [1.0, 1.0],  // Bottom-right
            corner3: [0.0, 1.0],  // Bottom-left
            scale: [1.0, 1.0],
            offset: [0.0, 0.0],
            rotation: 0.0,
            opacity: 1.0,
            blend_mode: 0,
        }
    }
}

impl InputMapping {
    /// Reset to default (full screen, no transform)
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    
    /// Get all corners as a flat array for shader upload
    pub fn corners_array(&self) -> [f32; 8] {
        [
            self.corner0[0], self.corner0[1],
            self.corner1[0], self.corner1[1],
            self.corner2[0], self.corner2[1],
            self.corner3[0], self.corner3[1],
        ]
    }
}

/// Commands for input changes
#[derive(Debug, Clone, PartialEq)]
pub enum InputCommand {
    None,
    StartWebcam { 
        device_index: usize, 
        width: u32, 
        height: u32, 
        fps: u32 
    },
    StartNdi { 
        source_name: String 
    },
    StartObs {
        source_name: String,
    },
    #[cfg(target_os = "macos")]
    StartSyphon {
        server_name: String,
    },
    StopInput,
    RefreshDevices,
}

/// Shared state accessible from multiple threads
#[derive(Debug)]
pub struct SharedState {
    // Output settings
    pub output_mode: OutputMode,
    pub output_fullscreen: bool,
    
    // NDI Input
    pub ndi_input1: NdiInputState,
    pub ndi_input2: NdiInputState,
    pub input1_command: InputCommand,
    pub input2_command: InputCommand,
    
    // NDI Output
    pub ndi_output: NdiOutputState,
    pub ndi_output_command: NdiOutputCommand,
    
    // Syphon Output (macOS)
    pub syphon_output: SyphonOutputState,
    
    // Audio
    pub audio: AudioState,
    
    // Effects parameters (simplified for now)
    pub effects_enabled: bool,
    pub effects_params: HashMap<String, f32>,
    
    // UI state
    pub show_preview: bool,
    pub ui_scale: f32,
    
    // Internal resolution
    pub internal_width: u32,
    pub internal_height: u32,
    
    // Input mapping for projection mapping
    pub input1_mapping: InputMapping,
    pub input2_mapping: InputMapping,
    
    // Mix parameters
    pub mix_amount: f32,  // 0 = input1 only, 1 = input2 only, 0.5 = equal mix
    
    // Video wall calibration
    pub videowall_calibration: Option<CalibrationController>,
    pub videowall_config: Option<VideoWallConfig>,
    pub videowall_enabled: bool,
    
    // Video matrix (grid-based mapping)
    pub video_matrix_config: VideoMatrixConfig,
    pub video_matrix_enabled: bool,
    
    // Matrix test pattern (AprilTag pattern for calibration)
    pub matrix_test_pattern: Option<(Vec<u8>, u32, u32)>, // (rgba_data, width, height)
    pub matrix_showing_test_pattern: bool, // True when test pattern should be displayed
    
    // Manual corner adjustment mode
    pub videowall_edit_mode: bool,
    pub videowall_edit_display: Option<u32>,  // Which display is being edited
    pub videowall_edit_corner: Option<usize>, // Which corner (0-3) is being dragged

    // Device discovery results (written by InputManager, read by GUI)
    pub discovered_webcams: Vec<String>,
    pub discovered_ndi_sources: Vec<String>,
    /// True while a background discovery pass is running.
    pub discovering_devices: bool,
}

impl SharedState {
    /// Create new shared state from config
    pub fn new(config: &crate::config::AppConfig) -> Self {
        let mut effects_params = HashMap::new();
        // Default effect parameters
        effects_params.insert("brightness".to_string(), 1.0);
        effects_params.insert("contrast".to_string(), 1.0);
        effects_params.insert("saturation".to_string(), 1.0);
        
        Self {
            output_mode: OutputMode::Processed,
            output_fullscreen: config.output_window.fullscreen,
            
            ndi_input1: NdiInputState {
                source_name: String::new(),
                is_active: false,
                width: 1920,
                height: 1080,
                fps: 60.0,
            },
            ndi_input2: NdiInputState {
                source_name: String::new(),
                is_active: false,
                width: 1920,
                height: 1080,
                fps: 60.0,
            },
            input1_command: InputCommand::None,
            input2_command: InputCommand::None,
            
            ndi_output: NdiOutputState {
                stream_name: "RustyMapper Output".to_string(),
                is_active: false,
                include_alpha: false,
                frame_skip: 0,
            },
            ndi_output_command: NdiOutputCommand::None,
            
            syphon_output: SyphonOutputState {
                server_name: "RustyMapper".to_string(),
                enabled: false,
            },
            
            audio: AudioState {
                fft: [0.0; 8],
                volume: 0.0,
                beat: false,
                bpm: 120.0,
                beat_phase: 0.0,
                enabled: config.audio_enabled,
                amplitude: 1.0,
                smoothing: 0.5,
            },
            
            effects_enabled: true,
            effects_params,
            
            show_preview: true,
            ui_scale: 1.0,
            
            internal_width: config.resolution.internal_width,
            internal_height: config.resolution.internal_height,
            
            input1_mapping: InputMapping::default(),
            input2_mapping: InputMapping::default(),
            
            mix_amount: 0.5,
            
            videowall_calibration: None,
            videowall_config: None,
            videowall_enabled: false,
            video_matrix_config: VideoMatrixConfig::new(GridSize::new(3, 3)),
            video_matrix_enabled: false,
            matrix_test_pattern: None,
            matrix_showing_test_pattern: false,
            videowall_edit_mode: false,
            videowall_edit_display: None,
            videowall_edit_corner: None,
            discovered_webcams: Vec::new(),
            discovered_ndi_sources: Vec::new(),
            discovering_devices: false,
        }
    }
    
    /// Get an effect parameter value
    pub fn get_effect_param(&self, name: &str) -> f32 {
        self.effects_params.get(name).copied().unwrap_or(1.0)
    }
    
    /// Set an effect parameter value
    pub fn set_effect_param(&mut self, name: &str, value: f32) {
        self.effects_params.insert(name.to_string(), value);
    }
    
    /// Toggle fullscreen state
    pub fn toggle_fullscreen(&mut self) {
        self.output_fullscreen = !self.output_fullscreen;
    }
    
    /// Toggle effects
    pub fn toggle_effects(&mut self) {
        self.effects_enabled = !self.effects_enabled;
    }
}
