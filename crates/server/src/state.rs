//! Application state

use meeting_agent_core::{Config, JobRegistry, MeetingStorage};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub storage: Arc<MeetingStorage>,
    pub jobs: Arc<JobRegistry>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        Self {
            storage: Arc::new(MeetingStorage),
            jobs: Arc::new(JobRegistry::new()),
            config,
        }
    }
}
