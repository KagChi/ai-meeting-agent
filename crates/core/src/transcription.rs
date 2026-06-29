//! Transcription client for OpenAI-compatible APIs

use crate::config::TranscriptionConfig;
use anyhow::{Context, Result};
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

/// Transcription client that communicates with OpenAI-compatible APIs
pub struct TranscriptionClient {
    client: reqwest::Client,
    config: TranscriptionConfig,
}

/// Request parameters for transcription
#[derive(Debug, Clone)]
pub struct TranscriptionRequest {
    /// Audio file path
    pub file_path: String,
    /// Response format (json, verbose_json, text, srt, vtt)
    pub response_format: Option<String>,
    /// Language code (ISO-639-1, e.g., "en", "zh")
    pub language: Option<String>,
    /// Optional prompt for context/spelling guidance
    pub prompt: Option<String>,
    /// Temperature (0.0-1.0)
    pub temperature: Option<f32>,
}

/// Response from transcription API (verbose_json format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResponse {
    /// The transcribed text
    pub text: String,
    /// Language detected/used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Duration in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    /// Transcript segments (only in verbose_json format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segments: Option<Vec<TranscriptSegment>>,
}

/// A segment of the transcript with timing information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    #[serde(default)]
    pub id: u32,
    pub start: f64,
    pub end: f64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<Vec<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_logprob: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compression_ratio: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_speech_prob: Option<f32>,
}

/// Parse a transcription API response.
///
/// Handles three shapes:
/// - OpenAI `verbose_json`: `{ text, language, duration, segments: [{ id, start, end, text, ... }] }`
/// - faster-whisper: `{ text, chunks: [{ text, timestamp: [start, end] }] }`
/// - text-only or unknown: best-effort deserialize
fn parse_response(raw: &str) -> Result<TranscriptionResponse> {
    let val: Value = serde_json::from_str(raw)
        .with_context(|| format!("Failed to parse JSON: {}", &raw[..raw.len().min(200)]))?;
    log::debug!(
        "transcription raw response: {}",
        serde_json::to_string_pretty(&val).unwrap_or_else(|_| raw.to_string())
    );

    if val.get("segments").is_some() {
        // OpenAI verbose_json shape
        serde_json::from_value::<TranscriptionResponse>(val)
            .context("Failed to parse OpenAI transcription response")
    } else if val.get("chunks").is_some() {
        // faster-whisper shape: { text, chunks: [{ text, timestamp: [start, end] }] }
        let text = val
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        let chunks = val["chunks"].as_array().cloned().unwrap_or_default();
        let segments: Vec<TranscriptSegment> = chunks
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let ts = c
                    .get("timestamp")
                    .and_then(|t| t.as_array())
                    .filter(|a| a.len() == 2);
                let start = ts.and_then(|a| a[0].as_f64()).unwrap_or(0.0);
                // null end → zero-duration segment
                let end = ts.and_then(|a| a[1].as_f64()).unwrap_or(start);
                TranscriptSegment {
                    id: i as u32,
                    start,
                    end,
                    text: c
                        .get("text")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string(),
                    tokens: None,
                    temperature: None,
                    avg_logprob: None,
                    compression_ratio: None,
                    no_speech_prob: None,
                }
            })
            .collect();
        Ok(TranscriptionResponse {
            text,
            language: None,
            duration: None,
            segments: Some(segments),
        })
    } else {
        // text-only or unknown — best-effort deserialize
        serde_json::from_value::<TranscriptionResponse>(val)
            .context("Failed to parse transcription response")
    }
}

