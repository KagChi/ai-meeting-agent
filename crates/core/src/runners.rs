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

/// Configuration for in-memory import jobs.
pub struct ImportMemoryConfig {
    pub job_id: String,
    pub audio_bytes: Vec<u8>,
    pub audio_filename: String,
    pub title: Option<String>,
    pub participants: Option<Vec<String>>,
    pub location: Option<String>,
    pub organizer: Option<String>,
    pub recording_date: Option<chrono::NaiveDateTime>,
    pub config: Config,
    pub storage: Arc<MeetingStorage>,
    pub registry: Arc<JobRegistry>,
    pub cancel_token: CancellationToken,
}

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
            move || audio::convert_to_wav(&path)
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

    // Optional speaker diarization. Resilient: on failure, log and proceed
    // without speaker labels rather than failing the whole import.
    let transcription = if config.diarize.enabled {
        registry.update_progress(
            job_id,
            ProgressEvent::new("diarizing", "Speaker diarization").with_percent(70.0),
        );
        check_cancelled(cancel_token)?;
        let diarizer_cfg = crate::diarize::DiarizerConfig {
            execution_mode: crate::diarize::resolve_execution_mode(&config.diarize.execution_mode),
            model_dir: config.diarize.model_dir.clone(),
        };
        match crate::diarize::Diarizer::diarize(final_audio, &transcription, &diarizer_cfg).await {
            Ok(labeled) => labeled,
            Err(e) => {
                log::warn!("[diarize] failed, proceeding without speaker labels: {e:#}");
                transcription
            }
        }
    } else {
        transcription
    };

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

/// In-memory import: accept audio bytes instead of file path.
/// No temporary files are created during processing.
pub async fn run_import_memory(cfg: ImportMemoryConfig) {
    log::info!(
        "Starting in-memory import job {} ({} bytes)",
        cfg.job_id,
        cfg.audio_bytes.len()
    );

    let result = run_import_memory_inner(&cfg).await;

    match result {
        Ok(()) => {
            log::info!("Import job {} completed successfully", cfg.job_id);
            cfg.registry.complete_job(&cfg.job_id);
        }
        Err(e) => {
            if cfg.cancel_token.is_cancelled() {
                log::info!("Import job {} was cancelled", cfg.job_id);
                if cfg.registry.get_job_state(&cfg.job_id) != Some(crate::jobs::JobState::Cancelled)
                {
                    cfg.registry.cancel_job(&cfg.job_id);
                }
            } else {
                log::error!("Import job {} failed: {}", cfg.job_id, e);
                cfg.registry.fail_job(&cfg.job_id, e.to_string());
            }
        }
    }
}

