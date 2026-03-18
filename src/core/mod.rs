//! # Core Module
//!
//! Core types and shared state for the application.

pub mod state;
pub mod vertex;

pub use state::{SharedState, NdiInputState, NdiOutputState, AudioState, OutputMode,
                NdiOutputCommand, InputCommand, InputMapping};
pub use vertex::{Vertex, VERTEX_SIZE};
