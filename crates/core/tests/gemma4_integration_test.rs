//! Gemma 4 integration test — self-hosted vLLM endpoint
//!
//! Verifies connectivity to the self-hosted Gemma 4 model served via vLLM at
//! `http://140.118.122.126:8001/v1`. Generates a meeting summary from a sample
//! transcript using template-based prompt construction and section parsing.
//!
//! Run with:
//!   cargo test --test gemma4_integration_test -- --nocapture --test-threads=1 --ignored
//!
//! Requires:
//!   - Network access to 140.118.122.126:8001
//!   - Transcript JSON at tests/output/transcript.json (from whisper_integration_test)

use meeting_agent_core::config::SummaryConfig;
use meeting_agent_core::models::{SummaryFormat, SummaryTemplate};
use meeting_agent_core::summary::{SummarizeOptions, SummaryClient};
use meeting_agent_core::transcription::{TranscriptSegment, TranscriptionResponse};
use std::path::PathBuf;

/// Build a SummaryConfig pointing at the self-hosted vLLM Gemma 4 endpoint.
fn vllm_config() -> SummaryConfig {
    SummaryConfig {
        provider: "openai".to_string(),
        api_key: Some("token-placeholder".to_string()),
        base_url: "http://140.118.122.126:8001/v1".to_string(),
        model: "gemma-4".to_string(),
        temperature: 0.3,
        max_tokens: 1024,
        language: Some("en".to_string()),
    }
}

/// Load the transcript from the Whisper test output, or fall back to a
/// hardcoded sample transcript if the file doesn't exist yet.
fn load_transcript() -> TranscriptionResponse {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();
    let transcript_path = workspace_root.join("tests/output/transcript.json");

    if transcript_path.exists() {
        let raw = std::fs::read_to_string(&transcript_path).unwrap();
        let val: serde_json::Value = serde_json::from_str(&raw).unwrap();

        // If it's our test output format with segments array
        if let Some(segments) = val.get("segments").and_then(|s| s.as_array()) {
            let segs: Vec<TranscriptSegment> = segments
                .iter()
                .map(|s| TranscriptSegment {
                    id: s["id"].as_u64().unwrap_or(0) as u32,
                    start: s["start"].as_f64().unwrap_or(0.0),
                    end: s["end"].as_f64().unwrap_or(0.0),
                    text: s["text"].as_str().unwrap_or("").to_string(),
                    timestamp: None,
                    tokens: None,
                    temperature: None,
                    avg_logprob: None,
                    compression_ratio: None,
                    no_speech_prob: None,
                    speaker: None,
                    refined_text: None,
                })
                .collect();

            let text = segs
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");

            // Only use the Whisper output if it has substantial content
            // (vLLM Whisper verbose_json may return empty segments for some chunks)
            if text.len() > 100 {
                return TranscriptionResponse {
                    text,
                    language: Some("en".to_string()),
                    duration: Some(segs.last().map(|s| s.end).unwrap_or(0.0)),
                    segments: Some(segs),
                    refined_text: None,
                };
            }
        }
    }

    // Fallback: hardcoded sample transcript (full Q3 planning meeting text)
    println!("[gemma4] transcript.json not found or empty, using hardcoded sample");
    let sample_text = "Welcome everyone to the Q3 planning meeting. Today we will discuss three main topics: the product roadmap, budget allocation, and team assignments. First, let us review the product roadmap for the third quarter. We plan to launch two new features: the dashboard redesign and the API integration module. The dashboard redesign is scheduled for completion by August fifteenth. The API integration module should be ready by September first. Next, regarding budget allocation. We have approved a budget of fifty thousand dollars for Q3 development. Thirty thousand will go to engineering, fifteen thousand to design, and five thousand to testing. Are there any questions about the budget? No questions, so let us move on. Finally, team assignments. Sarah will lead the dashboard redesign project. John will handle the API integration module. Mary will oversee testing and quality assurance for both projects. We will have weekly check-in meetings every Monday at ten AM. The next milestone review is scheduled for July twenty-fifth. Thank you everyone for attending. Let us have a productive quarter ahead.";

    TranscriptionResponse {
        text: sample_text.to_string(),
        language: Some("en".to_string()),
        duration: Some(66.0),
        segments: Some(vec![TranscriptSegment {
            id: 0,
            start: 0.0,
            end: 66.0,
            text: sample_text.to_string(),
            timestamp: None,
            tokens: None,
            temperature: None,
            avg_logprob: None,
            compression_ratio: None,
            no_speech_prob: None,
            speaker: None,
            refined_text: None,
        }]),
        refined_text: None,
    }
}

