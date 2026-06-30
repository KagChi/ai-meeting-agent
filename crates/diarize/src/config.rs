use std::path::PathBuf;

use crate::error::{DiarizeError, Result};

#[derive(Debug, Clone)]
pub struct DiarizeConfig {
    pub segmentation_model: PathBuf,
    pub embedding_model: PathBuf,
    pub num_clusters: i32,
    pub clustering_threshold: f32,
    /// Max request body size accepted by diarize-server, in bytes.
    /// Read from `DIARIZE_MAX_BODY_MB` (default 512 MB).
    pub max_body_bytes: usize,
}

impl DiarizeConfig {
    pub fn from_env() -> Result<Self> {
        let segmentation_model = std::env::var("DIARIZE_SEGMENTATION_MODEL")
            .map(PathBuf::from)
            .map_err(|_| DiarizeError::ConfigError("DIARIZE_SEGMENTATION_MODEL not set".into()))?;
        let embedding_model = std::env::var("DIARIZE_EMBEDDING_MODEL")
            .map(PathBuf::from)
            .map_err(|_| DiarizeError::ConfigError("DIARIZE_EMBEDDING_MODEL not set".into()))?;

        if !segmentation_model.exists() {
            return Err(DiarizeError::ConfigError(format!(
                "segmentation model not found: {}",
                segmentation_model.display()
            )));
        }
        if !embedding_model.exists() {
            return Err(DiarizeError::ConfigError(format!(
                "embedding model not found: {}",
                embedding_model.display()
            )));
        }

        let num_clusters = std::env::var("DIARIZE_NUM_SPEAKERS")
            .ok()
            .map(|s| s.parse::<i32>())
            .transpose()
            .map_err(|e| DiarizeError::ConfigError(format!("DIARIZE_NUM_SPEAKERS: {e}")))?
            .unwrap_or(0);

        let clustering_threshold = std::env::var("DIARIZE_CLUSTERING_THRESHOLD")
            .ok()
            .map(|s| s.parse::<f32>())
            .transpose()
            .map_err(|e| DiarizeError::ConfigError(format!("DIARIZE_CLUSTERING_THRESHOLD: {e}")))?
            .unwrap_or(0.5);

        let max_body_mb = std::env::var("DIARIZE_MAX_BODY_MB")
            .ok()
            .map(|s| s.parse::<u64>())
            .transpose()
            .map_err(|e| DiarizeError::ConfigError(format!("DIARIZE_MAX_BODY_MB: {e}")))?
            .unwrap_or(512);
        let max_body_bytes = (max_body_mb as usize)
            .saturating_mul(1024)
            .saturating_mul(1024);

        Ok(Self {
            segmentation_model,
            embedding_model,
            num_clusters,
            clustering_threshold,
            max_body_bytes,
        })
    }
}
