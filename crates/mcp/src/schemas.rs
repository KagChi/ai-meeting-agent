use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EmptyParams {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportFromFileRequest {
    /// Local filesystem path, or OpenClaw inbound media URI
    /// (`media://inbound/<filename>`). OpenClaw URIs resolve under
    /// `OPENCLAW_MEDIA_INBOUND_DIR` (default `~/.openclaw/media/inbound`).
    /// The path must be readable by the MCP server process.
    pub file_path: String,
    /// Optional meeting title.
    pub title: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportFromUrlRequest {
    /// HTTP(S) URL for the MCP server to download and import.
    pub url: String,
    /// Optional filename override (for extension detection). If not provided, extracts from URL.
    pub filename: Option<String>,
    /// Optional meeting title.
    pub title: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct JobIdRequest {
    pub job_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MeetingIdRequest {
    pub meeting_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateSummaryRequest {
    pub meeting_id: String,
    /// key_points, action_items, decisions, or full. Defaults to full.
    pub template: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetSummaryRequest {
    pub meeting_id: String,
    /// key_points, action_items, decisions, or full. Defaults to full.
    pub template: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateMeetingRequest {
    pub meeting_id: String,
    pub title: Option<String>,
    /// RFC3339 date/time, for example 2026-07-09T10:00:00Z.
    pub date: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportTranscriptRequest {
    pub meeting_id: String,
    /// srt, vtt, text, or json. Defaults to text.
    pub format: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExportTranscriptResponse {
    pub meeting_id: String,
    pub format: String,
    pub content: String,
}
