//! HTTP handlers for import endpoints
//!
//! - POST /import — multipart upload, spawn background job
//! - POST /import/validate — validate audio file
//! - GET /import/:job_id/status — poll job status
//! - GET /import/:job_id/events — SSE progress stream
//! - POST /import/:job_id/cancel — cancel running job

use crate::error::ApiError;
use crate::state::AppState;
use crate::types::{
    CancelImportResponse, ImportResponse, ImportValidationResponse, JobStatusResponse,
};
use axum::{
    body::Bytes,
    extract::{Multipart, Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json,
    },
};
use futures_util::stream::Stream;
use meeting_agent_core::jobs::ProgressEvent;
use meeting_agent_core::runners::run_import;
use std::convert::Infallible;
use std::path::PathBuf;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

/// Supported audio file extensions for import.
const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "wav", "m4a", "flac", "webm", "ogg", "opus", "aac", "wma",
];

/// POST /import
///
/// Accept multipart upload with `file` (audio) and optional `title` field.
/// Spawns a background transcription job. Returns 202 with job_id.
pub async fn create_import(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<axum::response::Response, ApiError> {
    let mut audio_bytes: Option<Bytes> = None;
    let mut audio_filename: Option<String> = None;
    let mut title: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::BadRequest(format!("Failed to read multipart field: {e}")))?
    {
        let field_name = field.name().unwrap_or("").to_string();

        match field_name.as_str() {
            "file" => {
                let filename = field.file_name().unwrap_or("audio.mp3").to_string();
                validate_audio_extension(&filename)?;
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| ApiError::BadRequest(format!("Failed to read file bytes: {e}")))?;
                audio_bytes = Some(bytes);
                audio_filename = Some(filename);
            }
            "title" => {
                let text = field.text().await.map_err(|e| {
                    ApiError::BadRequest(format!("Failed to read title field: {e}"))
                })?;
                if !text.trim().is_empty() {
                    title = Some(text);
                }
            }
            _ => {
                // Ignore unknown fields
            }
        }
    }

    let audio_bytes =
        audio_bytes.ok_or_else(|| ApiError::BadRequest("Missing 'file' field".to_string()))?;
    let audio_filename = audio_filename.unwrap_or_else(|| "audio.mp3".to_string());

    // Save upload to a tempfile so the background task can read it
    let mut temp_file = tempfile::Builder::new()
        .suffix(
            &PathBuf::from(&audio_filename)
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default(),
        )
        .tempfile()
        .map_err(|e| ApiError::InternalServerError(format!("Failed to create tempfile: {e}")))?;

    use std::io::Write;
    temp_file
        .write_all(&audio_bytes)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to write upload: {e}")))?;
    temp_file
        .flush()
        .map_err(|e| ApiError::InternalServerError(format!("Failed to flush upload: {e}")))?;

    // Persist the tempfile so it survives after the handler returns
    let persisted_path = temp_file
        .into_temp_path()
        .keep()
        .map_err(|e| ApiError::InternalServerError(format!("Failed to persist upload: {e}")))?;

    // Create job
    let job_id = state
        .jobs
        .create_job(meeting_agent_core::jobs::JobType::Import);
    let cancel_token = state
        .jobs
        .cancel_token(&job_id)
        .ok_or_else(|| ApiError::InternalServerError("Failed to get cancel token".to_string()))?;

    // Spawn background task
    let job_id_clone = job_id.clone();
    let config = state.config.read().await.clone();
    let storage = state.storage.clone();
    let registry = state.jobs.clone();
    let cancel_token_clone = cancel_token.clone();

    tokio::spawn(async move {
        run_import(
            job_id_clone,
            persisted_path,
            title,
            config,
            storage,
            registry,
            cancel_token_clone,
        )
        .await;
    });

    // Return 202 Accepted
    let job = state
        .jobs
        .get_job(&job_id)
        .ok_or_else(|| ApiError::InternalServerError("Job not found after creation".to_string()))?;

    Ok((
        StatusCode::ACCEPTED,
        Json(ImportResponse {
            job_id: job.id,
            status: job.state,
        }),
    )
        .into_response())
}

