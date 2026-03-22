//! # Input Module
//!
//! Handles multiple video input sources:
//! - Webcam capture (via nokhwa)
//! - NDI input (Network Device Interface)
//! - OBS (via NDI)
//! - Syphon (macOS only)
//!
//! Each input is independently selectable and refreshable.

use anyhow::Result;
use std::sync::{mpsc, Arc};

// NDI input support
pub mod ndi;
pub use ndi::{NdiReceiver, NdiFrame, list_ndi_sources, is_ndi_available};

// Webcam support (optional)
#[cfg(feature = "webcam")]
pub mod webcam;
#[cfg(feature = "webcam")]
pub use webcam::{WebcamCapture, WebcamFrame, list_cameras};

// Syphon input support (macOS only)
#[cfg(target_os = "macos")]
pub mod syphon_input;
#[cfg(target_os = "macos")]
pub use syphon_input::{SyphonInputReceiver, SyphonDiscovery, SyphonInputIntegration, SyphonServerInfo};

// Re-export syphon-wgpu types for convenience
#[cfg(target_os = "macos")]
pub use syphon_wgpu::SyphonWgpuInput;
// Note: InputFormat was removed in syphon-wgpu 0.3.0 - BGRA is now the only format

// Placeholder types when webcam is disabled
#[cfg(not(feature = "webcam"))]
pub struct WebcamFrame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>, // BGRA format (for wgpu Bgra8Unorm texture)
    pub timestamp: std::time::Instant,
}

#[cfg(not(feature = "webcam"))]
pub struct WebcamCapture;

#[cfg(not(feature = "webcam"))]
impl WebcamCapture {
    pub fn new(_device_index: usize, _width: u32, _height: u32, _fps: u32) -> anyhow::Result<Self> {
        Err(anyhow::anyhow!("Webcam support not compiled. Enable the 'webcam' feature."))
    }
    
    pub fn start(&mut self) -> anyhow::Result<mpsc::Receiver<WebcamFrame>> {
        unreachable!()
    }
    
    pub fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(not(feature = "webcam"))]
pub fn list_cameras() -> Vec<String> {
    Vec::new()
}

/// Type of input source
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputType {
    None,
    Webcam,
    Ndi,
    Obs,  // OBS via NDI
    #[cfg(target_os = "macos")]
    Syphon,
}

impl InputType {
    /// Get display name
    pub fn name(&self) -> &'static str {
        match self {
            InputType::None => "None",
            InputType::Webcam => "Webcam",
            InputType::Ndi => "NDI",
            InputType::Obs => "OBS (NDI)",
            #[cfg(target_os = "macos")]
            InputType::Syphon => "Syphon",
        }
    }
}

/// Information about available input devices
#[derive(Debug, Clone)]
pub struct InputDeviceInfo {
    pub index: usize,
    pub name: String,
    pub device_type: InputType,
}

/// Individual input source with its own capture
pub struct InputSource {
    pub input_type: InputType,
    pub device_index: i32,
    pub source_name: String,
    pub resolution: (u32, u32),
    pub active: bool,
    pub has_new_frame: bool,
    
    // Capture instances
    #[cfg(feature = "webcam")]
    webcam: Option<WebcamCapture>,
    #[cfg(not(feature = "webcam"))]
    webcam: Option<()>,
    
    frame_receiver: Option<mpsc::Receiver<WebcamFrame>>,
    ndi_receiver: Option<NdiReceiver>,
    
    // Syphon receiver (macOS only)
    #[cfg(target_os = "macos")]
    syphon_receiver: Option<SyphonInputReceiver>,
    
    // Current frame data (CPU fallback for webcam/NDI)
    current_frame: Option<Vec<u8>>,
    
    // wgpu resources for Syphon (stored for lazy initialization)
    #[cfg(target_os = "macos")]
    syphon_device: Option<Arc<wgpu::Device>>,
    #[cfg(target_os = "macos")]
    syphon_queue: Option<Arc<wgpu::Queue>>,
    
}

