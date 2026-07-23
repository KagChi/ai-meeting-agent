//! Orchestrator HTTP handlers (live-bot meeting-end → import).
//!
//! - POST /webhooks/vexa — Vexa meeting-ended webhook (optional secret)
//! - POST /orchestrator/import — manual dispatch
//! - GET  /orchestrator/runs/:id — run status

use crate::error::ApiError;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use meeting_agent_core::orchestrator::{self, OrchestratorImportRequest, OrchestratorRun};

/// POST /orchestrator/import
///
/// Manually start recording download + import (same pipeline as POST /import).
#[utoipa::path(
    post,
    path = "/orchestrator/import",
    tag = "orchestrator",
    request_body = OrchestratorImportRequest,
    responses(
        (status = 202, description = "Orchestrator import started", body = OrchestratorStartResult),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 503, description = "Orchestrator disabled", body = ErrorResponse)
    )
)]
pub async fn create_orchestrator_import(
    State(state): State<AppState>,
    Json(req): Json<OrchestratorImportRequest>,
) -> Result<axum::response::Response, ApiError> {
    let config = state.config.read().await.clone();
    if !config.orchestrator.enabled {
        return Err(ApiError::ServiceUnavailable(
            "Orchestrator disabled. Set ORCHESTRATOR_ENABLED=true".to_string(),
        ));
    }

    let orch = config.orchestrator.clone();
    let result = orchestrator::start_import_from_request(
        req,
        &orch,
        config,
        state.storage.clone(),
        state.jobs.clone(),
    )
    .await
    .map_err(map_orch_err)?;

    Ok((StatusCode::ACCEPTED, Json(result)).into_response())
}

/// POST /webhooks/vexa
///
/// Accept a Vexa (or Vexa-like) meeting-ended JSON payload and start import when completed.
/// Auth: optional `X-Webhook-Secret` matching `VEXA_WEBHOOK_SECRET` when configured.
/// Also accepts `X-API-Key` via the public route without server API key when webhook secret is set.
#[utoipa::path(
    post,
    path = "/webhooks/vexa",
    tag = "orchestrator",
    request_body = serde_json::Value,
    responses(
        (status = 202, description = "Webhook accepted", body = OrchestratorStartResult),
        (status = 401, description = "Invalid webhook secret"),
        (status = 503, description = "Orchestrator disabled")
    )
)]
pub async fn vexa_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<axum::response::Response, ApiError> {
    let config = state.config.read().await.clone();
    if !config.orchestrator.enabled {
        return Err(ApiError::ServiceUnavailable(
            "Orchestrator disabled. Set ORCHESTRATOR_ENABLED=true".to_string(),
        ));
    }

    if let Some(expected) = &config.orchestrator.webhook_secret {
        if !expected.is_empty() {
            let provided = headers
                .get("X-Webhook-Secret")
                .or_else(|| headers.get("x-webhook-secret"))
                .and_then(|v| v.to_str().ok());
            match provided {
                Some(p) if p == expected => {}
                _ => return Err(ApiError::Unauthorized),
            }
        }
    }

    let event = orchestrator::parse_vexa_webhook(&body);
    let orch = config.orchestrator.clone();

    let result = orchestrator::start_import_from_event(
        event,
        false,
        &orch,
        config,
        state.storage.clone(),
        state.jobs.clone(),
    )
    .await
    .map_err(map_orch_err)?;

    Ok((StatusCode::ACCEPTED, Json(result)).into_response())
}

/// GET /orchestrator/runs/:id
#[utoipa::path(
    get,
    path = "/orchestrator/runs/{id}",
    tag = "orchestrator",
    params(("id" = String, Path, description = "Orchestrator run id")),
    responses(
        (status = 200, description = "Run status", body = OrchestratorRun),
        (status = 404, description = "Not found")
    )
)]
pub async fn get_orchestrator_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<OrchestratorRun>, ApiError> {
    let run = state
        .storage
        .get_orchestrator_run(&id)
        .await
        .map_err(|e| ApiError::InternalServerError(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound(format!("Orchestrator run not found: {id}")))?;
    Ok(Json(run))
}

fn map_orch_err(e: anyhow::Error) -> ApiError {
    let msg = e.to_string();
    if msg.contains("disabled") {
        ApiError::ServiceUnavailable(msg)
    } else if msg.contains("Provide recording") || msg.contains("Cannot resolve") {
        ApiError::BadRequest(msg)
    } else {
        ApiError::InternalServerError(msg)
    }
}
