use anyhow::Result;
use colored::Colorize;
use dialoguer::{Confirm, Input, Password, Select};
use meeting_agent_core::{config::Config, fs};

const PROVIDERS: &[&str] = &["openai", "groq", "openrouter", "ollama", "custom"];

pub fn run() -> Result<()> {
    let config_path = fs::config_path()?;
    let mut config = Config::load(&config_path)?;

    println!(
        "{}",
        "Meeting Agent — Interactive Configuration".bold().green()
    );
    println!("{}", "─".repeat(50));

    edit_transcription(&mut config)?;
    edit_summary(&mut config)?;
    edit_server(&mut config)?;
    edit_diarize(&mut config)?;

    print_summary(&config);

    if !Confirm::new()
        .with_prompt("Save configuration?")
        .default(true)
        .interact()?
    {
        println!("{}", "Aborted — no changes written.".yellow());
        return Ok(());
    }

    config.save(&config_path)?;
    println!(
        "{} Configuration saved to {}",
        "✓".green().bold(),
        config_path.display()
    );
    Ok(())
}

fn edit_transcription(config: &mut Config) -> Result<()> {
    println!("\n{} Transcription", "▸".cyan());

    let idx = PROVIDERS
        .iter()
        .position(|p| *p == config.transcription.provider.as_str())
        .unwrap_or(0);
    let sel = Select::new()
        .with_prompt("Provider")
        .default(idx)
        .items(PROVIDERS)
        .interact()?;
    if PROVIDERS[sel] == "custom" {
        config.transcription.provider = Input::new()
            .with_prompt("Provider name")
            .with_initial_text(&config.transcription.provider)
            .interact_text()?;
    } else {
        config.transcription.provider = PROVIDERS[sel].to_string();
    }

    if Confirm::new()
        .with_prompt("Set API key?")
        .default(config.transcription.api_key.is_some())
        .interact()?
    {
        config.transcription.api_key = Some(
            Password::new()
                .with_prompt("API key")
                .allow_empty_password(true)
                .interact()?,
        );
    } else {
        config.transcription.api_key = None;
    }

    config.transcription.base_url = Input::new()
        .with_prompt("Base URL")
        .with_initial_text(&config.transcription.base_url)
        .interact_text()?;

    config.transcription.model = Input::new()
        .with_prompt("Model")
        .with_initial_text(&config.transcription.model)
        .interact_text()?;

    config.transcription.chunk_seconds = Input::new()
        .with_prompt("Chunk seconds (0 = no chunking)")
        .with_initial_text(config.transcription.chunk_seconds.to_string())
        .interact_text()?;

    config.transcription.chunk_concurrency = Input::new()
        .with_prompt("Chunk concurrency")
        .with_initial_text(config.transcription.chunk_concurrency.to_string())
        .interact_text()?;

    Ok(())
}

fn edit_summary(config: &mut Config) -> Result<()> {
    println!("\n{} Summary", "▸".cyan());

    let idx = PROVIDERS
        .iter()
        .position(|p| *p == config.summary.provider.as_str())
        .unwrap_or(0);
    let sel = Select::new()
        .with_prompt("Provider")
        .default(idx)
        .items(PROVIDERS)
        .interact()?;
    if PROVIDERS[sel] == "custom" {
        config.summary.provider = Input::new()
            .with_prompt("Provider name")
            .with_initial_text(&config.summary.provider)
            .interact_text()?;
    } else {
        config.summary.provider = PROVIDERS[sel].to_string();
    }

    if Confirm::new()
        .with_prompt("Set API key?")
        .default(config.summary.api_key.is_some())
        .interact()?
    {
        config.summary.api_key = Some(
            Password::new()
                .with_prompt("API key")
                .allow_empty_password(true)
                .interact()?,
        );
    } else {
        config.summary.api_key = None;
    }

    config.summary.base_url = Input::new()
        .with_prompt("Base URL")
        .with_initial_text(&config.summary.base_url)
        .interact_text()?;

    config.summary.model = Input::new()
        .with_prompt("Model")
        .with_initial_text(&config.summary.model)
        .interact_text()?;

    config.summary.temperature = Input::new()
        .with_prompt("Temperature (0.0–2.0)")
        .with_initial_text(config.summary.temperature.to_string())
        .interact_text()?;

    config.summary.max_tokens = Input::new()
        .with_prompt("Max tokens")
        .with_initial_text(config.summary.max_tokens.to_string())
        .interact_text()?;

    let lang: String = Input::new()
        .with_prompt("Language (blank = auto)")
        .with_initial_text(config.summary.language.clone().unwrap_or_default())
        .allow_empty(true)
        .interact_text()?;
    config.summary.language = if lang.trim().is_empty() {
        None
    } else {
        Some(lang.trim().to_string())
    };

    Ok(())
}

