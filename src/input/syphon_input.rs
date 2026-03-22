//! Syphon Input (macOS) - Zero-Copy BGRA
//!
//! High-performance Syphon input using syphon-wgpu for zero-copy GPU texture
//! sharing. Native BGRA format throughout - no pixel conversion.

use std::time::Instant;
use std::sync::Arc;

/// Re-export from syphon-core
pub use syphon_core::ServerInfo as SyphonServerInfo;

// Note: syphon-wgpu 0.3.0+ uses native BGRA format only, no format configuration needed

/// A received Syphon frame (BGRA pixel data for CPU fallback)
pub struct SyphonFrame {
    pub width: u32,
    pub height: u32,
    /// BGRA pixel data (only populated in CPU fallback mode)
    pub data: Vec<u8>,
    pub timestamp: Instant,
}

/// Zero-copy Syphon input receiver using native BGRA
/// 
/// Uses syphon-wgpu with Bgra format for maximum performance.
/// The received texture can be accessed directly for GPU-only pipelines.
pub struct SyphonInputReceiver {
    #[cfg(target_os = "macos")]
    client: Option<syphon_wgpu::SyphonWgpuInput>,
    server_name: Option<String>,
    resolution: (u32, u32),
    device: Option<Arc<wgpu::Device>>,
    queue: Option<Arc<wgpu::Queue>>,
}

impl SyphonInputReceiver {
    /// Create a new Syphon input receiver (native BGRA)
    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "macos")]
            client: None,
            server_name: None,
            resolution: (1920, 1080),
            device: None,
            queue: None,
        }
    }
    
    /// Check if Syphon is available
    pub fn is_available() -> bool {
        syphon_core::is_available()
    }
    
    /// Initialize with wgpu device and queue (required before connect)
    pub fn initialize(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.device = Some(Arc::new(device.clone()));
        self.queue = Some(Arc::new(queue.clone()));
    }
    
    /// Connect to a Syphon server by name
    pub fn connect(&mut self, server_name: impl Into<String>) -> anyhow::Result<()> {
        let server_name = server_name.into();
        
        if self.is_connected() {
            self.disconnect();
        }
        
        log::info!("[Syphon Input] Connecting to: {} (BGRA zero-copy)", server_name);
        
        #[cfg(target_os = "macos")]
        {
            let (device, queue) = self.device.as_ref()
                .and_then(|d| self.queue.as_ref().map(|q| (d, q)))
                .ok_or_else(|| anyhow::anyhow!("SyphonInputReceiver not initialized with device/queue"))?;
            
            let mut client = syphon_wgpu::SyphonWgpuInput::new(device, queue);
            // Note: syphon-wgpu 0.3.0+ uses native BGRA format automatically
            client.connect(&server_name)
                .map_err(|e| anyhow::anyhow!("Failed to connect: {:?}", e))?;
            
            self.client = Some(client);
        }
        
        self.server_name = Some(server_name);
        Ok(())
    }
    
    /// Try to receive a new frame as wgpu texture (zero-copy path)
    /// 
    /// Returns None if no new frame is available.
    /// The returned texture is in Bgra8Unorm format (native Syphon format).
    pub fn try_receive_texture(&mut self) -> Option<wgpu::Texture> {
        #[cfg(target_os = "macos")]
        {
            let client = self.client.as_mut()?;
            let device = self.device.as_ref()?;
            let queue = self.queue.as_ref()?;
            
            if client.receive_texture(device, queue) {
                if let Some(src) = client.output_texture() {
                    let (w, h) = (src.width(), src.height());
                    self.resolution = (w, h);

                    // GPU-to-GPU copy: create an owned texture the caller can hold.
                    let dst = device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("syphon_input_frame"),
                        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: src.format(),
                        usage: wgpu::TextureUsages::TEXTURE_BINDING
                            | wgpu::TextureUsages::COPY_DST
                            | wgpu::TextureUsages::COPY_SRC,
                        view_formats: &[],
                    });

                    let mut encoder = device.create_command_encoder(
                        &wgpu::CommandEncoderDescriptor { label: Some("syphon_copy") },
                    );
                    encoder.copy_texture_to_texture(
                        src.as_image_copy(),
                        dst.as_image_copy(),
                        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    );
                    queue.submit(std::iter::once(encoder.finish()));

                    return Some(dst);
                }
            }
        }
        
        None
    }
    
    /// Try to receive a new frame (CPU fallback for compatibility)
    /// 
    /// Note: This reads back from GPU and is not zero-copy.
    /// Prefer `try_receive_texture()` for performance.
    pub fn try_receive(&mut self) -> Option<SyphonFrame> {
        let texture = self.try_receive_texture()?;
        let width = texture.width();
        let height = texture.height();
        
        // CPU readback for compatibility with existing CPU-based pipeline
        // This is NOT zero-copy - for true zero-copy, use try_receive_texture()
        let data = self.read_texture_to_bgra(&texture)?;
        
        Some(SyphonFrame {
            width,
            height,
            data,
            timestamp: Instant::now(),
        })
    }
    
    /// Read texture data to BGRA bytes (CPU readback for compatibility)
    /// 
    /// Note: This is a temporary fallback. For production, modify the renderer
    /// to use try_receive_texture() directly instead of reading back to CPU.
    #[cfg(target_os = "macos")]
    fn read_texture_to_bgra(&self, texture: &wgpu::Texture) -> Option<Vec<u8>> {
        let device = self.device.as_ref()?;
        let queue = self.queue.as_ref()?;
        
        let width = texture.width();
        let height = texture.height();
        let bytes_per_row = width * 4;
        let buffer_size = (bytes_per_row * height) as u64;
        
        // Create staging buffer
        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Syphon Readback Buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        
        // Copy texture to buffer
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Syphon Readback Encoder"),
        });
        
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        
        queue.submit(std::iter::once(encoder.finish()));
        
        // Map and read data
        let buffer_slice = staging_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel::<bool>();
        
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result.is_ok());
        });
        
        // Poll until mapped
        device.poll(wgpu::PollType::Wait).ok();
        
        // Check if mapping succeeded and read data
        if rx.recv().ok()? {
            let data = buffer_slice.get_mapped_range();
            let bytes = data.to_vec();
            drop(data);
            staging_buffer.unmap();
            Some(bytes)
        } else {
            None
        }
    }
    
    #[cfg(not(target_os = "macos"))]
    fn read_texture_to_bgra(&self, _texture: &wgpu::Texture) -> Option<Vec<u8>> {
        None
    }
    
    /// Disconnect from current server
    pub fn disconnect(&mut self) {
        #[cfg(target_os = "macos")]
        {
            self.client = None;
        }
        self.server_name = None;
    }
    
    /// Check if connected
    pub fn is_connected(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.client.as_ref().map_or(false, |c| c.is_connected())
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }
    
    /// Get current resolution
    pub fn resolution(&self) -> (u32, u32) {
        self.resolution
    }
    
    /// Get connected server name
    pub fn server_name(&self) -> Option<&str> {
        self.server_name.as_deref()
    }
    
    /// Get reference to wgpu device (if initialized)
    pub fn device(&self) -> Option<&Arc<wgpu::Device>> {
        self.device.as_ref()
    }
    
    /// Get reference to wgpu queue (if initialized)
    pub fn queue(&self) -> Option<&Arc<wgpu::Queue>> {
        self.queue.as_ref()
    }
}

