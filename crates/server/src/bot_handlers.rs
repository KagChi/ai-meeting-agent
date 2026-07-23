//! Public bot API — proxies to internal services/meeting-bot.
//!
//! Meetily and other clients call these routes on meeting-agent-server only.

use crate::error::ApiError;
use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use meeting_agent_core::bots::MeetingBotClient;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
pub struct ListBotsQuery {
    pub limit: Option<String>,
    pub status: Option<String>,
}

async fn require_client(state: &AppState) -> Result<MeetingBotClient, ApiError> {
    let cfg = state.config.read().await;
    if !cfg.meeting_bot.enabled {
        return Err(ApiError::ServiceUnavailable(
            "Meeting bot disabled. Set MEETING_BOT_ENABLED=true and MEETING_BOT_URL".into(),
        ));
    }
    if cfg.meeting_bot.url.is_none() {
        return Err(ApiError::ServiceUnavailable(
            "MEETING_BOT_URL is not set".into(),
        ));
    }
    MeetingBotClient::from_config(&cfg.meeting_bot).map_err(|e| {
        ApiError::ServiceUnavailable(format!("meeting-bot client: {e}"))
    })
}

fn map_upstream(status: u16, body: Value) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    (code, Json(body)).into_response()
}

/// GET /bots/platforms
#[utoipa::path(
    get,
    path = "/bots/platforms",
    tag = "bots",
    responses((status = 200, description = "Supported platforms"))
)]
pub async fn list_platforms(State(state): State<AppState>) -> Result<Response, ApiError> {
    let client = require_client(&state).await?;
    let v = client.platforms().await.map_err(|e| {
        ApiError::ServiceUnavailable(format!("meeting-bot unreachable: {e}"))
    })?;
    Ok((StatusCode::OK, Json(v)).into_response())
}

/// GET /bots
#[utoipa::path(
    get,
    path = "/bots",
    tag = "bots",
    responses((status = 200, description = "List bot jobs"))
)]
pub async fn list_bots(
    State(state): State<AppState>,
    Query(q): Query<ListBotsQuery>,
) -> Result<Response, ApiError> {
    let client = require_client(&state).await?;
    let mut parts = Vec::new();
    if let Some(l) = &q.limit {
        parts.push(format!("limit={l}"));
    }
    if let Some(s) = &q.status {
        parts.push(format!("status={s}"));
    }
    let query = parts.join("&");
    let v = client.list_bots(&query).await.map_err(|e| {
        ApiError::ServiceUnavailable(format!("meeting-bot unreachable: {e}"))
    })?;
    Ok((StatusCode::OK, Json(v)).into_response())
}

/// GET /bots/:id
#[utoipa::path(
    get,
    path = "/bots/{id}",
    tag = "bots",
    params(("id" = String, Path, description = "Bot job id")),
    responses((status = 200, description = "Bot job"))
)]
pub async fn get_bot(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let client = require_client(&state).await?;
    let (status, v) = client.get_bot(&id).await.map_err(|e| {
        ApiError::ServiceUnavailable(format!("meeting-bot unreachable: {e}"))
    })?;
    Ok(map_upstream(status, v))
}

/// POST /bots
#[utoipa::path(
    post,
    path = "/bots",
    tag = "bots",
    request_body = serde_json::Value,
    responses((status = 202, description = "Bot job started"))
)]
pub async fn create_bot(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let client = require_client(&state).await?;
    let (status, v) = client.create_bot(&body).await.map_err(|e| {
        ApiError::ServiceUnavailable(format!("meeting-bot unreachable: {e}"))
    })?;
    Ok(map_upstream(status, v))
}

/// DELETE /bots/:id
#[utoipa::path(
    delete,
    path = "/bots/{id}",
    tag = "bots",
    params(("id" = String, Path, description = "Bot job id")),
    responses((status = 200, description = "Bot stopped"))
)]
pub async fn delete_bot(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let client = require_client(&state).await?;
    let (status, v) = client.delete_bot(&id).await.map_err(|e| {
        ApiError::ServiceUnavailable(format!("meeting-bot unreachable: {e}"))
    })?;
    Ok(map_upstream(status, v))
}

/// GET /bots/health — internal worker health (proxied)
pub async fn bot_worker_health(State(state): State<AppState>) -> Result<Response, ApiError> {
    let client = require_client(&state).await?;
    match client.health().await {
        Ok(v) => Ok((StatusCode::OK, Json(v)).into_response()),
        Err(e) => Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "ok": false, "error": e.to_string() })),
        )
            .into_response()),
    }
}

