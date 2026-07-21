//! Background job registry for import and summary processing
//!
//! In-memory store for tracking background jobs (transcription import,
//! summary generation). Thread-safe via Arc<Mutex<HashMap>>.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

/// Buffer size for progress event broadcast channels.
const EVENT_CHANNEL_CAPACITY: usize = 16;

/// Kind of background job.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum JobType {
    Import,
    Summary,
    Retranscribe,
    /// Background voice-bank speaker identification (non-blocking).
    Identify,
}

/// State of a background job.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Pending,
    Processing,
    Completed,
    Failed,
    Cancelled,
}

/// A single progress event emitted during job execution.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProgressEvent {
    pub stage: String,
    pub message: String,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
}

impl ProgressEvent {
    pub fn new(stage: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            stage: stage.into(),
            message: message.into(),
            timestamp: Utc::now(),
            percent: None,
        }
    }

    pub fn with_percent(mut self, percent: f64) -> Self {
        self.percent = Some(percent);
        self
    }
}

/// A background job (import or summary).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Job {
    pub id: String,
    pub job_type: JobType,
    pub state: JobState,
    pub progress: Vec<ProgressEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meeting_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Job {
    fn new(id: String, job_type: JobType) -> Self {
        let now = Utc::now();
        Self {
            id,
            job_type,
            state: JobState::Pending,
            progress: Vec::new(),
            meeting_id: None,
            template: None,
            error: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// True if the job has reached a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            JobState::Completed | JobState::Failed | JobState::Cancelled
        )
    }
}

/// Thread-safe registry holding all in-flight and recently completed jobs.
#[derive(Clone)]
pub struct JobRegistry {
    jobs: Arc<std::sync::Mutex<HashMap<String, Job>>>,
    cancel_tokens: Arc<std::sync::Mutex<HashMap<String, CancellationToken>>>,
    event_txs: Arc<std::sync::Mutex<HashMap<String, broadcast::Sender<ProgressEvent>>>>,
}

impl JobRegistry {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(std::sync::Mutex::new(HashMap::new())),
            cancel_tokens: Arc::new(std::sync::Mutex::new(HashMap::new())),
            event_txs: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Create a new pending job. Returns the job_id.
    pub fn create_job(&self, job_type: JobType) -> String {
        let job_id = uuid::Uuid::new_v4().to_string();
        let job = Job::new(job_id.clone(), job_type);
        let (tx, _rx) = broadcast::channel::<ProgressEvent>(EVENT_CHANNEL_CAPACITY);

        {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.insert(job_id.clone(), job);
        }
        {
            let mut tokens = self.cancel_tokens.lock().unwrap();
            tokens.insert(job_id.clone(), CancellationToken::new());
        }
        {
            let mut txs = self.event_txs.lock().unwrap();
            txs.insert(job_id.clone(), tx);
        }

        job_id
    }

    /// Get a clone of a job.
    pub fn get_job(&self, job_id: &str) -> Option<Job> {
        let jobs = self.jobs.lock().unwrap();
        jobs.get(job_id).cloned()
    }

    /// Get the current state of a job.
    pub fn get_job_state(&self, job_id: &str) -> Option<JobState> {
        let jobs = self.jobs.lock().unwrap();
        jobs.get(job_id).map(|j| j.state.clone())
    }

    /// Update job progress: append event, set state=Processing, broadcast.
    pub fn update_progress(&self, job_id: &str, event: ProgressEvent) {
        let should_broadcast = {
            let mut jobs = self.jobs.lock().unwrap();
            if let Some(job) = jobs.get_mut(job_id) {
                if job.is_terminal() {
                    return;
                }
                job.state = JobState::Processing;
                job.progress.push(event.clone());
                job.updated_at = Utc::now();
                true
            } else {
                false
            }
        };

        if should_broadcast {
            if let Some(tx) = self.event_txs.lock().unwrap().get(job_id) {
                let _ = tx.send(event);
            }
        }
    }

    /// Convenience: update progress with stage, message, and percent.
    pub fn update_progress_with_percent(
        &self,
        job_id: &str,
        stage: impl Into<String>,
        message: impl Into<String>,
        percent: f64,
    ) {
        let event = ProgressEvent::new(stage, message).with_percent(percent);
        self.update_progress(job_id, event);
    }

    /// Associate a meeting_id with a job.
    pub fn set_meeting_id(&self, job_id: &str, meeting_id: String) {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(job_id) {
            job.meeting_id = Some(meeting_id);
            job.updated_at = Utc::now();
        }
    }

    /// Associate a summary template with a job.
    pub fn set_template(&self, job_id: &str, template: String) {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(job_id) {
            job.template = Some(template);
            job.updated_at = Utc::now();
        }
    }

