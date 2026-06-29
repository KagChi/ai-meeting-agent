//! Meeting Agent Core
//!
//! Core business logic for the meeting agent system.
//! Handles file system operations, transcription, and summary generation.

pub mod audio;
pub mod config;
pub mod fs;
pub mod models;
pub mod transcription;

// Re-export commonly used types
pub use config::Config;
pub use models::{Meeting, Transcript};
pub use transcription::{TranscriptionClient, TranscriptionRequest, TranscriptionResponse};
