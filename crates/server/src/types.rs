use chrono::{DateTime, Utc};
use meeting_agent_core::jobs::{Job, JobState, JobType};
use meeting_agent_core::models::{
    Meeting, MeetingSearchResult, MeetingStatus, Summary, SummaryFormat, SummaryTemplate,
};
use meeting_agent_core::transcription::TranscriptionResponse;
use serde::{Deserialize, Serialize};

// === Request Types ===

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateMeetingRequest {
    pub title: String,
    #[serde(default)]
    pub date: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateMeetingRequest {
    pub title: Option<String>,
    pub date: Option<DateTime<Utc>>,
    /// Replace meeting participants list when provided (including empty list).
    #[serde(default)]
    pub participants: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateSummaryRequest {
    /// Full summary body (markdown or plain text matching format).
    pub content: String,
    #[serde(default)]
    pub format: Option<SummaryFormat>,
}

// === Response Types ===

/// Meeting response with all fields including metadata
///
/// Example response with metadata:
/// ```json
/// {
///   "id": "550e8400-e29b-41d4-a716-446655440000",
///   "title": "Team Standup 2024-03-15",
///   "date": "2024-03-15T09:00:00Z",
///   "duration_seconds": 1800,
///   "status": "ready",
///   "participants": ["Alice", "Bob", "Charlie"],
///   "location": "Conference Room A",
///   "organizer": "Alice",
///   "metadata_source": "filename",
///   "file_metadata": {
///     "codec": "aac",
///     "sample_rate": 44100,
///     "bit_rate": 128000,
///     "channels": 2,
///     "file_size_bytes": 5242880
///   },
///   "recording_date": "2024-03-15T09:00:00",
///   "audio_file": "http://localhost/meetings/550e8400-e29b-41d4-a716-446655440000/recording",
///   "created_at": "2024-03-15T08:50:00Z",
///   "updated_at": "2024-03-15T09:30:00Z"
/// }
/// ```
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ListMeetingsResponse {
    pub meetings: Vec<Meeting>,
    pub total: u64,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct MeetingResponse {
    #[serde(flatten)]
    pub meeting: Meeting,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TranscriptResponse {
    pub meeting_id: String,
    pub status: MeetingStatus,
    pub transcript: Option<TranscriptionResponse>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PaginationQuery {
    #[serde(default = "default_meetings_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_meetings_limit() -> u32 {
    20
}

fn default_search_limit() -> u32 {
    50
}

/// Query params for `GET /transcripts/search`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SearchTranscriptsQuery {
    /// Plain-text full-text search query (special characters are escaped for FTS5).
    pub q: String,
    #[serde(default = "default_search_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

/// Global transcript search response (meetings with matched segments).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SearchTranscriptsResponse {
    pub query: String,
    pub total_meetings: u64,
    pub limit: u32,
    pub offset: u32,
    pub meetings: Vec<MeetingSearchResult>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ErrorResponse {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

// === Import Types ===

/// Response when creating an import job (202 Accepted)
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ImportResponse {
    pub job_id: String,
    pub status: JobState,
}

/// Response for polling job status (import or summary)
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct JobStatusResponse {
    pub job_id: String,
    pub job_type: JobType,
    pub state: JobState,
    pub progress: Vec<meeting_agent_core::jobs::ProgressEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meeting_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Job> for JobStatusResponse {
    fn from(job: Job) -> Self {
        Self {
            created_at: job.created_at,
            updated_at: job.updated_at,
            progress: job.progress,
            job_id: job.id,
            job_type: job.job_type,
            state: job.state,
            meeting_id: job.meeting_id,
            template: job.template,
            error: job.error,
        }
    }
}

/// Response for import validation
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ImportValidationResponse {
    pub valid: bool,
    pub format: String,
    pub size: u64,
}

/// Response for cancel operation
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CancelImportResponse {
    pub job_id: String,
    pub cancelled: bool,
}

// === Summary Types ===

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateSummaryRequest {
    pub template: SummaryTemplate,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub format: Option<SummaryFormat>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SummaryResponse {
    #[serde(flatten)]
    pub summary: Summary,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ListSummariesResponse {
    pub meeting_id: String,
    pub summaries: Vec<Summary>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CreateSummaryResponse {
    pub job_id: String,
    pub status: JobState,
}

// === Config Types ===

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TranscriptionConfigResponse {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    pub chunk_seconds: f64,
    pub chunk_concurrency: usize,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SummaryConfigResponse {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u32,
    pub language: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ConfigResponse {
    pub transcription: TranscriptionConfigResponse,
    pub summary: SummaryConfigResponse,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateTranscriptionConfigRequest {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    pub chunk_seconds: f64,
    pub chunk_concurrency: usize,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateSummaryConfigRequest {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u32,
    pub language: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateConfigRequest {
    pub transcription: UpdateTranscriptionConfigRequest,
    pub summary: UpdateSummaryConfigRequest,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TranscriptVersionsResponse {
    pub meeting_id: String,
    pub versions: Vec<meeting_agent_core::models::TranscriptVersion>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_meeting_request_with_date() {
        let json = r#"{"title": "Test Meeting", "date": "2026-06-29T03:40:00Z"}"#;
        let req: CreateMeetingRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.title, "Test Meeting");
        assert!(req.date.is_some());
    }

    #[test]
    fn test_create_meeting_request_without_date() {
        let json = r#"{"title": "Test Meeting"}"#;
        let req: CreateMeetingRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.title, "Test Meeting");
        assert!(req.date.is_none());
    }

    #[test]
    fn test_update_meeting_request_partial() {
        let json = r#"{"title": "Updated Title"}"#;
        let req: UpdateMeetingRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.title, Some("Updated Title".to_string()));
        assert!(req.date.is_none());
    }

    #[test]
    fn test_update_meeting_request_empty() {
        let json = r#"{}"#;
        let req: UpdateMeetingRequest = serde_json::from_str(json).unwrap();
        assert!(req.title.is_none());
        assert!(req.date.is_none());
    }

    #[test]
    fn test_error_response_serialization() {
        let err = ErrorResponse {
            error: "Not Found".to_string(),
            details: Some("Meeting not found".to_string()),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("Not Found"));
        assert!(json.contains("Meeting not found"));
    }

    #[test]
    fn test_error_response_without_details() {
        let err = ErrorResponse {
            error: "Bad Request".to_string(),
            details: None,
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("Bad Request"));
        assert!(!json.contains("details"));
    }
}