    /// Mark a job as completed.
    pub fn complete_job(&self, job_id: &str) {
        let event = ProgressEvent::new("completed", "Job completed successfully");
        {
            let mut jobs = self.jobs.lock().unwrap();
            if let Some(job) = jobs.get_mut(job_id) {
                job.state = JobState::Completed;
                job.progress.push(event.clone());
                job.updated_at = Utc::now();
            }
        }
        self.broadcast_and_close(job_id, event);
    }

    /// Mark a job as failed.
    pub fn fail_job(&self, job_id: &str, error: String) {
        let event = ProgressEvent::new("failed", format!("Job failed: {error}"));
        {
            let mut jobs = self.jobs.lock().unwrap();
            if let Some(job) = jobs.get_mut(job_id) {
                job.state = JobState::Failed;
                job.error = Some(error);
                job.progress.push(event.clone());
                job.updated_at = Utc::now();
            }
        }
        self.broadcast_and_close(job_id, event);
    }

    /// Cancel a job. Returns true if the job was cancellable (not terminal).
    pub fn cancel_job(&self, job_id: &str) -> bool {
        let was_cancellable = {
            let mut jobs = self.jobs.lock().unwrap();
            if let Some(job) = jobs.get_mut(job_id) {
                if job.is_terminal() {
                    return false;
                }
                job.state = JobState::Cancelled;
                job.updated_at = Utc::now();
                true
            } else {
                false
            }
        };

        if was_cancellable {
            if let Some(token) = self.cancel_tokens.lock().unwrap().get(job_id).cloned() {
                token.cancel();
            }
            let event = ProgressEvent::new("cancelled", "Job cancelled by user");
            self.broadcast_and_close(job_id, event);
        }

        was_cancellable
    }

    /// Get the cancellation token for a job (if it exists and is not terminal).
    pub fn cancel_token(&self, job_id: &str) -> Option<CancellationToken> {
        let jobs = self.jobs.lock().unwrap();
        let job = jobs.get(job_id)?;
        if job.is_terminal() {
            return None;
        }
        drop(jobs);
        self.cancel_tokens.lock().unwrap().get(job_id).cloned()
    }

    /// Subscribe to a job's progress event stream.
    pub fn subscribe(&self, job_id: &str) -> Option<broadcast::Receiver<ProgressEvent>> {
        let txs = self.event_txs.lock().unwrap();
        txs.get(job_id).map(|tx| tx.subscribe())
    }

    /// List all jobs.
    pub fn list_jobs(&self) -> Vec<Job> {
        let jobs = self.jobs.lock().unwrap();
        let mut list: Vec<Job> = jobs.values().cloned().collect();
        list.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        list
    }

    /// Broadcast a final event then close the channel (drop sender).
    fn broadcast_and_close(&self, job_id: &str, event: ProgressEvent) {
        let tx = self.event_txs.lock().unwrap().remove(job_id);
        if let Some(tx) = tx {
            let _ = tx.send(event);
            // tx dropped here → channel closes, receivers get Closed error
        }
        self.cancel_tokens.lock().unwrap().remove(job_id);
    }
}

