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
    RenameSpeakersRequest, RenameSpeakersResponse, SearchTranscriptsQuery,
    SearchTranscriptsResponse, TranscriptResponse, UpdateMeetingRequest,
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

/// Global full-text search across all ready meetings' transcripts.
///
/// Returns meetings that contain matching segments, ordered by relevance.
/// Each meeting includes up to 10 top matching segments and a total match count.
#[utoipa::path(
    get,
    path = "/transcripts/search",
    tag = "transcripts",
    params(
        ("q" = String, Query, description = "Plain-text search query (special chars escaped)"),
        ("limit" = Option<u32>, Query, description = "Max meetings to return (default 50, max 500)"),
        ("offset" = Option<u32>, Query, description = "Meetings to skip (default 0)")
    ),
    responses(
        (status = 200, description = "Meetings with matched transcript segments", body = SearchTranscriptsResponse),
        (status = 400, description = "Invalid or empty query", body = ErrorResponse)
    )
)]
pub async fn search_all_transcripts(
    State(state): State<AppState>,
    Query(query): Query<SearchTranscriptsQuery>,
) -> Result<Json<SearchTranscriptsResponse>, ApiError> {
    let q = query.q.trim();
    if q.is_empty() {
        return Err(ApiError::BadRequest(
            "Query parameter 'q' must not be empty".to_string(),
        ));
    }
    if q.len() > 500 {
        return Err(ApiError::BadRequest(
            "Query parameter 'q' must be at most 500 characters".to_string(),
        ));
    }

    let limit = query.limit.clamp(1, 500);
    let (meetings, total_meetings) = state
        .storage
        .search_all_transcripts(q, limit, query.offset)
        .await?;

    Ok(Json(SearchTranscriptsResponse {
        query: q.to_string(),
        total_meetings,
        limit,
        offset: query.offset,
        meetings,
    }))
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
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<TranscriptResponse>, ApiError> {
    validation::validate_uuid(&id)?;

    // Check if meeting exists first
    let meeting = state.storage.get_meeting(&id).await?;

    // Parse optional version parameter
    let version = params
        .get("version")
        .and_then(|v| v.parse::<u32>().ok());

    // Try to get transcript
    let transcript = state.storage.get_transcript(&id, version).await.ok();

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
    validation::validate_update_request(
        &req.title,
        &req.date,
        &req.participants,
        &req.location,
        &req.organizer,
    )?;

    // Load existing meeting
    let mut meeting = state.storage.get_meeting(&id).await?;

    // Apply updates
    if let Some(title) = req.title {
        meeting.title = title;
    }
    if let Some(date) = req.date {
        meeting.date = date;
    }
    if let Some(participants) = req.participants {
        let cleaned: Vec<String> = participants
            .into_iter()
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect();
        meeting.participants = Some(cleaned);
    }
    if let Some(location) = req.location {
        let trimmed = location.trim().to_string();
        meeting.location = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        };
    }
    if let Some(organizer) = req.organizer {
        let trimmed = organizer.trim().to_string();
        meeting.organizer = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        };
    }

    // Update timestamp
    meeting.updated_at = chrono::Utc::now();

    // Save changes
    state.storage.update_meeting(&meeting).await?;

    Ok(Json(MeetingResponse { meeting }))
}

/// Bulk-rename diarization speaker labels on the latest transcript version.
#[utoipa::path(
    post,
    path = "/meetings/{id}/speakers/rename",
    tag = "transcripts",
    params(
        ("id" = String, Path, description = "Meeting ID or prefix")
    ),
    request_body = RenameSpeakersRequest,
    responses(
        (status = 200, description = "Speakers renamed", body = RenameSpeakersResponse),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 404, description = "Meeting or transcript not found", body = ErrorResponse)
    )
)]
pub async fn rename_speakers(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<RenameSpeakersRequest>,
) -> Result<Json<RenameSpeakersResponse>, ApiError> {
    validation::validate_uuid(&id)?;
    validation::validate_speaker_mapping(&req.mapping)?;

    // Ensure meeting exists (404 if missing)
    let _meeting = state.storage.get_meeting(&id).await?;

    let cleaned: std::collections::HashMap<String, String> = req
        .mapping
        .into_iter()
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, v)| !k.is_empty() && !v.is_empty())
        .collect();

    let updated = state
        .storage
        .rename_speakers(&id, &cleaned)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("not found") || msg.contains("Transcript not found") {
                ApiError::NotFound(msg)
            } else {
                ApiError::InternalServerError(msg)
            }
        })?;

    Ok(Json(RenameSpeakersResponse {
        updated_segments: updated,
        mapping: cleaned,
    }))
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

/// Retranscribe a meeting
#[utoipa::path(
    post,
    path = "/meetings/{id}/retranscribe",
    tag = "meetings",
    params(
        ("id" = String, Path, description = "Meeting ID or prefix")
    ),
    request_body = RetranscribeRequest,
    responses(
        (status = 202, description = "Retranscription job started", body = ImportResponse),
        (status = 404, description = "Meeting not found", body = ErrorResponse),
        (status = 409, description = "No audio file available", body = ErrorResponse)
    )
)]
pub async fn retranscribe_meeting(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<axum::response::Response, ApiError> {
    // Validate UUID
    validation::validate_uuid(&id)?;

    // Get meeting
    let meeting = state.storage.get_meeting(&id).await?;

    // Check if audio file exists
    let audio_path = state
        .storage
        .get_recording_path(&meeting.id)
        .await
        .map_err(|_| ApiError::Conflict("No audio file available for retranscription".to_string()))?;

    if !audio_path.exists() {
        return Err(ApiError::Conflict(
            "Audio file not found on disk".to_string(),
        ));
    }

    // Create retranscribe job
    let job_id = state
        .jobs
        .create_job(meeting_agent_core::jobs::JobType::Retranscribe);
    let cancel_token = state
        .jobs
        .cancel_token(&job_id)
        .ok_or_else(|| ApiError::InternalServerError("Failed to get cancel token".to_string()))?;

    // Set meeting_id on job
    state.jobs.set_meeting_id(&job_id, meeting.id.clone());

    // Spawn background retranscription task
    let job_id_clone = job_id.clone();
    let meeting_id = meeting.id.clone();
    let config = state.config.read().await.clone();
    let storage = state.storage.clone();
    let registry = state.jobs.clone();
    let cancel_token_clone = cancel_token.clone();

    tokio::spawn(async move {
        meeting_agent_core::runners::run_retranscribe(
            meeting_agent_core::runners::RetranscribeConfig {
                job_id: job_id_clone,
                meeting_id,
                audio_path,
                config,
                storage,
                registry,
                cancel_token: cancel_token_clone,
            },
        )
        .await;
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(crate::types::ImportResponse {
            job_id,
            status: meeting_agent_core::jobs::JobState::Pending,
        }),
    )
        .into_response())
}

/// List transcript versions for a meeting
#[utoipa::path(
    get,
    path = "/meetings/{id}/transcript/versions",
    tag = "transcripts",
    params(
        ("id" = String, Path, description = "Meeting ID or prefix")
    ),
    responses(
        (status = 200, description = "Transcript versions", body = TranscriptVersionsResponse),
        (status = 404, description = "Meeting not found", body = ErrorResponse)
    )
)]
pub async fn list_transcript_versions(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<crate::types::TranscriptVersionsResponse>, ApiError> {
    // Validate UUID
    validation::validate_uuid(&id)?;

    // Get versions
    let versions = state.storage.list_transcript_versions(&id).await?;

    Ok(Json(crate::types::TranscriptVersionsResponse {
        meeting_id: id,
        versions,
    }))
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
