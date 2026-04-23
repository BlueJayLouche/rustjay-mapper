//! # Application Handler
//!
//! Dual-window application handler implementing winit's ApplicationHandler.
//!
//! Manages:
//! - Output window: Fullscreen-capable, hidden cursor
//! - Control window: ImGui-based UI
//! - Shared wgpu resources between windows

use crate::config::AppConfig;
use crate::core::SharedState;
use crate::engine::RenderEngine;
use crate::gui::{ControlGui, ImGuiRenderer};
use crate::input::InputManager;
use crate::videowall::VideoMatrixConfig;

use anyhow::Result;
use std::sync::Arc;
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::Window;

mod commands;
mod events;
mod output_window;
mod update;

pub use output_window::{OutputWindow, OutputType};

/// Run the application
pub fn run_app(
    config: AppConfig,
    shared_state: Arc<std::sync::Mutex<SharedState>>,
) -> Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::new(config, shared_state);
    event_loop.run_app(&mut app)?;

    Ok(())
}

/// Main application state
pub(super) struct App {
    pub(super) config: AppConfig,
    pub(super) shared_state: Arc<std::sync::Mutex<SharedState>>,

    // Shared wgpu resources
    pub(super) wgpu_instance: Option<wgpu::Instance>,
    pub(super) wgpu_adapter: Option<wgpu::Adapter>,
    pub(super) wgpu_device: Option<Arc<wgpu::Device>>,
    pub(super) wgpu_queue: Option<Arc<wgpu::Queue>>,

    // Render engine (produces textures, no surface)
    pub(super) render_engine: Option<RenderEngine>,

    // Output windows
    pub(super) mapping_output: Option<OutputWindow>,
    pub(super) matrix_output: Option<OutputWindow>,

    // Control window
    pub(super) control_window: Option<Arc<Window>>,
    pub(super) control_gui: Option<ControlGui>,
    pub(super) imgui_renderer: Option<ImGuiRenderer>,

    // Input manager (handles webcam, NDI, OBS, Syphon)
    pub(super) input_manager: Option<InputManager>,

    // Modifier state
    pub(super) shift_pressed: bool,

    // Track last uploaded matrix pattern to avoid re-uploading
    pub(super) last_matrix_pattern: Option<(u32, u32)>,

    // Cache last video matrix config to avoid redundant updates
    pub(super) last_video_matrix_config: Option<VideoMatrixConfig>,
}

impl App {
    fn new(config: AppConfig, shared_state: Arc<std::sync::Mutex<SharedState>>) -> Self {
        Self {
            config,
            shared_state,
            wgpu_instance: None,
            wgpu_adapter: None,
            wgpu_device: None,
            wgpu_queue: None,
            render_engine: None,
            mapping_output: None,
            matrix_output: None,
            control_window: None,
            control_gui: None,
            imgui_renderer: None,
            input_manager: None,
            shift_pressed: false,
            last_matrix_pattern: None,
            last_video_matrix_config: None,
        }
    }

    /// Toggle fullscreen on the mapping output window
    pub(super) fn toggle_mapping_fullscreen(&mut self) {
        if let Some(ref output) = self.mapping_output {
            let mut state = self.shared_state.lock().unwrap();
            state.toggle_mapping_fullscreen();
            output.toggle_fullscreen();
            log::info!("Mapping fullscreen: {}", state.mapping_window_fullscreen);
        }
    }

    /// Toggle fullscreen on the matrix output window
    pub(super) fn toggle_matrix_fullscreen(&mut self) {
        if let Some(ref output) = self.matrix_output {
            let mut state = self.shared_state.lock().unwrap();
            state.toggle_matrix_fullscreen();
            output.toggle_fullscreen();
            log::info!("Matrix fullscreen: {}", state.matrix_window_fullscreen);
        }
    }
}
