//! # Core Module
//!
//! Core types and shared state for the application.

pub mod state;
pub mod vertex;

pub use state::{SharedState, NdiInputState, AudioState, OutputMode,
                InputCommand, InputMapping};
#[cfg(feature = "ndi")]
pub use state::{NdiOutputState, NdiOutputCommand};
pub use vertex::{Vertex, VERTEX_SIZE};
