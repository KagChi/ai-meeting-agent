//! Orchestrator domain models.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Status of a durable orchestrator run (SQLite).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum OrchestratorRunStatus {
    Received,
    Downloading,
    Importing,
    Completed,
    Failed,
    Skipped,
}

impl OrchestratorRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Received => "received",
            Self::Downloading => "downloading",
            Self::Importing => "importing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "downloading" => Self::Downloading,
            "importing" => Self::Importing,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "skipped" => Self::Skipped,
            _ => Self::Received,
        }
    }
}

/// Durable row for idempotent meeting-end → import.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OrchestratorRun {
    pub id: String,
    pub source: String,
    pub platform: Option<String>,
    pub native_meeting_id: Option<String>,
    pub recording_key: Option<String>,
    /// Stable idempotency key (unique).
    pub external_key: String,
    pub status: OrchestratorRunStatus,
    pub job_id: Option<String>,
    pub meeting_id: Option<String>,
    pub title: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Normalized meeting-ended event (from Vexa webhook or internal mapping).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MeetingEndedEvent {
    /// Capture platform: `teams` | `zoom` | `google_meet` | `jitsi` | …
    #[serde(default)]
    pub platform: Option<String>,
    /// Platform-native meeting id (from share link).
    #[serde(default)]
    pub native_meeting_id: Option<String>,
    /// Optional Vexa-side meeting uuid.
    #[serde(default)]
    pub meeting_id: Option<String>,
    /// Terminal status from bot spine (`completed`, `failed`, …).
    #[serde(default)]
    pub status: Option<String>,
    /// Direct download URL for the recording (preferred when present).
    #[serde(default)]
    pub recording_url: Option<String>,
    /// Object key in MinIO / object store.
    #[serde(default)]
    pub recording_key: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// Filename hint for conversion (e.g. `meeting.webm`).
    #[serde(default)]
    pub filename: Option<String>,
}

impl MeetingEndedEvent {
    /// True when the event is a successful completion (import-worthy).
    pub fn is_completed(&self) -> bool {
        match self.status.as_deref() {
            None | Some("") => true,
            Some(s) => {
                let s = s.to_ascii_lowercase();
                matches!(
                    s.as_str(),
                    "completed" | "complete" | "done" | "finished" | "stopped" | "success"
                )
            }
        }
    }

    /// Build a stable external key for idempotency.
    pub fn external_key(&self) -> String {
        if let (Some(p), Some(n)) = (&self.platform, &self.native_meeting_id) {
            if !p.is_empty() && !n.is_empty() {
                return format!("vexa:{}:{}", p.to_ascii_lowercase(), n);
            }
        }
        if let Some(id) = &self.meeting_id {
            if !id.is_empty() {
                return format!("vexa:meeting:{id}");
            }
        }
        if let Some(url) = &self.recording_url {
            if !url.is_empty() {
                return format!("vexa:url:{}", short_hash(url));
            }
        }
        if let Some(key) = &self.recording_key {
            if !key.is_empty() {
                return format!("vexa:object:{key}");
            }
        }
        format!("vexa:unknown:{}", uuid::Uuid::new_v4())
    }
}

/// Manual dispatch request (`POST /orchestrator/import`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OrchestratorImportRequest {
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub native_meeting_id: Option<String>,
    #[serde(default)]
    pub recording_url: Option<String>,
    #[serde(default)]
    pub recording_key: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    /// Force re-import even if an external_key already completed.
    #[serde(default)]
    pub force: bool,
}

impl OrchestratorImportRequest {
    pub fn into_event(self) -> MeetingEndedEvent {
        MeetingEndedEvent {
            platform: self.platform,
            native_meeting_id: self.native_meeting_id,
            meeting_id: None,
            status: Some("completed".to_string()),
            recording_url: self.recording_url,
            recording_key: self.recording_key,
            title: self.title,
            filename: self.filename,
        }
    }
}

/// Result of accepting an orchestrator import (async job already spawned or skipped).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OrchestratorStartResult {
    pub run_id: String,
    pub external_key: String,
    pub status: OrchestratorRunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meeting_id: Option<String>,
    /// True when an existing completed/in-flight run was reused.
    pub reused: bool,
}

fn short_hash(s: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:x}", h.finish())
}

/// Loose Vexa webhook JSON → [`MeetingEndedEvent`].
///
/// Accepts several field name variants seen across Vexa versions.
pub fn parse_vexa_webhook(value: &serde_json::Value) -> MeetingEndedEvent {
    let obj = value.as_object();
    let get_str = |keys: &[&str]| -> Option<String> {
        let o = obj?;
        for k in keys {
            if let Some(v) = o.get(*k) {
                if let Some(s) = v.as_str() {
                    if !s.is_empty() {
                        return Some(s.to_string());
                    }
                }
            }
        }
        // Nested `meeting` / `data` objects
        for nest in ["meeting", "data", "payload", "recording"] {
            if let Some(inner) = o.get(nest).and_then(|v| v.as_object()) {
                for k in keys {
                    if let Some(s) = inner.get(*k).and_then(|v| v.as_str()) {
                        if !s.is_empty() {
                            return Some(s.to_string());
                        }
                    }
                }
            }
        }
        None
    };

    MeetingEndedEvent {
        platform: get_str(&["platform", "meeting_platform"]),
        native_meeting_id: get_str(&[
            "native_meeting_id",
            "nativeMeetingId",
            "meeting_code",
            "meetingCode",
        ]),
        meeting_id: get_str(&["meeting_id", "meetingId", "id"]),
        status: get_str(&["status", "state", "completion_reason", "completionReason"]),
        recording_url: get_str(&[
            "recording_url",
            "recordingUrl",
            "download_url",
            "downloadUrl",
            "url",
            "audio_url",
            "audioUrl",
        ]),
        recording_key: get_str(&[
            "recording_key",
            "recordingKey",
            "object_key",
            "objectKey",
            "s3_key",
            "key",
        ]),
        title: get_str(&["title", "meeting_title", "bot_name", "botName", "name"]),
        filename: get_str(&["filename", "file_name", "fileName"]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_key_platform_native() {
        let e = MeetingEndedEvent {
            platform: Some("teams".into()),
            native_meeting_id: Some("abc-123".into()),
            meeting_id: None,
            status: Some("completed".into()),
            recording_url: None,
            recording_key: None,
            title: None,
            filename: None,
        };
        assert_eq!(e.external_key(), "vexa:teams:abc-123");
        assert!(e.is_completed());
    }

    #[test]
    fn parse_nested_webhook() {
        let v = serde_json::json!({
            "status": "completed",
            "meeting": {
                "platform": "google_meet",
                "native_meeting_id": "xxx-yyyy-zzz"
            },
            "recording": {
                "url": "http://minio:9000/vexa/rec.webm"
            }
        });
        let e = parse_vexa_webhook(&v);
        assert_eq!(e.platform.as_deref(), Some("google_meet"));
        assert_eq!(e.native_meeting_id.as_deref(), Some("xxx-yyyy-zzz"));
        assert_eq!(
            e.recording_url.as_deref(),
            Some("http://minio:9000/vexa/rec.webm")
        );
        assert!(e.is_completed());
    }

    #[test]
    fn failed_status_not_completed() {
        let e = MeetingEndedEvent {
            platform: Some("teams".into()),
            native_meeting_id: Some("x".into()),
            meeting_id: None,
            status: Some("failed".into()),
            recording_url: None,
            recording_key: None,
            title: None,
            filename: None,
        };
        assert!(!e.is_completed());
    }
}
