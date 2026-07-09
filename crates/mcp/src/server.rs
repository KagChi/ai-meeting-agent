use crate::{client::MeetingAgentClient, error::ClientError, schemas::*};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::TryStreamExt;
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResult, ContentBlock, Implementation,
        InitializeRequestParams, InitializeResult, ListToolsResult, PaginatedRequestParams,
        ServerCapabilities, ServerInfo, TextContent, Tool,
    },
    service::{RequestContext, RoleServer},
    ErrorData as McpError, ServerHandler,
};
use schemars::JsonSchema;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

#[derive(Clone)]
pub struct MeetingAgentMcpServer {
    client: MeetingAgentClient,
}

impl MeetingAgentMcpServer {
    pub fn new(client: MeetingAgentClient) -> Self {
        Self { client }
    }
}

impl ServerHandler for MeetingAgentMcpServer {
    fn get_info(&self) -> ServerInfo {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("meeting-agent-mcp", env!("CARGO_PKG_VERSION"))
                    .with_description("HTTP MCP wrapper for AI Meeting Agent API"),
            )
            .with_instructions("HTTP MCP wrapper for AI Meeting Agent API.".to_string())
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        Ok(self.get_info())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(vec![
            tool::<ImportMeetingAudioRequest>(
                "importMeetingAudio",
                "Import a meeting audio/video file from file_url, file_path accessible by MCP server, or small file_base64 payload. For large local remote uploads, use createUpload/uploadChunk/finishUpload.",
            ),
            tool::<CreateUploadRequest>(
                "createUpload",
                "Start a chunked remote upload. Returns upload_id.",
            ),
            tool::<UploadChunkRequest>(
                "uploadChunk",
                "Append one base64 chunk to an upload_id. Use repeated calls for large files.",
            ),
            tool::<FinishUploadRequest>(
                "finishUpload",
                "Finish chunked upload and import the uploaded file. Returns background job id.",
            ),
            tool::<JobIdRequest>("getJobStatus", "Get current import or summary job status."),
            tool::<JobIdRequest>(
                "streamJobEvents",
                "Read job progress events through meeting-agent SSE endpoint.",
            ),
            tool::<EmptyParams>("listMeetings", "List all meetings."),
            tool::<MeetingIdRequest>("getMeeting", "Get meeting details by full id or prefix."),
            tool::<MeetingIdRequest>("getTranscript", "Get meeting transcript with segments."),
            tool::<GenerateSummaryRequest>(
                "generateSummary",
                "Generate a meeting summary. Template defaults to full.",
            ),
            tool::<GetSummaryRequest>(
                "getSummary",
                "Get generated summary. Template defaults to full.",
            ),
            tool::<UpdateMeetingRequest>("updateMeeting", "Update meeting title and/or date."),
            tool::<MeetingIdRequest>("deleteMeeting", "Delete meeting and associated files."),
            tool::<JobIdRequest>("cancelJob", "Cancel a running import or summary job."),
            tool::<ExportTranscriptRequest>(
                "exportTranscript",
                "Export transcript as srt, vtt, text, or json.",
            ),
        ]))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let result = match request.name.as_ref() {
            "importMeetingAudio" => {
                let req: ImportMeetingAudioRequest = parse_arguments(&request.arguments)?;
                match (req.file_url, req.file_base64, req.filename, req.file_path) {
                    (Some(file_url), None, filename, None) => {
                        let path = download_url_to_upload(&file_url, filename.as_deref()).await?;
                        let result = self
                            .client
                            .import_meeting_audio(&path.to_string_lossy(), req.title.as_deref())
                            .await?;
                        tokio::spawn(async move {
                            if let Err(err) = tokio::fs::remove_file(&path).await {
                                tracing::warn!(%err, "failed to remove MCP URL download temp file");
                            }
                        });
                        result
                    }
                    (None, Some(file_base64), Some(filename), None) => {
                        let bytes = STANDARD.decode(file_base64).map_err(|err| {
                            ClientError::InvalidInput(format!("invalid file_base64: {err}"))
                        })?;
                        self.client
                            .import_meeting_audio_bytes(bytes, filename, req.title.as_deref())
                            .await?
                    }
                    (None, None, None, Some(file_path)) => {
                        self.client
                            .import_meeting_audio(&file_path, req.title.as_deref())
                            .await?
                    }
                    _ => {
                        return Err(ClientError::InvalidInput(
                            "provide exactly one import source: file_url, file_base64 + filename, or file_path accessible by the MCP server"
                                .to_string(),
                        )
                        .into())
                    }
                }
            }
            "createUpload" => {
                let req: CreateUploadRequest = parse_arguments(&request.arguments)?;
                create_upload(&req.filename).await?
            }
            "uploadChunk" => {
                let req: UploadChunkRequest = parse_arguments(&request.arguments)?;
                upload_chunk(&req.upload_id, &req.chunk_base64, req.offset).await?
            }
            "finishUpload" => {
                let req: FinishUploadRequest = parse_arguments(&request.arguments)?;
                let path = upload_path(&req.upload_id)?;
                let result = self
                    .client
                    .import_meeting_audio(&path.to_string_lossy(), req.title.as_deref())
                    .await?;
                tokio::spawn(async move {
                    if let Err(err) = tokio::fs::remove_file(&path).await {
                        tracing::warn!(%err, "failed to remove MCP upload temp file");
                    }
                });
                result
            }
            "getJobStatus" => {
                let req: JobIdRequest = parse_arguments(&request.arguments)?;
                self.client.get_job_status(&req.job_id).await?
            }
            "streamJobEvents" => {
                let req: JobIdRequest = parse_arguments(&request.arguments)?;
                let events = self.client.stream_job_events(&req.job_id).await?;
                return Ok(CallToolResult::success(vec![ContentBlock::Text(
                    TextContent::new(events),
                )]));
            }
            "listMeetings" => self.client.list_meetings().await?,
            "getMeeting" => {
                let req: MeetingIdRequest = parse_arguments(&request.arguments)?;
                self.client.get_meeting(&req.meeting_id).await?
            }
            "getTranscript" => {
                let req: MeetingIdRequest = parse_arguments(&request.arguments)?;
                self.client.get_transcript(&req.meeting_id).await?
            }
            "generateSummary" => {
                let req: GenerateSummaryRequest = parse_arguments(&request.arguments)?;
                let template = normalize_template(req.template.as_deref())?;
                self.client
                    .generate_summary(&req.meeting_id, template, req.language.as_deref())
                    .await?
            }
            "getSummary" => {
                let req: GetSummaryRequest = parse_arguments(&request.arguments)?;
                let template = normalize_template(req.template.as_deref())?;
                self.client.get_summary(&req.meeting_id, template).await?
            }
            "updateMeeting" => {
                let req: UpdateMeetingRequest = parse_arguments(&request.arguments)?;
                self.client
                    .update_meeting(&req.meeting_id, req.title, req.date)
                    .await?
            }
            "deleteMeeting" => {
                let req: MeetingIdRequest = parse_arguments(&request.arguments)?;
                self.client.delete_meeting(&req.meeting_id).await?
            }
            "cancelJob" => {
                let req: JobIdRequest = parse_arguments(&request.arguments)?;
                self.client.cancel_job(&req.job_id).await?
            }
            "exportTranscript" => {
                let req: ExportTranscriptRequest = parse_arguments(&request.arguments)?;
                let format = req
                    .format
                    .unwrap_or_else(|| "text".to_string())
                    .to_lowercase();
                let transcript = self.client.get_transcript(&req.meeting_id).await?;
                let content = export_transcript_content(&transcript, &format)?;
                serde_json::to_value(ExportTranscriptResponse {
                    meeting_id: req.meeting_id,
                    format,
                    content,
                })
                .map_err(ClientError::from)?
            }
            other => {
                return Err(McpError::invalid_params(
                    format!("Tool not found: {other}"),
                    None,
                ))
            }
        };

        let text = serde_json::to_string(&result).map_err(ClientError::from)?;
        Ok(CallToolResult::success(vec![ContentBlock::Text(
            TextContent::new(text),
        )]))
    }
}

