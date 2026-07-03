//! Whisper integration test — self-hosted vLLM endpoint
//!
//! Verifies connectivity to the self-hosted Whisper Large v3 model served via
//! vLLM at `http://140.118.122.126:8080/v1`. Generates a transcript from a
//! 2-minute test audio file using chunked parallel transcription.
//!
//! Run with:
//!   cargo test --test whisper_integration_test -- --nocapture --test-threads=1 --ignored
//!
//! Requires:
//!   - FFmpeg on PATH
//!   - Network access to 140.118.122.126:8080
//!   - Test audio at tests/assets/test_meeting.wav

use meeting_agent_core::config::TranscriptionConfig;
use meeting_agent_core::transcription::{TranscriptionClient, TranscriptionRequest};
use std::path::PathBuf;

/// Build a TranscriptionConfig pointing at the self-hosted vLLM Whisper endpoint.
fn vllm_config() -> TranscriptionConfig {
    TranscriptionConfig {
        provider: "openai".to_string(),
        api_key: Some("token-placeholder".to_string()),
        base_url: "http://140.118.122.126:8080/v1".to_string(),
        model: "openai/whisper-large-v3".to_string(),
        chunk_seconds: 30.0,
        chunk_concurrency: 2,
    }
}

/// Locate the test audio file relative to the workspace root.
fn test_audio_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();
    let audio_path = workspace_root.join("tests/assets/test_meeting.wav");
    assert!(
        audio_path.exists(),
        "Test audio not found at {}. Generate with: say -v Samantha -o tests/assets/test_meeting.aiff '...' && ffmpeg -y -i tests/assets/test_meeting.aiff -ar 16000 -ac 1 -codec:a pcm_s16le tests/assets/test_meeting.wav",
        audio_path.display()
    );
    audio_path
}

#[tokio::test]
#[ignore = "requires self-hosted vLLM endpoint at 140.118.122.126:8080"]
async fn whisper_vllm_connectivity() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .is_test(true)
        .try_init();

    let config = vllm_config();
    let client =
        TranscriptionClient::new(config.clone()).expect("Failed to create transcription client");

    let audio_path = test_audio_path();

    println!("========================================");
    println!("Whisper Integration Test");
    println!("========================================");
    println!("Endpoint:  {}", config.base_url);
    println!("Model:     {}", config.model);
    println!("Audio:     {}", audio_path.display());
    println!(
        "Chunk:     {}s (concurrency={})",
        config.chunk_seconds, config.chunk_concurrency
    );
    println!("========================================");

    let request = TranscriptionRequest {
        file_path: audio_path.to_string_lossy().to_string(),
        response_format: Some("verbose_json".to_string()),
        language: Some("en".to_string()),
        prompt: None,
        temperature: None,
    };

    let started = std::time::Instant::now();
    let result = client
        .transcribe_chunked(request, config.chunk_seconds, config.chunk_concurrency)
        .await;
    let elapsed = started.elapsed();

    println!(
        "\n--- Transcription completed in {:.2}s ---\n",
        elapsed.as_secs_f64()
    );

    let response = result.expect("Transcription failed");

    // --- Assertions ---
    assert!(
        !response.text.is_empty(),
        "Transcript text should not be empty"
    );
    println!(
        "Transcript text (first 200 chars): {:?}",
        &response.text[..response.text.len().min(200)]
    );

    let segments = response
        .segments
        .expect("verbose_json should return segments");
    assert!(!segments.is_empty(), "Should have at least one segment");
    println!("Segment count: {}", segments.len());

    // Print first few segments
    for (i, seg) in segments.iter().take(5).enumerate() {
        println!(
            "  Segment {}: [{:.2}s - {:.2}s] {:?}",
            i,
            seg.start,
            seg.end,
            &seg.text[..seg.text.len().min(80)]
        );
    }

    // Verify segment timing is monotonically increasing (chunked merge)
    for window in segments.windows(2) {
        assert!(
            window[0].end <= window[1].start + 0.5,
            "Segments should be roughly chronological: seg end {:.2} > next start {:.2}",
            window[0].end,
            window[1].start
        );
    }

    // --- Save transcript JSON ---
    let transcript_json = serde_json::json!({
        "segments": segments.iter().map(|s| serde_json::json!({
            "id": s.id,
            "start": s.start,
            "end": s.end,
            "text": s.text,
        })).collect::<Vec<_>>(),
    });

    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/output");
    std::fs::create_dir_all(&output_dir).expect("Failed to create output dir");

    let output_path = output_dir.join("transcript.json");
    std::fs::write(
        &output_path,
        serde_json::to_string_pretty(&transcript_json).unwrap(),
    )
    .expect("Failed to write transcript.json");

    println!("\nTranscript saved to: {}", output_path.display());
    println!("Full transcript text length: {} chars", response.text.len());
    println!("========================================");
    println!("PASS: Whisper integration test");
    println!("========================================");
}
