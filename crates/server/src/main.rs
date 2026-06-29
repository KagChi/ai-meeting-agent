//! Meeting Agent Server
//!
//! HTTP API server for the meeting agent system.

use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

mod auth;
mod error;
mod handlers;
mod import_handlers;
mod state;
mod summary_handlers;
mod types;
mod validation;

use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    env_logger::init();

    // Load environment variables
    dotenv::dotenv().ok();

    // Ensure data directory exists
    meeting_agent_core::fs::ensure_data_dir()?;

    // Load configuration
    let config_path = meeting_agent_core::fs::config_path()?;
    let config = meeting_agent_core::Config::load(&config_path)?;

    // Create application state
    let state = AppState::new(config.clone());

    // Build router
    let app = Router::new()
        .route("/health", get(handlers::health))
        .route("/version", get(handlers::version))
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
        .route("/meetings/:id/transcript", get(handlers::get_transcript))
        // Summary endpoints
        .route(
            "/meetings/:id/summary",
            get(summary_handlers::list_summaries).post(summary_handlers::create_summary),
        )
        .route(
            "/meetings/:id/summary/:template",
            get(summary_handlers::get_summary),
        )
        // Import endpoints
        .route("/import", post(import_handlers::create_import))
        .route("/import/validate", post(import_handlers::validate_import))
        // Job endpoints (shared by import + summary)
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
        ))
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    // Parse address
    let addr = SocketAddr::from(([127, 0, 0, 1], config.server.port));

    log::info!("Starting server on http://{}", addr);

    // Start server
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