fn tool<T: JsonSchema>(name: &'static str, description: &'static str) -> Tool {
    Tool::new(
        name,
        description,
        Arc::new(
            serde_json::to_value(schemars::schema_for!(T))
                .expect("schema serializes")
                .as_object()
                .expect("schema is object")
                .clone(),
        ),
    )
}

fn parse_arguments<T: serde::de::DeserializeOwned>(
    arguments: &Option<serde_json::Map<String, Value>>,
) -> Result<T, McpError> {
    serde_json::from_value(serde_json::to_value(arguments).unwrap_or(Value::Null))
        .map_err(|err| McpError::invalid_params(err.to_string(), None))
}

async fn create_upload(filename: &str) -> Result<Value, McpError> {
    let extension = std::path::Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| value.chars().all(|ch| ch.is_ascii_alphanumeric()))
        .unwrap_or("bin");
    let upload_id = format!("{}.{extension}", uuid::Uuid::new_v4());
    let path = upload_path(&upload_id)?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(ClientError::from)?;
    }
    tokio::fs::File::create(&path)
        .await
        .map_err(ClientError::from)?;
    Ok(serde_json::json!({ "upload_id": upload_id }))
}

async fn upload_chunk(
    upload_id: &str,
    chunk_base64: &str,
    offset: Option<u64>,
) -> Result<Value, McpError> {
    let path = upload_path(upload_id)?;
    let bytes = STANDARD
        .decode(chunk_base64)
        .map_err(|err| ClientError::InvalidInput(format!("invalid chunk_base64: {err}")))?;

    let current_size = tokio::fs::metadata(&path)
        .await
        .map_err(ClientError::from)?
        .len();
    if let Some(offset) = offset {
        if offset != current_size {
            return Err(ClientError::InvalidInput(format!(
                "chunk offset mismatch: expected {current_size}, got {offset}"
            ))
            .into());
        }
    }

    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .await
        .map_err(ClientError::from)?;
    file.write_all(&bytes).await.map_err(ClientError::from)?;
    file.flush().await.map_err(ClientError::from)?;

    Ok(serde_json::json!({
        "upload_id": upload_id,
        "bytes_received": current_size + bytes.len() as u64,
    }))
}

