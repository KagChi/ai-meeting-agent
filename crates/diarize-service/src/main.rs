use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use std::sync::Arc;
use tokio::fs;
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};

use meeting_agent_core::{
    config::DiarizeConfig, diarize::Diarizer, transcription::TranscriptionResponse,
};

#[derive(Clone)]
struct AppState {
    config: DiarizeConfig,
    temp_dir: std::path::PathBuf,
}

#[derive(Debug, Serialize)]
struct DiarizeResponse {
    transcript: TranscriptionResponse,
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: String,
    gpu_mode: String,
    version: String,
}

#[derive(Debug)]
enum AppError {
    InvalidRequest(String),
    IoError(std::io::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::InvalidRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::IoError(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("IO error: {}", err),
            ),
        };

        let body = serde_json::json!({
            "error": message,
        });

        (status, Json(body)).into_response()
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::IoError(err)
    }
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,meeting_agent_diarize_service=debug".into()),
        )
        .init();

    info!("Starting diarization service");

    // Env-only config (same DIARIZE_* vars as core). Force in-process + always-on.
    let mut config = DiarizeConfig::from_env();
    config.enabled = true;
    config.service_url = None;
    info!(
        "Diarization mode: {} model_dir={:?}",
        config.execution_mode, config.model_dir
    );

    let temp_dir = std::env::temp_dir().join("diarize-service");
    if let Err(e) = std::fs::create_dir_all(&temp_dir) {
        error!("Failed to create temp directory: {}", e);
        std::process::exit(1);
    }

    let state = AppState { config, temp_dir };

    let app = Router::new()
        .route("/v1/diarize", post(diarize_handler))
        .route("/health", get(health_handler))
        .layer(TraceLayer::new_for_http())
        .with_state(Arc::new(state));

    // Prefer DIARIZE_HOST/PORT; fall back to HOST/PORT for container defaults.
    let host = std::env::var("DIARIZE_HOST")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("DIARIZE_PORT")
        .or_else(|_| std::env::var("PORT"))
        .unwrap_or_else(|_| "8001".to_string());
    let addr = format!("{}:{}", host, port);

    info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind address");

    axum::serve(listener, app).await.expect("Server failed");
}

async fn health_handler(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        gpu_mode: state.config.execution_mode.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

async fn diarize_handler(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<DiarizeResponse>, AppError> {
    let mut audio_bytes: Option<Vec<u8>> = None;
    let mut transcript: Option<TranscriptionResponse> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::InvalidRequest(format!("Multipart error: {}", e)))?
    {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "audio" => {
                let data = field.bytes().await.map_err(|e| {
                    AppError::InvalidRequest(format!("Failed to read audio: {}", e))
                })?;
                audio_bytes = Some(data.to_vec());
            }
            "transcript" => {
                let data = field.text().await.map_err(|e| {
                    AppError::InvalidRequest(format!("Failed to read transcript: {}", e))
                })?;

                let parsed: TranscriptionResponse = serde_json::from_str(&data).map_err(|e| {
                    AppError::InvalidRequest(format!("Invalid transcript JSON: {}", e))
                })?;

                transcript = Some(parsed);
            }
            _ => {
                warn!("Unknown field in multipart form: {}", name);
            }
        }
    }

    let audio_bytes =
        audio_bytes.ok_or_else(|| AppError::InvalidRequest("Missing 'audio' field".to_string()))?;

    let transcript = transcript
        .ok_or_else(|| AppError::InvalidRequest("Missing 'transcript' field".to_string()))?;

    let temp_filename = format!("diarize-{}.audio", uuid::Uuid::new_v4());
    let temp_path = state.temp_dir.join(&temp_filename);

    fs::write(&temp_path, &audio_bytes).await?;

    let segment_count = transcript.segments.as_ref().map(|s| s.len()).unwrap_or(0);
    info!(
        "Processing diarization request: {} bytes audio, {} segments",
        audio_bytes.len(),
        segment_count
    );

    let result = match Diarizer::diarize(&temp_path, &transcript, &state.config).await {
        Ok(diarized) => {
            info!("Diarization completed successfully");
            DiarizeResponse {
                transcript: diarized,
                success: true,
                error: None,
            }
        }
        Err(e) => {
            error!("Diarization failed: {}", e);
            DiarizeResponse {
                transcript,
                success: false,
                error: Some(format!("{}", e)),
            }
        }
    };

    if let Err(e) = fs::remove_file(&temp_path).await {
        warn!("Failed to remove temp file {}: {}", temp_path.display(), e);
    }

    Ok(Json(result))
}
