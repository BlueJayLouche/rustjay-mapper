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
use crate::engine::WgpuEngine;
use crate::gui::{ControlGui, ImGuiRenderer};
use crate::input::InputManager;
#[cfg(feature = "ndi")]
use crate::ndi::NdiOutputSender;
use crate::videowall::VideoMatrixConfig;

use anyhow::Result;
use std::sync::Arc;
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::Window;

mod commands;
mod events;
mod update;

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

    // Output window
    pub(super) output_window: Option<Arc<Window>>,
    pub(super) output_engine: Option<WgpuEngine>,

    // Control window
    pub(super) control_window: Option<Arc<Window>>,
    pub(super) control_gui: Option<ControlGui>,
    pub(super) imgui_renderer: Option<ImGuiRenderer>,

    // Input manager (handles webcam, NDI, OBS, Syphon)
    pub(super) input_manager: Option<InputManager>,

    // NDI output
    #[cfg(feature = "ndi")]
    pub(super) ndi_output: Option<NdiOutputSender>,

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
            output_window: None,
            output_engine: None,
            control_window: None,
            control_gui: None,
            imgui_renderer: None,
            input_manager: None,
            #[cfg(feature = "ndi")]
            ndi_output: None,
            shift_pressed: false,
            last_matrix_pattern: None,
            last_video_matrix_config: None,
        }
    }

    /// Toggle fullscreen on output window
    pub(super) fn toggle_fullscreen(&mut self) {
        if let Some(ref output_window) = self.output_window {
            let mut state = self.shared_state.lock().unwrap();
            state.toggle_fullscreen();

            let fullscreen_mode = if state.output_fullscreen {
                Some(winit::window::Fullscreen::Borderless(None))
            } else {
                None
            };

            output_window.set_fullscreen(fullscreen_mode);
            log::info!("Fullscreen: {}", state.output_fullscreen);
        }
    }
}