fn upload_path(upload_id: &str) -> Result<PathBuf, McpError> {
    if !upload_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '.')
    {
        return Err(ClientError::InvalidInput("invalid upload_id".to_string()).into());
    }
    Ok(std::env::temp_dir()
        .join("meeting-agent-mcp-uploads")
        .join(upload_id))
}

async fn download_url_to_upload(
    file_url: &str,
    filename: Option<&str>,
) -> Result<PathBuf, McpError> {
    let url = reqwest::Url::parse(file_url)
        .map_err(|err| ClientError::InvalidInput(format!("invalid file_url: {err}")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(
            ClientError::InvalidInput("file_url must use http or https".to_string()).into(),
        );
    }

    let filename = filename
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            url.path_segments()
                .and_then(|mut segments| segments.next_back())
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "download.bin".to_string());
    let extension = std::path::Path::new(&filename)
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| value.chars().all(|ch| ch.is_ascii_alphanumeric()))
        .unwrap_or("bin");
    let upload_id = format!("{}.{extension}", uuid::Uuid::new_v4());
    let path = upload_path(&upload_id)?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(ClientError::from)?;
    }

    let response = reqwest::get(url).await.map_err(ClientError::from)?;
    if !response.status().is_success() {
        return Err(ClientError::Api {
            status: response.status(),
            message: "failed to download file_url".to_string(),
        }
        .into());
    }

    let mut file = tokio::fs::File::create(&path)
        .await
        .map_err(ClientError::from)?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.try_next().await.map_err(ClientError::from)? {
        file.write_all(&chunk).await.map_err(ClientError::from)?;
    }
    file.flush().await.map_err(ClientError::from)?;
    Ok(path)
}

fn normalize_template(template: Option<&str>) -> Result<&'static str, McpError> {
    match template.unwrap_or("full").to_lowercase().as_str() {
        "key_points" | "keypoints" | "key-points" => Ok("key_points"),
        "action_items" | "actionitems" | "action-items" => Ok("action_items"),
        "decisions" => Ok("decisions"),
        "full" => Ok("full"),
        other => Err(ClientError::InvalidInput(format!(
            "invalid template '{other}'. Use key_points, action_items, decisions, or full"
        ))
        .into()),
    }
}

fn export_transcript_content(transcript: &Value, format: &str) -> Result<String, McpError> {
    let transcript_value = transcript
        .get("transcript")
        .ok_or_else(|| ClientError::InvalidInput("missing transcript field".to_string()))?;

    match format {
        "json" => Ok(serde_json::to_string_pretty(transcript_value).map_err(ClientError::from)?),
        "text" => Ok(transcript_value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()),
        "srt" => Ok(format_segments(transcript_value, true)),
        "vtt" => {
            let mut out = "WEBVTT\n\n".to_string();
            out.push_str(&format_segments(transcript_value, false));
            Ok(out)
        }
        other => Err(ClientError::InvalidInput(format!(
            "invalid export format '{other}'. Use srt, vtt, text, or json"
        ))
        .into()),
    }
}

fn format_segments(transcript: &Value, srt: bool) -> String {
    let mut out = String::new();
    let segments = transcript
        .get("segments")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);

    for (index, segment) in segments.iter().enumerate() {
        let start = segment
            .get("start")
            .and_then(Value::as_f64)
            .unwrap_or_default();
        let end = segment
            .get("end")
            .and_then(Value::as_f64)
            .unwrap_or_default();
        let text = segment
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();

        if srt {
            out.push_str(&format!(
                "{}\n{} --> {}\n{}\n\n",
                index + 1,
                format_timestamp(start, ":"),
                format_timestamp(end, ":"),
                text
            ));
        } else {
            out.push_str(&format!(
                "{} --> {}\n{}\n\n",
                format_timestamp(start, "."),
                format_timestamp(end, "."),
                text
            ));
        }
    }

    out
}

fn format_timestamp(seconds: f64, sep: &str) -> String {
    let h = (seconds / 3600.0) as u32;
    let m = ((seconds % 3600.0) / 60.0) as u32;
    let s = (seconds % 60.0) as u32;
    let ms = ((seconds % 1.0) * 1000.0) as u32;
    format!("{h:02}{sep}{m:02}{sep}{s:02}{sep}{ms:03}")
}