/// POST /import/validate
///
/// Validate an audio file without importing it. Returns format + size.
pub async fn validate_import(
    mut multipart: Multipart,
) -> Result<Json<ImportValidationResponse>, ApiError> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::BadRequest(format!("Failed to read multipart field: {e}")))?
    {
        if field.name() == Some("file") {
            let filename = field.file_name().unwrap_or("").to_string();
            let bytes = field
                .bytes()
                .await
                .map_err(|e| ApiError::BadRequest(format!("Failed to read file bytes: {e}")))?;

            let format = PathBuf::from(&filename)
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_lowercase())
                .unwrap_or_default();

            let valid = !format.is_empty() && AUDIO_EXTENSIONS.contains(&format.as_str());

            return Ok(Json(ImportValidationResponse {
                valid,
                format,
                size: bytes.len() as u64,
            }));
        }
    }

    Err(ApiError::BadRequest("Missing 'file' field".to_string()))
}

/// GET /import/:job_id/status
///
/// Poll the current status of an import job.
pub async fn get_import_status(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<JobStatusResponse>, ApiError> {
    let job = state
        .jobs
        .get_job(&job_id)
        .ok_or_else(|| ApiError::NotFound(format!("Import job not found: {job_id}")))?;

    Ok(Json(JobStatusResponse::from(job)))
}

/// GET /import/:job_id/events
///
/// Server-Sent Events stream of progress updates for an import job.
pub async fn get_import_events(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let job = state
        .jobs
        .get_job(&job_id)
        .ok_or_else(|| ApiError::NotFound(format!("Import job not found: {job_id}")))?;

    // Get receiver before checking terminal state to avoid race
    let rx = state.jobs.subscribe(&job_id);

    let initial_events = job.progress.clone();
    let is_terminal = job.is_terminal();

    // Build stream: first replay existing progress events, then live events.
    // The broadcast channel is closed (sender dropped) when the job reaches a
    // terminal state, which ends the live stream naturally.
    let replay_stream = futures_util::stream::iter(initial_events.into_iter().map(Ok));
    let live_stream: Box<dyn Stream<Item = Result<ProgressEvent, Infallible>> + Send + Unpin> =
        if is_terminal {
            Box::new(futures_util::stream::empty())
        } else {
            match rx {
                Some(rx) => Box::new(
                    BroadcastStream::new(rx)
                        .filter_map(|res| match res {
                            Ok(event) => Some(Ok(event)),
                            Err(_) => None,
                        })
                        .map(|r| r),
                ),
                None => Box::new(futures_util::stream::empty()),
            }
        };

    let stream = replay_stream.chain(live_stream).map(|event_result| {
        let event = event_result.unwrap();
        let json = serde_json::to_string(&event).unwrap_or_default();
        Ok::<_, Infallible>(Event::default().data(json))
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// POST /import/:job_id/cancel
///
/// Cancel a running import job. Returns 409 if job already terminal.
pub async fn cancel_import(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<CancelImportResponse>, ApiError> {
    let job = state
        .jobs
        .get_job(&job_id)
        .ok_or_else(|| ApiError::NotFound(format!("Import job not found: {job_id}")))?;

    if job.is_terminal() {
        return Err(ApiError::Conflict(format!(
            "Job {job_id} is already in terminal state: {:?}",
            job.state
        )));
    }

    let cancelled = state.jobs.cancel_job(&job_id);

    Ok(Json(CancelImportResponse { job_id, cancelled }))
}

/// Validate that the filename has a supported audio extension.
fn validate_audio_extension(filename: &str) -> Result<(), ApiError> {
    let ext = PathBuf::from(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase());

    match ext {
        Some(e) if AUDIO_EXTENSIONS.contains(&e.as_str()) => Ok(()),
        Some(e) => Err(ApiError::BadRequest(format!(
            "Unsupported audio format: '{e}'. Supported: {}",
            AUDIO_EXTENSIONS.join(", ")
        ))),
        None => Err(ApiError::BadRequest(
            "File has no extension. Cannot determine audio format.".to_string(),
        )),
    }
}
