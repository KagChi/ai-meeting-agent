use anyhow::{Context, Result};
use ffmpeg_sidecar::command::FfmpegCommand;
use ffmpeg_sidecar::ffprobe;
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

/// Probe the duration of an audio file in seconds using ffprobe.
///
/// Uses `ffprobe -show_format -of json` and reads `format.duration`.
/// Falls back to stderr parsing if JSON parse fails.
pub fn probe_duration(path: &Path) -> Result<f64> {
    let output = std::process::Command::new(ffprobe::ffprobe_path())
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(path)
        .output()
        .context("Failed to spawn ffprobe")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffprobe failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let duration: f64 = stdout
        .trim()
        .parse()
        .with_context(|| format!("Failed to parse ffprobe duration: {:?}", stdout.trim()))?;
    Ok(duration)
}

/// Split an audio file into N-second chunks using ffmpeg's segment muxer.
///
/// Uses `-c copy` to avoid re-encoding (fast, lossless). Returns ordered list
/// of temp mp3 chunk paths. Caller is responsible for cleanup.
///
/// The output pattern uses `%03d` zero-padded indices so lexicographic sort
/// matches chronological order.
pub fn chunk_audio(path: &Path, segment_seconds: f64) -> Result<Vec<PathBuf>> {
    let temp_dir = std::env::temp_dir();
    let session_id = uuid::Uuid::new_v4();
    let output_pattern = temp_dir.join(format!("meeting-agent-chunk-{}-%03d.mp3", session_id));

    FfmpegCommand::new()
        .input(path.to_str().context("Invalid input path")?)
        .args(["-f", "segment"])
        .args(["-segment_time", &segment_seconds.to_string()])
        .args(["-c", "copy"])
        .overwrite()
        .output(output_pattern.to_str().context("Invalid output pattern")?)
        .spawn()
        .context("Failed to spawn FFmpeg for chunking")?
        .wait()
        .context("FFmpeg chunking failed")?;

    // Collect generated chunk files matching the session id pattern
    let prefix = format!("meeting-agent-chunk-{}-", session_id);
    let mut chunks: Vec<PathBuf> = std::fs::read_dir(&temp_dir)
        .with_context(|| format!("Failed to read temp dir: {:?}", temp_dir))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(&prefix) && n.ends_with(".mp3"))
                .unwrap_or(false)
        })
        .collect();

    // Sort by filename → chronological via %03d padding
    chunks.sort();

    if chunks.is_empty() {
        anyhow::bail!("FFmpeg chunking produced no output files");
    }

    Ok(chunks)
}
