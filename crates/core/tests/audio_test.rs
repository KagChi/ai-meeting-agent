use meeting_agent_core::audio::{
    chunk_audio_memory, convert_to_mp3_memory, probe_duration_from_bytes,
};

/// Generate a minimal valid MP3 file in memory for testing
/// This is a silent 1-second MP3 at 16kHz mono
fn generate_test_mp3() -> Vec<u8> {
    // Minimal MP3 header + silent frames (approximately 1 second)
    // This is a simplified test fixture - real MP3 has complex structure
    // For proper testing, we'd use a real audio file from test fixtures
    vec![
        0xFF, 0xFB, 0x90, 0x00, // MP3 sync word + header
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Silent frame data
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]
}

#[test]
#[ignore] // Requires FFmpeg to be installed
fn test_convert_to_mp3_memory_basic() {
    let input = generate_test_mp3();
    let result = convert_to_mp3_memory(&input);

    assert!(result.is_ok(), "Conversion should succeed");
    let output = result.unwrap();
    assert!(!output.is_empty(), "Output should not be empty");

    // Check MP3 signature (first 2 bytes should be FF FB/FA/F3/F2)
    assert_eq!(output[0], 0xFF, "MP3 should start with 0xFF");
    assert!(
        output[1] & 0xE0 == 0xE0,
        "Second byte should have MP3 sync bits"
    );
}

#[test]
#[ignore] // Requires FFmpeg to be installed
fn test_probe_duration_from_bytes() {
    let input = generate_test_mp3();
    let result = probe_duration_from_bytes(&input);

    assert!(result.is_ok(), "Duration probe should succeed");
    let duration = result.unwrap();
    assert!(duration > 0.0, "Duration should be positive");
    // Our test fixture is approximately 1 second
    assert!(duration < 2.0, "Duration should be less than 2 seconds");
}

#[test]
#[ignore] // Requires FFmpeg to be installed
fn test_chunk_audio_memory_basic() {
    let input = generate_test_mp3();

    // Try to chunk into 0.5 second segments
    let result = chunk_audio_memory(&input, 0.5);

    assert!(result.is_ok(), "Chunking should succeed");
    let chunks = result.unwrap();

    // Should produce at least 1 chunk (our test audio is ~1 second)
    assert!(!chunks.is_empty(), "Should produce at least one chunk");

    // Each chunk should be valid MP3
    for (i, chunk) in chunks.iter().enumerate() {
        assert!(!chunk.is_empty(), "Chunk {} should not be empty", i);
        assert_eq!(chunk[0], 0xFF, "Chunk {} should start with 0xFF", i);
    }
}

#[test]
#[ignore] // Requires FFmpeg to be installed
fn test_convert_empty_input() {
    let empty: Vec<u8> = vec![];
    let result = convert_to_mp3_memory(&empty);

    // Empty input should fail
    assert!(result.is_err(), "Empty input should produce error");
}

#[test]
#[ignore] // Requires FFmpeg to be installed
fn test_chunk_audio_memory_zero_duration() {
    let input = generate_test_mp3();

    // Zero segment duration should fail
    let result = chunk_audio_memory(&input, 0.0);
    assert!(result.is_err(), "Zero segment duration should fail");
}
