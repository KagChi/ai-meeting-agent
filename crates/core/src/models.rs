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

/// Audio file metadata
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
    /// Audio file metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_metadata: Option<FileMetadata>,
    /// Recording date (may differ from meeting.date)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording_date: Option<NaiveDateTime>,
    /// Path to audio file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_file: Option<String>,
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
    /// Internship meeting notes format (`docs/meetings/YYYY-MM-DD-meeting-notes.md`).
    #[serde(rename = "meetingnotes", alias = "meeting_notes")]
    MeetingNotes,
}

/// Output format for summary content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum SummaryFormat {
    /// Structured markdown with headings and bullet points (default).
    Markdown,
    /// Plain text without markdown formatting.
    RawText,
}

impl Default for SummaryFormat {
    fn default() -> Self {
        Self::Markdown
    }
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

/// A meeting summary. One per (template, format) combination per meeting.
///
/// `content` holds formatted output based on `format` field:
/// - `Markdown`: structured with ## headings and bullet points
/// - `RawText`: plain text without markdown formatting
///
/// The `key_points`, `action_items`, `decisions` Vec fields hold parsed sections
/// (only populated for markdown format).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Summary {
    pub id: String,
    pub meeting_id: String,
    pub template: SummaryTemplate,
    pub format: SummaryFormat,
    pub language: Option<String>,
    pub status: SummaryStatus,
    /// Formatted content (markdown or raw text based on format field).
    pub content: String,
    /// Parsed key points (populated only for markdown format).
    pub key_points: Vec<String>,
    /// Parsed action items (populated only for markdown format).
    pub action_items: Vec<String>,
    /// Parsed decisions (populated only for markdown format).
    pub decisions: Vec<String>,
    pub provider: String,
    pub model: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Transcript version metadata for tracking retranscriptions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TranscriptVersion {
    pub id: i64,
    pub meeting_id: String,
    pub version: u32,
    pub provider: String,
    pub model: String,
    pub language: Option<String>,
    pub segment_count: u32,
    pub created_at: DateTime<Utc>,
}

/// A transcript segment that matched a global search query.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MatchedSegment {
    pub segment_id: u32,
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person_id: Option<String>,
}

/// Source of a voiceprint enrollment sample.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum VoiceprintSampleSource {
    Upload,
    MeetingTurn,
}

/// How the current voiceprint centroid was enrolled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum VoiceprintEnrolledFrom {
    Sample,
    MeetingTurn,
}

/// A person in the voice bank (stable identity across meetings).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Person {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Person {
    pub fn new(name: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            aliases: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }
}

/// Embedding centroid for a person (match key for identification).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Voiceprint {
    pub id: String,
    pub person_id: String,
    /// Embedding model id (e.g. wespeaker-voxceleb-resnet34).
    pub model: String,
    pub dim: u32,
    /// L2-normalized f32 embedding (not serialized in API by default).
    #[serde(skip_serializing)]
    pub centroid: Vec<f32>,
    pub enrolled_from: VoiceprintEnrolledFrom,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Metadata for an enrollment audio sample (bytes on disk at `audio_path`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct VoiceprintSample {
    pub id: String,
    pub person_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voiceprint_id: Option<String>,
    /// Relative path under the storage base (e.g. voiceprints/{person_id}/samples/{id}.wav).
    pub audio_path: String,
    pub duration_s: f64,
    pub source: VoiceprintSampleSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meeting_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segment_ids: Vec<u32>,
    pub created_at: DateTime<Utc>,
}

/// A meeting with matched transcript segments from global search.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MeetingSearchResult {
    pub id: String,
    pub title: String,
    pub date: DateTime<Utc>,
    pub duration_seconds: Option<u64>,
    pub status: MeetingStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub participants: Option<Vec<String>>,
    /// Top matching segments (capped; see `match_count` for total).
    pub matched_segments: Vec<MatchedSegment>,
    /// Total matching segments in this meeting (may exceed matched_segments.len).
    pub match_count: usize,
    /// FTS5 relevance (lower = better match).
    pub relevance_score: f64,
}
