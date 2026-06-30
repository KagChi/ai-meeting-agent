//! Application state

use meeting_agent_core::{fs, Config, JobRegistry, MeetingStorage};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub config_path: PathBuf,
    pub storage: Arc<MeetingStorage>,
    pub jobs: Arc<JobRegistry>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        Self {
            storage: Arc::new(MeetingStorage::new()),
            jobs: Arc::new(JobRegistry::new()),
            config_path: fs::config_path().expect("Failed to determine config path"),
            config: Arc::new(RwLock::new(config)),
        }
    }
}
