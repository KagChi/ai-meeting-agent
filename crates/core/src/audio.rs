use anyhow::{Context, Result};
use ffmpeg_sidecar::command::FfmpegCommand;
use ffmpeg_sidecar::ffprobe;
use std::path::{Path, PathBuf};

/// Whisper API supported formats
const WHISPER_SUPPORTED_FORMATS: &[&str] = &["mp3", "wav"];

/// Check if audio file needs conversion to WAV
pub fn needs_conversion(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => !WHISPER_SUPPORTED_FORMATS.contains(&ext.to_lowercase().as_str()),
        None => true, // No extension = assume needs conversion
    }
}

/// Convert audio file to WAV format
/// Returns path to WAV file (temp file in system temp dir)
pub fn convert_to_wav(input_path: &Path) -> Result<PathBuf> {
    // Create temp output path
    let temp_dir = std::env::temp_dir();
    let output_path = temp_dir.join(format!("meeting-agent-{}.wav", uuid::Uuid::new_v4()));

    // Path-based FFmpeg: seekable input (required for m4a/mp4 moov atoms)
    let status = FfmpegCommand::new()
        .input(input_path.to_str().context("Invalid input path")?)
        .args(["-vn"])
        .args(["-ac", "1"])
        .args(["-ar", "16000"])
        .args(["-acodec", "pcm_s16le"])
        .args(["-f", "wav"])
        .overwrite()
        .output(output_path.to_str().context("Invalid output path")?)
        .spawn()
        .context("Failed to spawn FFmpeg process")?
        .wait()
        .context("FFmpeg process execution failed")?;

    if !status.success() {
        anyhow::bail!("FFmpeg conversion failed with status: {}", status);
    }

    let meta = std::fs::metadata(&output_path)
        .with_context(|| format!("FFmpeg produced no output at {}", output_path.display()))?;
    if meta.len() < 44 {
        let _ = std::fs::remove_file(&output_path);
        anyhow::bail!(
            "FFmpeg produced empty/invalid WAV ({} bytes) from {}",
            meta.len(),
            input_path.display()
        );
    }

    Ok(output_path)
}

/// Write audio bytes to a temp file (preserving extension), convert to WAV via path-based FFmpeg.
/// Returns path to the converted WAV temp file. Caller is responsible for cleanup.
///
/// Temp-file conversion is required for containers like m4a/mp4 that need seekable input
/// (stdin/pipe demux often fails with "moov atom not found").
pub fn convert_bytes_to_wav(input_bytes: &[u8], filename: &str) -> Result<PathBuf> {
    if input_bytes.is_empty() {
        anyhow::bail!("Audio data is empty");
    }

    let temp_dir = std::env::temp_dir();
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "bin".to_string());
    let input_path = temp_dir.join(format!(
        "meeting-agent-in-{}.{}",
        uuid::Uuid::new_v4(),
        ext
    ));

    std::fs::write(&input_path, input_bytes).with_context(|| {
        format!(
            "Failed to write {} bytes to temp input {}",
            input_bytes.len(),
            input_path.display()
        )
    })?;

    let result = convert_to_wav(&input_path);
    if let Err(e) = std::fs::remove_file(&input_path) {
        log::warn!(
            "failed to remove temp input {}: {}",
            input_path.display(),
            e
        );
    }
    result
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
/// Reads `format.duration`. If missing/`N/A`, falls back to stream duration.
pub fn probe_duration(path: &Path) -> Result<f64> {
    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

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

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let duration_str = stdout.trim();

    if !output.status.success() {
        anyhow::bail!(
            "ffprobe failed for {} ({} bytes): {}",
            path.display(),
            file_size,
            stderr.trim()
        );
    }

    if duration_str == "N/A" || duration_str.is_empty() {
        return probe_duration_stream_fallback(path, file_size);
    }

    let duration: f64 = duration_str.parse().with_context(|| {
        format!(
            "Failed to parse ffprobe duration for {} ({} bytes): {:?}",
            path.display(),
            file_size,
            duration_str
        )
    })?;

    if duration <= 0.0 {
        anyhow::bail!(
            "Invalid audio duration {:.3}s for {} ({} bytes)",
            duration,
            path.display(),
            file_size
        );
    }

    Ok(duration)
}

