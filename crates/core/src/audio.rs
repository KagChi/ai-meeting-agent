use anyhow::{Context, Result};
use ffmpeg_sidecar::command::FfmpegCommand;
use std::path::{Path, PathBuf};

/// Whisper API supported formats
const WHISPER_SUPPORTED_FORMATS: &[&str] = &["mp3", "wav"];

/// Check if audio file needs conversion to MP3
pub fn needs_conversion(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => !WHISPER_SUPPORTED_FORMATS.contains(&ext.to_lowercase().as_str()),
        None => true, // No extension = assume needs conversion
    }
}

/// Convert audio file to MP3 format
/// Returns path to MP3 file (temp file in system temp dir)
pub fn convert_to_mp3(input_path: &Path) -> Result<PathBuf> {
    // Create temp output path
    let temp_dir = std::env::temp_dir();
    let output_path = temp_dir.join(format!("meeting-agent-{}.mp3", uuid::Uuid::new_v4()));

    // Convert using ffmpeg-sidecar
    FfmpegCommand::new()
        .input(input_path.to_str().context("Invalid input path")?)
        .args(["-codec:a", "libmp3lame"])
        .args(["-qscale:a", "2"]) // VBR quality 2 (~190kbps)
        .overwrite() // -y flag
        .output(output_path.to_str().context("Invalid output path")?)
        .spawn()
        .context("Failed to spawn FFmpeg process")?
        .wait()
        .context("FFmpeg conversion failed")?;

    Ok(output_path)
}

/// Check if FFmpeg is available by attempting to run version command
pub fn ensure_ffmpeg_interactive() -> Result<()> {
    // Try to spawn FFmpeg with version check
    match FfmpegCommand::new().args(["-version"]).spawn() {
        Ok(mut child) => {
            // FFmpeg spawned successfully, wait for it to complete
            let _ = child.wait();
            Ok(())
        }
        Err(_) => {
            // FFmpeg not available - return error so CLI can prompt
            anyhow::bail!("FFmpeg not found on system")
        }
    }
}

/// Download FFmpeg binary (called after user confirms)
pub fn download_ffmpeg() -> Result<()> {
    ffmpeg_sidecar::download::auto_download().context("Failed to download FFmpeg binary")
}