impl Default for SyphonInputReceiver {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SyphonInputReceiver {
    fn drop(&mut self) {
        self.disconnect();
    }
}

/// Syphon server discovery
pub struct SyphonDiscovery;

impl SyphonDiscovery {
    /// Create new discovery
    pub fn new() -> Self {
        Self
    }
    
    /// Discover available Syphon servers
    pub fn discover_servers(&self) -> Vec<SyphonServerInfo> {
        syphon_core::SyphonServerDirectory::servers()
    }
    
    /// Check if a server exists
    pub fn is_server_available(&self, name: &str) -> bool {
        syphon_core::SyphonServerDirectory::server_exists(name)
    }
}

impl Default for SyphonDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience integration struct for zero-copy BGRA input
pub struct SyphonInputIntegration {
    receiver: Option<SyphonInputReceiver>,
    discovery: SyphonDiscovery,
    cached_servers: Vec<SyphonServerInfo>,
    last_discovery: Option<Instant>,
    device: Option<Arc<wgpu::Device>>,
    queue: Option<Arc<wgpu::Queue>>,
}

impl SyphonInputIntegration {
    /// Create new integration
    pub fn new() -> Self {
        Self {
            receiver: None,
            discovery: SyphonDiscovery::new(),
            cached_servers: Vec::new(),
            last_discovery: None,
            device: None,
            queue: None,
        }
    }
    
    /// Initialize with wgpu device and queue
    pub fn initialize(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.device = Some(Arc::new(device.clone()));
        self.queue = Some(Arc::new(queue.clone()));
        
        if let Some(ref mut receiver) = self.receiver {
            receiver.initialize(device, queue);
        }
    }
    
    /// Check if Syphon is available
    pub fn is_available() -> bool {
        SyphonInputReceiver::is_available()
    }
    
    /// Refresh server list
    pub fn refresh_servers(&mut self) {
        self.cached_servers = self.discovery.discover_servers();
        self.last_discovery = Some(Instant::now());
    }
    
    /// Get cached servers
    pub fn servers(&self) -> &[SyphonServerInfo] {
        &self.cached_servers
    }
    
    /// Connect to a server
    pub fn connect(&mut self, server_name: &str) -> anyhow::Result<()> {
        self.disconnect();
        
        let (device, queue) = self.device.as_ref()
            .and_then(|d| self.queue.as_ref().map(|q| (d, q)))
            .ok_or_else(|| anyhow::anyhow!("SyphonInputIntegration not initialized with device/queue"))?;
        
        let mut receiver = SyphonInputReceiver::new();
        receiver.initialize(device, queue);
        receiver.connect(server_name)?;
        self.receiver = Some(receiver);
        
        Ok(())
    }
    
    /// Disconnect
    pub fn disconnect(&mut self) {
        self.receiver = None;
    }
    
    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.receiver.as_ref().map_or(false, |r| r.is_connected())
    }
    
    /// Get latest frame as texture (zero-copy path - preferred)
    pub fn get_frame_texture(&mut self) -> Option<wgpu::Texture> {
        self.receiver.as_mut()?.try_receive_texture()
    }
    
    /// Get latest frame (CPU fallback - for compatibility)
    pub fn get_frame(&mut self) -> Option<SyphonFrame> {
        self.receiver.as_mut()?.try_receive()
    }
    
    /// Update (refresh discovery periodically)
    pub fn update(&mut self) {
        if self.last_discovery.map_or(true, |t| t.elapsed().as_secs() > 5) {
            self.refresh_servers();
        }
    }
}

impl Default for SyphonInputIntegration {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_receiver_creation() {
        let receiver = SyphonInputReceiver::new();
        assert!(!receiver.is_connected());
    }
}
