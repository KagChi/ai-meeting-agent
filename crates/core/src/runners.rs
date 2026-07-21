//! Background job runners.
//!
//! Two pipelines share the same pattern: an async entry point spawns
//! `run_*_inner`, which reports progress via `JobRegistry` and checks
//! cancellation between steps. On Ok → `complete_job`; on Err →
//! `cancel_job` (if cancelled) or `fail_job`.

use crate::audio;
use crate::config::Config;
use crate::jobs::{JobRegistry, ProgressEvent};
use crate::models::{Meeting, SummaryFormat, SummaryStatus, SummaryTemplate};
use crate::storage::MeetingStorage;
use crate::summary::{SummarizeOptions, SummaryClient};
use crate::transcription::{TranscriptionClient, TranscriptionRequest, TranscriptionResponse};
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
    storage.create_meeting(&meeting).await?;
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

    let transcription =
        maybe_diarize(final_audio, transcription, config, registry, job_id).await;
    check_cancelled(cancel_token)?;

    let transcription =
        maybe_identify(final_audio, transcription, storage, config, registry, job_id).await;
    check_cancelled(cancel_token)?;

    let transcription = refine_transcript(transcription, config, registry, job_id).await;

    registry.update_progress(
        job_id,
        ProgressEvent::new("saving", "Saving transcript and audio").with_percent(90.0),
    );

    storage
        .save_audio(&meeting.id, &final_audio.to_path_buf())
        .await?;
    
    let duration_seconds = transcription.duration.map(|d| d as u64).unwrap_or(0);
    storage
        .save_transcript(
            &meeting.id,
            &transcription,
            &config.transcription.provider,
            &config.transcription.model,
            duration_seconds,
        )
        .await?;

    storage
        .mark_transcription_complete(
            &meeting.id,
            &config.transcription.provider,
            &config.transcription.model,
            Some(duration_seconds),
        )
        .await?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn run_summary(
    job_id: String,
    meeting_id: String,
    template: SummaryTemplate,
    format: SummaryFormat,
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
        format,
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
                // {:#} includes the full anyhow chain (e.g. SQLite CHECK / constraint detail)
                registry.fail_job(&job_id, format!("{e:#}"));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_summary_inner(
    job_id: &str,
    meeting_id: &str,
    template: SummaryTemplate,
    format: SummaryFormat,
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
    let meeting = storage
        .get_meeting(meeting_id)
        .await
        .context("Failed to load meeting")?;
    let transcript = storage
        .get_transcript(meeting_id, None)
        .await
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
        format: format.clone(),
        language: language.clone(),
        meeting: crate::summary::MeetingContext {
            title: Some(meeting.title.clone()),
            date: Some(meeting.date.to_rfc3339()),
            participants: meeting.participants.clone(),
        },
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
        format,
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
        .await
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

/// Import from uploaded audio bytes.
/// Writes a temp file (preserving extension) and converts via path-based FFmpeg
/// so containers like m4a/mp4 demux correctly.
pub async fn run_import_memory(cfg: ImportMemoryConfig) {
    log::info!(
        "Starting import job {} ({} bytes, file={})",
        cfg.job_id,
        cfg.audio_bytes.len(),
        cfg.audio_filename
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
    if cfg.audio_bytes.is_empty() {
        anyhow::bail!("Audio data is empty");
    }

    cfg.registry.update_progress(
        &cfg.job_id,
        ProgressEvent::new("converting", "Preparing audio file"),
    );

    let needs_conversion = audio::needs_conversion_by_filename(&cfg.audio_filename);

    // Write original upload to temp with real extension (m4a/mp4 need seekable input)
    let temp_dir = std::env::temp_dir();
    let ext = std::path::Path::new(&cfg.audio_filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "bin".to_string());
    let original_temp = temp_dir.join(format!(
        "meeting_agent_import_{}.{}",
        uuid::Uuid::new_v4(),
        ext
    ));
    log::info!(
        "[import_memory] writing {} bytes to {}",
        cfg.audio_bytes.len(),
        original_temp.display()
    );
    tokio::fs::write(&original_temp, &cfg.audio_bytes)
        .await
        .context("Failed to write uploaded audio to temp file")?;

    let work_result = run_import_memory_work(cfg, needs_conversion, &original_temp).await;

    if let Err(e) = tokio::fs::remove_file(&original_temp).await {
        log::warn!(
            "[import_memory] failed to remove temp file {}: {}",
            original_temp.display(),
            e
        );
    }

    work_result
}

async fn run_import_memory_work(
    cfg: &ImportMemoryConfig,
    needs_conversion: bool,
    original_temp: &PathBuf,
) -> Result<()> {
    check_cancelled(&cfg.cancel_token)?;

    let converted_temp: Option<PathBuf>;
    let working_path = if needs_conversion {
        log::info!(
            "[import_memory] converting {} ({}) to WAV via path-based FFmpeg",
            original_temp.display(),
            cfg.audio_bytes.len()
        );
        let converted = tokio::task::spawn_blocking({
            let path = original_temp.clone();
            move || audio::convert_to_wav(&path)
        })
        .await??;
        log::info!(
            "[import_memory] conversion complete: {}",
            converted.display()
        );
        converted_temp = Some(converted.clone());
        converted
    } else {
        log::info!(
            "[import_memory] no conversion needed, using {}",
            original_temp.display()
        );
        converted_temp = None;
        original_temp.clone()
    };

    let cleanup_converted = |path: &Option<PathBuf>| {
        if let Some(p) = path {
            if let Err(e) = std::fs::remove_file(p) {
                log::warn!(
                    "[import_memory] failed to remove converted temp {}: {}",
                    p.display(),
                    e
                );
            }
        }
    };

    let result = run_import_memory_pipeline(cfg, needs_conversion, &working_path).await;
    cleanup_converted(&converted_temp);
    result
}

async fn run_import_memory_pipeline(
    cfg: &ImportMemoryConfig,
    needs_conversion: bool,
    working_path: &PathBuf,
) -> Result<()> {
    // Fail early with a clear error before creating a meeting
    let probed = tokio::task::spawn_blocking({
        let path = working_path.clone();
        move || audio::probe_duration(&path)
    })
    .await?
    .context("Failed to probe source audio duration")?;
    log::info!(
        "[import_memory] probed duration={:.2}s for {}",
        probed,
        working_path.display()
    );

    check_cancelled(&cfg.cancel_token)?;

    cfg.registry.update_progress(
        &cfg.job_id,
        ProgressEvent::new("processing", "Creating meeting record"),
    );

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

    let mut meeting = Meeting::new("Temporary Title".to_string());
    let filename_path = std::path::Path::new(&cfg.audio_filename);
    crate::metadata::enrich_meeting_with_metadata(&mut meeting, filename_path, user_metadata)?;

    cfg.storage.create_meeting(&meeting).await?;
    cfg.registry.set_meeting_id(&cfg.job_id, meeting.id.clone());

    check_cancelled(&cfg.cancel_token)?;

    cfg.registry.update_progress(
        &cfg.job_id,
        ProgressEvent::new("transcribing", "Sending audio to transcription API").with_percent(10.0),
    );

    log::info!(
        "[import_memory] starting transcription for {} (chunk_seconds={}, concurrency={})",
        working_path.display(),
        cfg.config.transcription.chunk_seconds,
        cfg.config.transcription.chunk_concurrency
    );

    let transcription_client = TranscriptionClient::new(cfg.config.transcription.clone())?;

    let transcription = transcription_client
        .transcribe_chunked(
            crate::transcription::TranscriptionRequest {
                file_path: working_path.to_string_lossy().to_string(),
                response_format: Some("verbose_json".to_string()),
                language: None,
                prompt: None,
                temperature: None,
            },
            cfg.config.transcription.chunk_seconds,
            cfg.config.transcription.chunk_concurrency,
        )
        .await?;

    log::info!(
        "[import_memory] transcription complete: {} segments, duration={:.2}s",
        transcription
            .segments
            .as_ref()
            .map(|s| s.len())
            .unwrap_or(0),
        transcription.duration.unwrap_or(0.0)
    );

    check_cancelled(&cfg.cancel_token)?;

    let transcription = maybe_diarize(
        working_path,
        transcription,
        &cfg.config,
        &cfg.registry,
        &cfg.job_id,
    )
    .await;
    check_cancelled(&cfg.cancel_token)?;

    let transcription = maybe_identify(
        working_path,
        transcription,
        &cfg.storage,
        &cfg.config,
        &cfg.registry,
        &cfg.job_id,
    )
    .await;
    check_cancelled(&cfg.cancel_token)?;

    let transcription =
        refine_transcript(transcription, &cfg.config, &cfg.registry, &cfg.job_id).await;

    cfg.registry.update_progress(
        &cfg.job_id,
        ProgressEvent::new("saving", "Saving transcript and audio").with_percent(90.0),
    );

    let saved_audio_filename = if needs_conversion {
        let stem = std::path::Path::new(&cfg.audio_filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("recording");
        format!("{stem}.wav")
    } else {
        cfg.audio_filename.clone()
    };

    let working_audio = tokio::fs::read(working_path)
        .await
        .with_context(|| format!("Failed to read working audio {}", working_path.display()))?;

    cfg.storage
        .save_audio_from_bytes(&meeting.id, &working_audio, &saved_audio_filename)
        .await?;

    let duration_seconds = transcription.duration.map(|d| d as u64).unwrap_or(0);
    cfg.storage
        .save_transcript(
            &meeting.id,
            &transcription,
            &cfg.config.transcription.provider,
            &cfg.config.transcription.model,
            duration_seconds,
        )
        .await?;

    cfg.storage
        .mark_transcription_complete(
            &meeting.id,
            &cfg.config.transcription.provider,
            &cfg.config.transcription.model,
            Some(duration_seconds),
        )
        .await?;

    Ok(())
}

/// Optional speaker diarization. Soft-fails: on error, returns unlabeled transcript.
async fn maybe_diarize(
    audio_path: &std::path::Path,
    transcription: TranscriptionResponse,
    config: &Config,
    registry: &Arc<JobRegistry>,
    job_id: &str,
) -> TranscriptionResponse {
    #[cfg(feature = "diarization")]
    {
        if !config.diarize.enabled {
            return transcription;
        }
        registry.update_progress(
            job_id,
            ProgressEvent::new("diarizing", "Speaker diarization").with_percent(70.0),
        );
        match crate::diarize::Diarizer::diarize(audio_path, &transcription, &config.diarize).await {
            Ok(labeled) => labeled,
            Err(e) => {
                log::warn!("[diarize] failed, proceeding without speaker labels: {e:#}");
                transcription
            }
        }
    }
    #[cfg(not(feature = "diarization"))]
    {
        let _ = (audio_path, registry, job_id);
        if config.diarize.enabled {
            log::warn!("[diarize] feature not enabled, skipping speaker diarization");
        }
        transcription
    }
}

/// Optional voiceprint identify after diarize. Soft-fails: keeps diar labels.
///
/// Runs when diarization is enabled and the voice bank has at least one centroid.
async fn maybe_identify(
    audio_path: &std::path::Path,
    mut transcription: TranscriptionResponse,
    storage: &Arc<MeetingStorage>,
    config: &Config,
    registry: &Arc<JobRegistry>,
    job_id: &str,
) -> TranscriptionResponse {
    #[cfg(feature = "diarization")]
    {
        if !config.diarize.enabled {
            return transcription;
        }
        let has_labels = transcription
            .segments
            .as_ref()
            .map(|s| s.iter().any(|seg| seg.speaker.is_some()))
            .unwrap_or(false);
        if !has_labels {
            return transcription;
        }
        match storage.list_voiceprints().await {
            Ok(bank) if bank.is_empty() => {
                log::info!("[identify] voice bank empty; skip");
                return transcription;
            }
            Err(e) => {
                log::warn!("[identify] list voiceprints failed: {e:#}");
                return transcription;
            }
            Ok(_) => {}
        }
        registry.update_progress(
            job_id,
            ProgressEvent::new("identifying", "Speaker identification").with_percent(78.0),
        );
        match crate::voiceprint::identify_transcript(
            audio_path,
            &mut transcription,
            storage,
            &config.diarize,
            crate::voiceprint::DEFAULT_IDENTIFY_THRESHOLD,
        )
        .await
        {
            Ok(result) => {
                log::info!(
                    "[identify] matched={} guests={} skipped={}",
                    result.matched,
                    result.guests,
                    result.skipped
                );
                transcription
            }
            Err(e) => {
                log::warn!("[identify] failed, keeping diarization labels: {e:#}");
                transcription
            }
        }
    }
    #[cfg(not(feature = "diarization"))]
    {
        let _ = (audio_path, storage, config, registry, job_id);
        transcription
    }
}

async fn refine_transcript(
    transcription: TranscriptionResponse,
    config: &Config,
    registry: &Arc<JobRegistry>,
    job_id: &str,
) -> TranscriptionResponse {
    if config.summary.base_url.trim().is_empty() {
        log::warn!("[refine] skipped: summary.base_url is empty");
        return transcription;
    }

    log::info!(
        "[refine] improving transcript with LLM at {} (model: {})",
        config.summary.resolve_base_url(),
        config.summary.model
    );
    registry.update_progress(
        job_id,
        ProgressEvent::new("refining", "Refining transcript with LLM").with_percent(85.0),
    );

    let summary_client = match SummaryClient::new(config.summary.clone()) {
        Ok(client) => client,
        Err(e) => {
            log::warn!("[refine] could not create client: {:#}", e);
            return transcription;
        }
    };

    match summary_client.refine(&transcription).await {
        Ok(refined) => {
            log::info!(
                "[refine] completed successfully (segments refined: {})",
                refined.segment_refined.len()
            );
            let mut response = transcription;
            if let Some(segments) = response.segments.as_mut() {
                for (seg, refined_line) in segments.iter_mut().zip(refined.segment_refined.into_iter()) {
                    let trimmed = refined_line.trim();
                    if !trimmed.is_empty() {
                        seg.refined_text = Some(trimmed.to_string());
                    }
                }
            }
            response.refined_text = Some(refined.refined_text);
            response
        }
        Err(e) => {
            log::warn!("[refine] failed: {:#}", e);
            transcription
        }
    }
}

/// Configuration for retranscribe jobs.
pub struct RetranscribeConfig {
    pub job_id: String,
    pub meeting_id: String,
    pub audio_path: PathBuf,
    pub config: Config,
    pub storage: Arc<MeetingStorage>,
    pub registry: Arc<JobRegistry>,
    pub cancel_token: CancellationToken,
}

pub async fn run_retranscribe(cfg: RetranscribeConfig) {
    log::info!("Starting retranscribe job {}", cfg.job_id);

    let result = run_retranscribe_inner(&cfg).await;

    match result {
        Ok(_) => {
            log::info!("Retranscribe job {} completed successfully", cfg.job_id);
            cfg.registry.complete_job(&cfg.job_id);
        }
        Err(e) if cfg.cancel_token.is_cancelled() => {
            log::warn!("Retranscribe job {} cancelled: {:#}", cfg.job_id, e);
            cfg.registry.cancel_job(&cfg.job_id);
        }
        Err(e) => {
            log::error!("Retranscribe job {} failed: {:#}", cfg.job_id, e);
            cfg.registry
                .fail_job(&cfg.job_id, format!("Retranscription failed: {:#}", e));
        }
    }
}

async fn run_retranscribe_inner(cfg: &RetranscribeConfig) -> Result<()> {
    // Check cancellation
    if cfg.cancel_token.is_cancelled() {
        anyhow::bail!("Job cancelled before starting");
    }

    cfg.registry.update_progress(
        &cfg.job_id,
        ProgressEvent::new("loading", "Loading audio file"),
    );

    // Load audio metadata to get duration
    let duration_seconds = audio::probe_duration(&cfg.audio_path)
        .context("Failed to get audio duration")? as u64;

    cfg.registry.update_progress(
        &cfg.job_id,
        ProgressEvent::new("transcribing", "Starting transcription").with_percent(10.0),
    );

    // Create transcription client
    let transcription_client = TranscriptionClient::new(cfg.config.transcription.clone())?;
    let transcription_request = TranscriptionRequest {
        file_path: cfg.audio_path.to_string_lossy().to_string(),
        response_format: Some("verbose_json".to_string()),
        language: None,
        prompt: None,
        temperature: None,
    };

    // Transcribe audio
    let transcription = transcription_client
        .transcribe_chunked(
            transcription_request,
            cfg.config.transcription.chunk_seconds,
            cfg.config.transcription.chunk_concurrency,
        )
        .await?;

    if cfg.cancel_token.is_cancelled() {
        anyhow::bail!("Job cancelled before diarization");
    }

    // Same as import: diarize (+ optional identify) before refine so Enhance keeps labels
    let transcription = maybe_diarize(
        &cfg.audio_path,
        transcription,
        &cfg.config,
        &cfg.registry,
        &cfg.job_id,
    )
    .await;

    if cfg.cancel_token.is_cancelled() {
        anyhow::bail!("Job cancelled before identification");
    }

    let transcription = maybe_identify(
        &cfg.audio_path,
        transcription,
        &cfg.storage,
        &cfg.config,
        &cfg.registry,
        &cfg.job_id,
    )
    .await;

    if cfg.cancel_token.is_cancelled() {
        anyhow::bail!("Job cancelled before refinement");
    }

    let transcription = refine_transcript(
        transcription,
        &cfg.config,
        &cfg.registry,
        &cfg.job_id,
    )
    .await;

    // Check cancellation before saving
    if cfg.cancel_token.is_cancelled() {
        anyhow::bail!("Job cancelled before saving");
    }

    cfg.registry.update_progress(
        &cfg.job_id,
        ProgressEvent::new("saving", "Saving new transcript version").with_percent(95.0),
    );

    // Save new transcript version
    cfg.storage
        .save_transcript(
            &cfg.meeting_id,
            &transcription,
            &cfg.config.transcription.provider,
            &cfg.config.transcription.model,
            duration_seconds,
        )
        .await?;

    Ok(())
}

