//! Meeting Agent CLI
//!
//! Command-line interface for the meeting agent system.

mod commands;

use clap::{Parser, Subcommand};

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
        /// Port to listen on (overrides config)
        #[arg(short, long)]
        port: Option<u16>,

        /// Host to bind to (overrides config)
        #[arg(long)]
        host: Option<String>,
    },
    /// Import an audio file and transcribe it
    ///
    /// Metadata can be provided via CLI flags or extracted from the filename.
    /// Precedence: User-provided flags > Filename parsing > Default values
    ///
    /// Supported filename patterns:
    ///   - YYYY-MM-DD_HH-MM_Topic.ext (e.g., 2026-07-09_14-30_Weekly_Standup.mp3)
    ///   - YYYY-MM-DD_Topic.ext (e.g., 2026-07-09_Project_Review.wav)
    ///   - Meeting_YYYYMMDD.ext (e.g., Meeting_20260709.mp3)
    ///   - Topic_only.ext (e.g., Weekly_Team_Sync.wav)
    ///
    /// Examples:
    ///   # Basic import (metadata from filename)
    ///   meeting-agent import 2026-07-09_14-30_Weekly_Standup.mp3
    ///
    ///   # Override title from filename
    ///   meeting-agent import meeting.mp3 --title "Q2 Planning"
    ///
    ///   # Full metadata (overrides filename)
    ///   meeting-agent import meeting.mp3 \
    ///     --title "Sprint Review" \
    ///     --participants Alice,Bob,Charlie \
    ///     --location "Conference Room A" \
    ///     --organizer Alice \
    ///     --recording-date "2026-07-09 14:30:00"
    #[command(verbatim_doc_comment)]
    Import {
        /// Path to audio file
        file: String,

        /// Meeting title (overrides filename-based title)
        #[arg(short, long, help = "Meeting title (overrides filename-based title)")]
        title: Option<String>,

        /// Meeting participants (comma-separated, e.g., Alice,Bob,Charlie)
        #[arg(
            short,
            long,
            value_delimiter = ',',
            help = "Meeting participants (comma-separated, e.g., Alice,Bob,Charlie)"
        )]
        participants: Option<Vec<String>>,

        /// Meeting location (physical or virtual)
        #[arg(short, long, help = "Meeting location (physical or virtual)")]
        location: Option<String>,

        /// Meeting organizer
        #[arg(short, long, help = "Meeting organizer")]
        organizer: Option<String>,

        /// Recording date and time (format: YYYY-MM-DD HH:MM:SS or YYYY-MM-DD, overrides filename)
        #[arg(
            short = 'd',
            long,
            help = "Recording date and time (format: YYYY-MM-DD HH:MM:SS or YYYY-MM-DD, overrides filename)"
        )]
        recording_date: Option<String>,
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

        /// Summary template (full, key-points, action-items, decisions)
        #[arg(short, long, default_value = "full")]
        template: String,

        /// Output format (markdown, raw-text)
        #[arg(short, long, default_value = "markdown")]
        format: String,

        /// Summary language (e.g., en, zh, ja)
        #[arg(short, long)]
        language: Option<String>,
    },
    /// Export meeting transcript
    Export {
        /// Meeting ID
        id: String,

        /// Export format (srt, vtt, text, json)
        #[arg(short, long, default_value = "srt")]
        format: String,

        /// Output file path (stdout if omitted)
        #[arg(short, long)]
        output: Option<String>,
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
        /// Configuration key (e.g., transcription.api_key)
        key: String,
        /// Configuration value
        value: String,
    },
    /// Edit configuration interactively
    Edit,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Server { port, host } => commands::server::start(port, host).await,
        Commands::Import {
            file,
            title,
            participants,
            location,
            organizer,
            recording_date,
        } => {
            commands::import::run(
                file,
                title,
                participants,
                location,
                organizer,
                recording_date,
            )
            .await
        }
        Commands::List => commands::list::run().await,
        Commands::Show { id } => commands::show::run(id).await,
        Commands::Summarize {
            id,
            template,
            format,
            language,
        } => commands::summarize::run(id, template, Some(format), language).await,
        Commands::Export { id, format, output } => commands::export::run(id, format, output).await,
        Commands::Config { command } => match command {
            ConfigCommands::Show => commands::config::show(),
            ConfigCommands::Set { key, value } => commands::config::set(key, value),
            ConfigCommands::Edit => commands::interactive::run(),
        },
    }
}
