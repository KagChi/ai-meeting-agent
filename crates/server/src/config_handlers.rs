use crate::error::ApiError;
use crate::state::AppState;
use crate::types::{
    ConfigResponse, SummaryConfigResponse, TranscriptionConfigResponse, UpdateConfigRequest,
    UpdateSummaryConfigRequest, UpdateTranscriptionConfigRequest,
};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use meeting_agent_core::config_validation::{resolve_secret, validate_config, MASK_SENTINEL};

/// GET /config - Get current configuration
#[utoipa::path(
    get,
    path = "/config",
    tag = "config",
    responses(
        (status = 200, description = "Current configuration", body = ConfigResponse)
    )
)]
pub async fn get_config(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let config = state.config.read().await;

    let response = ConfigResponse {
        transcription: TranscriptionConfigResponse {
            provider: config.transcription.provider.clone(),
            api_key: config
                .transcription
                .api_key
                .as_ref()
                .map(|_| MASK_SENTINEL.to_string()),
            base_url: config.transcription.base_url.clone(),
            model: config.transcription.model.clone(),
            chunk_seconds: config.transcription.chunk_seconds,
            chunk_concurrency: config.transcription.chunk_concurrency,
        },
        summary: SummaryConfigResponse {
            provider: config.summary.provider.clone(),
            api_key: config
                .summary
                .api_key
                .as_ref()
                .map(|_| MASK_SENTINEL.to_string()),
            base_url: config.summary.base_url.clone(),
            model: config.summary.model.clone(),
            temperature: config.summary.temperature,
            max_tokens: config.summary.max_tokens,
            language: config.summary.language.clone(),
        },
    };

    Ok((StatusCode::OK, Json(response)))
}

/// PUT /config - Update entire configuration
#[utoipa::path(
    put,
    path = "/config",
    tag = "config",
    request_body = UpdateConfigRequest,
    responses(
        (status = 200, description = "Configuration updated", body = ConfigResponse),
        (status = 400, description = "Invalid configuration", body = ErrorResponse)
    )
)]
pub async fn update_config(
    State(state): State<AppState>,
    Json(req): Json<UpdateConfigRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let mut config = state.config.write().await;

    // Resolve secrets (****  = keep existing)
    let transcription_api_key =
        resolve_secret(&req.transcription.api_key, &config.transcription.api_key);
    let summary_api_key = resolve_secret(&req.summary.api_key, &config.summary.api_key);

    // Apply updates
    config.transcription.provider = req.transcription.provider;
    config.transcription.api_key = transcription_api_key;
    config.transcription.base_url = req.transcription.base_url;
    config.transcription.model = req.transcription.model;
    config.transcription.chunk_seconds = req.transcription.chunk_seconds;
    config.transcription.chunk_concurrency = req.transcription.chunk_concurrency;

    config.summary.provider = req.summary.provider;
    config.summary.api_key = summary_api_key;
    config.summary.base_url = req.summary.base_url;
    config.summary.model = req.summary.model;
    config.summary.temperature = req.summary.temperature;
    config.summary.max_tokens = req.summary.max_tokens;
    config.summary.language = req.summary.language;

    // Validate updated config
    validate_config(&config).map_err(|errors| {
        ApiError::BadRequest(format!(
            "Configuration validation failed: {}",
            errors.join(", ")
        ))
    })?;

    // Clone config for saving (to drop write lock before I/O)
    let config_to_save = config.clone();
    let config_path = state.config_path.clone();

    // Drop write lock before I/O
    drop(config);

    // Persist to disk (config files are tiny, blocking is acceptable)
    config_to_save.save(&config_path)?;

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({"message": "Configuration updated successfully"})),
    ))
}

/// GET /config/transcription - Get transcription configuration
#[utoipa::path(
    get,
    path = "/config/transcription",
    tag = "config",
    responses(
        (status = 200, description = "Transcription configuration", body = TranscriptionConfigResponse)
    )
)]
pub async fn get_transcription_config(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let config = state.config.read().await;

    let response = TranscriptionConfigResponse {
        provider: config.transcription.provider.clone(),
        api_key: config
            .transcription
            .api_key
            .as_ref()
            .map(|_| MASK_SENTINEL.to_string()),
        base_url: config.transcription.base_url.clone(),
        model: config.transcription.model.clone(),
        chunk_seconds: config.transcription.chunk_seconds,
        chunk_concurrency: config.transcription.chunk_concurrency,
    };

    Ok((StatusCode::OK, Json(response)))
}

