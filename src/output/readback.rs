//! # Double-Buffered GPU→CPU Readback Pool
//!
//! Provides non-blocking GPU-to-CPU texture readback for CPU-path outputs (NDI).
//!
//! ## How it works
//!
//! Two staging buffer slots rotate in lockstep with the render loop:
//!
//! ```text
//! Frame N:   submit_copy(slot A) ─── GPU copies texture into slot A's buffer
//! Frame N+1: harvest(slot A)     ─── CPU maps slot A (should be ready by now)
//!            submit_copy(slot B) ─── GPU copies texture into slot B's buffer
//! Frame N+2: harvest(slot B)     ─── CPU maps slot B
//!            ...
//! ```
//!
//! `harvest_previous()` is non-blocking — if the GPU hasn't finished mapping the
//! buffer yet it returns `None` rather than stalling. This means the render thread
//! is never blocked waiting for a GPU readback to complete.

use std::sync::mpsc;

/// One staging slot.
struct Slot {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    state: SlotState,
}

enum SlotState {
    /// Ready to accept a new copy.
    Available,
    /// A copy has been submitted; waiting for the map callback.
    Pending {
        /// Receives `true` when the buffer is mapped and ready to read.
        ready_rx: mpsc::Receiver<()>,
    },
}

pub struct ReadbackPool {
    slots: [Option<Slot>; 2],
    /// Index of the slot we will harvest next frame.
    harvest_idx: usize,
    /// Index of the slot we will write into this frame.
    write_idx: usize,
}

impl ReadbackPool {
    pub fn new() -> Self {
        Self {
            slots: [None, None],
            harvest_idx: 0,
            write_idx: 0,
        }
    }

    /// Attempt to harvest a completed readback from the previous frame.
    ///
    /// Returns `Some((bgra_bytes, width, height))` if the GPU has finished
    /// mapping the buffer, `None` otherwise. Never blocks.
    pub fn harvest_previous(&mut self) -> Option<(Vec<u8>, u32, u32)> {
        let slot = self.slots[self.harvest_idx].as_mut()?;

        let ready = match &slot.state {
            SlotState::Pending { ready_rx } => ready_rx.try_recv().is_ok(),
            SlotState::Available => return None,
        };

        if !ready {
            return None;
        }

        // Buffer is mapped — read it out.
        let data = {
            let view = slot.buffer.slice(..).get_mapped_range();
            view.to_vec()
        };
        let (w, h) = (slot.width, slot.height);

        slot.buffer.unmap();
        slot.state = SlotState::Available;

        Some((data, w, h))
    }

    /// Submit a GPU texture copy into the current write slot, then advance both indices.
    ///
    /// The buffer is sized to fit `texture`. If the slot's existing buffer is too small
    /// it is recreated. Call this once per frame after rendering.
    pub fn submit_copy(
        &mut self,
        texture: &wgpu::Texture,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) {
        let width = texture.width();
        let height = texture.height();
        // BGRA8 = 4 bytes per pixel, rows must be aligned to COPY_BYTES_PER_ROW_ALIGNMENT
        let bytes_per_pixel = 4u32;
        let unpadded_bytes_per_row = width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = (unpadded_bytes_per_row + align - 1) / align * align;
        let buffer_size = (padded_bytes_per_row * height) as u64;

        let slot = &mut self.slots[self.write_idx];

        // (Re)create buffer if needed.
        let needs_new_buffer = slot.as_ref().map_or(true, |s| {
            s.buffer.size() < buffer_size || !matches!(s.state, SlotState::Available)
        });

        if needs_new_buffer {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("readback_pool_staging"),
                size: buffer_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            *slot = Some(Slot {
                buffer,
                width,
                height,
                state: SlotState::Available,
            });
        }

        let slot = slot.as_mut().unwrap();
        slot.width = width;
        slot.height = height;

        // Copy texture → staging buffer.
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback_pool_encoder"),
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::ImageCopyBuffer {
                buffer: &slot.buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        queue.submit(std::iter::once(encoder.finish()));

        // Request async map; signal via channel when done.
        let (tx, rx) = mpsc::channel::<()>();
        slot.buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
            if result.is_ok() {
                let _ = tx.send(());
            }
        });

        slot.state = SlotState::Pending { ready_rx: rx };

        // Advance indices (wraps at 2).
        self.harvest_idx = self.write_idx;
        self.write_idx = 1 - self.write_idx;
    }
}
