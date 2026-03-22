//! # Output Module
//!
//! Handles all video output destinations:
//! - NDI network output (CPU path via double-buffered readback pool)
//! - Syphon (macOS, zero-copy GPU texture sharing)
//! - Spout (Windows, planned)
//! - v4l2loopback (Linux, planned)
//!
//! ## Frame flow
//!
//! ```text
//! render_target (wgpu::Texture, BGRA)
//!   ├─ Syphon  → direct GPU texture publish (zero-copy)
//!   └─ NDI     → ReadbackPool → staging buffer → harvest → NdiOutputSender thread
//! ```
//!
//! The ReadbackPool ensures NDI readback never blocks the render thread.

use std::sync::Arc;

mod readback;
use readback::ReadbackPool;

/// Trait for all local GPU-sharing output mechanisms (Syphon, Spout, etc.)
pub trait LocalOutput: Send {
    fn initialize(&mut self, width: u32, height: u32) -> anyhow::Result<()>;
    fn submit_frame(
        &mut self,
        texture: &wgpu::Texture,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> anyhow::Result<()>;
    fn is_connected(&self) -> bool;
    fn name(&self) -> &str;
    fn shutdown(&mut self);
}

#[cfg(target_os = "macos")]
pub mod syphon;
#[cfg(target_os = "macos")]
pub use syphon::SyphonOutput;

/// Manages all active output destinations.
pub struct OutputManager {
    /// NDI network output.
    ndi_output: Option<crate::ndi::NdiOutputSender>,

    /// Readback pool for CPU-path outputs (NDI).
    /// Shared across all CPU-path destinations to amortize the cost.
    readback_pool: ReadbackPool,

    /// Syphon GPU-sharing output (macOS, zero-copy).
    #[cfg(target_os = "macos")]
    syphon_output: Option<SyphonOutput>,

    frame_count: u64,
}

impl OutputManager {
    pub fn new() -> Self {
        Self {
            ndi_output: None,
            readback_pool: ReadbackPool::new(),
            #[cfg(target_os = "macos")]
            syphon_output: None,
            frame_count: 0,
        }
    }

    // ── NDI ──────────────────────────────────────────────────────────────────

    pub fn start_ndi(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        include_alpha: bool,
    ) -> anyhow::Result<()> {
        let sender = crate::ndi::NdiOutputSender::new(name, width, height, include_alpha)?;
        self.ndi_output = Some(sender);
        log::info!("NDI output started: {} ({}x{})", name, width, height);
        Ok(())
    }

    pub fn stop_ndi(&mut self) {
        if self.ndi_output.take().is_some() {
            log::info!("NDI output stopped");
        }
    }

    pub fn is_ndi_active(&self) -> bool {
        self.ndi_output.is_some()
    }

    // ── Syphon (macOS) ────────────────────────────────────────────────────────

    #[cfg(target_os = "macos")]
    pub fn start_syphon(
        &mut self,
        server_name: &str,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
    ) -> anyhow::Result<()> {
        let mut syphon = SyphonOutput::new(server_name, device, queue)?;
        syphon.initialize(1920, 1080)?;
        self.syphon_output = Some(syphon);
        log::info!("Syphon output started: {}", server_name);
        Ok(())
    }

    #[cfg(target_os = "macos")]
    pub fn stop_syphon(&mut self) {
        if let Some(mut syphon) = self.syphon_output.take() {
            syphon.shutdown();
            log::info!("Syphon output stopped");
        }
    }

    #[cfg(target_os = "macos")]
    pub fn is_syphon_active(&self) -> bool {
        self.syphon_output.is_some()
    }

    // ── Frame submission ──────────────────────────────────────────────────────

    /// Submit the current render target to all active outputs.
    ///
    /// Call this once per frame after the render pass completes.
    ///
    /// - Syphon: zero-copy GPU texture publish (no CPU involvement)
    /// - NDI: double-buffered async readback — harvests the *previous* frame's
    ///   data and submits a new copy, so the render thread is never blocked.
    pub fn submit_frame(
        &mut self,
        texture: &wgpu::Texture,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) {
        self.frame_count += 1;

        // ── GPU-path outputs (zero-copy) ──────────────────────────────────────
        #[cfg(target_os = "macos")]
        if let Some(syphon) = &mut self.syphon_output {
            if let Err(e) = syphon.submit_frame(texture, device, queue) {
                log::error!("Syphon output error: {}", e);
            }
        }

        // ── CPU-path outputs (via readback pool) ──────────────────────────────
        let needs_readback = self.ndi_output.is_some();

        if needs_readback {
            // Harvest the previous frame's readback (non-blocking).
            if let Some((data, w, h)) = self.readback_pool.harvest_previous() {
                if let Some(ndi) = &self.ndi_output {
                    // Stride-strip: readback rows are aligned to
                    // COPY_BYTES_PER_ROW_ALIGNMENT; NDI wants tight BGRA.
                    let tight = strip_row_padding(&data, w, h);
                    ndi.submit_frame(&tight, w, h);
                }
            }

            // Submit copy of the current frame into the pool.
            self.readback_pool.submit_copy(texture, device, queue);
        }
    }

    pub fn shutdown(&mut self) {
        self.stop_ndi();
        #[cfg(target_os = "macos")]
        self.stop_syphon();
    }
}

impl Default for OutputManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for OutputManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Remove row-alignment padding from a GPU readback buffer.
///
/// wgpu requires rows to be aligned to `COPY_BYTES_PER_ROW_ALIGNMENT` (256)
/// bytes. NDI (and most CPU consumers) expect tightly-packed BGRA rows.
fn strip_row_padding(padded: &[u8], width: u32, height: u32) -> Vec<u8> {
    let bytes_per_pixel = 4usize;
    let tight_row = width as usize * bytes_per_pixel;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
    let padded_row = (tight_row + align - 1) / align * align;

    let mut out = Vec::with_capacity(tight_row * height as usize);
    for row in 0..height as usize {
        let src_start = row * padded_row;
        let src_end = src_start + tight_row;
        if src_end <= padded.len() {
            out.extend_from_slice(&padded[src_start..src_end]);
        }
    }
    out
}
