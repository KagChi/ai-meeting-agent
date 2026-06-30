//! Background job runners.
//!
//! Two pipelines share the same pattern: an async entry point spawns
//! `run_*_inner`, which reports progress via `JobRegistry` and checks
//! cancellation between steps. On Ok → `complete_job`; on Err →
//! `cancel_job` (if cancelled) or `fail_job`.

use crate::audio;
use crate::config::Config;
use crate::jobs::{JobRegistry, ProgressEvent};
use crate::models::{Meeting, SummaryStatus, SummaryTemplate};
use crate::storage::MeetingStorage;
use crate::summary::{SummarizeOptions, SummaryClient};
use crate::transcription::{TranscriptionClient, TranscriptionRequest};
use anyhow::{Context, Result};
use chrono::Utc;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

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
        .transcribe_chunked(
            transcription_request,
            config.transcription.chunk_seconds,
            config.transcription.chunk_concurrency,
        )
        .await?;

    check_cancelled(cancel_token)?;

    registry.update_progress(
        job_id,
        ProgressEvent::new("saving", "Saving transcript and audio").with_percent(90.0),
    );

    storage.save_audio(&meeting.id, &final_audio.to_path_buf())?;
    storage.save_transcript(&meeting.id, &transcription)?;

    let duration_seconds = transcription.duration.map(|d| d as u64);
    storage.mark_transcription_complete(
        &meeting.id,
        &config.transcription.provider,
        &config.transcription.model,
        duration_seconds,
    )?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn run_summary(
    job_id: String,
    meeting_id: String,
    template: SummaryTemplate,
    language: Option<String>,
    config: Config,
    storage: Arc<MeetingStorage>,
    registry: Arc<JobRegistry>,
    cancel_token: CancellationToken,
) {
    let result = run_summary_inner(
        &job_id,
        &meeting_id,
        template,
        language,
        &config,
        &storage,
        &registry,
        &cancel_token,
    )
    .await;

    match result {
        Ok(()) => {
            registry.complete_job(&job_id);
        }
        Err(e) => {
            if cancel_token.is_cancelled() {
                registry.cancel_job(&job_id);
            } else {
                registry.fail_job(&job_id, e.to_string());
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_summary_inner(
    job_id: &str,
    meeting_id: &str,
    template: SummaryTemplate,
    language: Option<String>,
    config: &Config,
    storage: &Arc<MeetingStorage>,
    registry: &Arc<JobRegistry>,
    cancel_token: &CancellationToken,
) -> Result<()> {
    registry.update_progress(
        job_id,
        ProgressEvent::new("initializing", "Starting summary generation"),
    );

    check_cancelled(cancel_token)?;

    registry.update_progress(
        job_id,
        ProgressEvent::new("loading_transcript", "Loading meeting transcript"),
    );
    let transcript = storage
        .get_transcript(meeting_id)
        .context("Failed to load transcript")?;

    check_cancelled(cancel_token)?;

    registry.update_progress_with_percent(
        job_id,
        "generating",
        "Generating summary with LLM",
        50.0,
    );

    let client =
        SummaryClient::new(config.summary.clone()).context("Failed to create summary client")?;

    let options = SummarizeOptions {
        template: template.clone(),
        language: language.clone(),
    };

    let result = client
        .summarize(&transcript, &options)
        .await
        .context("Summary generation failed")?;

    check_cancelled(cancel_token)?;

    registry.update_progress_with_percent(job_id, "saving", "Saving summary", 90.0);

    let summary = crate::models::Summary {
        id: uuid::Uuid::new_v4().to_string(),
        meeting_id: meeting_id.to_string(),
        template,
        language,
        status: SummaryStatus::Completed,
        content: result.content,
        key_points: result.key_points,
        action_items: result.action_items,
        decisions: result.decisions,
        provider: config.summary.provider.clone(),
        model: config.summary.model.clone(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    storage
        .save_summary(meeting_id, &summary)
        .context("Failed to save summary")?;

    registry.update_progress_with_percent(job_id, "done", "Summary complete", 100.0);

    Ok(())
}

fn check_cancelled(cancel_token: &CancellationToken) -> Result<()> {
    if cancel_token.is_cancelled() {
        anyhow::bail!("Job cancelled");
    }
    Ok(())
}