/// PUT /config/transcription - Update transcription configuration
#[utoipa::path(
    put,
    path = "/config/transcription",
    tag = "config",
    request_body = UpdateTranscriptionConfigRequest,
    responses(
        (status = 200, description = "Transcription configuration updated"),
        (status = 400, description = "Invalid configuration", body = ErrorResponse)
    )
)]
pub async fn update_transcription_config(
    State(state): State<AppState>,
    Json(req): Json<UpdateTranscriptionConfigRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let mut config = state.config.write().await;

    // Resolve secret
    let api_key = resolve_secret(&req.api_key, &config.transcription.api_key);

    // Apply updates
    config.transcription.provider = req.provider;
    config.transcription.api_key = api_key;
    config.transcription.base_url = req.base_url;
    config.transcription.model = req.model;
    config.transcription.chunk_seconds = req.chunk_seconds;
    config.transcription.chunk_concurrency = req.chunk_concurrency;

    // Validate updated config
    validate_config(&config).map_err(|errors| {
        ApiError::BadRequest(format!(
            "Configuration validation failed: {}",
            errors.join(", ")
        ))
    })?;

    // Clone config for saving (to drop write lock before I/O)
    let config_to_save = config.clone();
    let config_path = state.config_path.clone();

    // Drop write lock before I/O
    drop(config);

    // Persist to disk (config files are tiny, blocking is acceptable)
    config_to_save.save(&config_path)?;

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({"message": "Transcription configuration updated successfully"})),
    ))
}

/// GET /config/summary - Get summary configuration
#[utoipa::path(
    get,
    path = "/config/summary",
    tag = "config",
    responses(
        (status = 200, description = "Summary configuration", body = SummaryConfigResponse)
    )
)]
pub async fn get_summary_config(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let config = state.config.read().await;

    let response = SummaryConfigResponse {
        provider: config.summary.provider.clone(),
        api_key: config
            .summary
            .api_key
            .as_ref()
            .map(|_| MASK_SENTINEL.to_string()),
        base_url: config.summary.base_url.clone(),
        model: config.summary.model.clone(),
        temperature: config.summary.temperature,
        max_tokens: config.summary.max_tokens,
        language: config.summary.language.clone(),
    };

    Ok((StatusCode::OK, Json(response)))
}

/// PUT /config/summary - Update summary configuration
#[utoipa::path(
    put,
    path = "/config/summary",
    tag = "config",
    request_body = UpdateSummaryConfigRequest,
    responses(
        (status = 200, description = "Summary configuration updated"),
        (status = 400, description = "Invalid configuration", body = ErrorResponse)
    )
)]
pub async fn update_summary_config(
    State(state): State<AppState>,
    Json(req): Json<UpdateSummaryConfigRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let mut config = state.config.write().await;

    // Resolve secret
    let api_key = resolve_secret(&req.api_key, &config.summary.api_key);

    // Apply updates
    config.summary.provider = req.provider;
    config.summary.api_key = api_key;
    config.summary.base_url = req.base_url;
    config.summary.model = req.model;
    config.summary.temperature = req.temperature;
    config.summary.max_tokens = req.max_tokens;
    config.summary.language = req.language;

    // Validate updated config
    validate_config(&config).map_err(|errors| {
        ApiError::BadRequest(format!(
            "Configuration validation failed: {}",
            errors.join(", ")
        ))
    })?;

    // Clone config for saving (to drop write lock before I/O)
    let config_to_save = config.clone();
    let config_path = state.config_path.clone();

    // Drop write lock before I/O
    drop(config);

    // Persist to disk (config files are tiny, blocking is acceptable)
    config_to_save.save(&config_path)?;

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({"message": "Summary configuration updated successfully"})),
    ))
}
