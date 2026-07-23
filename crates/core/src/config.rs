//! Configuration management

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub transcription: TranscriptionConfig,
    pub summary: SummaryConfig,
    pub server: ServerConfig,
    #[serde(default)]
    pub diarize: DiarizeConfig,
    #[serde(default)]
    pub orchestrator: crate::orchestrator::OrchestratorConfig,
    #[serde(default)]
    pub meeting_bot: crate::bots::MeetingBotConfig,
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

/// Configuration for the optional speaker-diarization step.
///
/// When `enabled` is false (the default), the import pipeline skips
/// diarization entirely and `TranscriptionResponse.segments` carry no
/// `speaker` field. When true, the pipeline runs diarization either:
/// - Via HTTP to a separate diarization service (if `service_url` is set)
/// - In-process using `speakrs` (if `service_url` is None)
///
/// The HTTP mode allows GPU-based diarization to run in a dedicated
/// container with proper CUDA/GPU support, while the main service remains
/// lightweight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizeConfig {
    /// Whether to run diarization during import. Default false.
    #[serde(default)]
    pub enabled: bool,
    /// speakrs execution backend: `auto` (default, GPU priority with CPU
    /// fallback), `cpu`, `coreml`, `coreml-fast`, `cuda`, `cuda-fast`,
    /// or `migraphx`. Only used for in-process mode.
    #[serde(default = "default_diarize_execution_mode")]
    pub execution_mode: String,
    /// Optional local model directory. `None` = download models on first
    /// use via speakrs `online` feature. Only used for in-process mode.
    #[serde(default)]
    pub model_dir: Option<PathBuf>,
    /// Optional HTTP service URL. When set, diarization is performed via
    /// HTTP POST to this endpoint instead of in-process. Example:
    /// `http://diarize-service:8001`
    #[serde(default)]
    pub service_url: Option<String>,
    /// Voiceprint embedding model id.
    ///
    /// - `wespeaker-voxceleb-CAM++_LM` (default, 512-dim, ORT + kaldi fbank)
    /// - `wespeaker-voxceleb-resnet34` (256-dim, speakrs EmbeddingModel)
    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,
    /// Expected embedding dimension for the selected model.
    #[serde(default = "default_embedding_dim")]
    pub embedding_dim: u32,
}

fn default_diarize_execution_mode() -> String {
    "auto".to_string()
}

fn default_embedding_model() -> String {
    "wespeaker-voxceleb-CAM++_LM".to_string()
}

fn default_embedding_dim() -> u32 {
    512
}

impl Default for DiarizeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            execution_mode: default_diarize_execution_mode(),
            model_dir: None,
            service_url: None,
            embedding_model: default_embedding_model(),
            embedding_dim: default_embedding_dim(),
        }
    }
}

impl DiarizeConfig {
    /// Build config from defaults + environment overrides.
    ///
    /// Env vars (same as `Config::load`):
    /// - `DIARIZE_ENABLED` (1/true/yes)
    /// - `DIARIZE_EXECUTION_MODE` (auto|cpu|coreml|coreml-fast|cuda|cuda-fast|migraphx)
    /// - `DIARIZE_MODEL_DIR` (local model path; blank = download on first use)
    /// - `DIARIZE_SERVICE_URL` (HTTP service URL; blank = in-process)
    /// - `DIARIZE_EMBEDDING_MODEL` (wespeaker-voxceleb-CAM++_LM | wespeaker-voxceleb-resnet34)
    /// - `DIARIZE_EMBEDDING_DIM` (512 for CAM++, 256 for ResNet34)
    pub fn from_env() -> Self {
        let mut config = Self::default();
        config.apply_env();
        config
    }

    /// Whether the configured embedding model is CAM++ (ORT fbank path).
    pub fn uses_campplus_embedding(&self) -> bool {
        let m = self.embedding_model.to_ascii_lowercase();
        m.contains("cam++") || m.contains("campplus")
    }

    /// Apply `DIARIZE_*` environment overrides onto this config.
    pub fn apply_env(&mut self) {
        if let Ok(enabled) = std::env::var("DIARIZE_ENABLED") {
            self.enabled = matches!(enabled.to_lowercase().as_str(), "1" | "true" | "yes");
        }
        if let Ok(mode) = std::env::var("DIARIZE_EXECUTION_MODE") {
            let mode = mode.trim();
            if !mode.is_empty() {
                self.execution_mode = mode.to_string();
            }
        }
        if let Ok(dir) = std::env::var("DIARIZE_MODEL_DIR") {
            if !dir.trim().is_empty() {
                self.model_dir = Some(PathBuf::from(dir));
            }
        }
        if let Ok(url) = std::env::var("DIARIZE_SERVICE_URL") {
            if !url.trim().is_empty() {
                self.service_url = Some(url);
            }
        }
        if let Ok(model) = std::env::var("DIARIZE_EMBEDDING_MODEL") {
            let model = model.trim();
            if !model.is_empty() {
                self.embedding_model = model.to_string();
                // Auto-fill dim when user only sets model name.
                if std::env::var("DIARIZE_EMBEDDING_DIM").is_err() {
                    self.embedding_dim = if self.uses_campplus_embedding() {
                        512
                    } else {
                        256
                    };
                }
            }
        }
        if let Ok(dim) = std::env::var("DIARIZE_EMBEDDING_DIM") {
            if let Ok(d) = dim.trim().parse::<u32>() {
                if d > 0 {
                    self.embedding_dim = d;
                }
            }
        }
    }
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
                host: "0.0.0.0".to_string(),
                api_key: None,
            },
            diarize: DiarizeConfig::default(),
            orchestrator: crate::orchestrator::OrchestratorConfig::default(),
            meeting_bot: crate::bots::MeetingBotConfig::default(),
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
    /// - DIARIZE_ENABLED (1/true/yes to enable speaker diarization)
    /// - DIARIZE_EXECUTION_MODE (auto|cpu|coreml|coreml-fast|cuda|cuda-fast|migraphx)
    /// - DIARIZE_MODEL_DIR (path to local speakrs model dir; blank = download on first use)
    /// - DIARIZE_SERVICE_URL (HTTP diarize service; blank = in-process)
    /// - MEETING_AGENT_PORT (server listen port)
    /// - MEETING_AGENT_HOST (server bind host)
    /// - MEETING_AGENT_API_KEY (server auth key; empty = open access)
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

        // Diarization overrides (shared with diarize-service via DiarizeConfig::apply_env)
        config.diarize.apply_env();

        // Live-bot orchestrator (Vexa meeting-end → import)
        config.orchestrator.apply_env();

        // Internal meeting-bot worker (Teams join + record)
        config.meeting_bot.apply_env();

        // Server overrides (MEETING_AGENT_* env vars documented in .env.example)
        if let Ok(port) = std::env::var("MEETING_AGENT_PORT") {
            if let Ok(p) = port.parse::<u16>() {
                config.server.port = p;
            }
        }
        if let Ok(host) = std::env::var("MEETING_AGENT_HOST") {
            config.server.host = host;
        }
        if let Ok(api_key) = std::env::var("MEETING_AGENT_API_KEY") {
            config.server.api_key = Some(api_key);
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
        // Restrict file permissions to owner-only on Unix (config may hold API keys).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| anyhow::anyhow!("Failed to set config file permissions: {e}"))?;
        }
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
