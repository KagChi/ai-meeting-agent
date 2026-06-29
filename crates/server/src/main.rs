//! Meeting Agent Server
//!
//! HTTP API server for the meeting agent system.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    dotenv::dotenv().ok();

    meeting_agent_core::fs::ensure_data_dir()?;

    let config_path = meeting_agent_core::fs::config_path()?;
    let config = meeting_agent_core::Config::load(&config_path)?;

    meeting_agent_server::run(config).await
}
