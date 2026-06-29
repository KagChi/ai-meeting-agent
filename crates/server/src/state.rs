//! Application state

use meeting_agent_core::{Config, MeetingStorage};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub storage: Arc<MeetingStorage>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            storage: Arc::new(MeetingStorage),
        }
    }
}
