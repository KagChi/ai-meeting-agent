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
    /// Max audio duration (seconds) per transcription request. Files longer
    /// than this are split into chunks via ffmpeg. 0 disables chunking.
    #[serde(default = "default_chunk_seconds")]
    pub chunk_seconds: f64,
    /// Max concurrent chunk transcription requests.
    #[serde(default = "default_chunk_concurrency")]
    pub chunk_concurrency: usize,
}

fn default_chunk_seconds() -> f64 {
    600.0
}

fn default_chunk_concurrency() -> usize {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryConfig {
    pub provider: String,
    pub api_key: Option<String>,
    #[serde(default = "default_summary_base_url")]
    pub base_url: String,
    pub model: String,
    #[serde(default = "default_summary_temperature")]
    pub temperature: f32,
    #[serde(default = "default_summary_max_tokens")]
    pub max_tokens: u32,
    #[serde(default)]
    pub language: Option<String>,
}

fn default_summary_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_summary_temperature() -> f32 {
    0.3
}

fn default_summary_max_tokens() -> u32 {
    1024
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
                chunk_seconds: default_chunk_seconds(),
                chunk_concurrency: default_chunk_concurrency(),
            },
            summary: SummaryConfig {
                provider: "openai".to_string(),
                api_key: None,
                base_url: default_summary_base_url(),
                model: "gpt-4o-mini".to_string(),
                temperature: default_summary_temperature(),
                max_tokens: default_summary_max_tokens(),
                language: None,
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
    /// - TRANSCRIPTION_CHUNK_SECONDS (max seconds per request; 0 disables chunking)
    /// - TRANSCRIPTION_CHUNK_CONCURRENCY (parallel chunk requests)
    /// - SUMMARY_PROVIDER
    /// - SUMMARY_API_KEY
    /// - SUMMARY_BASE_URL
    /// - SUMMARY_MODEL
    /// - SUMMARY_TEMPERATURE
    /// - SUMMARY_MAX_TOKENS
    /// - SUMMARY_LANGUAGE
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
        if let Ok(chunk_seconds) = std::env::var("TRANSCRIPTION_CHUNK_SECONDS") {
            if let Ok(c) = chunk_seconds.parse::<f64>() {
                config.transcription.chunk_seconds = c;
            }
        }
        if let Ok(chunk_concurrency) = std::env::var("TRANSCRIPTION_CHUNK_CONCURRENCY") {
            if let Ok(c) = chunk_concurrency.parse::<usize>() {
                config.transcription.chunk_concurrency = c.max(1);
            }
        }

        if let Ok(provider) = std::env::var("SUMMARY_PROVIDER") {
            config.summary.provider = provider;
        }
        if let Ok(api_key) = std::env::var("SUMMARY_API_KEY") {
            config.summary.api_key = Some(api_key);
        }
        if let Ok(base_url) = std::env::var("SUMMARY_BASE_URL") {
            config.summary.base_url = base_url;
        }
        if let Ok(model) = std::env::var("SUMMARY_MODEL") {
            config.summary.model = model;
        }
        if let Ok(temperature) = std::env::var("SUMMARY_TEMPERATURE") {
            if let Ok(t) = temperature.parse::<f32>() {
                config.summary.temperature = t;
            }
        }
        if let Ok(max_tokens) = std::env::var("SUMMARY_MAX_TOKENS") {
            if let Ok(m) = max_tokens.parse::<u32>() {
                config.summary.max_tokens = m;
            }
        }
        if let Ok(language) = std::env::var("SUMMARY_LANGUAGE") {
            config.summary.language = Some(language);
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

impl SummaryConfig {
    /// Resolve the chat completions endpoint URL for the configured provider.
    ///
    /// Known providers map to their canonical base URLs when `base_url` is
    /// empty. Otherwise the raw `base_url` is used (appending
    /// `/chat/completions` if missing a path).
    pub fn resolve_base_url(&self) -> String {
        let base = if self.base_url.is_empty() {
            match self.provider.to_lowercase().as_str() {
                "openai" => "https://api.openai.com/v1".to_string(),
                "groq" => "https://api.groq.com/openai/v1".to_string(),
                "openrouter" => "https://openrouter.ai/api/v1".to_string(),
                "ollama" => "http://localhost:11434/v1".to_string(),
                _ => "https://api.openai.com/v1".to_string(),
            }
        } else {
            self.base_url.trim_end_matches('/').to_string()
        };

        if base.ends_with("/chat/completions") {
            base
        } else {
            format!("{base}/chat/completions")
        }
    }
}
