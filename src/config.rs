//! # Configuration
//!
//! Application configuration loaded from TOML files.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Window configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    pub width: u32,
    pub height: u32,
    pub title: String,
    pub fullscreen: bool,
    pub resizable: bool,
    pub decorated: bool,
    pub vsync: bool,
    pub fps: u32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            title: "Rusty Mapper Output".to_string(),
            fullscreen: false,
            resizable: true,
            decorated: true,
            vsync: true,
            fps: 60,
        }
    }
}

/// Control window configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlWindowConfig {
    pub width: u32,
    pub height: u32,
    pub title: String,
}

impl Default for ControlWindowConfig {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            title: "Rusty Mapper Control".to_string(),
        }
    }
}

/// Internal resolution configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionConfig {
    pub internal_width: u32,
    pub internal_height: u32,
}

impl ResolutionConfig {
    /// Get dimensions as tuple
    pub fn dimensions(&self) -> (u32, u32) {
        (self.internal_width, self.internal_height)
    }
}

impl Default for ResolutionConfig {
    fn default() -> Self {
        Self {
            internal_width: 1920,
            internal_height: 1080,
        }
    }
}

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub output_window: WindowConfig,
    pub control_window: ControlWindowConfig,
    pub resolution: ResolutionConfig,
    pub audio_enabled: bool,
    pub ndi_input_enabled: bool,
    pub ndi_output_enabled: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            output_window: WindowConfig::default(),
            control_window: ControlWindowConfig::default(),
            resolution: ResolutionConfig::default(),
            audio_enabled: true,
            ndi_input_enabled: true,
            ndi_output_enabled: true,
        }
    }
}

impl AppConfig {
    /// Load configuration from file or return defaults
    pub fn load_or_default() -> Self {
        let config_path = "config.toml";
        
        if Path::new(config_path).exists() {
            match std::fs::read_to_string(config_path) {
                Ok(contents) => {
                    match toml::from_str(&contents) {
                        Ok(config) => {
                            log::info!("Loaded configuration from {}", config_path);
                            return config;
                        }
                        Err(e) => {
                            log::warn!("Failed to parse config.toml: {}", e);
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Failed to read config.toml: {}", e);
                }
            }
        }
        
        // Return default and try to save it
        let config = Self::default();
        if let Ok(toml) = toml::to_string_pretty(&config) {
            let _ = std::fs::write(config_path, toml);
        }
        
        config
    }
    
    /// Save configuration to file
    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let toml = toml::to_string_pretty(self)?;
        std::fs::write(path, toml)?;
        Ok(())
    }
}
