use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiarizeError {
    #[error("failed to decode audio: {0}")]
    AudioDecodeError(String),

    #[error("diarization pipeline failed: {0}")]
    PipelineError(String),

    #[error("config error: {0}")]
    ConfigError(String),
}

impl From<speakrs::PipelineError> for DiarizeError {
    fn from(e: speakrs::PipelineError) -> Self {
        DiarizeError::PipelineError(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, DiarizeError>;
