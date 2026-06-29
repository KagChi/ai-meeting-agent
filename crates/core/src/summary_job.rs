//! Summary job runner — background task that generates a meeting summary.
//!
//! Mirrors the import.rs pattern: `run_summary` spawns the pipeline,
//! `run_summary_inner` does the actual work with progress updates and
//! cancellation checks between steps.

use crate::config::Config;
use crate::jobs::JobRegistry;
use crate::jobs::ProgressEvent;
use crate::models::{SummaryStatus, SummaryTemplate};
use crate::storage::MeetingStorage;
use crate::summary::{SummarizeOptions, SummaryClient};
use anyhow::{Context, Result};
use chrono::Utc;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

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
    storage: &MeetingStorage,
    registry: &JobRegistry,
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
