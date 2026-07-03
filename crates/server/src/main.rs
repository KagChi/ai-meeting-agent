//! Meeting Agent Server
//!
//! HTTP API server for the meeting agent system.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env first so RUST_LOG is available when env_logger initializes.
    dotenv::dotenv().ok();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    meeting_agent_core::fs::ensure_data_dir()?;

    let config_path = meeting_agent_core::fs::config_path()?;
    let config = meeting_agent_core::Config::load(&config_path)?;

    meeting_agent_server::run(config).await
}
