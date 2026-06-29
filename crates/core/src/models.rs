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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub text: String,
    pub key_points: Vec<String>,
    pub action_items: Vec<String>,
    pub generated_at: DateTime<Utc>,
}