impl InputSource {
    pub fn new() -> Self {
        Self {
            input_type: InputType::None,
            device_index: -1,
            source_name: String::new(),
            resolution: (1920, 1080),
            active: false,
            has_new_frame: false,
            webcam: None,
            frame_receiver: None,
            ndi_receiver: None,
            #[cfg(target_os = "macos")]
            syphon_receiver: None,
            current_frame: None,
            #[cfg(target_os = "macos")]
            syphon_device: None,
            #[cfg(target_os = "macos")]
            syphon_queue: None,
        }
    }
    
    /// Initialize Syphon with wgpu device and queue (macOS only)
    #[cfg(target_os = "macos")]
    pub fn initialize_syphon(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.syphon_device = Some(Arc::new(device.clone()));
        self.syphon_queue = Some(Arc::new(queue.clone()));
        
        // Also initialize the receiver if it exists
        if let Some(ref mut receiver) = self.syphon_receiver {
            receiver.initialize(device, queue);
        }
    }
    
    /// Start webcam capture
    #[cfg(feature = "webcam")]
    pub fn start_webcam(&mut self, device_index: usize, width: u32, height: u32, fps: u32) -> Result<()> {
        self.stop();
        
        let mut webcam = WebcamCapture::new(device_index, width, height, fps)?;
        let receiver = webcam.start()?;
        
        self.input_type = InputType::Webcam;
        self.device_index = device_index as i32;
        self.resolution = (width, height);
        self.active = true;
        self.webcam = Some(webcam);
        self.frame_receiver = Some(receiver);
        
        log::info!("Started webcam {} at {}x{}@{}fps", device_index, width, height, fps);
        
        Ok(())
    }
    
    /// Start webcam (placeholder when disabled)
    #[cfg(not(feature = "webcam"))]
    pub fn start_webcam(&mut self, _device_index: usize, _width: u32, _height: u32, _fps: u32) -> Result<()> {
        Err(anyhow::anyhow!("Webcam support not compiled. Enable the 'webcam' feature."))
    }
    
    /// Start NDI receiver
    pub fn start_ndi(&mut self, source_name: impl Into<String>) -> Result<()> {
        self.stop();
        
        let source_name = source_name.into();
        let mut ndi = NdiReceiver::new(source_name.clone());
        ndi.start()?;
        
        self.input_type = InputType::Ndi;
        self.source_name = source_name.clone();
        self.active = true;
        self.ndi_receiver = Some(ndi);
        
        log::info!("Started NDI input: {}", source_name);
        
        Ok(())
    }
    
    /// Start OBS (via NDI)
    pub fn start_obs(&mut self, source_name: impl Into<String>) -> Result<()> {
        // OBS via NDI uses the same mechanism as regular NDI
        self.start_ndi(source_name)?;
        self.input_type = InputType::Obs;
        Ok(())
    }
    
    /// Start Syphon receiver (macOS only, requires wgpu initialization)
    #[cfg(target_os = "macos")]
    pub fn start_syphon(&mut self, server_name: impl Into<String>) -> Result<()> {
        // Use stored wgpu resources if available
        let server_name = server_name.into();
        
        // Clone the Arcs to avoid borrow issues
        let device = self.syphon_device.clone();
        let queue = self.syphon_queue.clone();
        
        if let (Some(device), Some(queue)) = (device, queue) {
            self.start_syphon_with_wgpu(&server_name, &device, &queue)
        } else {
            Err(anyhow::anyhow!("InputSource not initialized with wgpu device/queue. Call initialize_syphon() first."))
        }
    }
    