fn probe_duration_stream_fallback(path: &Path, file_size: u64) -> Result<f64> {
    let output = std::process::Command::new(ffprobe::ffprobe_path())
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("a:0")
        .arg("-show_entries")
        .arg("stream=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(path)
        .output()
        .context("Failed to spawn ffprobe stream fallback")?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let duration_str = stdout.trim();

    if !output.status.success() || duration_str == "N/A" || duration_str.is_empty() {
        anyhow::bail!(
            "ffprobe could not determine duration for {} ({} bytes): {}",
            path.display(),
            file_size,
            if stderr.trim().is_empty() {
                duration_str
            } else {
                stderr.trim()
            }
        );
    }

    let duration: f64 = duration_str.parse().with_context(|| {
        format!(
            "Failed to parse stream duration for {} ({} bytes): {:?}",
            path.display(),
            file_size,
            duration_str
        )
    })?;

    if duration <= 0.0 {
        anyhow::bail!(
            "Invalid stream duration {:.3}s for {} ({} bytes)",
            duration,
            path.display(),
            file_size
        );
    }

    Ok(duration)
}

/// Split an audio file into N-second chunks using ffmpeg's segment muxer.
///
/// Re-encodes to ensure valid MP3 chunks (stream copy can produce corrupted
/// segments when boundaries don't align with keyframes). Returns ordered list
/// of temp mp3 chunk paths. Caller is responsible for cleanup.
///
/// The output pattern uses `%03d` zero-padded indices so lexicographic sort
/// matches chronological order.
pub fn chunk_audio(path: &Path, segment_seconds: f64) -> Result<Vec<PathBuf>> {
    let temp_dir = std::env::temp_dir();
    let session_id = uuid::Uuid::new_v4();
    let output_pattern = temp_dir.join(format!("meeting-agent-chunk-{}-%03d.wav", session_id));

    FfmpegCommand::new()
        .input(path.to_str().context("Invalid input path")?)
        .args(["-f", "segment"])
        .args(["-segment_time", &segment_seconds.to_string()])
        .args(["-c:a", "pcm_s16le"])
        .args(["-ar", "16000"])
        .args(["-ac", "1"])
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
                .map(|n| n.starts_with(&prefix) && n.ends_with(".wav"))
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

/// Probe duration from audio bytes in memory
///
/// Uses ffprobe with pipe:0 (stdin) to read audio from memory.
/// Returns duration in seconds.
pub fn probe_duration_from_bytes(audio_bytes: &[u8]) -> Result<f64> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    log::info!(
        "[probe_duration_from_bytes] probing {} bytes",
        audio_bytes.len()
    );

    // Check if bytes look like valid audio (basic heuristic)
    if audio_bytes.len() < 100 {
        log::error!(
            "[probe_duration_from_bytes] audio data too small: {} bytes",
            audio_bytes.len()
        );
        anyhow::bail!("Audio data too small: {} bytes", audio_bytes.len());
    }

    let mut child = Command::new(ffprobe::ffprobe_path())
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg("pipe:0") // stdin
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn ffprobe")?;

    // Write audio bytes to stdin
    if let Some(mut stdin) = child.stdin.take() {
        match stdin.write_all(audio_bytes) {
            Ok(_) => {
                log::debug!(
                    "[probe_duration_from_bytes] wrote {} bytes to ffprobe stdin",
                    audio_bytes.len()
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                log::warn!("[probe_duration_from_bytes] broken pipe while writing to ffprobe (ffprobe may have rejected input early)");
                // Don't return error here - let ffprobe output tell us what went wrong
            }
            Err(e) => {
                return Err(e).context("Failed to write to ffprobe stdin");
            }
        }
        drop(stdin);
    }

    let output = child
        .wait_with_output()
        .context("Failed to wait for ffprobe")?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Always log what ffprobe said, regardless of exit status
    log::info!(
        "[probe_duration_from_bytes] ffprobe exit status: {:?}, stderr len: {}, stdout len: {}",
        output.status.code(),
        stderr.len(),
        stdout.len()
    );

    if !stderr.trim().is_empty() {
        log::info!(
            "[probe_duration_from_bytes] ffprobe stderr: {}",
            stderr.trim()
        );
    }

    if !stdout.trim().is_empty() {
        log::info!(
            "[probe_duration_from_bytes] ffprobe stdout: {}",
            stdout.trim()
        );
    }

    if !output.status.success() {
        log::error!(
            "[probe_duration_from_bytes] ffprobe failed with status {:?}",
            output.status.code()
        );
        anyhow::bail!(
            "ffprobe failed (exit {}): {}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        );
    }

    // Log any warnings from stderr even on success
    if !stderr.trim().is_empty() {
        log::warn!(
            "[probe_duration_from_bytes] ffprobe stderr: {}",
            stderr.trim()
        );
    }

    let duration_str = stdout.trim();

    // Handle "N/A" case - ffprobe couldn't determine duration from container metadata
    if duration_str == "N/A" || duration_str.is_empty() {
        log::warn!("[probe_duration_from_bytes] ffprobe returned N/A for duration, audio may lack container metadata");

        // Fall back to counting frames - more expensive but works for headerless/piped audio
        return probe_duration_from_bytes_fallback(audio_bytes);
    }

    let duration: f64 = duration_str
        .parse()
        .with_context(|| format!("Failed to parse duration: {:?}", duration_str))?;

    log::info!("[probe_duration_from_bytes] duration={:.2}s", duration);
    Ok(duration)
}

/// Fallback method to probe duration by counting frames/packets when container metadata is unavailable
fn probe_duration_from_bytes_fallback(audio_bytes: &[u8]) -> Result<f64> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    log::info!(
        "[probe_duration_from_bytes_fallback] counting frames for {} bytes",
        audio_bytes.len()
    );

    // Use ffprobe to count frames and calculate duration
    let mut child = Command::new(ffprobe::ffprobe_path())
        .arg("-v")
        .arg("error")
        .arg("-count_packets")
        .arg("-select_streams")
        .arg("a:0")
        .arg("-show_entries")
        .arg("stream=nb_read_packets,sample_rate")
        .arg("-of")
        .arg("csv=p=0")
        .arg("pipe:0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn ffprobe for fallback")?;

    if let Some(mut stdin) = child.stdin.take() {
        match stdin.write_all(audio_bytes) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                log::warn!("[probe_duration_from_bytes_fallback] broken pipe during fallback");
            }
            Err(e) => {
                return Err(e).context("Failed to write to ffprobe stdin (fallback)");
            }
        }
        drop(stdin);
    }

    let output = child
        .wait_with_output()
        .context("Failed to wait for ffprobe (fallback)")?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    if !output.status.success() {
        log::error!(
            "[probe_duration_from_bytes_fallback] ffprobe failed: {}",
            stderr.trim()
        );
        anyhow::bail!("ffprobe fallback failed: {}", stderr.trim());
    }

    log::info!(
        "[probe_duration_from_bytes_fallback] ffprobe output: {}",
        stdout.trim()
    );

    // Parse CSV output: nb_read_packets,sample_rate
    let parts: Vec<&str> = stdout.trim().split(',').collect();
    if parts.len() < 2 {
        anyhow::bail!("Unexpected ffprobe output format: {}", stdout.trim());
    }

    // For now, if we can't determine duration precisely, use a rough estimate
    // based on file size and typical bitrates
    log::warn!("[probe_duration_from_bytes_fallback] frame counting not fully implemented, using size-based estimate");

    // Rough estimate: assume 128kbps average bitrate
    let estimated_duration = (audio_bytes.len() as f64 * 8.0) / (128.0 * 1000.0);
    log::info!(
        "[probe_duration_from_bytes_fallback] estimated duration={:.2}s (from {} bytes)",
        estimated_duration,
        audio_bytes.len()
    );

    Ok(estimated_duration)
}

