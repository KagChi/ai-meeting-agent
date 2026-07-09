//! Domain models

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Source of meeting metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum MetadataSource {
    /// User-provided metadata
    UserProvided,
    /// Extracted from calendar bot
    CalendarBot,
    /// Parsed from filename
    Filename,
    /// Extracted via FFprobe
    FFprobe,
    /// Default/fallback values
    Default,
}

/// Audio/video file metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FileMetadata {
    /// Audio/video codec (e.g., "aac", "opus", "h264")
    pub codec: Option<String>,
    /// Audio sample rate in Hz
    pub sample_rate: Option<u32>,
    /// Audio bit rate in bits/s
    pub bit_rate: Option<u64>,
    /// Number of audio channels
    pub channels: Option<u8>,
    /// File size in bytes
    pub file_size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub date: DateTime<Utc>,
    pub duration_seconds: Option<u64>,
    pub status: MeetingStatus,
    pub transcription: Option<TranscriptionInfo>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// List of meeting participants
    #[serde(skip_serializing_if = "Option::is_none")]
    pub participants: Option<Vec<String>>,
    /// Physical or virtual location
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Meeting organizer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organizer: Option<String>,
    /// Source of the metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_source: Option<MetadataSource>,
    /// Audio/video file metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_metadata: Option<FileMetadata>,
    /// Recording date (may differ from meeting.date)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording_date: Option<NaiveDateTime>,
    /// Path to audio file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_file: Option<String>,
    /// Path to video file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_file: Option<String>,
    /// Platform source (e.g., "Upload", "Teams", "Zoom", "Meet")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum MeetingStatus {
    Importing,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TranscriptionInfo {
    pub provider: String,
    pub model: String,
    pub completed_at: DateTime<Utc>,
}

impl Meeting {
    pub fn new(title: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            title,
            date: now,
            duration_seconds: None,
            status: MeetingStatus::Importing,
            transcription: None,
            created_at: now,
            updated_at: now,
            participants: None,
            location: None,
            organizer: None,
            metadata_source: None,
            file_metadata: None,
            recording_date: None,
            audio_file: None,
            video_file: None,
            platform: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Transcript {
    pub segments: Vec<TranscriptSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TranscriptSegment {
    pub id: u32,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Template for summary generation. User picks per request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum SummaryTemplate {
    KeyPoints,
    ActionItems,
    Decisions,
    /// All three sections (key points, action items, decisions) in one LLM call.
    Full,
}

/// Status of a summary generation job.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum SummaryStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

/// A meeting summary. One per template per meeting.
///
/// `content` always holds raw LLM output (markdown). The `key_points`,
/// `action_items`, `decisions` Vec fields hold parsed sections — populated
/// for `Full` template (all three) or single-template requests (matching
/// field only); others left empty.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Summary {
    pub id: String,
    pub meeting_id: String,
    pub template: SummaryTemplate,
    pub language: Option<String>,
    pub status: SummaryStatus,
    /// Raw LLM output (markdown).
    pub content: String,
    /// Parsed key points (Full or KeyPoints template).
    pub key_points: Vec<String>,
    /// Parsed action items (Full or ActionItems template).
    pub action_items: Vec<String>,
    /// Parsed decisions (Full or Decisions template).
    pub decisions: Vec<String>,
    pub provider: String,
    pub model: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