    /// Start Syphon receiver with explicit wgpu device and queue (macOS only)
    #[cfg(target_os = "macos")]
    pub fn start_syphon_with_wgpu(&mut self, server_name: &str, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<()> {
        self.stop();
        
        let mut receiver = SyphonInputReceiver::new();
        receiver.initialize(device, queue);
        receiver.connect(server_name)?;
        
        self.input_type = InputType::Syphon;
        self.source_name = server_name.to_string();
        self.active = true;
        self.syphon_receiver = Some(receiver);
        // Store the wgpu resources for future use
        self.syphon_device = Some(Arc::new(device.clone()));
        self.syphon_queue = Some(Arc::new(queue.clone()));
        
        log::info!("Started Syphon input: {}", server_name);
        
        Ok(())
    }
    
    /// Start Syphon (stub on non-macOS)
    #[cfg(not(target_os = "macos"))]
    pub fn start_syphon(&mut self, _server_name: impl Into<String>) -> Result<()> {
        Err(anyhow::anyhow!("Syphon is only available on macOS"))
    }
    
    /// Stop the input source
    pub fn stop(&mut self) {
        if !self.active {
            return;
        }
        
        log::info!("Stopping input source ({:?})", self.input_type);
        
        self.active = false;
        self.has_new_frame = false;
        
        // Stop webcam
        #[cfg(feature = "webcam")]
        if let Some(mut webcam) = self.webcam.take() {
            let _ = webcam.stop();
        }
        
        // Stop NDI
        if let Some(mut ndi) = self.ndi_receiver.take() {
            ndi.stop();
        }
        
        // Stop Syphon
        #[cfg(target_os = "macos")]
        {
            self.syphon_receiver = None;
            // Keep syphon_device and syphon_queue for potential reconnect
        }
        
        self.frame_receiver = None;
        self.current_frame = None;
        self.input_type = InputType::None;
        self.device_index = -1;
        self.source_name.clear();
    }
    
    /// Update - check for new frames
    pub fn update(&mut self) {
        if !self.active {
            return;
        }
        
        // Handle webcam frames
        if let Some(ref receiver) = self.frame_receiver {
            let mut latest_frame: Option<WebcamFrame> = None;
            
            // Drain the channel (keep only latest)
            while let Ok(frame) = receiver.try_recv() {
                latest_frame = Some(frame);
            }
            
            if let Some(frame) = latest_frame {
                self.resolution = (frame.width, frame.height);
                self.current_frame = Some(frame.data);
                self.has_new_frame = true;
            }
        }
        
        // Handle NDI frames
        if let Some(ref mut ndi) = self.ndi_receiver {
            if let Some(frame) = ndi.get_latest_frame() {
                self.resolution = (frame.width, frame.height);
                self.current_frame = Some(frame.data);
                self.has_new_frame = true;
            }
        }
        
        // Handle Syphon frames (zero-copy texture path)
        #[cfg(target_os = "macos")]
        if let Some(ref mut syphon) = self.syphon_receiver {
            // Receive into the receiver's cached texture (no per-frame allocation)
            if let Some(texture) = syphon.try_receive_texture() {
                self.resolution = (texture.width(), texture.height());
                self.has_new_frame = true;
                log::trace!("[Input] Syphon texture received: {}x{}",
                    self.resolution.0, self.resolution.1);
            }
        }
    }
    
    /// Check if there's a new frame
    pub fn has_frame(&self) -> bool {
        self.has_new_frame
    }
    
    /// Take the current frame (consumes it)
    pub fn take_frame(&mut self) -> Option<Vec<u8>> {
        self.has_new_frame = false;
        self.current_frame.take()
    }
    
    /// Get the Syphon texture reference if available (zero-copy path, macOS only)
    ///
    /// This is the preferred method for Syphon input as it avoids CPU readback.
    /// The texture is owned by the receiver and reused across frames.
    #[cfg(target_os = "macos")]
    pub fn take_syphon_texture(&mut self) -> Option<&wgpu::Texture> {
        self.has_new_frame = false;
        self.syphon_receiver.as_ref()?.cached_texture.as_ref()
    }

    /// Stub for non-macOS platforms
    #[cfg(not(target_os = "macos"))]
    pub fn take_syphon_texture(&mut self) -> Option<&wgpu::Texture> {
        None
    }
    
    /// Get current resolution
    pub fn resolution(&self) -> (u32, u32) {
        self.resolution
    }
    
    /// Check if input is active
    pub fn is_active(&self) -> bool {
        self.active
    }
    
    /// Get input type
    pub fn input_type(&self) -> InputType {
        self.input_type
    }
    
    /// Get source name/display info
    pub fn source_info(&self) -> String {
        match self.input_type {
            InputType::None => "None".to_string(),
            InputType::Webcam => format!("Webcam {}", self.device_index),
            InputType::Ndi | InputType::Obs => self.source_name.clone(),
            #[cfg(target_os = "macos")]
            InputType::Syphon => self.source_name.clone(),
        }
    }
}

impl Drop for InputSource {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Results returned by the background discovery thread.
#[derive(Default)]
struct DiscoveryResults {
    webcam_devices: Option<Vec<String>>,
    ndi_sources: Option<Vec<String>>,
}

/// Manages multiple input sources.
pub struct InputManager {
    /// Input 1 (primary)
    pub input1: InputSource,
    /// Input 2 (secondary)
    pub input2: InputSource,