impl TranscriptionClient {
    /// Create a new transcription client
    pub fn new(config: TranscriptionConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300)) // 5 minutes timeout for large files
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self { client, config })
    }

    /// Transcribe an audio file
    pub async fn transcribe(&self, request: TranscriptionRequest) -> Result<TranscriptionResponse> {
        log::debug!("Starting transcription for file: {}", request.file_path);
        let file_path = Path::new(&request.file_path);

        if !file_path.exists() {
            anyhow::bail!("Audio file not found: {}", request.file_path);
        }

        // Read the audio file once
        let file_bytes = tokio::fs::read(file_path)
            .await
            .context("Failed to read audio file")?;

        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio.m4a")
            .to_string();

        // Build the API URL (OpenAI-compatible: {base}/audio/transcriptions)
        let url = format!(
            "{}/audio/transcriptions?language=auto",
            self.config.base_url
        );

        // Prepare authorization header
        let api_key = self
            .config
            .api_key
            .as_ref()
            .context("TRANSCRIPTION_API_KEY is required but not set")?;

        // Send request with retry logic
        let response = self
            .send_with_retry(&url, api_key, &file_bytes, &file_name, &request)
            .await?;

        // Parse response (OpenAI verbose_json OR faster-whisper chunks)
        let raw = response
            .text()
            .await
            .context("Failed to read response body")?;
        let transcription = parse_response(&raw)?;
        Ok(transcription)
    }

    /// Send request with retry logic for transient failures
    async fn send_with_retry(
        &self,
        url: &str,
        api_key: &str,
        file_bytes: &[u8],
        file_name: &str,
        request: &TranscriptionRequest,
    ) -> Result<reqwest::Response> {
        let max_retries = 3;
        let mut last_error = None;

        for attempt in 1..=max_retries {
            // Build fresh multipart form for each attempt
            let file_part = multipart::Part::bytes(file_bytes.to_vec())
                .file_name(file_name.to_string())
                .mime_str("audio/m4a")?;

            let mut form = multipart::Form::new()
                .part("file", file_part)
                .text("model", self.config.model.clone());

            // Add optional parameters
            if let Some(ref format) = request.response_format {
                form = form.text("response_format", format.clone());
            } else {
                // Default to verbose_json for rich transcript data
                form = form.text("response_format", "verbose_json");
            }

            if let Some(ref language) = request.language {
                form = form.text("language", language.clone());
            }

            if let Some(ref prompt) = request.prompt {
                form = form.text("prompt", prompt.clone());
            }

            if let Some(temperature) = request.temperature {
                form = form.text("temperature", temperature.to_string());
            }

            let response = self
                .client
                .post(url)
                .header("Authorization", format!("Bearer {}", api_key))
                .multipart(form)
                .send()
                .await;

            match response {
                Ok(resp) if resp.status().is_success() => {
                    return Ok(resp);
                }
                Ok(resp) if resp.status().is_server_error() && attempt < max_retries => {
                    // Retry on 5xx errors
                    let status = resp.status();
                    let error_text = resp.text().await.unwrap_or_default();
                    log::warn!(
                        "Attempt {}/{} failed with status {}: {}. Retrying...",
                        attempt,
                        max_retries,
                        status,
                        error_text
                    );
                    last_error = Some(anyhow::anyhow!("Server error {}: {}", status, error_text));
                    tokio::time::sleep(Duration::from_secs(2u64.pow(attempt - 1))).await;
                }
                Ok(resp) => {
                    // Client error (4xx) or final retry exhausted
                    let status = resp.status();
                    let error_text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("API request failed with status {}: {}", status, error_text);
                }
                Err(e) if attempt < max_retries => {
                    // Network error, retry
                    log::warn!(
                        "Attempt {}/{} failed: {}. Retrying...",
                        attempt,
                        max_retries,
                        e
                    );
                    last_error = Some(anyhow::anyhow!("Network error: {}", e));
                    tokio::time::sleep(Duration::from_secs(2u64.pow(attempt - 1))).await;
                }
                Err(e) => {
                    // Final retry exhausted
                    anyhow::bail!("Network error after {} attempts: {}", max_retries, e);
                }
            }
        }

        Err(last_error
            .unwrap_or_else(|| anyhow::anyhow!("Request failed after {} retries", max_retries)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transcription_request_creation() {
        let request = TranscriptionRequest {
            file_path: "test.m4a".to_string(),
            response_format: Some("verbose_json".to_string()),
            language: Some("en".to_string()),
            prompt: None,
            temperature: Some(0.0),
        };

        assert_eq!(request.file_path, "test.m4a");
        assert_eq!(request.response_format, Some("verbose_json".to_string()));
        assert_eq!(request.language, Some("en".to_string()));
    }

    #[test]
    fn test_parse_faster_whisper_chunks() {
        let raw = r#"{
            "chunks": [
                {
                    "text": "ああ いつものように",
                    "timestamp": [0.88, 4.0]
                },
                {
                    "text": "過ぎる日々に あくびが出る",
                    "timestamp": [4.0, 8.5]
                }
            ],
            "text": "ああ いつものように 過ぎる日々に あくびが出る"
        }"#;
        let resp = parse_response(raw).expect("parse failed");
        assert_eq!(resp.text, "ああ いつものように 過ぎる日々に あくびが出る");
        assert_eq!(resp.language, None);
        assert_eq!(resp.duration, None);
        let segments = resp.segments.expect("no segments");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].id, 0);
        assert!((segments[0].start - 0.88).abs() < 1e-9);
        assert!((segments[0].end - 4.0).abs() < 1e-9);
        assert_eq!(segments[0].text, "ああ いつものように");
        assert_eq!(segments[1].id, 1);
        assert!((segments[1].start - 4.0).abs() < 1e-9);
        assert!((segments[1].end - 8.5).abs() < 1e-9);
        assert_eq!(segments[1].text, "過ぎる日々に あくびが出る");
    }

    #[test]
    fn test_parse_faster_whisper_null_end() {
        // Last chunk with null end timestamp → zero-duration
        let raw = r#"{
            "chunks": [
                {"text": "final segment", "timestamp": [10.0, null]}
            ],
            "text": "final segment"
        }"#;
        let resp = parse_response(raw).expect("parse failed");
        let segments = resp.segments.expect("no segments");
        assert_eq!(segments.len(), 1);
        assert!((segments[0].start - 10.0).abs() < 1e-9);
        // null end falls back to start (zero-duration)
        assert!((segments[0].end - 10.0).abs() < 1e-9);
    }

    #[test]
    fn test_parse_openai_verbose_json() {
        let raw = r#"{
            "text": "hello world",
            "language": "en",
            "duration": 5.5,
            "segments": [
                {"id": 0, "start": 0.0, "end": 2.5, "text": "hello"},
                {"id": 1, "start": 2.5, "end": 5.5, "text": "world"}
            ]
        }"#;
        let resp = parse_response(raw).expect("parse failed");
        assert_eq!(resp.text, "hello world");
        assert_eq!(resp.language.as_deref(), Some("en"));
        assert!((resp.duration.unwrap() - 5.5).abs() < 1e-9);
        let segments = resp.segments.expect("no segments");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "hello");
        assert_eq!(segments[1].text, "world");
    }

    #[test]
    fn test_parse_text_only() {
        let raw = r#"{"text": "just plain text"}"#;
        let resp = parse_response(raw).expect("parse failed");
        assert_eq!(resp.text, "just plain text");
        assert!(resp.segments.is_none());
        assert!(resp.language.is_none());
        assert!(resp.duration.is_none());
    }
}
