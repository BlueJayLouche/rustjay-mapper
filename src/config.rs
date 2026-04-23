//! # Configuration
//!
//! Application configuration loaded from TOML files.
//! 
//! Config files are stored in platform-specific directories:
//! - macOS: `~/Library/Application Support/rusty_mapper/config.toml`
//! - Windows: `%APPDATA%/rusty_mapper/config.toml`
//! - Linux: `~/.config/rusty_mapper/config.toml`

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

/// Legacy application configuration for migration
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyAppConfig {
    pub output_window: WindowConfig,
    pub control_window: ControlWindowConfig,
    pub resolution: ResolutionConfig,
    pub audio_enabled: bool,
    #[cfg(feature = "ndi")]
    pub ndi_input_enabled: bool,
    #[cfg(feature = "ndi")]
    pub ndi_output_enabled: bool,
}

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub mapping_window: WindowConfig,
    #[serde(default)]
    pub matrix_window: WindowConfig,
    pub control_window: ControlWindowConfig,
    pub resolution: ResolutionConfig,
    pub audio_enabled: bool,
    #[cfg(feature = "ndi")]
    pub ndi_input_enabled: bool,
    #[cfg(feature = "ndi")]
    pub ndi_output_enabled: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        let mut mapping_window = WindowConfig::default();
        mapping_window.title = "Rusty Mapper — Mapping".to_string();
        let mut matrix_window = WindowConfig::default();
        matrix_window.title = "Rusty Mapper — Matrix".to_string();
        Self {
            mapping_window,
            matrix_window,
            control_window: ControlWindowConfig::default(),
            resolution: ResolutionConfig::default(),
            audio_enabled: true,
            #[cfg(feature = "ndi")]
            ndi_input_enabled: true,
            #[cfg(feature = "ndi")]
            ndi_output_enabled: true,
        }
    }
}

impl AppConfig {
    /// Get the default config directory path
    fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rusty_mapper")
    }

    /// Get the default config file path
    fn config_file_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    /// Load configuration from file or return defaults
    /// 
    /// Checks locations in this order:
    /// 1. Platform-specific config dir (e.g., `~/.config/rusty_mapper/config.toml`)
    /// 2. Legacy location (`./config.toml` in current working directory)
    pub fn load_or_default() -> Self {
        // First try the proper config directory
        let config_path = Self::config_file_path();
        
        if config_path.exists() {
            match std::fs::read_to_string(&config_path) {
                Ok(contents) => {
                    // Try new format first
                    match toml::from_str::<Self>(&contents) {
                        Ok(config) => {
                            log::info!("Loaded configuration from {}", config_path.display());
                            return config;
                        }
                        Err(_) => {
                            // Try legacy format and migrate
                            match toml::from_str::<LegacyAppConfig>(&contents) {
                                Ok(legacy) => {
                                    log::info!("Loaded legacy configuration from {}", config_path.display());
                                    let mut config = Self::from_legacy(legacy);
                                    if let Err(e) = config.save_default() {
                                        log::warn!("Failed to migrate config: {}", e);
                                    } else {
                                        log::info!("Migrated config to new format");
                                    }
                                    return config;
                                }
                                Err(e) => {
                                    log::warn!("Failed to parse config at {}: {}", config_path.display(), e);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Failed to read config from {}: {}", config_path.display(), e);
                }
            }
        }

        // Check legacy location (backward compatibility)
        let legacy_path = Path::new("config.toml");
        if legacy_path.exists() {
            match std::fs::read_to_string(legacy_path) {
                Ok(contents) => {
                    match toml::from_str::<LegacyAppConfig>(&contents) {
                        Ok(legacy) => {
                            log::info!("Loaded configuration from legacy location: {}", legacy_path.display());
                            let mut config = Self::from_legacy(legacy);
                            // Migrate to new location
                            if let Err(e) = config.save_default() {
                                log::warn!("Failed to migrate config to new location: {}", e);
                            } else {
                                log::info!("Migrated config to new location: {}", config_path.display());
                            }
                            return config;
                        }
                        Err(e) => {
                            log::warn!("Failed to parse legacy config.toml: {}", e);
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Failed to read legacy config.toml: {}", e);
                }
            }
        }
        
        // Return default and try to save it
        let config = Self::default();
        if let Err(e) = config.save_default() {
            log::warn!("Failed to save default config: {}", e);
        }
        
        config
    }
    
    /// Convert a legacy config (single output window) to the new format
    fn from_legacy(legacy: LegacyAppConfig) -> Self {
        Self {
            mapping_window: legacy.output_window.clone(),
            matrix_window: legacy.output_window,
            control_window: legacy.control_window,
            resolution: legacy.resolution,
            audio_enabled: legacy.audio_enabled,
            #[cfg(feature = "ndi")]
            ndi_input_enabled: legacy.ndi_input_enabled,
            #[cfg(feature = "ndi")]
            ndi_output_enabled: legacy.ndi_output_enabled,
        }
    }
    
    /// Save configuration to the default location
    pub fn save_default(&self) -> anyhow::Result<()> {
        let config_path = Self::config_file_path();
        
        // Ensure parent directory exists
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        self.save_to_path(&config_path)
    }

    /// Save configuration to a specific path
    pub fn save_to_path(&self, path: &Path) -> anyhow::Result<()> {
        let toml = toml::to_string_pretty(self)?;
        std::fs::write(path, toml)?;
        log::info!("Saved configuration to {}", path.display());
        Ok(())
    }

    /// Save configuration to file (legacy method, uses default path)
    #[deprecated(since = "0.2.0", note = "Use save_default() or save_to_path() instead")]
    pub fn save(&self, _path: &str) -> anyhow::Result<()> {
        self.save_default()
    }
}
