//! Configuration management

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub transcription: TranscriptionConfig,
    pub summary: SummaryConfig,
    pub server: ServerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionConfig {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryConfig {
    pub provider: String,
    pub api_key: Option<String>,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
    pub api_key: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            transcription: TranscriptionConfig {
                provider: "openai".to_string(),
                api_key: None,
                base_url: "https://api.openai.com/v1".to_string(),
                model: "whisper-1".to_string(),
            },
            summary: SummaryConfig {
                provider: "openai".to_string(),
                api_key: None,
                model: "gpt-4o-mini".to_string(),
            },
            server: ServerConfig {
                port: 8080,
                host: "127.0.0.1".to_string(),
                api_key: None,
            },
        }
    }
}

impl Config {
    /// Load config from file, or create default if not exists
    /// Environment variables override file values:
    /// - TRANSCRIPTION_PROVIDER
    /// - TRANSCRIPTION_API_KEY
    /// - TRANSCRIPTION_BASE_URL
    /// - TRANSCRIPTION_MODEL
    pub fn load(path: &PathBuf) -> anyhow::Result<Self> {
        let mut config = if path.exists() {
            let content = std::fs::read_to_string(path)?;
            serde_json::from_str(&content)?
        } else {
            let config = Config::default();
            config.save(path)?;
            config
        };

        // Override with environment variables if present
        if let Ok(provider) = std::env::var("TRANSCRIPTION_PROVIDER") {
            config.transcription.provider = provider;
        }
        if let Ok(api_key) = std::env::var("TRANSCRIPTION_API_KEY") {
            config.transcription.api_key = Some(api_key);
        }
        if let Ok(base_url) = std::env::var("TRANSCRIPTION_BASE_URL") {
            config.transcription.base_url = base_url;
        }
        if let Ok(model) = std::env::var("TRANSCRIPTION_MODEL") {
            config.transcription.model = model;
        }

        Ok(config)
    }

    /// Save config to file
    pub fn save(&self, path: &PathBuf) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}
