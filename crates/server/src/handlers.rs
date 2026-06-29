//! HTTP handlers

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};

use crate::state::AppState;

/// Health check endpoint
pub async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok"
    }))
}

/// Version endpoint
pub async fn version() -> Json<Value> {
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "name": env!("CARGO_PKG_NAME")
    }))
}

/// List all meetings
pub async fn list_meetings(State(_state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    // TODO: Implement actual listing logic
    Ok(Json(json!({
        "meetings": []
    })))
}

/// Get a specific meeting
pub async fn get_meeting(
    State(_state): State<AppState>,
    Path(_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    // TODO: Implement actual retrieval logic
    Err(StatusCode::NOT_FOUND)
}
