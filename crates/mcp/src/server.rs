use crate::{client::MeetingAgentClient, error::ClientError, schemas::*};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, ContentBlock, Implementation, ProtocolVersion, ServerCapabilities,
        ServerInfo,
    },
    service::RequestContext,
    tool, tool_handler, tool_router, ErrorData as McpError, Json, RoleServer, ServerHandler,
};
use serde_json::Value;

#[derive(Clone)]
pub struct MeetingAgentMcpServer {
    client: MeetingAgentClient,
    tool_router: ToolRouter<Self>,
}

impl MeetingAgentMcpServer {
    pub fn new(client: MeetingAgentClient) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router(router = tool_router)]
impl MeetingAgentMcpServer {
    #[tool(
        name = "importMeetingAudio",
        description = "Import a meeting audio/video file. Returns background job id."
    )]
    pub async fn import_meeting_audio(
        &self,
        Parameters(req): Parameters<ImportMeetingAudioRequest>,
    ) -> Result<Json<Value>, McpError> {
        let value = self
            .client
            .import_meeting_audio(&req.file_path, req.title.as_deref())
            .await?;
        Ok(Json(value))
    }

    #[tool(
        name = "getJobStatus",
        description = "Get current import or summary job status."
    )]
    pub async fn get_job_status(
        &self,
        Parameters(req): Parameters<JobIdRequest>,
    ) -> Result<Json<Value>, McpError> {
        Ok(Json(self.client.get_job_status(&req.job_id).await?))
    }

    #[tool(
        name = "streamJobEvents",
        description = "Read job progress events through meeting-agent SSE endpoint."
    )]
    pub async fn stream_job_events(
        &self,
        Parameters(req): Parameters<JobIdRequest>,
    ) -> Result<CallToolResult, McpError> {
        let events = self.client.stream_job_events(&req.job_id).await?;
        Ok(CallToolResult::success(vec![ContentBlock::text(events)]))
    }

    #[tool(name = "listMeetings", description = "List all meetings.")]
    pub async fn list_meetings(&self) -> Result<Json<Value>, McpError> {
        Ok(Json(self.client.list_meetings().await?))
    }

    #[tool(
        name = "getMeeting",
        description = "Get meeting details by full id or prefix."
    )]
    pub async fn get_meeting(
        &self,
        Parameters(req): Parameters<MeetingIdRequest>,
    ) -> Result<Json<Value>, McpError> {
        Ok(Json(self.client.get_meeting(&req.meeting_id).await?))
    }

    #[tool(
        name = "getTranscript",
        description = "Get meeting transcript with segments."
    )]
    pub async fn get_transcript(
        &self,
        Parameters(req): Parameters<MeetingIdRequest>,
    ) -> Result<Json<Value>, McpError> {
        Ok(Json(self.client.get_transcript(&req.meeting_id).await?))
    }

    #[tool(
        name = "generateSummary",
        description = "Generate a meeting summary. Template defaults to full."
    )]
    pub async fn generate_summary(
        &self,
        Parameters(req): Parameters<GenerateSummaryRequest>,
    ) -> Result<Json<Value>, McpError> {
        let template = normalize_template(req.template.as_deref())?;
        let value = self
            .client
            .generate_summary(&req.meeting_id, template, req.language.as_deref())
            .await?;
        Ok(Json(value))
    }

    #[tool(
        name = "getSummary",
        description = "Get generated summary. Template defaults to full."
    )]
    pub async fn get_summary(
        &self,
        Parameters(req): Parameters<GetSummaryRequest>,
    ) -> Result<Json<Value>, McpError> {
        let template = normalize_template(req.template.as_deref())?;
        Ok(Json(
            self.client.get_summary(&req.meeting_id, template).await?,
        ))
    }

    #[tool(
        name = "updateMeeting",
        description = "Update meeting title and/or date."
    )]
    pub async fn update_meeting(
        &self,
        Parameters(req): Parameters<UpdateMeetingRequest>,
    ) -> Result<Json<Value>, McpError> {
        let value = self
            .client
            .update_meeting(&req.meeting_id, req.title, req.date)
            .await?;
        Ok(Json(value))
    }

    #[tool(
        name = "deleteMeeting",
        description = "Delete meeting and associated files."
    )]
    pub async fn delete_meeting(
        &self,
        Parameters(req): Parameters<MeetingIdRequest>,
    ) -> Result<Json<Value>, McpError> {
        Ok(Json(self.client.delete_meeting(&req.meeting_id).await?))
    }

    #[tool(
        name = "cancelJob",
        description = "Cancel a running import or summary job."
    )]
    pub async fn cancel_job(
        &self,
        Parameters(req): Parameters<JobIdRequest>,
    ) -> Result<Json<Value>, McpError> {
        Ok(Json(self.client.cancel_job(&req.job_id).await?))
    }

    #[tool(
        name = "exportTranscript",
        description = "Export transcript as srt, vtt, text, or json."
    )]
    pub async fn export_transcript(
        &self,
        Parameters(req): Parameters<ExportTranscriptRequest>,
    ) -> Result<Json<ExportTranscriptResponse>, McpError> {
        let format = req
            .format
            .unwrap_or_else(|| "text".to_string())
            .to_lowercase();
        let transcript = self.client.get_transcript(&req.meeting_id).await?;
        let content = export_transcript_content(&transcript, &format)?;
        Ok(Json(ExportTranscriptResponse {
            meeting_id: req.meeting_id,
            format,
            content,
        }))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MeetingAgentMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions("HTTP MCP wrapper for AI Meeting Agent API.".to_string())
    }

    async fn initialize(
        &self,
        _request: rmcp::model::InitializeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::InitializeResult, McpError> {
        Ok(self.get_info())
    }
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
