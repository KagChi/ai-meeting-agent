//! Meeting Agent Server
//!
//! HTTP API server library for the meeting agent system.

pub mod auth;
pub mod error;
pub mod handlers;
pub mod import_handlers;
pub mod state;
pub mod summary_handlers;
pub mod types;
pub mod validation;

pub use state::AppState;

use axum::{middleware, routing::get, routing::post, Router};
use std::net::SocketAddr;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

/// Run the API server with the given configuration.
pub async fn run(config: meeting_agent_core::Config) -> anyhow::Result<()> {
    let state = AppState::new(config.clone());

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
        .route(
            "/meetings/:id/summary",
            get(summary_handlers::list_summaries).post(summary_handlers::create_summary),
        )
        .route(
            "/meetings/:id/summary/:template",
            get(summary_handlers::get_summary),
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
        ))
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let host: std::net::IpAddr = config
        .server
        .host
        .parse()
        .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)));
    let addr = SocketAddr::from((host, config.server.port));

    log::info!("Starting server on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