async fn run_import_memory_inner(cfg: &ImportMemoryConfig) -> Result<()> {
    cfg.registry.update_progress(
        &cfg.job_id,
        ProgressEvent::new("converting", "Preparing audio file"),
    );

    // Check if conversion is needed based on filename
    let working_audio = if audio::needs_conversion_by_filename(&cfg.audio_filename) {
        check_cancelled(&cfg.cancel_token)?;
        log::info!(
            "[import_memory] converting {} bytes to WAV in memory",
            cfg.audio_bytes.len()
        );
        let converted = tokio::task::spawn_blocking({
            let bytes = cfg.audio_bytes.clone();
            move || audio::convert_to_wav_memory(&bytes)
        })
        .await??;
        log::info!(
            "[import_memory] conversion complete: {} bytes",
            converted.len()
        );
        converted
    } else {
        log::info!(
            "[import_memory] no conversion needed, using original {} bytes",
            cfg.audio_bytes.len()
        );
        cfg.audio_bytes.clone()
    };

    check_cancelled(&cfg.cancel_token)?;

    cfg.registry.update_progress(
        &cfg.job_id,
        ProgressEvent::new("processing", "Creating meeting record"),
    );

    // Build user metadata from config
    let user_metadata = if cfg.title.is_some()
        || cfg.participants.is_some()
        || cfg.location.is_some()
        || cfg.organizer.is_some()
        || cfg.recording_date.is_some()
    {
        Some(crate::metadata::UserMetadata {
            title: cfg.title.clone(),
            date: cfg.recording_date,
            participants: cfg.participants.clone(),
            location: cfg.location.clone(),
            organizer: cfg.organizer.clone(),
        })
    } else {
        None
    };

    // Create meeting and enrich with metadata
    let mut meeting = Meeting::new("Temporary Title".to_string());
    let filename_path = std::path::Path::new(&cfg.audio_filename);
    crate::metadata::enrich_meeting_with_metadata(&mut meeting, filename_path, user_metadata)?;

    cfg.storage.create_meeting(&meeting)?;
    cfg.registry.set_meeting_id(&cfg.job_id, meeting.id.clone());

    check_cancelled(&cfg.cancel_token)?;

    cfg.registry.update_progress(
        &cfg.job_id,
        ProgressEvent::new("transcribing", "Sending audio to transcription API").with_percent(10.0),
    );

    let transcription_client = TranscriptionClient::new(cfg.config.transcription.clone())?;

    let transcription = transcription_client
        .transcribe_chunked_memory(
            &working_audio,
            &cfg.audio_filename,
            crate::transcription::ChunkedMemoryConfig {
                response_format: Some("verbose_json".to_string()),
                language: None,
                prompt: None,
                temperature: None,
                chunk_seconds: cfg.config.transcription.chunk_seconds,
                concurrency: cfg.config.transcription.chunk_concurrency,
            },
        )
        .await?;

    check_cancelled(&cfg.cancel_token)?;

    // Optional speaker diarization. Resilient: on failure, log and proceed
    // without speaker labels rather than failing the whole import.
    let transcription = if cfg.config.diarize.enabled {
        cfg.registry.update_progress(
            &cfg.job_id,
            ProgressEvent::new("diarizing", "Speaker diarization").with_percent(70.0),
        );
        check_cancelled(&cfg.cancel_token)?;

        // For diarization, we need to write bytes to a temp file since
        // the diarizer expects a file path. This is a compromise until
        // we refactor the diarizer to accept bytes.
        log::info!("[import_memory] diarization enabled, writing temp file for diarizer");
        let temp_audio = {
            let temp_dir = std::env::temp_dir();
            let temp_path = temp_dir.join(format!(
                "meeting-agent-diarize-{}.wav",
                uuid::Uuid::new_v4()
            ));
            tokio::fs::write(&temp_path, &working_audio).await?;
            temp_path
        };

        let diarizer_cfg = crate::diarize::DiarizerConfig {
            execution_mode: crate::diarize::resolve_execution_mode(
                &cfg.config.diarize.execution_mode,
            ),
            model_dir: cfg.config.diarize.model_dir.clone(),
        };

        let result =
            match crate::diarize::Diarizer::diarize(&temp_audio, &transcription, &diarizer_cfg)
                .await
            {
                Ok(labeled) => labeled,
                Err(e) => {
                    log::warn!("[diarize] failed, proceeding without speaker labels: {e:#}");
                    transcription
                }
            };

        // Clean up temp file
        if let Err(e) = tokio::fs::remove_file(&temp_audio).await {
            log::warn!("[import_memory] failed to remove diarization temp file: {e}");
        }

        result
    } else {
        transcription
    };

    cfg.registry.update_progress(
        &cfg.job_id,
        ProgressEvent::new("saving", "Saving transcript and audio").with_percent(90.0),
    );

    cfg.storage
        .save_audio_from_bytes(&meeting.id, &working_audio, &cfg.audio_filename)?;
    cfg.storage.save_transcript(&meeting.id, &transcription)?;

    let duration_seconds = transcription.duration.map(|d| d as u64);
    cfg.storage.mark_transcription_complete(
        &meeting.id,
        &cfg.config.transcription.provider,
        &cfg.config.transcription.model,
        duration_seconds,
    )?;

    Ok(())
}
