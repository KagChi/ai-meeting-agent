//! Meeting Agent Core
//!
//! Core business logic for the meeting agent system.
//! Handles file system operations, transcription, and summary generation.

pub mod audio;
pub mod config;
pub mod config_validation;
pub mod db;
#[cfg(feature = "diarization")]
pub mod diarize;
pub mod fs;
pub mod jobs;
pub mod metadata;
pub mod models;
pub mod runners;
pub mod storage;
pub mod summary;
pub mod transcription;
pub mod voiceprint;

// Re-export commonly used types
pub use config::Config;
pub use jobs::{Job, JobRegistry, JobState, JobType};
pub use models::{
    Meeting, Person, Summary, SummaryTemplate, Transcript, Voiceprint, VoiceprintEnrolledFrom,
    VoiceprintSample, VoiceprintSampleSource,
};
pub use storage::MeetingStorage;
pub use summary::{MeetingContext, SummarizeOptions, SummaryClient};
pub use transcription::{TranscriptionClient, TranscriptionRequest, TranscriptionResponse};