    // Cached device lists (last known good)
    webcam_devices: Vec<String>,
    ndi_sources: Vec<String>,

    // Async discovery
    discovery_rx: Option<std::sync::mpsc::Receiver<DiscoveryResults>>,
    is_discovering: bool,

    // wgpu resources for Syphon (macOS only)
    #[cfg(target_os = "macos")]
    device: Option<Arc<wgpu::Device>>,
    #[cfg(target_os = "macos")]
    queue: Option<Arc<wgpu::Queue>>,
}

impl InputManager {
    pub fn new() -> Self {
        Self {
            input1: InputSource::new(),
            input2: InputSource::new(),
            webcam_devices: Vec::new(),
            ndi_sources: Vec::new(),
            discovery_rx: None,
            is_discovering: false,
            #[cfg(target_os = "macos")]
            device: None,
            #[cfg(target_os = "macos")]
            queue: None,
        }
    }

    /// Initialize with wgpu device and queue (required for Syphon on macOS).
    pub fn initialize(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        #[cfg(target_os = "macos")]
        {
            self.device = Some(Arc::new(device.clone()));
            self.queue = Some(Arc::new(queue.clone()));
            self.input1.initialize_syphon(device, queue);
            self.input2.initialize_syphon(device, queue);
        }
    }

    /// Kick off a background device discovery pass (non-blocking).
    ///
    /// Call this when the user opens the input picker or presses "Refresh".
    /// Poll `is_discovering()` and call `poll_discovery()` each frame to
    /// collect results when they arrive.
    pub fn kick_discovery(&mut self) {
        if self.is_discovering {
            return; // Already in progress
        }

        let (tx, rx) = std::sync::mpsc::channel();
        self.discovery_rx = Some(rx);
        self.is_discovering = true;

        std::thread::spawn(move || {
            let mut results = DiscoveryResults::default();

            // Webcam enumeration (may panic on some platforms — catch it)
            #[cfg(feature = "webcam")]
            {
                results.webcam_devices = Some(
                    std::panic::catch_unwind(list_cameras).unwrap_or_else(|_| {
                        log::error!("[InputManager] Webcam enumeration panicked");
                        Vec::new()
                    }),
                );
            }
            #[cfg(not(feature = "webcam"))]
            {
                results.webcam_devices = Some(Vec::new());
            }

            // NDI source discovery (blocks for timeout_ms internally)
            results.ndi_sources = Some(list_ndi_sources(2000));

            let _ = tx.send(results);
        });

        log::debug!("[InputManager] Device discovery started");
    }

    /// Returns `true` if a background discovery pass is currently running.
    pub fn is_discovering(&self) -> bool {
        self.is_discovering
    }

