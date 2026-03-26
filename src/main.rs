//! # Rusty Mapper
//! 
//! A high-performance projection mapping application written in Rust.
//!
//! ## Features
//! - NDI input and output with dedicated threads
//! - Dual-window architecture (control + fullscreen output)
//! - GPU-accelerated rendering via wgpu
//! - Hidden cursor on output window for clean projection
//!
//! ## Architecture
//! See DESIGN.md for detailed architecture documentation.

use env_logger;
use log::info;
use std::sync::{Arc, Mutex};

mod app;
mod audio;
mod config;
mod core;
mod engine;
mod gui;
mod input;
#[cfg(feature = "ndi")]
mod ndi;
mod output;
mod videowall;

use config::AppConfig;
use core::SharedState;

/// Application entry point
///
/// Creates the event loop and initializes both windows.
fn main() -> anyhow::Result<()> {
    // Initialize logging with filters to reduce noise from wgpu/naga
    env_logger::Builder::from_default_env()
        .filter_module("wgpu_hal::metal", log::LevelFilter::Warn)
        .filter_module("naga", log::LevelFilter::Warn)
        .filter_module("wgpu_core", log::LevelFilter::Warn)
        .init();
    
    info!("Starting Rusty Mapper v{}", env!("CARGO_PKG_VERSION"));
    
    // Load configuration
    let config = AppConfig::load_or_default();
    info!("Configuration loaded: {:?}", config);
    
    // Create shared state for inter-window communication
    let shared_state = Arc::new(Mutex::new(SharedState::new(&config)));
    
    // Run the application
    app::run_app(config, shared_state)?;
    
    Ok(())
}
