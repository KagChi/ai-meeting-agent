use rmcp::ErrorData as McpError;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("meeting-agent API error {status}: {message}")]
    Api {
        status: reqwest::StatusCode,
        message: String,
    },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

pub type Result<T> = std::result::Result<T, ClientError>;

impl From<ClientError> for McpError {
    fn from(err: ClientError) -> Self {
        match err {
            ClientError::InvalidInput(message) => McpError::invalid_params(message, None),
            ClientError::Api { status, message } => {
                McpError::internal_error(message, Some(json!({ "http_status": status.as_u16() })))
            }
            other => McpError::internal_error(other.to_string(), None),
        }
    }
}
