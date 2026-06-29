//! HTTP handlers

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::state::AppState;
use crate::types::{
    CreateMeetingRequest, ListMeetingsResponse, MeetingResponse, TranscriptResponse,
    UpdateMeetingRequest,
};
use crate::validation;

/// Health check endpoint
pub async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok"
    }))
}

/// Version endpoint
pub async fn version() -> Json<Value> {
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "name": env!("CARGO_PKG_NAME")
    }))
}

/// List all meetings
pub async fn list_meetings(
    State(state): State<AppState>,
) -> Result<Json<ListMeetingsResponse>, ApiError> {
    let meetings = state.storage.list_meetings()?;
    Ok(Json(ListMeetingsResponse { meetings }))
}

/// Get a specific meeting
pub async fn get_meeting(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<MeetingResponse>, ApiError> {
    validation::validate_uuid(&id)?;
    let meeting = state.storage.get_meeting(&id)?;
    Ok(Json(MeetingResponse { meeting }))
}

/// Get meeting transcript
pub async fn get_transcript(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TranscriptResponse>, ApiError> {
    validation::validate_uuid(&id)?;

    // Check if meeting exists first
    let meeting = state.storage.get_meeting(&id)?;

    // Try to get transcript
    let transcript = state.storage.get_transcript(&id).ok();

    Ok(Json(TranscriptResponse {
        meeting_id: meeting.id.clone(),
        status: meeting.status.clone(),
        transcript,
    }))
}

/// Create a new meeting
pub async fn create_meeting(
    State(state): State<AppState>,
    Json(req): Json<CreateMeetingRequest>,
) -> Result<(StatusCode, Json<MeetingResponse>), ApiError> {
    // Validate title
    validation::validate_meeting_title(&req.title)?;

    // Create meeting
    let mut meeting = meeting_agent_core::models::Meeting::new(req.title);

    // Override date if provided
    if let Some(date) = req.date {
        meeting.date = date;
    }

    // Save to storage
    state.storage.create_meeting(&meeting)?;

    Ok((StatusCode::CREATED, Json(MeetingResponse { meeting })))
}

/// Update an existing meeting
pub async fn update_meeting(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateMeetingRequest>,
) -> Result<Json<MeetingResponse>, ApiError> {
    // Validate UUID
    validation::validate_uuid(&id)?;

    // Validate update request has at least one field
    validation::validate_update_request(&req.title, &req.date)?;

    // Load existing meeting
    let mut meeting = state.storage.get_meeting(&id)?;

    // Apply updates
    if let Some(title) = req.title {
        meeting.title = title;
    }
    if let Some(date) = req.date {
        meeting.date = date;
    }

    // Update timestamp
    meeting.updated_at = chrono::Utc::now();

    // Save changes
    state.storage.update_meeting(&meeting)?;

    Ok(Json(MeetingResponse { meeting }))
}

/// Delete a meeting
pub async fn delete_meeting(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    // Validate UUID
    validation::validate_uuid(&id)?;

    // Delete meeting and all associated files
    state.storage.delete_meeting(&id)?;

    Ok(StatusCode::NO_CONTENT)
}
