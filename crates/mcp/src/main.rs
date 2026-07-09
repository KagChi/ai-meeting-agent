use meeting_agent_mcp::{Config, MeetingAgentClient, MeetingAgentMcpServer};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use tokio_util::sync::CancellationToken;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "meeting_agent_mcp=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let client = MeetingAgentClient::new(
        config.meeting_agent_base_url.clone(),
        config.meeting_agent_api_key.clone(),
    );
    let cancellation = CancellationToken::new();
    let service = StreamableHttpService::new(
        move || Ok(MeetingAgentMcpServer::new(client.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default().with_cancellation_token(cancellation.child_token()),
    );

    let app = axum::Router::new()
        .route("/health", axum::routing::get(health))
        .nest_service("/mcp", service)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let bind_addr = config.bind_addr();
    tracing::info!(%bind_addr, "meeting-agent MCP server listening");
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            if let Err(err) = tokio::signal::ctrl_c().await {
                tracing::error!(%err, "failed to listen for shutdown signal");
            }
            cancellation.cancel();
        })
        .await?;

    Ok(())
}

async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "status": "ok" }))
}
