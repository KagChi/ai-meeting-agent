//! Meeting Agent Core
//!
//! Core business logic for the meeting agent system.
//! Handles file system operations, transcription, and summary generation.

pub mod audio;
pub mod config;
pub mod fs;
pub mod import;
pub mod jobs;
pub mod models;
pub mod storage;
pub mod summary;
pub mod summary_job;
pub mod transcription;

// Re-export commonly used types
pub use config::Config;
pub use jobs::{Job, JobRegistry, JobState, JobType};
pub use models::{Meeting, Summary, SummaryTemplate, Transcript};
pub use storage::MeetingStorage;
pub use summary::{SummarizeOptions, SummaryClient};
pub use transcription::{TranscriptionClient, TranscriptionRequest, TranscriptionResponse};