/// Split audio bytes into N-second chunks in memory
///
/// Uses a seek-based approach: invokes FFmpeg multiple times with -ss and -t flags.
/// Each chunk is encoded as WAV in memory. Returns a vector of WAV byte chunks.
/// No temporary files are created.
///
/// This approach is simpler than pipe-based chunking but requires multiple FFmpeg
/// invocations. For typical meeting recordings, the overhead is acceptable.
pub fn chunk_audio_memory(audio_bytes: &[u8], segment_seconds: f64) -> Result<Vec<Vec<u8>>> {
    // First, probe the duration
    let total_duration = probe_duration_from_bytes(audio_bytes)?;

    // Calculate number of chunks
    let chunk_count = (total_duration / segment_seconds).ceil() as usize;

    let mut chunks = Vec::with_capacity(chunk_count);

    // Generate each chunk by seeking to the appropriate timestamp
    for i in 0..chunk_count {
        let start = i as f64 * segment_seconds;
        let duration = segment_seconds.min(total_duration - start);

        let chunk_bytes = chunk_audio_at_offset(audio_bytes, start, duration)?;
        chunks.push(chunk_bytes);
    }

    if chunks.is_empty() {
        anyhow::bail!("Audio chunking produced no chunks");
    }

    Ok(chunks)
}

