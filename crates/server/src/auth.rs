use crate::error::ApiError;
use crate::state::AppState;
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

/// Authentication middleware
///
/// If API key is configured in state.config.server.api_key:
/// - Require X-API-Key header to match
/// - Return 401 if missing or mismatched
///
/// If no API key is configured:
/// - Allow all requests (open access)
/// - Log warning on first request
pub async fn auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    // Check if API key is configured
    let config = state.config.read().await;
    if let Some(expected_key) = &config.server.api_key {
        // API key is set - require authentication
        let auth_header = request
            .headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .or_else(|| {
                request
                    .headers()
                    .get("X-API-Key")
                    .and_then(|v| v.to_str().ok())
            });

        match auth_header {
            Some(key) if key == expected_key => {
                // Valid key - proceed
                Ok(next.run(request).await)
            }
            Some(_) => {
                // Invalid key
                Err(ApiError::Unauthorized)
            }
            None => {
                // Missing key
                Err(ApiError::Unauthorized)
            }
        }
    } else {
        // No API key configured - allow open access
        // Log warning (only once per server start)
        static WARNED: std::sync::Once = std::sync::Once::new();
        WARNED.call_once(|| {
            log::warn!("No API key configured - server is running with open access");
        });

        Ok(next.run(request).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        middleware,
        routing::get,
        Router,
    };
    use meeting_agent_core::{Config, MeetingStorage};
    use tempfile::TempDir;
    use tower::ServiceExt;

    async fn dummy_handler() -> &'static str {
        "ok"
    }

    async fn test_state(config: Config) -> (TempDir, AppState) {
        let dir = TempDir::new().unwrap();
        let storage = MeetingStorage::in_memory(dir.path().to_path_buf())
            .await
            .unwrap();
        let config_path = dir.path().join("config.json");
        let state = AppState::with_storage(config, storage, config_path).await;
        (dir, state)
    }

    #[tokio::test]
    async fn test_auth_with_valid_key() {
        let mut config = Config::default();
        config.server.api_key = Some("test-key".to_string());
        let (_dir, state) = test_state(config).await;

        let app = Router::new()
            .route("/test", get(dummy_handler))
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ))
            .with_state(state);

        let req = Request::builder()
            .uri("/test")
            .header("X-API-Key", "test-key")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auth_with_invalid_key() {
        let mut config = Config::default();
        config.server.api_key = Some("test-key".to_string());
        let (_dir, state) = test_state(config).await;

        let app = Router::new()
            .route("/test", get(dummy_handler))
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ))
            .with_state(state);

        let req = Request::builder()
            .uri("/test")
            .header("X-API-Key", "wrong-key")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_with_missing_key() {
        let mut config = Config::default();
        config.server.api_key = Some("test-key".to_string());
        let (_dir, state) = test_state(config).await;

        let app = Router::new()
            .route("/test", get(dummy_handler))
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ))
            .with_state(state);

        let req = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_open_access() {
        let config = Config::default(); // No API key
        let (_dir, state) = test_state(config).await;

        let app = Router::new()
            .route("/test", get(dummy_handler))
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ))
            .with_state(state);

        let req = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
