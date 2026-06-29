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

        /// Summary template (full, key-points, action-items, decisions)
        #[arg(short, long, default_value = "full")]
        template: String,

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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let cli = Cli::parse();

    match cli.command {
        Commands::Server { port, host } => commands::server::start(port, host).await,
        Commands::Import { file, title } => commands::import::run(file, title).await,
        Commands::List => commands::list::run().await,
        Commands::Show { id } => commands::show::run(id).await,
        Commands::Summarize {
            id,
            template,
            language,
        } => commands::summarize::run(id, template, language).await,
        Commands::Export { id, format, output } => commands::export::run(id, format, output).await,
        Commands::Config { command } => match command {
            ConfigCommands::Show => commands::config::show(),
            ConfigCommands::Set { key, value } => commands::config::set(key, value),
        },
    }
}
