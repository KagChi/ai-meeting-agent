use anyhow::Result;
use colored::Colorize;
use meeting_agent_core::{config::Config, fs};

pub async fn start(port: Option<u16>, host: Option<String>) -> Result<()> {
    env_logger::init();

    fs::ensure_data_dir()?;

    let config_path = fs::config_path()?;
    let mut config = Config::load(&config_path)?;

    if let Some(p) = port {
        config.server.port = p;
    }
    if let Some(h) = host {
        config.server.host = h;
    }

    println!(
        "{} Starting server on http://{}:{}",
        "▸".cyan(),
        config.server.host,
        config.server.port
    );

    meeting_agent_server::run(config).await
}
