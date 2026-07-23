//! Orchestrator configuration (env-overridable).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorConfig {
    /// Master switch. When false, HTTP handlers return 503.
    #[serde(default)]
    pub enabled: bool,
    /// Vexa gateway base, e.g. `http://127.0.0.1:18056`.
    #[serde(default)]
    pub vexa_api_base: Option<String>,
    /// Vexa API key (`X-API-Key`).
    #[serde(default)]
    pub vexa_api_key: Option<String>,
    /// Optional shared secret for `POST /webhooks/vexa` (`X-Webhook-Secret`).
    #[serde(default)]
    pub webhook_secret: Option<String>,
    /// Optional MinIO/S3 endpoint if recordings are fetched by object key.
    #[serde(default)]
    pub minio_endpoint: Option<String>,
    #[serde(default)]
    pub minio_access_key: Option<String>,
    #[serde(default)]
    pub minio_secret_key: Option<String>,
    #[serde(default)]
    pub minio_bucket: Option<String>,
    #[serde(default)]
    pub minio_secure: bool,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            vexa_api_base: None,
            vexa_api_key: None,
            webhook_secret: None,
            minio_endpoint: None,
            minio_access_key: None,
            minio_secret_key: None,
            minio_bucket: None,
            minio_secure: false,
        }
    }
}

impl OrchestratorConfig {
    pub fn apply_env(&mut self) {
        if let Ok(v) = std::env::var("ORCHESTRATOR_ENABLED") {
            self.enabled = matches!(v.to_lowercase().as_str(), "1" | "true" | "yes");
        }
        if let Ok(v) = std::env::var("VEXA_API_BASE") {
            let v = v.trim();
            if !v.is_empty() {
                self.vexa_api_base = Some(v.trim_end_matches('/').to_string());
            }
        }
        if let Ok(v) = std::env::var("VEXA_API_KEY") {
            if !v.trim().is_empty() {
                self.vexa_api_key = Some(v);
            }
        }
        if let Ok(v) = std::env::var("VEXA_WEBHOOK_SECRET") {
            if !v.trim().is_empty() {
                self.webhook_secret = Some(v);
            }
        }
        if let Ok(v) = std::env::var("MINIO_ENDPOINT") {
            if !v.trim().is_empty() {
                self.minio_endpoint = Some(v.trim_end_matches('/').to_string());
            }
        }
        if let Ok(v) = std::env::var("MINIO_ACCESS_KEY") {
            if !v.trim().is_empty() {
                self.minio_access_key = Some(v);
            }
        }
        if let Ok(v) = std::env::var("MINIO_SECRET_KEY") {
            if !v.trim().is_empty() {
                self.minio_secret_key = Some(v);
            }
        }
        if let Ok(v) = std::env::var("MINIO_BUCKET") {
            if !v.trim().is_empty() {
                self.minio_bucket = Some(v);
            }
        }
        if let Ok(v) = std::env::var("MINIO_SECURE") {
            self.minio_secure = matches!(v.to_lowercase().as_str(), "1" | "true" | "yes");
        }
    }

    pub fn from_env() -> Self {
        let mut c = Self::default();
        c.apply_env();
        c
    }
}
