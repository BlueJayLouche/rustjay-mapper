//! # Audio Input
//!
//! Audio capture for amplitude monitoring.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

/// Audio input handler
pub struct AudioInput {
    sample_buffer: Arc<Mutex<Vec<f32>>>,
    stream: Option<cpal::Stream>,
    amplitude: f32,
    smoothing: f32,
}

impl AudioInput {
    pub fn new() -> Self {
        let sample_buffer = Arc::new(Mutex::new(Vec::with_capacity(1024)));

        Self {
            sample_buffer,
            stream: None,
            amplitude: 1.0,
            smoothing: 0.5,
        }
    }

    /// Initialize default audio input
    pub fn initialize(&mut self) -> anyhow::Result<()> {
        let host = cpal::default_host();
        let device = host.default_input_device()
            .ok_or_else(|| anyhow::anyhow!("No input device available"))?;

        let config = device.default_input_config()?;

        log::info!("Audio input: {:?}", config);

        let sample_buffer = Arc::clone(&self.sample_buffer);

        let stream = device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let mut buffer = sample_buffer.lock().unwrap();
                buffer.extend_from_slice(data);
                // Keep only the latest samples
                if buffer.len() > 4096 {
                    let drain_to = buffer.len() - 4096;
                    buffer.drain(..drain_to);
                }
            },
            move |err| {
                log::error!("Audio stream error: {}", err);
            },
            None,
        )?;

        stream.play()?;
        self.stream = Some(stream);

        Ok(())
    }

    /// Set amplitude multiplier
    pub fn set_amplitude(&mut self, amp: f32) {
        self.amplitude = amp;
    }

    /// Set smoothing factor
    pub fn set_smoothing(&mut self, smoothing: f32) {
        self.smoothing = smoothing;
    }
}
