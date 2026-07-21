//! Meeting Agent Server
//!
//! HTTP API server library for the meeting agent system.

pub mod auth;
pub mod config_handlers;
pub mod error;
pub mod handlers;
pub mod import_handlers;
pub mod logging;
pub mod openapi;
pub mod state;
pub mod summary_handlers;
pub mod types;
pub mod validation;

pub use state::AppState;

use axum::{extract::DefaultBodyLimit, middleware, routing::get, routing::post, Router};
use std::net::SocketAddr;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

/// Build the router with all routes configured. Exposed for testing.
pub fn build_router(state: AppState) -> Router {
    // Public routes — no auth middleware.
    // Health and version must be reachable by load balancers, monitoring,
    // Docker healthchecks, and k8s liveness/readiness probes without credentials.
    let public_routes = Router::new()
        // Swagger UI - publicly accessible (no auth)
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", openapi::ApiDoc::openapi()))
        .route("/health", get(handlers::health))
        .route("/version", get(handlers::version));

    // Protected routes — auth middleware applies to all of these.
    let protected_routes = Router::new()
        .route(
            "/config",
            get(config_handlers::get_config).put(config_handlers::update_config),
        )
        .route(
            "/config/transcription",
            get(config_handlers::get_transcription_config)
                .put(config_handlers::update_transcription_config),
        )
        .route(
            "/config/summary",
            get(config_handlers::get_summary_config).put(config_handlers::update_summary_config),
        )
        .route(
            "/transcripts/search",
            get(handlers::search_all_transcripts),
        )
        .route(
            "/meetings",
            get(handlers::list_meetings).post(handlers::create_meeting),
        )
        .route(
            "/meetings/:id",
            get(handlers::get_meeting)
                .patch(handlers::update_meeting)
                .delete(handlers::delete_meeting),
        )
        .route("/meetings/:id/recording", get(handlers::get_recording))
        .route("/meetings/:id/retranscribe", post(handlers::retranscribe_meeting))
        .route("/meetings/:id/transcript", get(handlers::get_transcript))
        .route(
            "/meetings/:id/transcript/versions",
            get(handlers::list_transcript_versions),
        )
        .route(
            "/meetings/:id/summary",
            get(summary_handlers::list_summaries).post(summary_handlers::create_summary),
        )
        .route(
            "/meetings/:id/summary/:template",
            get(summary_handlers::get_summary).put(summary_handlers::update_summary),
        )
        .route("/import", post(import_handlers::create_import))
        .route("/import/validate", post(import_handlers::validate_import))
        .route(
            "/jobs/:job_id/status",
            get(import_handlers::get_import_status),
        )
        .route(
            "/jobs/:job_id/events",
            get(import_handlers::get_import_events),
        )
        .route("/jobs/:job_id/cancel", post(import_handlers::cancel_import))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ));

    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .with_state(state)
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024)) // 2GB max recording size
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn(logging::log_request))
}

/// Run the API server with the given configuration.
pub async fn run(config: meeting_agent_core::Config) -> anyhow::Result<()> {
    // Extract server config values before moving config into AppState
    let host_str = config.server.host.clone();
    let port = config.server.port;

    let state = AppState::new(config).await?;
    let app = build_router(state);

    let host: std::net::IpAddr = host_str
        .parse()
        .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)));
    let addr = SocketAddr::from((host, port));

    log::info!("Starting server on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