#[tokio::test]
#[ignore = "requires self-hosted vLLM endpoint at 140.118.122.126:8001"]
async fn gemma4_vllm_summary_full() {
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .is_test(true)
        .try_init();

    let config = vllm_config();
    let client = SummaryClient::new(config.clone()).expect("Failed to create summary client");

    let transcript = load_transcript();

    println!("========================================");
    println!("Gemma 4 Integration Test");
    println!("========================================");
    println!("Endpoint:  {}", config.base_url);
    println!("Model:     {}", config.model);
    println!("Temp:      {}", config.temperature);
    println!("Max tokens: {}", config.max_tokens);
    println!("Template:  full");
    println!("========================================");
    println!(
        "Transcript text (first 120 chars): {:?}",
        &transcript.text[..transcript.text.len().min(120)]
    );
    println!("========================================");

    let options = SummarizeOptions {
        template: SummaryTemplate::Full,
        format: SummaryFormat::Markdown,
        language: Some("en".to_string()),
        meeting: Default::default(),
    };

    let started = std::time::Instant::now();
    let result = client.summarize(&transcript, &options).await;
    let elapsed = started.elapsed();

    println!(
        "\n--- Summary completed in {:.2}s ---\n",
        elapsed.as_secs_f64()
    );

    let summary = result.expect("Summary generation failed");

    // --- Assertions ---
    assert!(
        !summary.content.is_empty(),
        "Summary content should not be empty"
    );
    println!(
        "Raw LLM output (first 500 chars):\n{}\n",
        &summary.content[..summary.content.len().min(500)]
    );

    println!("Parsed sections:");
    println!("  key_points ({}):", summary.key_points.len());
    for (i, kp) in summary.key_points.iter().take(5).enumerate() {
        println!("    {}. {}", i + 1, kp);
    }

    println!("  action_items ({}):", summary.action_items.len());
    for (i, ai) in summary.action_items.iter().take(5).enumerate() {
        println!("    {}. {}", i + 1, ai);
    }

    println!("  decisions ({}):", summary.decisions.len());
    for (i, d) in summary.decisions.iter().take(5).enumerate() {
        println!("    {}. {}", i + 1, d);
    }

    // Full template should populate at least one section
    let total_parsed =
        summary.key_points.len() + summary.action_items.len() + summary.decisions.len();
    assert!(
        total_parsed > 0,
        "Full template should parse at least one section (key_points, action_items, or decisions)"
    );

    // --- Save summary JSON ---
    let summary_json = serde_json::json!({
        "template": "full",
        "language": "en",
        "content": summary.content,
        "key_points": summary.key_points,
        "action_items": summary.action_items,
        "decisions": summary.decisions,
        "provider": "openai",
        "model": "gemma-4",
    });

    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/output");
    std::fs::create_dir_all(&output_dir).expect("Failed to create output dir");

    let output_path = output_dir.join("summary.json");
    std::fs::write(
        &output_path,
        serde_json::to_string_pretty(&summary_json).unwrap(),
    )
    .expect("Failed to write summary.json");

    println!("\nSummary saved to: {}", output_path.display());
    println!("Content length: {} chars", summary.content.len());
    println!(
        "Parsed: {} key points, {} action items, {} decisions",
        summary.key_points.len(),
        summary.action_items.len(),
        summary.decisions.len()
    );
    println!("========================================");
    println!("PASS: Gemma 4 integration test");
    println!("========================================");
}
