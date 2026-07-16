use anyhow::Result;
use colored::Colorize;
use meeting_agent_core::{config::Config, fs};

pub fn show() -> Result<()> {
    let config_path = fs::config_path()?;
    let config = Config::load(&config_path)?;

    println!("{}", "Current Configuration".bold().green());
    println!("{}", "─".repeat(50));
    println!("{} {}", "Path:".bold(), config_path.display());

    println!("\n{} Transcription", "▸".cyan());
    println!("  provider:          {}", config.transcription.provider);
    println!(
        "  api_key:           {}",
        mask_secret(config.transcription.api_key.as_deref().unwrap_or(""))
    );
    println!("  base_url:          {}", config.transcription.base_url);
    println!("  model:             {}", config.transcription.model);
    println!(
        "  chunk_seconds:     {}",
        config.transcription.chunk_seconds
    );
    println!(
        "  chunk_concurrency: {}",
        config.transcription.chunk_concurrency
    );

    println!("\n{} Summary", "▸".cyan());
    println!("  provider:    {}", config.summary.provider);
    println!(
        "  api_key:     {}",
        mask_secret(config.summary.api_key.as_deref().unwrap_or(""))
    );
    println!("  base_url:    {}", config.summary.base_url);
    println!("  model:       {}", config.summary.model);
    println!("  temperature: {}", config.summary.temperature);
    println!("  max_tokens:  {}", config.summary.max_tokens);
    if let Some(lang) = &config.summary.language {
        println!("  language:    {}", lang);
    } else {
        println!("  language:    (auto)");
    }

    println!("\n{} Server", "▸".cyan());
    println!("  port:    {}", config.server.port);
    println!("  host:    {}", config.server.host);
    println!(
        "  api_key: {}",
        mask_secret(config.server.api_key.as_deref().unwrap_or(""))
    );

    println!("\n{} Diarization", "▸".cyan());
    println!("  enabled:        {}", config.diarize.enabled);
    println!(
        "  service_url:    {}",
        config
            .diarize
            .service_url
            .as_deref()
            .unwrap_or("(in-process)")
    );
    println!("  execution_mode: {}", config.diarize.execution_mode);
    println!(
        "  model_dir:      {}",
        config
            .diarize
            .model_dir
            .as_ref()
            .map(|d| d.display().to_string())
            .unwrap_or_else(|| "(download)".to_string())
    );

    Ok(())
}

pub fn set(key: String, value: String) -> Result<()> {
    let config_path = fs::config_path()?;
    let mut config = Config::load(&config_path)?;

    match key.as_str() {
        "transcription.provider" => config.transcription.provider = value.clone(),
        "transcription.api_key" => config.transcription.api_key = Some(value.clone()),
        "transcription.base_url" => config.transcription.base_url = value.clone(),
        "transcription.model" => config.transcription.model = value.clone(),
        "transcription.chunk_seconds" => config.transcription.chunk_seconds = value.parse()?,
        "transcription.chunk_concurrency" => {
            config.transcription.chunk_concurrency = value.parse::<usize>()?.max(1)
        }
        "summary.provider" => config.summary.provider = value.clone(),
        "summary.api_key" => config.summary.api_key = Some(value.clone()),
        "summary.base_url" => config.summary.base_url = value.clone(),
        "summary.model" => config.summary.model = value.clone(),
        "summary.temperature" => config.summary.temperature = value.parse()?,
        "summary.max_tokens" => config.summary.max_tokens = value.parse()?,
        "summary.language" => config.summary.language = Some(value.clone()),
        "server.port" => config.server.port = value.parse()?,
        "server.host" => config.server.host = value.clone(),
        "server.api_key" => config.server.api_key = Some(value.clone()),
        "diarize.enabled" => {
            config.diarize.enabled = matches!(value.to_lowercase().as_str(), "1" | "true" | "yes")
        }
        "diarize.service_url" => {
            config.diarize.service_url = if value.trim().is_empty() {
                None
            } else {
                Some(value.trim().to_string())
            };
        }
        "diarize.execution_mode" => config.diarize.execution_mode = value.clone(),
        "diarize.model_dir" => {
            config.diarize.model_dir = if value.trim().is_empty() {
                None
            } else {
                Some(std::path::PathBuf::from(value.trim()))
            };
        }
        other => anyhow::bail!("Unknown config key: {}", other),
    }

    config.save(&config_path)?;
    println!("{} Set {} = {}", "✓".green().bold(), key, value);

    Ok(())
}

fn mask_secret(s: &str) -> String {
    if s.is_empty() {
        return "(not set)".to_string();
    }
    if s.len() <= 8 {
        return "****".to_string();
    }
    format!("{}****{}", &s[..4], &s[s.len() - 4..])
}