/// Helper function to extract a chunk at a specific timestamp
fn chunk_audio_at_offset(audio_bytes: &[u8], start: f64, duration: f64) -> Result<Vec<u8>> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new(ffmpeg_sidecar::paths::ffmpeg_path())
        .arg("-ss")
        .arg(start.to_string())
        .arg("-t")
        .arg(duration.to_string())
        .arg("-i")
        .arg("pipe:0") // stdin
        .args(["-f", "wav"])
        .args(["-c:a", "pcm_s16le"])
        .args(["-ar", "16000"])
        .args(["-ac", "1"])
        .arg("pipe:1") // stdout
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn FFmpeg for chunk")?;

    // Write input to stdin
    if let Some(mut stdin) = child.stdin.take() {
        // Ignore broken pipe errors - FFmpeg may close stdin early if input is invalid
        match stdin.write_all(audio_bytes) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                // FFmpeg closed stdin early, let the exit status tell us what went wrong
            }
            Err(e) => return Err(anyhow::Error::from(e).context("Failed to write to FFmpeg stdin")),
        }
        drop(stdin);
    }

    let output = child
        .wait_with_output()
        .context("Failed to wait for FFmpeg chunk process")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("FFmpeg chunking at offset {} failed: {}", start, stderr);
    }

    Ok(output.stdout)
}

/// Helper to determine if conversion is needed based on filename extension
pub fn needs_conversion_by_filename(filename: &str) -> bool {
    let path = Path::new(filename);
    needs_conversion(path)
}

/// Extract one time span from an audio file as mono 16 kHz PCM WAV bytes.
pub fn extract_span_to_wav_bytes(path: &Path, start_s: f64, end_s: f64) -> Result<Vec<u8>> {
    let duration = (end_s - start_s).max(0.0);
    if duration < 0.05 {
        anyhow::bail!("span too short: {start_s:.3}-{end_s:.3}");
    }
    let input = path.to_str().context("Invalid input path")?;
    let child = std::process::Command::new(ffmpeg_sidecar::paths::ffmpeg_path())
        .arg("-ss")
        .arg(start_s.to_string())
        .arg("-t")
        .arg(duration.to_string())
        .arg("-i")
        .arg(input)
        .args(["-vn"])
        .args(["-f", "wav"])
        .args(["-c:a", "pcm_s16le"])
        .args(["-ar", "16000"])
        .args(["-ac", "1"])
        .arg("pipe:1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn FFmpeg for span extract")?;

    let output = child
        .wait_with_output()
        .context("Failed to wait for FFmpeg span extract")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "FFmpeg extract span {start_s:.2}-{end_s:.2} failed: {}",
            stderr
        );
    }
    if output.stdout.is_empty() {
        anyhow::bail!("FFmpeg extract produced empty WAV for {start_s:.2}-{end_s:.2}");
    }
    Ok(output.stdout)
}