impl Default for JobRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_job_returns_pending() {
        let registry = JobRegistry::new();
        let job_id = registry.create_job(JobType::Import);
        let job = registry.get_job(&job_id).unwrap();
        assert_eq!(job.state, JobState::Pending);
        assert_eq!(job.job_type, JobType::Import);
        assert!(job.progress.is_empty());
        assert!(job.meeting_id.is_none());
        assert!(!job.is_terminal());
    }

    #[test]
    fn test_update_progress_sets_processing() {
        let registry = JobRegistry::new();
        let job_id = registry.create_job(JobType::Import);
        registry.update_progress(&job_id, ProgressEvent::new("transcribing", "Started"));
        let job = registry.get_job(&job_id).unwrap();
        assert_eq!(job.state, JobState::Processing);
        assert_eq!(job.progress.len(), 1);
        assert_eq!(job.progress[0].stage, "transcribing");
    }

    #[test]
    fn test_update_progress_with_percent() {
        let registry = JobRegistry::new();
        let job_id = registry.create_job(JobType::Summary);
        registry.update_progress_with_percent(&job_id, "generating", "50% done", 50.0);
        let job = registry.get_job(&job_id).unwrap();
        assert_eq!(job.progress.len(), 1);
        assert_eq!(job.progress[0].percent, Some(50.0));
    }

    #[test]
    fn test_complete_job_sets_completed() {
        let registry = JobRegistry::new();
        let job_id = registry.create_job(JobType::Import);
        registry.complete_job(&job_id);
        let job = registry.get_job(&job_id).unwrap();
        assert_eq!(job.state, JobState::Completed);
        assert!(job.is_terminal());
    }

    #[test]
    fn test_fail_job_sets_failed_with_error() {
        let registry = JobRegistry::new();
        let job_id = registry.create_job(JobType::Import);
        registry.fail_job(&job_id, "network error".to_string());
        let job = registry.get_job(&job_id).unwrap();
        assert_eq!(job.state, JobState::Failed);
        assert_eq!(job.error, Some("network error".to_string()));
        assert!(job.is_terminal());
    }

    #[test]
    fn test_cancel_job_sets_cancelled() {
        let registry = JobRegistry::new();
        let job_id = registry.create_job(JobType::Import);
        assert!(registry.cancel_job(&job_id));
        let job = registry.get_job(&job_id).unwrap();
        assert_eq!(job.state, JobState::Cancelled);
        assert!(job.is_terminal());
    }

    #[test]
    fn test_cancel_terminal_job_returns_false() {
        let registry = JobRegistry::new();
        let job_id = registry.create_job(JobType::Import);
        registry.complete_job(&job_id);
        assert!(!registry.cancel_job(&job_id));
    }

    #[test]
    fn test_cancel_token_cancels() {
        let registry = JobRegistry::new();
        let job_id = registry.create_job(JobType::Import);
        let token = registry.cancel_token(&job_id).unwrap();
        registry.cancel_job(&job_id);
        assert!(token.is_cancelled());
    }

    #[test]
    fn test_cancel_token_none_after_terminal() {
        let registry = JobRegistry::new();
        let job_id = registry.create_job(JobType::Import);
        registry.fail_job(&job_id, "err".to_string());
        assert!(registry.cancel_token(&job_id).is_none());
    }

    #[test]
    fn test_set_meeting_id() {
        let registry = JobRegistry::new();
        let job_id = registry.create_job(JobType::Import);
        registry.set_meeting_id(&job_id, "meeting-123".to_string());
        let job = registry.get_job(&job_id).unwrap();
        assert_eq!(job.meeting_id, Some("meeting-123".to_string()));
    }

    #[test]
    fn test_set_template() {
        let registry = JobRegistry::new();
        let job_id = registry.create_job(JobType::Summary);
        registry.set_template(&job_id, "full".to_string());
        let job = registry.get_job(&job_id).unwrap();
        assert_eq!(job.template, Some("full".to_string()));
    }

    #[test]
    fn test_subscribe_receives_events() {
        let registry = JobRegistry::new();
        let job_id = registry.create_job(JobType::Import);
        let mut rx = registry.subscribe(&job_id).unwrap();
        registry.update_progress(&job_id, ProgressEvent::new("transcribing", "Started"));
        let event = rx.try_recv().unwrap();
        assert_eq!(event.stage, "transcribing");
    }

    #[test]
    fn test_subscribe_none_after_close() {
        let registry = JobRegistry::new();
        let job_id = registry.create_job(JobType::Import);
        let _rx = registry.subscribe(&job_id).unwrap();
        registry.complete_job(&job_id);
        // After close, sender dropped — subscribe returns None
        assert!(registry.subscribe(&job_id).is_none());
    }

    #[test]
    fn test_update_progress_ignored_after_terminal() {
        let registry = JobRegistry::new();
        let job_id = registry.create_job(JobType::Import);
        registry.complete_job(&job_id);
        registry.update_progress(&job_id, ProgressEvent::new("transcribing", "late event"));
        let job = registry.get_job(&job_id).unwrap();
        // Only the "completed" event from complete_job
        assert_eq!(job.progress.len(), 1);
    }

    #[test]
    fn test_list_jobs_sorted_by_created_desc() {
        let registry = JobRegistry::new();
        let id1 = registry.create_job(JobType::Import);
        std::thread::sleep(std::time::Duration::from_millis(2));
        let id2 = registry.create_job(JobType::Import);
        let list = registry.list_jobs();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, id2);
        assert_eq!(list[1].id, id1);
    }

    #[test]
    fn test_progress_event_with_percent() {
        let event = ProgressEvent::new("transcribing", "50% done").with_percent(50.0);
        assert_eq!(event.percent, Some(50.0));
    }

    #[test]
    fn test_get_job_nonexistent() {
        let registry = JobRegistry::new();
        assert!(registry.get_job("nonexistent").is_none());
    }

    #[test]
    fn test_registry_clone_shares_state() {
        let registry = JobRegistry::new();
        let registry2 = registry.clone();
        let job_id = registry.create_job(JobType::Import);
        assert!(registry2.get_job(&job_id).is_some());
    }
}
