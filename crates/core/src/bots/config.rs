//! Config for proxying to services/meeting-bot.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MeetingBotConfig {
    /// When false, public `/bots` routes return 503.
    #[serde(default)]
    pub enabled: bool,
    /// Base URL of the internal meeting-bot service (no trailing slash).
    #[serde(default)]
    pub url: Option<String>,
    /// Optional X-API-Key sent to meeting-bot.
    #[serde(default)]
    pub api_key: Option<String>,
}

impl MeetingBotConfig {
    pub fn apply_env(&mut self) {
        if let Ok(v) = std::env::var("MEETING_BOT_ENABLED") {
            self.enabled = matches!(v.to_lowercase().as_str(), "1" | "true" | "yes");
        }
        if let Ok(v) = std::env::var("MEETING_BOT_URL") {
            let v = v.trim().trim_end_matches('/');
            if !v.is_empty() {
                self.url = Some(v.to_string());
            }
        }
        if let Ok(v) = std::env::var("MEETING_BOT_INTERNAL_KEY") {
            if !v.trim().is_empty() {
                self.api_key = Some(v);
            }
        }
        // Convenience: if URL set and enabled not explicitly false, leave enabled as set by env only.
        if self.url.is_some() && std::env::var("MEETING_BOT_ENABLED").is_err() {
            // Do not auto-enable; require MEETING_BOT_ENABLED=true
        }
    }

    pub fn base_url(&self) -> Option<&str> {
        self.url.as_deref()
    }
}
