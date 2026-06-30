use anyhow::Result;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use meeting_agent_core::{
    config::Config, fs, models::Meeting, storage::MeetingStorage,
    transcription::TranscriptionClient, transcription::TranscriptionRequest,
};
use std::io::{self, Write};
use std::path::PathBuf;

pub async fn run(file: String, title: Option<String>) -> Result<()> {
    let file_path = PathBuf::from(&file);
    if !file_path.exists() {
        anyhow::bail!("Audio file not found: {}", file);
    }

    println!("{}", "Importing audio file...".bold());
    println!("  File: {}", file);

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

    fs::ensure_data_dir()?;

    let config_path = fs::config_path()?;
    let config = Config::load(&config_path)?;

    let client = TranscriptionClient::new(config.transcription.clone())?;

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    pb.set_message("Uploading and transcribing audio...");
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let request = TranscriptionRequest {
        file_path: audio_file_to_transcribe.to_string_lossy().to_string(),
        response_format: Some("verbose_json".to_string()),
        language: None,
        prompt: None,
        temperature: Some(0.0),
    };

    let response = client
        .transcribe_chunked(
            request,
            config.transcription.chunk_seconds,
            config.transcription.chunk_concurrency,
        )
        .await?;
    pb.finish_with_message("Transcription complete!".green().to_string());

    // Optional speaker diarization (must run before temp audio deletion).
    let response = if config.diarize.enabled {
        let pb2 = ProgressBar::new_spinner();
        pb2.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .unwrap(),
        );
        pb2.set_message("Diarizing speakers...");
        pb2.enable_steady_tick(std::time::Duration::from_millis(100));

        let diarize_client =
            meeting_agent_core::diarize::DiarizeClient::new(config.diarize.base_url.clone());
        match diarize_client
            .diarize(
                &audio_file_to_transcribe,
                &response,
                config.diarize.num_speakers,
            )
            .await
        {
            Ok(resp) => {
                pb2.finish_with_message("Diarization complete!".green().to_string());
                meeting_agent_core::diarize::merge_speakers(response, resp)
            }
            Err(e) => {
                pb2.finish_with_message(format!("Diarization failed: {}", e).yellow().to_string());
                response
            }
        }
    } else {
        response
    };

    if temp_file_created {
        let _ = std::fs::remove_file(&audio_file_to_transcribe);
    }

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

    let meeting_title = title.unwrap_or_else(|| {
        file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled Meeting")
            .to_string()
    });

    let meeting = Meeting::new(meeting_title);
    let storage = MeetingStorage;

    storage.create_meeting(&meeting)?;
    storage.save_audio(&meeting.id, &file_path)?;
    storage.save_transcript(&meeting.id, &response)?;

    let duration_seconds = response.duration.map(|d| d as u64);
    storage.mark_transcription_complete(
        &meeting.id,
        &config.transcription.provider,
        &config.transcription.model,
        duration_seconds,
    )?;

    println!(
        "\n{} Meeting saved with ID: {}",
        "✓".green().bold(),
        meeting.id.cyan()
    );
    println!(
        "  View with: {} {}",
        "meeting-agent show".yellow(),
        meeting.id
    );

    Ok(())
}
