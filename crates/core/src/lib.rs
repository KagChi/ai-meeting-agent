//! Meeting Agent Core
//!
//! Core business logic for the meeting agent system.
//! Handles file system operations, transcription, and summary generation.

pub mod config;
pub mod fs;
pub mod models;

// Re-export commonly used types
pub use config::Config;
pub use models::{Meeting, Transcript};
