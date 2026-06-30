//! Application state

use meeting_agent_core::{Config, JobRegistry, MeetingStorage};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub storage: Arc<MeetingStorage>,
    pub jobs: Arc<JobRegistry>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        Self {
            storage: Arc::new(MeetingStorage),
            jobs: Arc::new(JobRegistry::new()),
            config: Arc::new(RwLock::new(config)),
        }
    }
}
