//! Orchestrator service: idempotent meeting-end → import.

use super::config::OrchestratorConfig;
use super::models::{
    MeetingEndedEvent, OrchestratorImportRequest, OrchestratorRun, OrchestratorRunStatus,
    OrchestratorStartResult,
};
use super::vexa::VexaClient;
use crate::config::Config;
use crate::jobs::{JobRegistry, JobType, ProgressEvent};
use crate::runners::{self, ImportMemoryConfig};
use crate::storage::MeetingStorage;
use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

/// Start import from a normalized meeting-ended event (webhook path).
pub async fn start_import_from_event(
    event: MeetingEndedEvent,
    force: bool,
    orch_config: &OrchestratorConfig,
    app_config: Config,
    storage: Arc<MeetingStorage>,
    registry: Arc<JobRegistry>,
) -> Result<OrchestratorStartResult> {
    if !orch_config.enabled {
        bail!("Orchestrator is disabled (set ORCHESTRATOR_ENABLED=true)");
    }

    if !event.is_completed() {
        let external_key = event.external_key();
        let run = create_skipped_run(&storage, &event, &external_key, "meeting status is not completed")
            .await?;
        return Ok(OrchestratorStartResult {
            run_id: run.id,
            external_key: run.external_key,
            status: OrchestratorRunStatus::Skipped,
            job_id: None,
            meeting_id: None,
            reused: false,
        });
    }

    let external_key = event.external_key();

    if !force {
        if let Some(existing) = storage.get_orchestrator_run_by_key(&external_key).await? {
            let reusable = matches!(
                existing.status,
                OrchestratorRunStatus::Completed
                    | OrchestratorRunStatus::Importing
                    | OrchestratorRunStatus::Downloading
                    | OrchestratorRunStatus::Received
            );
            if reusable && existing.status != OrchestratorRunStatus::Failed {
                log::info!(
                    "Reusing orchestrator run {} for key {external_key} (status={:?})",
                    existing.id,
                    existing.status
                );
                return Ok(OrchestratorStartResult {
                    run_id: existing.id,
                    external_key: existing.external_key,
                    status: existing.status,
                    job_id: existing.job_id,
                    meeting_id: existing.meeting_id,
                    reused: true,
                });
            }
        }
    }

    let mut run = OrchestratorRun {
        id: Uuid::new_v4().to_string(),
        source: "vexa".to_string(),
        platform: event.platform.clone(),
        native_meeting_id: event.native_meeting_id.clone(),
        recording_key: event.recording_key.clone(),
        external_key: external_key.clone(),
        status: OrchestratorRunStatus::Received,
        job_id: None,
        meeting_id: None,
        title: event.title.clone(),
        error: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    storage.insert_orchestrator_run(&run).await?;

    let job_id = registry.create_job(JobType::Import);
    run.job_id = Some(job_id.clone());
    run.status = OrchestratorRunStatus::Downloading;
    run.updated_at = Utc::now();
    storage.update_orchestrator_run(&run).await?;

    let cancel_token = registry
        .cancel_token(&job_id)
        .context("Failed to get cancel token for orchestrator import job")?;

    registry.update_progress(
        &job_id,
        ProgressEvent::new("orchestrator", "Downloading recording from Vexa/MinIO"),
    );

    let orch_config = orch_config.clone();
    let event = event.clone();
    let storage_bg = storage.clone();
    let registry_bg = registry.clone();
    let run_id = run.id.clone();
    let job_id_bg = job_id.clone();
    let title = event.title.clone();
    let platform = event.platform.clone();

    tokio::spawn(async move {
        let outcome = async {
            let client = VexaClient::new(orch_config)?;
            let recording = client.download_recording(&event).await?;

            storage_bg
                .set_orchestrator_run_status(
                    &run_id,
                    OrchestratorRunStatus::Importing,
                    None,
                    None,
                    None,
                )
                .await?;

            registry_bg.update_progress(
                &job_id_bg,
                ProgressEvent::new("orchestrator", "Starting import pipeline"),
            );

            runners::run_import_memory(ImportMemoryConfig {
                job_id: job_id_bg.clone(),
                audio_bytes: recording.bytes,
                audio_filename: recording.filename,
                title,
                participants: None,
                location: None,
                organizer: None,
                recording_date: None,
                platform,
                config: app_config,
                storage: storage_bg.clone(),
                registry: registry_bg.clone(),
                cancel_token,
            })
            .await;

            // After import job finishes, link meeting_id from job registry
            let meeting_id = registry_bg
                .get_job(&job_id_bg)
                .and_then(|j| j.meeting_id);
            let job_state = registry_bg.get_job_state(&job_id_bg);

            match job_state {
                Some(crate::jobs::JobState::Completed) => {
                    storage_bg
                        .set_orchestrator_run_status(
                            &run_id,
                            OrchestratorRunStatus::Completed,
                            Some(&job_id_bg),
                            meeting_id.as_deref(),
                            None,
                        )
                        .await?;
                }
                Some(crate::jobs::JobState::Cancelled) => {
                    storage_bg
                        .set_orchestrator_run_status(
                            &run_id,
                            OrchestratorRunStatus::Failed,
                            Some(&job_id_bg),
                            meeting_id.as_deref(),
                            Some("import cancelled"),
                        )
                        .await?;
                }
                _ => {
                    let err = registry_bg
                        .get_job(&job_id_bg)
                        .and_then(|j| j.error)
                        .unwrap_or_else(|| "import failed".to_string());
                    storage_bg
                        .set_orchestrator_run_status(
                            &run_id,
                            OrchestratorRunStatus::Failed,
                            Some(&job_id_bg),
                            meeting_id.as_deref(),
                            Some(&err),
                        )
                        .await?;
                }
            }
            Ok::<(), anyhow::Error>(())
        }
        .await;

        if let Err(e) = outcome {
            log::error!("Orchestrator run {run_id} failed: {e:#}");
            registry_bg.fail_job(&job_id_bg, e.to_string());
            let _ = storage_bg
                .set_orchestrator_run_status(
                    &run_id,
                    OrchestratorRunStatus::Failed,
                    Some(&job_id_bg),
                    None,
                    Some(&e.to_string()),
                )
                .await;
        }
    });

    Ok(OrchestratorStartResult {
        run_id: run.id,
        external_key,
        status: OrchestratorRunStatus::Downloading,
        job_id: Some(job_id),
        meeting_id: None,
        reused: false,
    })
}

/// Manual dispatch entry point.
pub async fn start_import_from_request(
    req: OrchestratorImportRequest,
    orch_config: &OrchestratorConfig,
    app_config: Config,
    storage: Arc<MeetingStorage>,
    registry: Arc<JobRegistry>,
) -> Result<OrchestratorStartResult> {
    let force = req.force;
    let event = req.into_event();
    if event.recording_url.is_none()
        && event.recording_key.is_none()
        && (event.platform.is_none() || event.native_meeting_id.is_none())
        && orch_config.vexa_api_base.is_none()
    {
        bail!(
            "Provide recording_url, or recording_key (+ MINIO_*), or platform + native_meeting_id (+ VEXA_API_BASE)"
        );
    }
    start_import_from_event(event, force, orch_config, app_config, storage, registry).await
}

async fn create_skipped_run(
    storage: &MeetingStorage,
    event: &MeetingEndedEvent,
    external_key: &str,
    reason: &str,
) -> Result<OrchestratorRun> {
    let run = OrchestratorRun {
        id: Uuid::new_v4().to_string(),
        source: "vexa".to_string(),
        platform: event.platform.clone(),
        native_meeting_id: event.native_meeting_id.clone(),
        recording_key: event.recording_key.clone(),
        external_key: external_key.to_string(),
        status: OrchestratorRunStatus::Skipped,
        job_id: None,
        meeting_id: None,
        title: event.title.clone(),
        error: Some(reason.to_string()),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    // Best-effort insert; ignore unique conflicts by returning existing
    if let Err(e) = storage.insert_orchestrator_run(&run).await {
        if let Ok(Some(existing)) = storage.get_orchestrator_run_by_key(external_key).await {
            return Ok(existing);
        }
        return Err(e);
    }
    Ok(run)
}
