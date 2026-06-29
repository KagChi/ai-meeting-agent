//! Meeting Agent CLI
//!
//! Command-line interface for the meeting agent system.

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
    let cli = Cli::parse();

    match cli.command {
        Commands::Server { port, host } => {
            println!("Starting server on {}:{}", host, port);
            // TODO: Start server (will be implemented in Phase 8)
            Ok(())
        }
        Commands::Import { file, title } => {
            println!("Importing: {}", file);
            if let Some(t) = title {
                println!("Title: {}", t);
            }
            // TODO: Implement import logic
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