/// Concatenate multiple time spans from one recording into a single mono 16 kHz WAV.
///
/// Spans are extracted individually then concatenated with ffmpeg's concat demuxer.
/// Empty / failed spans are skipped. Returns error if no span yields audio.
pub fn extract_spans_to_wav_bytes(path: &Path, spans: &[(f64, f64)]) -> Result<Vec<u8>> {
    let mut valid: Vec<(f64, f64)> = spans
        .iter()
        .copied()
        .filter(|(a, b)| b - a >= 0.05)
        .collect();
    if valid.is_empty() {
        anyhow::bail!("no valid spans to extract");
    }
    // Cap total extract work: keep longest spans first if many.
    if valid.len() > 40 {
        valid.sort_by(|a, b| (b.1 - b.0).partial_cmp(&(a.1 - a.0)).unwrap_or(std::cmp::Ordering::Equal));
        valid.truncate(40);
        valid.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    }

    if valid.len() == 1 {
        return extract_span_to_wav_bytes(path, valid[0].0, valid[0].1);
    }

    let temp_dir = std::env::temp_dir();
    let session = uuid::Uuid::new_v4();
    let mut part_paths: Vec<PathBuf> = Vec::new();
    let mut cleanup = Vec::new();

    for (i, (start, end)) in valid.iter().enumerate() {
        match extract_span_to_wav_bytes(path, *start, *end) {
            Ok(bytes) => {
                let p = temp_dir.join(format!("meeting-agent-enroll-{session}-{i:03}.wav"));
                std::fs::write(&p, &bytes)
                    .with_context(|| format!("Failed to write temp span {}", p.display()))?;
                part_paths.push(p.clone());
                cleanup.push(p);
            }
            Err(e) => {
                log::debug!(
                    "[audio] skip enroll span {start:.2}-{end:.2}: {e:#}"
                );
            }
        }
    }

    if part_paths.is_empty() {
        anyhow::bail!("all span extracts failed");
    }
    if part_paths.len() == 1 {
        let bytes = std::fs::read(&part_paths[0])
            .with_context(|| format!("Failed to read {}", part_paths[0].display()))?;
        for p in &cleanup {
            let _ = std::fs::remove_file(p);
        }
        return Ok(bytes);
    }

    let list_path = temp_dir.join(format!("meeting-agent-enroll-{session}-list.txt"));
    let mut list_body = String::new();
    for p in &part_paths {
        // ffmpeg concat demuxer: single-quoted paths, escape ' as '\''
        let s = p.to_string_lossy().replace('\'', "'\\''");
        list_body.push_str(&format!("file '{s}'\n"));
    }
    std::fs::write(&list_path, list_body)
        .with_context(|| format!("Failed to write concat list {}", list_path.display()))?;
    cleanup.push(list_path.clone());

    let out_path = temp_dir.join(format!("meeting-agent-enroll-{session}-out.wav"));
    cleanup.push(out_path.clone());

    let status = FfmpegCommand::new()
        .args(["-f", "concat"])
        .args(["-safe", "0"])
        .input(list_path.to_str().context("Invalid list path")?)
        .args(["-c", "copy"])
        .overwrite()
        .output(out_path.to_str().context("Invalid out path")?)
        .spawn()
        .context("Failed to spawn FFmpeg concat")?
        .wait()
        .context("Failed to wait for FFmpeg concat")?;

    let result = if status.success() {
        std::fs::read(&out_path).with_context(|| format!("Failed to read {}", out_path.display()))
    } else {
        Err(anyhow::anyhow!("FFmpeg concat of enroll spans failed"))
    };

    for p in &cleanup {
        let _ = std::fs::remove_file(p);
    }
    result
}
