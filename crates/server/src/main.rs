//! Meeting Agent Server
//!
//! HTTP API server for the meeting agent system.

use axum::{routing::get, Router};
use std::net::SocketAddr;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

mod handlers;
mod state;

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
        .route("/meetings", get(handlers::list_meetings))
        .route("/meetings/:id", get(handlers::get_meeting))
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
