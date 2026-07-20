//! HTTP handlers

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use meeting_agent_core::models::Meeting;
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::state::AppState;
use crate::types::{
    CreateMeetingRequest, ListMeetingsResponse, MeetingResponse, PaginationQuery,
    TranscriptResponse, UpdateMeetingRequest,
};
use crate::validation;

/// Health check endpoint
#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Service is healthy")
    )
)]
pub async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok"
    }))
}

/// Version endpoint
#[utoipa::path(
    get,
    path = "/version",
    responses(
        (status = 200, description = "Service version information")
    )
)]
pub async fn version() -> Json<Value> {
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "name": env!("CARGO_PKG_NAME")
    }))
}

/// List all meetings
///
/// Returns all meetings with their metadata. Each meeting includes optional metadata fields:
/// participants, location, organizer, metadata_source, file_metadata, recording_date, audio_file.
#[utoipa::path(
    get,
    path = "/meetings",
    tag = "meetings",
    responses(
        (status = 200, description = "List of all meetings with metadata", body = ListMeetingsResponse)
    )
)]
pub async fn list_meetings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<ListMeetingsResponse>, ApiError> {
    let limit = query.limit.clamp(1, 100);
    let meetings = state
        .storage
        .list_meetings_paginated(limit, query.offset)
        .await?
        .into_iter()
        .map(|meeting| with_recording_url(meeting, &headers))
        .collect();
    let total = state.storage.count_meetings().await?;
    Ok(Json(ListMeetingsResponse {
        meetings,
        total,
        limit,
        offset: query.offset,
    }))
}

/// Get a specific meeting
///
/// Returns meeting details including metadata fields:
/// - `participants`: List of meeting participants
/// - `location`: Physical or virtual location
/// - `organizer`: Meeting organizer
/// - `metadata_source`: Source of metadata (user_provided, calendar_bot, filename, ffprobe, default)
/// - `file_metadata`: Audio file metadata (codec, sample_rate, bit_rate, channels, file_size_bytes)
/// - `recording_date`: Recording date (may differ from meeting date)
/// - `audio_file`: Path to audio file
#[utoipa::path(
    get,
    path = "/meetings/{id}",
    tag = "meetings",
    params(
        ("id" = String, Path, description = "Meeting ID or prefix")
    ),
    responses(
        (status = 200, description = "Meeting details with metadata", body = MeetingResponse),
        (status = 404, description = "Meeting not found", body = ErrorResponse)
    )
)]
pub async fn get_meeting(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<MeetingResponse>, ApiError> {
    validation::validate_uuid(&id)?;
    let meeting = with_recording_url(state.storage.get_meeting(&id).await?, &headers);
    Ok(Json(MeetingResponse { meeting }))
}

/// Get meeting recording file.
pub async fn get_recording(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    validation::validate_uuid(&id)?;
    let path = state.storage.get_recording_path(&id).await?;
    let mime = meeting_agent_core::storage::MeetingStorage::recording_mime_type(&path);
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| ApiError::InternalServerError(format!("Failed to read recording: {e}")))?;

    let mut response = bytes.into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    Ok(response)
}

/// Get meeting transcript
#[utoipa::path(
    get,
    path = "/meetings/{id}/transcript",
    tag = "transcripts",
    params(
        ("id" = String, Path, description = "Meeting ID or prefix")
    ),
    responses(
        (status = 200, description = "Meeting transcript", body = TranscriptResponse),
        (status = 404, description = "Meeting not found", body = ErrorResponse)
    )
)]
pub async fn get_transcript(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TranscriptResponse>, ApiError> {
    validation::validate_uuid(&id)?;

    // Check if meeting exists first
    let meeting = state.storage.get_meeting(&id).await?;

    // Try to get transcript
    let transcript = state.storage.get_transcript(&id).await.ok();

    Ok(Json(TranscriptResponse {
        meeting_id: meeting.id.clone(),
        status: meeting.status.clone(),
        transcript,
    }))
}

/// Create a new meeting
///
/// Creates a new meeting with optional metadata. Metadata fields can be populated
/// through the import endpoint, which automatically extracts metadata from files.
///
/// Response includes all metadata fields (see GET /meetings/{id} for details).
#[utoipa::path(
    post,
    path = "/meetings",
    tag = "meetings",
    request_body = CreateMeetingRequest,
    responses(
        (status = 201, description = "Meeting created with metadata", body = MeetingResponse),
        (status = 400, description = "Invalid request", body = ErrorResponse)
    )
)]
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
    state.storage.create_meeting(&meeting).await?;

    Ok((StatusCode::CREATED, Json(MeetingResponse { meeting })))
}

/// Update an existing meeting
#[utoipa::path(
    patch,
    path = "/meetings/{id}",
    tag = "meetings",
    params(
        ("id" = String, Path, description = "Meeting ID or prefix")
    ),
    request_body = UpdateMeetingRequest,
    responses(
        (status = 200, description = "Meeting updated", body = MeetingResponse),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 404, description = "Meeting not found", body = ErrorResponse)
    )
)]
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
    let mut meeting = state.storage.get_meeting(&id).await?;

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
    state.storage.update_meeting(&meeting).await?;

    Ok(Json(MeetingResponse { meeting }))
}

/// Delete a meeting
#[utoipa::path(
    delete,
    path = "/meetings/{id}",
    tag = "meetings",
    params(
        ("id" = String, Path, description = "Meeting ID or prefix")
    ),
    responses(
        (status = 204, description = "Meeting deleted"),
        (status = 404, description = "Meeting not found", body = ErrorResponse)
    )
)]
pub async fn delete_meeting(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    // Validate UUID
    validation::validate_uuid(&id)?;

    // Delete meeting and all associated files
    state.storage.delete_meeting(&id).await?;

    Ok(StatusCode::NO_CONTENT)
}

fn with_recording_url(mut meeting: Meeting, headers: &HeaderMap) -> Meeting {
    if meeting.audio_file.is_some() {
        let proto = headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("http");
        let host = headers
            .get("x-forwarded-host")
            .or_else(|| headers.get(header::HOST))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("localhost");
        meeting.audio_file = Some(format!(
            "{proto}://{host}/meetings/{}/recording",
            meeting.id
        ));
    }
    meeting
}