fn edit_server(config: &mut Config) -> Result<()> {
    println!("\n{} Server", "▸".cyan());

    config.server.port = Input::new()
        .with_prompt("Port")
        .with_initial_text(config.server.port.to_string())
        .interact_text()?;

    config.server.host = Input::new()
        .with_prompt("Host")
        .with_initial_text(&config.server.host)
        .interact_text()?;

    if Confirm::new()
        .with_prompt("Set API key (for HTTP auth)?")
        .default(config.server.api_key.is_some())
        .interact()?
    {
        config.server.api_key = Some(
            Password::new()
                .with_prompt("Server API key")
                .allow_empty_password(true)
                .interact()?,
        );
    } else {
        config.server.api_key = None;
    }

    Ok(())
}

fn edit_diarize(config: &mut Config) -> Result<()> {
    println!("\n{} Diarization", "▸".cyan());

    config.diarize.enabled = Confirm::new()
        .with_prompt("Enable speaker diarization?")
        .default(config.diarize.enabled)
        .interact()?;

    if !config.diarize.enabled {
        return Ok(());
    }

    config.diarize.base_url = Input::new()
        .with_prompt("Diarize-server base URL")
        .with_initial_text(&config.diarize.base_url)
        .interact_text()?;

    let ns: String = Input::new()
        .with_prompt("Number of speakers (blank = auto-detect)")
        .with_initial_text(
            config
                .diarize
                .num_speakers
                .map(|n| n.to_string())
                .unwrap_or_default(),
        )
        .allow_empty(true)
        .interact_text()?;
    config.diarize.num_speakers = ns.trim().parse::<i32>().ok();

    config.diarize.timeout_secs = Input::new()
        .with_prompt("Request timeout (seconds)")
        .with_initial_text(config.diarize.timeout_secs.to_string())
        .interact_text()?;

    Ok(())
}

fn print_summary(config: &Config) {
    println!("\n{}", "Configuration Summary".bold().green());
    println!("{}", "─".repeat(50));

    println!("{} Transcription", "▸".cyan());
    println!("  provider:  {}", config.transcription.provider);
    println!("  api_key:   {}", mask(&config.transcription.api_key));
    println!("  base_url:  {}", config.transcription.base_url);
    println!("  model:     {}", config.transcription.model);
    println!(
        "  chunk:     {}s @ {}x",
        config.transcription.chunk_seconds, config.transcription.chunk_concurrency
    );

    println!("{} Summary", "▸".cyan());
    println!("  provider:    {}", config.summary.provider);
    println!("  api_key:     {}", mask(&config.summary.api_key));
    println!("  base_url:    {}", config.summary.base_url);
    println!("  model:       {}", config.summary.model);
    println!("  temperature: {}", config.summary.temperature);
    println!("  max_tokens:  {}", config.summary.max_tokens);
    println!(
        "  language:    {}",
        config.summary.language.as_deref().unwrap_or("(auto)")
    );

    println!("{} Server", "▸".cyan());
    println!("  port:    {}", config.server.port);
    println!("  host:    {}", config.server.host);
    println!("  api_key: {}", mask(&config.server.api_key));

    println!("{} Diarization", "▸".cyan());
    println!("  enabled: {}", config.diarize.enabled);
    if config.diarize.enabled {
        println!("  base_url:      {}", config.diarize.base_url);
        println!(
            "  num_speakers:  {}",
            config
                .diarize
                .num_speakers
                .map(|n| n.to_string())
                .unwrap_or_else(|| "(auto)".to_string())
        );
        println!("  timeout_secs:  {}", config.diarize.timeout_secs);
    }
}

fn mask(s: &Option<String>) -> String {
    match s {
        None => "(not set)".to_string(),
        Some(_) => "****".to_string(),
    }
}
