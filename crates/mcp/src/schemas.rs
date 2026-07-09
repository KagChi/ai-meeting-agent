use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EmptyParams {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportMeetingAudioRequest {
    /// Path to audio/video file accessible by MCP server process. For remote MCP, prefer file_base64 + filename.
    pub file_path: Option<String>,
    /// HTTP(S) URL for the MCP server to download, then import.
    pub file_url: Option<String>,
    /// Base64-encoded audio/video file bytes. Use this when MCP server runs remotely.
    pub file_base64: Option<String>,
    /// Original filename for file_base64, including extension (for example meeting.mp3).
    pub filename: Option<String>,
    /// Optional meeting title.
    pub title: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportFromFileRequest {
    /// Path to audio/video file accessible by MCP server process.
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
pub struct ImportFromBase64Request {
    /// Base64-encoded audio/video file bytes.
    pub data: String,
    /// Original filename, including extension (e.g., meeting.mp3). Required for format detection.
    pub filename: String,
    /// Optional meeting title.
    pub title: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateUploadRequest {
    /// Original filename, including extension (for example meeting.mp3).
    pub filename: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UploadChunkRequest {
    /// Upload id returned by createUpload.
    pub upload_id: String,
    /// Base64-encoded chunk bytes. Keep chunks small enough for MCP transport, for example 4-16 MiB raw bytes.
    pub chunk_base64: String,
    /// Optional expected byte offset before appending this chunk.
    pub offset: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FinishUploadRequest {
    /// Upload id returned by createUpload.
    pub upload_id: String,
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
