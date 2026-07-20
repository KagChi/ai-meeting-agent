use crate::error::ApiError;
use crate::state::AppState;
use crate::types::{
    CreateSummaryRequest, CreateSummaryResponse, ListSummariesResponse, SummaryResponse,
};
use crate::validation;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use meeting_agent_core::jobs::JobType;
use meeting_agent_core::models::{MeetingStatus, SummaryFormat};
use meeting_agent_core::runners::run_summary;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct FormatQuery {
    #[serde(default)]
    pub format: Option<SummaryFormat>,
}

#[utoipa::path(
    post,
    path = "/meetings/{id}/summary",
    tag = "summaries",
    params(
        ("id" = String, Path, description = "Meeting ID or prefix")
    ),
    request_body = CreateSummaryRequest,
    responses(
        (status = 202, description = "Summary generation started", body = CreateSummaryResponse),
        (status = 404, description = "Meeting not found", body = ErrorResponse),
        (status = 409, description = "Meeting not ready", body = ErrorResponse)
    )
)]
pub async fn create_summary(
    State(state): State<AppState>,
    Path(meeting_id): Path<String>,
    Json(req): Json<CreateSummaryRequest>,
) -> Result<axum::response::Response, ApiError> {
    validation::validate_uuid(&meeting_id)?;

    let meeting = state.storage.get_meeting(&meeting_id).await?;

    if meeting.status != MeetingStatus::Ready {
        return Err(ApiError::Conflict(format!(
            "Meeting {meeting_id} is not ready for summary (status: {:?}). Wait for import to complete.",
            meeting.status
        )));
    }

    let language = if let Some(lang) = req.language {
        Some(lang)
    } else {
        state.config.read().await.summary.language.clone()
    };

    let format = req.format.unwrap_or_default(); // Default to markdown

    let job_id = state.jobs.create_job(JobType::Summary);
    state
        .jobs
        .set_template(&job_id, format!("{:?}", req.template));
    state.jobs.set_meeting_id(&job_id, meeting_id.clone());

    let cancel_token = state
        .jobs
        .cancel_token(&job_id)
        .ok_or_else(|| ApiError::InternalServerError("Failed to get cancel token".to_string()))?;

    let config = state.config.read().await.clone();
    let storage = state.storage.clone();
    let registry = state.jobs.clone();
    let cancel_token_clone = cancel_token.clone();
    let job_id_clone = job_id.clone();
    let template = req.template;

    tokio::spawn(async move {
        run_summary(
            job_id_clone,
            meeting_id,
            template,
            format,
            language,
            config,
            storage,
            registry,
            cancel_token_clone,
        )
        .await;
    });

    let job = state
        .jobs
        .get_job(&job_id)
        .ok_or_else(|| ApiError::InternalServerError("Job not found after creation".to_string()))?;

    Ok((
        StatusCode::ACCEPTED,
        Json(CreateSummaryResponse {
            job_id: job.id,
            status: job.state,
        }),
    )
        .into_response())
}

#[utoipa::path(
    get,
    path = "/meetings/{id}/summary",
    tag = "summaries",
    params(
        ("id" = String, Path, description = "Meeting ID or prefix")
    ),
    responses(
        (status = 200, description = "List of summaries", body = ListSummariesResponse),
        (status = 404, description = "Meeting not found", body = ErrorResponse)
    )
)]
pub async fn list_summaries(
    State(state): State<AppState>,
    Path(meeting_id): Path<String>,
) -> Result<Json<ListSummariesResponse>, ApiError> {
    validation::validate_uuid(&meeting_id)?;
    let _meeting = state.storage.get_meeting(&meeting_id).await?;
    let summaries = state.storage.list_summaries(&meeting_id).await?;
    Ok(Json(ListSummariesResponse {
        meeting_id,
        summaries,
    }))
}

#[utoipa::path(
    get,
    path = "/meetings/{id}/summary/{template}",
    tag = "summaries",
    params(
        ("id" = String, Path, description = "Meeting ID or prefix"),
        ("template" = String, Path, description = "Summary template: key_points, action_items, decisions, or full"),
        ("format" = Option<String>, Query, description = "Output format: markdown (default) or rawtext")
    ),
    responses(
        (status = 200, description = "Summary content", body = SummaryResponse),
        (status = 404, description = "Meeting or summary not found", body = ErrorResponse),
        (status = 400, description = "Invalid template or format", body = ErrorResponse)
    )
)]
pub async fn get_summary(
    State(state): State<AppState>,
    Path((meeting_id, template)): Path<(String, String)>,
    Query(query): Query<FormatQuery>,
) -> Result<Json<SummaryResponse>, ApiError> {
    validation::validate_uuid(&meeting_id)?;
    let _meeting = state.storage.get_meeting(&meeting_id).await?;

    let template = parse_template(&template)?;
    let format = query.format.unwrap_or_default(); // Default to markdown
    let summary = state.storage.get_summary(&meeting_id, template, format).await?;
    Ok(Json(SummaryResponse { summary }))
}

fn parse_template(s: &str) -> Result<meeting_agent_core::models::SummaryTemplate, ApiError> {
    match s.to_lowercase().as_str() {
        "key_points" => Ok(meeting_agent_core::models::SummaryTemplate::KeyPoints),
        "action_items" => Ok(meeting_agent_core::models::SummaryTemplate::ActionItems),
        "decisions" => Ok(meeting_agent_core::models::SummaryTemplate::Decisions),
        "full" => Ok(meeting_agent_core::models::SummaryTemplate::Full),
        other => Err(ApiError::BadRequest(format!(
            "Invalid summary template '{other}'. Valid: key_points, action_items, decisions, full"
        ))),
    }
}
