use crate::types::ErrorResponse;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

#[derive(Debug)]
pub enum ApiError {
    NotFound(String),            // 404
    BadRequest(String),          // 400
    Unauthorized,                // 401
    InternalServerError(String), // 500
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error, details) = match self {
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, "Not Found".to_string(), Some(msg)),
            ApiError::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                "Bad Request".to_string(),
                Some(msg),
            ),
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string(), None),
            ApiError::InternalServerError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error".to_string(),
                Some(msg),
            ),
        };

        let body = Json(ErrorResponse { error, details });
        (status, body).into_response()
    }
}

// Conversion from anyhow::Error
impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        let msg = err.to_string();
        if msg.contains("not found") || msg.contains("Not found") {
            ApiError::NotFound(msg)
        } else {
            ApiError::InternalServerError(msg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_not_found_error_response() {
        let err = ApiError::NotFound("Meeting not found".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_bad_request_error_response() {
        let err = ApiError::BadRequest("Invalid title".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_unauthorized_error_response() {
        let err = ApiError::Unauthorized;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_internal_server_error_response() {
        let err = ApiError::InternalServerError("Database error".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_from_anyhow_not_found() {
        let err = anyhow::anyhow!("Meeting not found");
        let api_err = ApiError::from(err);
        assert!(matches!(api_err, ApiError::NotFound(_)));
    }

    #[test]
    fn test_from_anyhow_generic() {
        let err = anyhow::anyhow!("Some random error");
        let api_err = ApiError::from(err);
        assert!(matches!(api_err, ApiError::InternalServerError(_)));
    }
}
