//! Meeting Agent CLI
//!
//! Command-line interface for the meeting agent system.

use clap::{Parser, Subcommand};
use meeting_agent_core::{
    config::Config, fs, models::Meeting, transcription::TranscriptionClient,
    transcription::TranscriptionRequest,
};

#[derive(Parser)]
#[command(name = "meeting-agent")]
#[command(about = "A standalone meeting agent API & CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the API server
    Server {
        /// Port to listen on
        #[arg(short, long, default_value = "8080")]
        port: u16,

        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },
    /// Import an audio file
    Import {
        /// Path to audio file
        file: String,

        /// Meeting title
        #[arg(short, long)]
        title: Option<String>,
    },
    /// List all meetings
    List,
    /// Show meeting details
    Show {
        /// Meeting ID
        id: String,
    },
    /// Generate summary for a meeting
    Summarize {
        /// Meeting ID
        id: String,
    },
    /// Export meeting transcript
    Export {
        /// Meeting ID
        id: String,

        /// Export format (srt, vtt, json)
        #[arg(short, long, default_value = "srt")]
        format: String,
    },
    /// Manage configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Show current configuration
    Show,
    /// Set a configuration value
    Set {
        /// Configuration key (e.g., transcription.provider)
        key: String,
        /// Configuration value
        value: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load environment variables
    dotenv::dotenv().ok();

    let cli = Cli::parse();

    match cli.command {
        Commands::Server { port, host } => {
            println!("Starting server on {}:{}", host, port);
            // TODO: Start server (will be implemented in Phase 8)
            Ok(())
        }
        Commands::Import { file, title } => {
            import_audio(file, title).await?;
            Ok(())
        }
        Commands::List => {
            println!("Listing meetings...");
            // TODO: Implement list logic
            Ok(())
        }
        Commands::Show { id } => {
            println!("Showing meeting: {}", id);
            // TODO: Implement show logic
            Ok(())
        }
        Commands::Summarize { id } => {
            println!("Generating summary for: {}", id);
            // TODO: Implement summarize logic
            Ok(())
        }
        Commands::Export { id, format } => {
            println!("Exporting meeting {} as {}", id, format);
            // TODO: Implement export logic
            Ok(())
        }
        Commands::Config { command } => {
            match command {
                ConfigCommands::Show => {
                    println!("Current configuration:");
                    // TODO: Implement config show logic
                    Ok(())
                }
                ConfigCommands::Set { key, value } => {
                    println!("Setting {} = {}", key, value);
                    // TODO: Implement config set logic
                    Ok(())
                }
            }
        }
    }
}

/// Import and transcribe an audio file
async fn import_audio(file: String, title: Option<String>) -> anyhow::Result<()> {
    use colored::Colorize;
    use indicatif::{ProgressBar, ProgressStyle};
    use std::io::{self, Write};
    use std::path::PathBuf;

    // Validate file exists
    let file_path = PathBuf::from(&file);
    if !file_path.exists() {
        anyhow::bail!("Audio file not found: {}", file);
    }

    println!("{}", "Importing audio file...".bold());
    println!("  File: {}", file);

    // Check FFmpeg availability - ask permission to download if missing
    if meeting_agent_core::audio::ensure_ffmpeg_interactive().is_err() {
        print!("FFmpeg not found. Download now? [Y/n]: ");
        io::stdout().flush()?;

        let mut response = String::new();
        io::stdin().read_line(&mut response)?;
        let response = response.trim().to_lowercase();

        if response.is_empty() || response == "y" || response == "yes" {
            println!("Downloading FFmpeg...");
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.green} {msg}")
                    .unwrap(),
            );
            pb.set_message("Downloading FFmpeg binary...");
            pb.enable_steady_tick(std::time::Duration::from_millis(100));

            meeting_agent_core::audio::download_ffmpeg()?;

            pb.finish_with_message("FFmpeg downloaded successfully".green().to_string());
        } else {
            anyhow::bail!("FFmpeg is required for audio conversion. Aborted.");
        }
    }

    // Convert to MP3 if needed (only non-Whisper-supported formats)
    let (audio_file_to_transcribe, temp_file_created) =
        if meeting_agent_core::audio::needs_conversion(&file_path) {
            println!("{}", "Converting audio to MP3...".yellow());
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.green} {msg}")
                    .unwrap(),
            );
            pb.set_message("Converting audio format...");
            pb.enable_steady_tick(std::time::Duration::from_millis(100));

            let converted = meeting_agent_core::audio::convert_to_mp3(&file_path)?;

            pb.finish_with_message("Conversion complete".green().to_string());
            (converted, true)
        } else {
            println!(
                "{}",
                "Audio format supported by Whisper API, skipping conversion".green()
            );
            (file_path.clone(), false)
        };

    // Ensure data directory exists
    fs::ensure_data_dir()?;

    // Load configuration
    let config_path = fs::config_path()?;
    let config = Config::load(&config_path)?;

    // Create transcription client
    let client = TranscriptionClient::new(config.transcription.clone())?;

    // Show progress spinner
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    pb.set_message("Uploading and transcribing audio...");
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    // Transcribe the audio file
    let request = TranscriptionRequest {
        file_path: audio_file_to_transcribe.to_string_lossy().to_string(),
        response_format: Some("verbose_json".to_string()),
        language: None,
        prompt: None,
        temperature: Some(0.0),
    };

    let response = client.transcribe(request).await?;
    pb.finish_with_message("Transcription complete!".green().to_string());

    // Cleanup temp file if created
    if temp_file_created {
        let _ = std::fs::remove_file(&audio_file_to_transcribe);
    }

    // Display results
    println!("\n{}", "Transcript:".bold().green());
    println!("{}", "─".repeat(60));
    println!("{}", response.text);
    println!("{}", "─".repeat(60));

    if let Some(duration) = response.duration {
        println!("\n{}: {:.2}s", "Duration".bold(), duration);
    }

    if let Some(language) = &response.language {
        println!("{}: {}", "Language".bold(), language);
    }

    if let Some(segments) = &response.segments {
        println!("{}: {}", "Segments".bold(), segments.len());
    }

    // Create a meeting and save the transcript
    let meeting_title = title.unwrap_or_else(|| {
        file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled Meeting")
            .to_string()
    });

    let meeting = Meeting::new(meeting_title);

    // Create meeting directory
    let meeting_path = fs::meeting_dir(&meeting.id.to_string())?;
    std::fs::create_dir_all(&meeting_path)?;

    // Save meeting metadata
    let meeting_json = serde_json::to_string_pretty(&meeting)?;
    std::fs::write(meeting_path.join("meeting.json"), meeting_json)?;

    // Save transcript
    let transcript_json = serde_json::to_string_pretty(&response)?;
    std::fs::write(meeting_path.join("transcript.json"), transcript_json)?;

    println!(
        "\n{} Meeting saved with ID: {}",
        "✓".green().bold(),
        meeting.id.to_string().cyan()
    );
    println!(
        "  View with: {} {}",
        "meeting-agent show".yellow(),
        meeting.id
    );

    Ok(())
}
