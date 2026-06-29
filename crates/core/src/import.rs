//! Background import processing
//!
//! Runs transcription pipeline as a background tokio task.
//! Reports progress via JobRegistry, supports cancellation via CancellationToken.

use crate::audio;
use crate::jobs::{JobRegistry, ProgressEvent};
use crate::models::Meeting;
use crate::storage::MeetingStorage;
use crate::transcription::{TranscriptionClient, TranscriptionRequest};
use crate::Config;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Run the full import pipeline as a background task.
///
/// Steps:
/// 1. Convert audio to mp3 if needed (ffmpeg)
/// 2. Create Meeting record (status=Importing)
/// 3. Transcribe via TranscriptionClient
/// 4. Save audio + transcript
/// 5. Mark meeting as Ready
///
/// On error: fail_job + mark_transcription_failed (if meeting created).
/// On cancel: job already marked Cancelled by registry; mark_transcription_failed if meeting created.
pub async fn run_import(
    job_id: String,
    audio_path: PathBuf,
    title: Option<String>,
    config: Config,
    storage: Arc<MeetingStorage>,
    registry: Arc<JobRegistry>,
    cancel_token: CancellationToken,
) {
    log::info!("Starting import job {}", job_id);

    let result = run_import_inner(
        &job_id,
        &audio_path,
        title,
        &config,
        &storage,
        &registry,
        &cancel_token,
    )
    .await;

    match result {
        Ok(()) => {
            log::info!("Import job {} completed successfully", job_id);
            registry.complete_job(&job_id);
        }
        Err(e) => {
            if cancel_token.is_cancelled() {
                log::info!("Import job {} was cancelled", job_id);
                // cancel_job already called by the cancel endpoint; ensure state
                // If not already cancelled (e.g. internal cancel), set it
                if registry.get_job_state(&job_id) != Some(crate::jobs::JobState::Cancelled) {
                    registry.cancel_job(&job_id);
                }
            } else {
                log::error!("Import job {} failed: {}", job_id, e);
                registry.fail_job(&job_id, e.to_string());
            }
        }
    }
}

async fn run_import_inner(
    job_id: &str,
    audio_path: &PathBuf,
    title: Option<String>,
    config: &Config,
    storage: &Arc<MeetingStorage>,
    registry: &Arc<JobRegistry>,
    cancel_token: &CancellationToken,
) -> Result<()> {
    // Step 1: Convert audio if needed
    registry.update_progress(
        job_id,
        ProgressEvent::new("converting", "Preparing audio file"),
    );

    let working_audio = if audio::needs_conversion(audio_path) {
        check_cancelled(cancel_token)?;
        let converted = tokio::task::spawn_blocking({
            let path = audio_path.clone();
            move || audio::convert_to_mp3(&path)
        })
        .await??;
        Some(converted)
    } else {
        None
    };

    let final_audio = working_audio.as_ref().unwrap_or(audio_path);

    check_cancelled(cancel_token)?;

    // Step 2: Create meeting
    registry.update_progress(
        job_id,
        ProgressEvent::new("processing", "Creating meeting record"),
    );

    let meeting_title = title.unwrap_or_else(|| {
        final_audio
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled Meeting")
            .to_string()
    });

    let meeting = Meeting::new(meeting_title);
    storage.create_meeting(&meeting)?;
    registry.set_meeting_id(job_id, meeting.id.clone());

    check_cancelled(cancel_token)?;

    // Step 3: Transcribe
    registry.update_progress(
        job_id,
        ProgressEvent::new("transcribing", "Sending audio to transcription API").with_percent(10.0),
    );

    let transcription_client = TranscriptionClient::new(config.transcription.clone())?;
    let transcription_request = TranscriptionRequest {
        file_path: final_audio.to_string_lossy().to_string(),
        response_format: Some("verbose_json".to_string()),
        language: None,
        prompt: None,
        temperature: None,
    };

    let transcription = transcription_client
        .transcribe(transcription_request)
        .await?;

    check_cancelled(cancel_token)?;

    // Step 4: Save audio + transcript
    registry.update_progress(
        job_id,
        ProgressEvent::new("saving", "Saving transcript and audio").with_percent(90.0),
    );

    storage.save_audio(&meeting.id, &final_audio.to_path_buf())?;
    storage.save_transcript(&meeting.id, &transcription)?;

    // Step 5: Mark complete
    let duration_seconds = transcription.duration.map(|d| d as u64);
    storage.mark_transcription_complete(
        &meeting.id,
        &config.transcription.provider,
        &config.transcription.model,
        duration_seconds,
    )?;

    Ok(())
}

/// Check if the job has been cancelled. Returns error if cancelled.
fn check_cancelled(cancel_token: &CancellationToken) -> Result<()> {
    if cancel_token.is_cancelled() {
        anyhow::bail!("Job cancelled");
    }
    Ok(())
}
