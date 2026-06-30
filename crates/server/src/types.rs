use chrono::{DateTime, Utc};
use meeting_agent_core::jobs::{Job, JobState, JobType};
use meeting_agent_core::models::{Meeting, MeetingStatus, Summary, SummaryTemplate};
use meeting_agent_core::transcription::TranscriptionResponse;
use serde::{Deserialize, Serialize};

// === Request Types ===

#[derive(Debug, Deserialize)]
pub struct CreateMeetingRequest {
    pub title: String,
    #[serde(default)]
    pub date: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateMeetingRequest {
    pub title: Option<String>,
    pub date: Option<DateTime<Utc>>,
}

// === Response Types ===

#[derive(Debug, Serialize)]
pub struct ListMeetingsResponse {
    pub meetings: Vec<Meeting>,
}

#[derive(Debug, Serialize)]
pub struct MeetingResponse {
    #[serde(flatten)]
    pub meeting: Meeting,
}

#[derive(Debug, Serialize)]
pub struct TranscriptResponse {
    pub meeting_id: String,
    pub status: MeetingStatus,
    pub transcript: Option<TranscriptionResponse>,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

// === Import Types ===

/// Response when creating an import job (202 Accepted)
#[derive(Debug, Serialize)]
pub struct ImportResponse {
    pub job_id: String,
    pub status: JobState,
}

/// Response for polling job status (import or summary)
#[derive(Debug, Serialize)]
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
#[derive(Debug, Serialize)]
pub struct ImportValidationResponse {
    pub valid: bool,
    pub format: String,
    pub size: u64,
}

/// Response for cancel operation
#[derive(Debug, Serialize)]
pub struct CancelImportResponse {
    pub job_id: String,
    pub cancelled: bool,
}

// === Summary Types ===

#[derive(Debug, Deserialize)]
pub struct CreateSummaryRequest {
    pub template: SummaryTemplate,
    #[serde(default)]
    pub language: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SummaryResponse {
    #[serde(flatten)]
    pub summary: Summary,
}

#[derive(Debug, Serialize)]
pub struct ListSummariesResponse {
    pub meeting_id: String,
    pub summaries: Vec<Summary>,
}

#[derive(Debug, Serialize)]
pub struct CreateSummaryResponse {
    pub job_id: String,
    pub status: JobState,
}

// === Config Types ===

#[derive(Debug, Serialize)]
pub struct TranscriptionConfigResponse {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    pub chunk_seconds: f64,
    pub chunk_concurrency: usize,
}

#[derive(Debug, Serialize)]
pub struct SummaryConfigResponse {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u32,
    pub language: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ConfigResponse {
    pub transcription: TranscriptionConfigResponse,
    pub summary: SummaryConfigResponse,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTranscriptionConfigRequest {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    pub chunk_seconds: f64,
    pub chunk_concurrency: usize,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSummaryConfigRequest {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u32,
    pub language: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateConfigRequest {
    pub transcription: UpdateTranscriptionConfigRequest,
    pub summary: UpdateSummaryConfigRequest,
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
