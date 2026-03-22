//! # NDI Output Sender
//!
//! Sends video frames as an NDI stream.
//!
//! Architecture:
//! - Dedicated send thread to avoid blocking render loop
//! - Bounded channel for frame queue (drops old frames if consumer is slow)
//! - Low-latency design: minimal buffering, immediate send

use grafton_ndi::{NDI, Sender, SenderOptions, VideoFrame, VideoFrameBuilder, PixelFormat};
use crossbeam::channel::{self, Sender as ChannelSender, Receiver};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread::{self, JoinHandle};
use std::time::Instant;

/// NDI video frame data (CPU side)
pub struct FrameData {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>, // BGRA or BGRX format
    pub has_alpha: bool,
    pub timestamp: Instant,
}

/// NDI output sender
pub struct NdiOutputSender {
    name: String,
    width: u32,
    height: u32,
    include_alpha: bool,
    frame_tx: ChannelSender<FrameData>,
    running: Arc<AtomicBool>,
    /// Thread handle — only the owner holds Some; clones hold None
    thread_handle: Option<JoinHandle<()>>,
    /// Whether this is the original sender (owner) or a clone
    /// Only the owner should stop the thread on drop
    is_owner: bool,
}

impl NdiOutputSender {
    /// Create and start a new NDI output sender
    ///
    /// # Arguments
    /// * `name` - The NDI source name that will appear to receivers
    /// * `width` - Output width in pixels
    /// * `height` - Output height in pixels
    /// * `include_alpha` - Whether to include alpha channel (BGRA vs BGRX)
    pub fn new(name: impl Into<String>, width: u32, height: u32, include_alpha: bool) -> anyhow::Result<Self> {
        let name = name.into();
        // Validate dimensions
        if width == 0 || height == 0 {
            return Err(anyhow::anyhow!("Invalid dimensions: {}x{}", width, height));
        }
        
        let ndi = NDI::new()
            .map_err(|e| anyhow::anyhow!("Failed to initialize NDI: {:?}", e))?;
        
        // Create bounded channel for frame queue (keep only latest 2 frames)
        let (frame_tx, frame_rx) = channel::bounded(2);
        
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);
        
        let name_clone = name.clone();
        
        // Spawn send thread
        let thread_handle = thread::spawn(move || {
            Self::send_thread(
                ndi,
                name_clone,
                width,
                height,
                include_alpha,
                frame_rx,
                running_clone,
            );
        });
        
        Ok(Self {
            name,
            width,
            height,
            include_alpha,
            frame_tx,
            running,
            thread_handle: Some(thread_handle),
            is_owner: true,
        })
    }
    
    /// Send thread that owns the NDI sender and processes frames
    fn send_thread(
        ndi: NDI,
        name: String,
        width: u32,
        height: u32,
        include_alpha: bool,
        frame_rx: Receiver<FrameData>,
        running: Arc<AtomicBool>,
    ) {
        // Create NDI sender (video clock enabled as required by NDI SDK)
        let options = SenderOptions::builder(&name)
            .clock_video(true)
            .clock_audio(false)
            .build();
        
        let sender = match Sender::new(&ndi, &options) {
            Ok(s) => s,
            Err(e) => {
                log::error!("[NDI OUTPUT] Failed to create NDI sender: {:?}", e);
                return;
            }
        };
        
        let pixel_format = if include_alpha {
            PixelFormat::BGRA
        } else {
            PixelFormat::BGRX
        };
        
        let mut frame_count = 0u64;
        let mut last_log = Instant::now();
        
        while running.load(Ordering::SeqCst) {
            // Try to receive frame with timeout
            match frame_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(frame_data) => {
                    frame_count += 1;
                    // Silently process frames - no per-frame logging in normal operation
                    
                    // Calculate expected buffer size
                    let buffer_size = pixel_format.buffer_size(frame_data.width as i32, frame_data.height as i32);
                    
                    // Validate data length
                    if frame_data.data.len() < buffer_size {
                        log::warn!("[NDI OUTPUT] Frame {} data too small (expected {}, got {})", 
                            frame_count, buffer_size, frame_data.data.len());
                        continue;
                    }
                    
                    // Create and send NDI video frame
                    let mut frame = match VideoFrameBuilder::new()
                        .resolution(frame_data.width as i32, frame_data.height as i32)
                        .pixel_format(pixel_format)
                        .frame_rate(60, 1)
                        .aspect_ratio(frame_data.width as f32 / frame_data.height as f32)
                        .build() {
                        Ok(f) => f,
                        Err(e) => {
                            log::error!("[NDI OUTPUT] Failed to build video frame: {:?}", e);
                            continue;
                        }
                    };
                    
                    // Copy data and send
                    let copy_len = buffer_size.min(frame.data.len());
                    frame.data[..copy_len].copy_from_slice(&frame_data.data[..copy_len]);
                    sender.send_video(&frame);
                    
                    // Log stats periodically (every 30 seconds in production)
                    if last_log.elapsed().as_secs() >= 30 {
                        log::info!("[NDI OUTPUT] {} frames sent to '{}'", frame_count, name);
                        last_log = Instant::now();
                    }
                }
                Err(channel::RecvTimeoutError::Timeout) => {
                    // No frame available, continue loop
                }
                Err(channel::RecvTimeoutError::Disconnected) => {
                    break;
                }
            }
        }
        
    }
    
    /// Submit a frame for sending (takes ownership to avoid copies)
    ///
    /// The data should be in BGRA format (native for macOS/NDI).
    /// No conversion is performed - data is sent directly to NDI.
    /// If the channel is full, the oldest frame will be dropped.
    pub fn submit_frame(&self, bgra_data: Vec<u8>, width: u32, height: u32) {
        // Validate dimensions match
        if width != self.width || height != self.height {
            log::warn!("[NDI OUTPUT] Frame size mismatch: expected {}x{}, got {}x{}",
                self.width, self.height, width, height);
            return;
        }

        // Validate data is not empty
        if bgra_data.is_empty() {
            log::warn!("[NDI OUTPUT] Empty frame data received");
            return;
        }

        // Data is already BGRA — take ownership, no copy
        let frame = FrameData {
            width,
            height,
            data: bgra_data,
            has_alpha: self.include_alpha,
            timestamp: Instant::now(),
        };
        
        // Try to send (non-blocking)
        match self.frame_tx.try_send(frame) {
            Ok(_) => {
                log::debug!("[NDI OUTPUT] Frame queued: {}x{}", width, height);
            }
            Err(channel::TrySendError::Full(_)) => {
                // Channel full, drop this frame for low latency
                log::debug!("[NDI OUTPUT] Frame dropped - channel full");
            }
            Err(channel::TrySendError::Disconnected(_)) => {
                log::warn!("[NDI OUTPUT] Frame channel disconnected");
            }
        }
    }
    
    /// Stop the NDI sender and join the thread
    /// Only the owner should call this - clones share the running flag
    pub fn stop(&mut self) {
        if !self.is_owner {
            return;
        }
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
    
    /// Check if sender is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
    
    /// Get output dimensions
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

impl Clone for NdiOutputSender {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            width: self.width,
            height: self.height,
            include_alpha: self.include_alpha,
            frame_tx: self.frame_tx.clone(),
            running: Arc::clone(&self.running),
            thread_handle: None,
            is_owner: false,
        }
    }
}

impl Drop for NdiOutputSender {
    fn drop(&mut self) {
        if self.is_owner {
            self.stop();
        }
        // Clones don't control the thread - silently drop them
    }
}

/// Check if NDI output is available
pub fn is_ndi_output_available() -> bool {
    NDI::new().is_ok()
}


