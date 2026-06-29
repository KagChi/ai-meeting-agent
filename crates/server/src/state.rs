//! Application state

use meeting_agent_core::Config;

#[derive(Clone)]
pub struct AppState {
    #[allow(dead_code)]
    pub config: Config,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}
