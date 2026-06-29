//! Domain models

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub date: DateTime<Utc>,
    pub duration_seconds: Option<u64>,
    pub status: MeetingStatus,
    pub transcription: Option<TranscriptionInfo>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeetingStatus {
    Importing,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcript {
    pub segments: Vec<TranscriptSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub id: u32,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Template for summary generation. User picks per request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
