use chrono::{DateTime, Utc};
use meeting_agent_core::models::{Meeting, MeetingStatus};
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
