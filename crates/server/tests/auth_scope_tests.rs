//! Integration tests verifying that /health and /version are publicly
//! accessible (no auth required) while protected endpoints still enforce
//! the API key middleware.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use meeting_agent_core::{Config, JobRegistry, MeetingStorage};
use meeting_agent_server::AppState;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt;

/// Build an AppState with the given API key and a temp data dir.
async fn app_state_with_key(api_key: Option<&str>) -> (AppState, tempfile::TempDir) {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = MeetingStorage::with_base(temp_dir.path().to_path_buf())
        .await
        .unwrap();
    let config_path = temp_dir.path().join("config.json");
    let mut config = Config::default();
    config.server.api_key = api_key.map(|k| k.to_string());
    config.save(&config_path).unwrap();

    let state = AppState {
        config: Arc::new(RwLock::new(config)),
        config_path,
        storage: Arc::new(storage),
        jobs: Arc::new(JobRegistry::new()),
    };

    (state, temp_dir)
}

#[tokio::test(flavor = "multi_thread")]
async fn test_health_accessible_without_auth_when_key_set() {
    let (state, _temp_dir) = app_state_with_key(Some("secret-key")).await;
    let app = meeting_agent_server::build_router(state);

    let request = Request::builder()
        .uri("/health")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /health must be reachable without auth header even when API key is configured"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_version_accessible_without_auth_when_key_set() {
    let (state, _temp_dir) = app_state_with_key(Some("secret-key")).await;
    let app = meeting_agent_server::build_router(state);

    let request = Request::builder()
        .uri("/version")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /version must be reachable without auth header even when API key is configured"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_protected_endpoint_requires_auth() {
    let (state, _temp_dir) = app_state_with_key(Some("secret-key")).await;
    let app = meeting_agent_server::build_router(state);

    // /meetings is a protected endpoint
    let request = Request::builder()
        .uri("/meetings")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "GET /meetings without auth header must return 401 when API key is configured"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_protected_endpoint_accepts_valid_key() {
    let (state, _temp_dir) = app_state_with_key(Some("secret-key")).await;
    let app = meeting_agent_server::build_router(state);

    let request = Request::builder()
        .uri("/meetings")
        .method("GET")
        .header("X-API-Key", "secret-key")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /meetings with valid key must return 200"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_health_accessible_with_auth_disabled() {
    // No API key configured — everything is open
    let (state, _temp_dir) = app_state_with_key(None).await;
    let app = meeting_agent_server::build_router(state);

    let request = Request::builder()
        .uri("/health")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /health must be 200 when no API key is configured"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_config_endpoint_still_requires_auth() {
    // Config endpoint is sensitive — must remain protected
    let (state, _temp_dir) = app_state_with_key(Some("secret-key")).await;
    let app = meeting_agent_server::build_router(state);

    let request = Request::builder()
        .uri("/config")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "GET /config without auth must return 401 — config must stay protected"
    );
}
