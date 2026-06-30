use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiarizeError {
    #[error("missing required field: {0}")]
    MissingField(&'static str),

    #[error("invalid num_speakers: {0}")]
    InvalidNumSpeakers(String),

    #[error("unsupported audio format: {0} (only mp3 and wav allowed)")]
    UnsupportedAudioFormat(String),

    #[error("failed to parse transcript JSON: {0}")]
    TranscriptParseError(String),

    #[error("failed to decode audio: {0}")]
    AudioDecodeError(String),

    #[error("model load failed: {0}")]
    ModelLoadError(String),

    #[error("diarization failed: {0}")]
    DiarizationFailed(String),

    #[error("config error: {0}")]
    ConfigError(String),
}

impl From<serde_json::Error> for DiarizeError {
    fn from(e: serde_json::Error) -> Self {
        DiarizeError::TranscriptParseError(e.to_string())
    }
}

impl IntoResponse for DiarizeError {
    fn into_response(self) -> axum::response::Response {
        let (status, msg) = match &self {
            DiarizeError::MissingField(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            DiarizeError::InvalidNumSpeakers(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            DiarizeError::UnsupportedAudioFormat(_) => {
                (StatusCode::UNPROCESSABLE_ENTITY, self.to_string())
            }
            DiarizeError::TranscriptParseError(_) => {
                (StatusCode::UNPROCESSABLE_ENTITY, self.to_string())
            }
            DiarizeError::AudioDecodeError(_) => {
                (StatusCode::UNPROCESSABLE_ENTITY, self.to_string())
            }
            DiarizeError::ModelLoadError(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            DiarizeError::DiarizationFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            DiarizeError::ConfigError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}

pub type Result<T> = std::result::Result<T, DiarizeError>;
