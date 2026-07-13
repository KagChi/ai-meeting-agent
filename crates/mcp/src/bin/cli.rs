use meeting_agent_mcp::{Config, MeetingAgentClient, MeetingAgentMcpServer};
use rmcp::ServiceExt;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "meeting_agent_mcp=warn".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    let client = MeetingAgentClient::new(
        config.meeting_agent_base_url.clone(),
        config.meeting_agent_api_key.clone(),
    );
    let server = MeetingAgentMcpServer::new(client)
        .serve(rmcp::transport::stdio())
        .await?;
    server.waiting().await?;

    Ok(())
}