    /// Poll for completed discovery results (non-blocking, call every frame).
    ///
    /// Returns `true` if new results arrived and the device lists were updated.
    pub fn poll_discovery(&mut self) -> bool {
        let rx = match &self.discovery_rx {
            Some(rx) => rx,
            None => return false,
        };

        match rx.try_recv() {
            Ok(results) => {
                if let Some(devices) = results.webcam_devices {
                    log::info!("[InputManager] Found {} webcam(s)", devices.len());
                    self.webcam_devices = devices;
                }
                if let Some(sources) = results.ndi_sources {
                    log::info!("[InputManager] Found {} NDI source(s)", sources.len());
                    self.ndi_sources = sources;
                }
                self.is_discovering = false;
                self.discovery_rx = None;
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                log::warn!("[InputManager] Discovery thread exited unexpectedly");
                self.is_discovering = false;
                self.discovery_rx = None;
                false
            }
        }
    }

    /// Get cached webcam device list. Call `kick_discovery()` to refresh.
    pub fn get_webcam_devices(&self) -> &[String] {
        &self.webcam_devices
    }

    /// Get cached NDI source list. Call `kick_discovery()` to refresh.
    pub fn get_ndi_sources(&self) -> &[String] {
        &self.ndi_sources
    }
    
    /// Update all inputs (poll for new frames)
    pub fn update(&mut self) {
        self.input1.update();
        self.input2.update();
    }
    
    /// Start webcam on input 1
    pub fn start_input1_webcam(&mut self, device_index: usize, width: u32, height: u32, fps: u32) -> Result<()> {
        self.input1.start_webcam(device_index, width, height, fps)
    }
    
    /// Start webcam on input 2
    pub fn start_input2_webcam(&mut self, device_index: usize, width: u32, height: u32, fps: u32) -> Result<()> {
        self.input2.start_webcam(device_index, width, height, fps)
    }
    
    /// Start NDI on input 1
    pub fn start_input1_ndi(&mut self, source_name: impl Into<String>) -> Result<()> {
        self.input1.start_ndi(source_name)
    }
    
    /// Start NDI on input 2
    pub fn start_input2_ndi(&mut self, source_name: impl Into<String>) -> Result<()> {
        self.input2.start_ndi(source_name)
    }
    
    /// Start OBS on input 1
    pub fn start_input1_obs(&mut self, source_name: impl Into<String>) -> Result<()> {
        self.input1.start_obs(source_name)
    }
    
    /// Start OBS on input 2
    pub fn start_input2_obs(&mut self, source_name: impl Into<String>) -> Result<()> {
        self.input2.start_obs(source_name)
    }
    
    /// Start Syphon on input 1
    pub fn start_input1_syphon(&mut self, server_name: impl Into<String>) -> Result<()> {
        let server_name = server_name.into();
        
        #[cfg(target_os = "macos")]
        {
            if let (Some(device), Some(queue)) = (&self.device, &self.queue) {
                self.input1.start_syphon_with_wgpu(&server_name, device, queue)
            } else {
                Err(anyhow::anyhow!("InputManager not initialized with wgpu device/queue. Call initialize() first."))
            }
        }
        
        #[cfg(not(target_os = "macos"))]
        {
            let _ = server_name;
            Err(anyhow::anyhow!("Syphon is only available on macOS"))
        }
    }
    
    /// Start Syphon on input 2
    pub fn start_input2_syphon(&mut self, server_name: impl Into<String>) -> Result<()> {
        let server_name = server_name.into();
        
        #[cfg(target_os = "macos")]
        {
            if let (Some(device), Some(queue)) = (&self.device, &self.queue) {
                self.input2.start_syphon_with_wgpu(&server_name, device, queue)
            } else {
                Err(anyhow::anyhow!("InputManager not initialized with wgpu device/queue. Call initialize() first."))
            }
        }
        
        #[cfg(not(target_os = "macos"))]
        {
            let _ = server_name;
            Err(anyhow::anyhow!("Syphon is only available on macOS"))
        }
    }
    
    /// Stop input 1
    pub fn stop_input1(&mut self) {
        self.input1.stop();
    }
    
    /// Stop input 2
    pub fn stop_input2(&mut self) {
        self.input2.stop();
    }
}

impl Default for InputManager {
    fn default() -> Self {
        Self::new()
    }
}
