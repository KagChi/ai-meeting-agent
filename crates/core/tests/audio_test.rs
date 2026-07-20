use meeting_agent_core::audio::{
    chunk_audio_memory, convert_bytes_to_wav, convert_to_wav, probe_duration,
    probe_duration_from_bytes,
};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp(prefix: &str, ext: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}.{ext}"))
}

/// Generate a short m4a via ffmpeg for path-based conversion tests.
fn generate_test_m4a() -> Vec<u8> {
    let out = unique_temp("meeting-agent-test", "m4a");
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=f=440:d=1",
            "-c:a",
            "aac",
            "-b:a",
            "64k",
        ])
        .arg(&out)
        .status()
        .expect("spawn ffmpeg (install ffmpeg or skip #[ignore] tests)");
    assert!(status.success(), "ffmpeg should generate test m4a");
    let bytes = std::fs::read(&out).expect("read test m4a");
    let _ = std::fs::remove_file(&out);
    bytes
}

#[test]
#[ignore] // Requires FFmpeg to be installed
fn test_convert_m4a_bytes_to_wav_path() {
    let input = generate_test_m4a();
    assert!(input.len() > 100, "test m4a should not be tiny");

    let wav_path = convert_bytes_to_wav(&input, "recording.m4a").expect("convert m4a");
    let meta = std::fs::metadata(&wav_path).expect("wav metadata");
    assert!(
        meta.len() > 1000,
        "converted WAV should be substantial, got {}",
        meta.len()
    );

    let duration = probe_duration(&wav_path).expect("probe duration");
    assert!(duration > 0.5 && duration < 2.0, "duration={duration}");

    let bytes = std::fs::read(&wav_path).expect("read wav");
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    let _ = std::fs::remove_file(&wav_path);
}

#[test]
#[ignore] // Requires FFmpeg to be installed
fn test_convert_to_wav_path() {
    let input = generate_test_m4a();
    let temp = unique_temp("meeting-agent-in", "m4a");
    std::fs::write(&temp, &input).expect("write m4a");

    let wav_path = convert_to_wav(&temp).expect("convert_to_wav");
    let duration = probe_duration(&wav_path).expect("probe");
    assert!(duration > 0.0);

    let _ = std::fs::remove_file(&temp);
    let _ = std::fs::remove_file(&wav_path);
}

#[test]
fn test_convert_empty_bytes_fails() {
    let result = convert_bytes_to_wav(&[], "audio.m4a");
    assert!(result.is_err(), "Empty input should produce error");
}

#[test]
#[ignore] // Requires FFmpeg to be installed
fn test_probe_duration_from_bytes() {
    let input = generate_test_m4a();
    let wav_path = convert_bytes_to_wav(&input, "t.m4a").expect("convert");
    let wav = std::fs::read(&wav_path).expect("read");
    let _ = std::fs::remove_file(&wav_path);

    let result = probe_duration_from_bytes(&wav);
    assert!(result.is_ok(), "Duration probe should succeed: {result:?}");
    let duration = result.unwrap();
    assert!(duration > 0.0, "Duration should be positive");
}

#[test]
#[ignore] // Requires FFmpeg to be installed
fn test_chunk_audio_memory_basic() {
    let input = generate_test_m4a();
    let wav_path = convert_bytes_to_wav(&input, "t.m4a").expect("convert");
    let wav = std::fs::read(&wav_path).expect("read");
    let _ = std::fs::remove_file(&wav_path);

    let result = chunk_audio_memory(&wav, 0.5);
    assert!(result.is_ok(), "Chunking should succeed: {result:?}");
    let chunks = result.unwrap();
    assert!(!chunks.is_empty(), "Should produce at least one chunk");
    for (i, chunk) in chunks.iter().enumerate() {
        assert!(!chunk.is_empty(), "Chunk {i} should not be empty");
        assert_eq!(&chunk[0..4], b"RIFF", "Chunk {i} should start with RIFF");
    }
}

#[test]
#[ignore] // Requires FFmpeg to be installed
fn test_chunk_audio_memory_zero_duration() {
    let input = generate_test_m4a();
    let wav_path = convert_bytes_to_wav(&input, "t.m4a").expect("convert");
    let wav = std::fs::read(&wav_path).expect("read");
    let _ = std::fs::remove_file(&wav_path);

    let result = chunk_audio_memory(&wav, 0.0);
    assert!(result.is_err(), "Zero segment duration should fail");
}
