use crate::error::ApiError;
use crate::state::AppState;
use crate::types::{
    CreateSummaryRequest, CreateSummaryResponse, ListSummariesResponse, SummaryResponse,
};
use crate::validation;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use meeting_agent_core::jobs::JobType;
use meeting_agent_core::models::MeetingStatus;
use meeting_agent_core::summary_job::run_summary;

pub async fn create_summary(
    State(state): State<AppState>,
    Path(meeting_id): Path<String>,
    Json(req): Json<CreateSummaryRequest>,
) -> Result<axum::response::Response, ApiError> {
    validation::validate_uuid(&meeting_id)?;

    let meeting = state.storage.get_meeting(&meeting_id)?;

    if meeting.status != MeetingStatus::Ready {
        return Err(ApiError::Conflict(format!(
            "Meeting {meeting_id} is not ready for summary (status: {:?}). Wait for import to complete.",
            meeting.status
        )));
    }

    let language = req
        .language
        .or_else(|| state.config.summary.language.clone());

    let job_id = state.jobs.create_job(JobType::Summary);
    state
        .jobs
        .set_template(&job_id, format!("{:?}", req.template));
    state.jobs.set_meeting_id(&job_id, meeting_id.clone());

    let cancel_token = state
        .jobs
        .cancel_token(&job_id)
        .ok_or_else(|| ApiError::InternalServerError("Failed to get cancel token".to_string()))?;

    let config = state.config.clone();
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

pub async fn list_summaries(
    State(state): State<AppState>,
    Path(meeting_id): Path<String>,
) -> Result<Json<ListSummariesResponse>, ApiError> {
    validation::validate_uuid(&meeting_id)?;
    let _meeting = state.storage.get_meeting(&meeting_id)?;
    let summaries = state.storage.list_summaries(&meeting_id)?;
    Ok(Json(ListSummariesResponse {
        meeting_id,
        summaries,
    }))
}

pub async fn get_summary(
    State(state): State<AppState>,
    Path((meeting_id, template)): Path<(String, String)>,
) -> Result<Json<SummaryResponse>, ApiError> {
    validation::validate_uuid(&meeting_id)?;
    let _meeting = state.storage.get_meeting(&meeting_id)?;

    let template = parse_template(&template)?;
    let summary = state.storage.get_summary(&meeting_id, template)?;
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
