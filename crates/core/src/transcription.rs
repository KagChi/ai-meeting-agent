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
    /// Duration in seconds (server may return string or number)
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opt_f64_loose"
    )]
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

/// Deserialize `Option<f64>` tolerating number, numeric string, or null/missing.
/// Some servers (e.g. faster-whisper) return `duration` as a JSON string.
fn deserialize_opt_f64_loose<'de, D>(deserializer: D) -> std::result::Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<Value> = Option::deserialize(deserializer)?;
    match opt {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n
            .as_f64()
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid f64 number: {}", n))),
        Some(Value::String(s)) => s
            .parse::<f64>()
            .map(Some)
            .map_err(|e| serde::de::Error::custom(format!("invalid f64 string {:?}: {}", s, e))),
        Some(other) => Err(serde::de::Error::custom(format!(
            "expected number or string for f64, got {}",
            other
        ))),
    }
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
        let started = std::time::Instant::now();
        let file_path = Path::new(&request.file_path);
        log::info!("[transcribe] file={}", request.file_path);

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
            "{}/audio/transcriptions",
            self.config.base_url
        );

        log::info!("[transcribe] {} bytes → POST {}", file_bytes.len(), url);

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
        log::info!(
            "[transcribe] done in {:.1}s: {} segments, {} chars text, duration={:.1}s",
            started.elapsed().as_secs_f64(),
            transcription.segments.as_ref().map_or(0, |s| s.len()),
            transcription.text.chars().count(),
            transcription.duration.unwrap_or(0.0),
        );
        Ok(transcription)
    }

    /// Transcribe an audio file, splitting into chunks if it exceeds the
    /// configured `chunk_seconds`. Chunks are transcribed in parallel (up to
    /// `concurrency` at a time) and merged into a single response with
    /// segment timestamps offset by cumulative chunk durations.
    ///
    /// If `chunk_seconds <= 0.0` or the file duration is within the limit,
    /// delegates to `transcribe()` (single request).
    pub async fn transcribe_chunked(
        &self,
        request: TranscriptionRequest,
        chunk_seconds: f64,
        concurrency: usize,
    ) -> Result<TranscriptionResponse> {
        let started = std::time::Instant::now();

        // No chunking requested
        if chunk_seconds <= 0.0 {
            log::info!("[chunked] chunk_seconds=0 → single request (chunking disabled)");
            return self.transcribe(request).await;
        }

        // Probe source duration
        let path = std::path::Path::new(&request.file_path);
        let total_duration =
            crate::audio::probe_duration(path).context("Failed to probe source audio duration")?;

        // Within limit → single request
        if total_duration <= chunk_seconds {
            log::info!(
                "[chunked] audio={:.1}s limit={:.1}s → single request (under limit)",
                total_duration,
                chunk_seconds
            );
            return self.transcribe(request).await;
        }

        log::info!(
            "[chunked] audio={:.1}s limit={:.1}s → will chunk",
            total_duration,
            chunk_seconds
        );

        // Chunk via ffmpeg segment muxer
        let chunks =
            crate::audio::chunk_audio(path, chunk_seconds).context("Failed to chunk audio")?;
        let chunk_count = chunks.len();
        log::info!(
            "[chunked] split into {} chunks via ffmpeg (segment_time={}s, copy codec)",
            chunk_count,
            chunk_seconds
        );

        // Probe each chunk's actual duration upfront (deterministic offsets)
        let mut chunk_durations: Vec<f64> = Vec::with_capacity(chunk_count);
        for (i, chunk) in chunks.iter().enumerate() {
            let d = crate::audio::probe_duration(chunk)
                .with_context(|| format!("Failed to probe chunk {} duration", i))?;
            chunk_durations.push(d);
        }
        log::info!(
            "[chunked] probed {} chunk durations: [{}] (total={:.1}s)",
            chunk_count,
            chunk_durations
                .iter()
                .map(|d| format!("{:.1}s", d))
                .collect::<Vec<_>>()
                .join(", "),
            chunk_durations.iter().sum::<f64>(),
        );

        // Parallel transcription with concurrency cap.
        // Each task returns (index, response) so we can restore order after
        // JoinSet (which returns results in completion order, not spawn order).
        let concurrency = concurrency.max(1);
        log::info!(
            "[chunked] transcribing {} chunks (parallel, max {} concurrent)",
            chunk_count,
            concurrency
        );
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
        let mut tasks = tokio::task::JoinSet::new();

        for (i, chunk) in chunks.iter().enumerate() {
            let permit = semaphore.clone();
            let chunk_request = TranscriptionRequest {
                file_path: chunk.to_string_lossy().to_string(),
                response_format: request.response_format.clone(),
                language: request.language.clone(),
                prompt: request.prompt.clone(),
                temperature: request.temperature,
            };
            let client = self.client.clone();
            let config = self.config.clone();
            let total = chunk_count;
            let chunk_dur = chunk_durations[i];
            tasks.spawn(async move {
                let _permit = permit.acquire().await.expect("semaphore closed");
                let chunk_start = std::time::Instant::now();
                log::info!(
                    "[chunked]   → chunk {}/{} ({:.1}s) starting",
                    i + 1,
                    total,
                    chunk_dur
                );
                let tmp_client = TranscriptionClient { client, config };
                match tmp_client.transcribe(chunk_request).await {
                    Ok(resp) => {
                        log::info!(
                            "[chunked]   → chunk {}/{} ({:.1}s) done in {:.1}s ({} segments)",
                            i + 1,
                            total,
                            chunk_dur,
                            chunk_start.elapsed().as_secs_f64(),
                            resp.segments.as_ref().map_or(0, |s| s.len()),
                        );
                        Ok::<(usize, TranscriptionResponse), anyhow::Error>((i, resp))
                    }
                    Err(e) => {
                        log::error!(
                            "[chunked]   → chunk {}/{} ({:.1}s) FAILED in {:.1}s: {}",
                            i + 1,
                            total,
                            chunk_dur,
                            chunk_start.elapsed().as_secs_f64(),
                            e
                        );
                        Err(e)
                    }
                }
            });
        }

        // Collect (index, response) and restore chronological order.
        let mut results: Vec<(usize, TranscriptionResponse)> = Vec::with_capacity(chunk_count);
        while let Some(res) = tasks.join_next().await {
            match res {
                Ok(Ok(pair)) => results.push(pair),
                Ok(Err(e)) => return Err(e).context("Chunk transcription failed"),
                Err(join_err) if join_err.is_cancelled() => continue,
                Err(join_err) => return Err(anyhow::anyhow!("Chunk task panicked: {}", join_err)),
            }
        }
        results.sort_by_key(|(i, _)| *i);
        log::info!("[chunked] all {} chunks transcribed", chunk_count);

        // Cleanup chunk temp files (best-effort)
        let mut cleaned = 0usize;
        for chunk in &chunks {
            if let Err(e) = std::fs::remove_file(chunk) {
                log::warn!("[chunked] failed to delete chunk {:?}: {}", chunk, e);
            } else {
                cleaned += 1;
            }
        }
        log::info!(
            "[chunked] cleaned up {}/{} temp chunk files",
            cleaned,
            chunk_count
        );

        let responses: Vec<TranscriptionResponse> = results.into_iter().map(|(_, r)| r).collect();
        let merged = merge_chunk_responses(responses, chunk_durations);
        log::info!(
            "[chunked] merged: {} segments, {} chars text, {:.1}s total duration",
            merged.segments.as_ref().map_or(0, |s| s.len()),
            merged.text.chars().count(),
            merged.duration.unwrap_or(0.0),
        );
        log::info!(
            "[chunked] total elapsed: {:.1}s",
            started.elapsed().as_secs_f64()
        );
        Ok(merged)
    }
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
                .mime_str("application/octet-stream")?;

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

            if let Some(ref prompt) = request.prompt {
                form = form.text("prompt", prompt.clone());
            } else if let Some(ref lang) = request.language {
                let default_prompt = match lang.as_str() {
                    "id" => Some("Berikut adalah transkripsi percakapan yang jelas dan terstruktur:"),
                    "en" => Some("The following is a clear and structured transcription:"),
                    "ja" => Some("以下は明確で構造化された文字起こしです。"),
                    "ko" => Some("다음은 명확하고 구조화된 대화 녹취록입니다:"),
                    "zh" => Some("以下是清晰且结构化的转录内容："),
                    _ => None, 
                };

                if let Some(p) = default_prompt {
                    form = form.text("prompt", p);
                }
            }

            if let Some(temperature) = request.temperature {
                form = form.text("temperature", temperature.to_string());
            } else {
                form = form.text("temperature", "0.0");
            }

            form = form.text("timestamp_granularities[]", "word");

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

/// Merge per-chunk transcription responses into a single response.
///
/// - Segment timestamps are offset by cumulative chunk durations so they
///   reflect absolute time in the original audio.
/// - Segment IDs are renumbered globally (0, 1, 2, ...) to avoid collisions
///   (each chunk's segments restart at 0).
/// - Text is concatenated with a space separator.
/// - Duration is the sum of all chunk durations.
/// - Language is taken from the first chunk that reports one.
fn merge_chunk_responses(
    responses: Vec<TranscriptionResponse>,
    chunk_durations: Vec<f64>,
) -> TranscriptionResponse {
    let mut text_parts: Vec<String> = Vec::with_capacity(responses.len());
    let mut all_segments: Vec<TranscriptSegment> = Vec::new();
    let mut language: Option<String> = None;
    let mut total_duration: f64 = 0.0;
    let mut global_id: u32 = 0;
    let mut offset: f64 = 0.0;

    for (i, resp) in responses.iter().enumerate() {
        // Offset for this chunk = sum of prior chunk durations
        let chunk_offset = if i < chunk_durations.len() {
            chunk_durations[i]
        } else {
            // Fallback: use response duration if probed duration missing
            resp.duration.unwrap_or(0.0)
        };

        if !resp.text.trim().is_empty() {
            text_parts.push(resp.text.trim().to_string());
        }

        if language.is_none() {
            language = resp.language.clone();
        }

        if let Some(segments) = &resp.segments {
            for seg in segments {
                all_segments.push(TranscriptSegment {
                    id: global_id,
                    start: seg.start + offset,
                    end: seg.end + offset,
                    text: seg.text.clone(),
                    tokens: seg.tokens.clone(),
                    temperature: seg.temperature,
                    avg_logprob: seg.avg_logprob,
                    compression_ratio: seg.compression_ratio,
                    no_speech_prob: seg.no_speech_prob,
                });
                global_id += 1;
            }
        }

        offset += chunk_offset;
        total_duration += chunk_offset;
    }

    TranscriptionResponse {
        text: text_parts.join(" "),
        language,
        duration: Some(total_duration),
        segments: if all_segments.is_empty() {
            None
        } else {
            Some(all_segments)
        },
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

    #[test]
    fn test_parse_openai_verbose_json_string_duration() {
        // Some servers (faster-whisper) return `duration` as a JSON string.
        let raw = r#"{
            "text": "hello world",
            "language": "ja",
            "duration": "30.0234375",
            "segments": [
                {"id": 0, "start": 0.0, "end": 1.0, "text": "hello"},
                {"id": 1, "start": 1.0, "end": 2.0, "text": "world"}
            ]
        }"#;
        let resp = parse_response(raw).expect("parse failed");
        assert_eq!(resp.text, "hello world");
        assert_eq!(resp.language.as_deref(), Some("ja"));
        assert!((resp.duration.unwrap() - 30.0234375).abs() < 1e-9);
        let segments = resp.segments.expect("no segments");
        assert_eq!(segments.len(), 2);
    }

    #[test]
    fn test_merge_chunk_responses_offsets_and_renumbers() {
        let chunk1 = TranscriptionResponse {
            text: "hello there".to_string(),
            language: Some("en".to_string()),
            duration: Some(600.0),
            segments: Some(vec![
                TranscriptSegment {
                    id: 0,
                    start: 0.0,
                    end: 2.0,
                    text: "hello".to_string(),
                    tokens: None,
                    temperature: None,
                    avg_logprob: None,
                    compression_ratio: None,
                    no_speech_prob: None,
                },
                TranscriptSegment {
                    id: 1,
                    start: 2.0,
                    end: 4.0,
                    text: "there".to_string(),
                    tokens: None,
                    temperature: None,
                    avg_logprob: None,
                    compression_ratio: None,
                    no_speech_prob: None,
                },
            ]),
        };
        let chunk2 = TranscriptionResponse {
            text: "general kenobi".to_string(),
            language: Some("en".to_string()),
            duration: Some(500.0),
            segments: Some(vec![
                TranscriptSegment {
                    id: 0,
                    start: 0.0,
                    end: 1.5,
                    text: "general".to_string(),
                    tokens: None,
                    temperature: None,
                    avg_logprob: None,
                    compression_ratio: None,
                    no_speech_prob: None,
                },
                TranscriptSegment {
                    id: 1,
                    start: 1.5,
                    end: 3.0,
                    text: "kenobi".to_string(),
                    tokens: None,
                    temperature: None,
                    avg_logprob: None,
                    compression_ratio: None,
                    no_speech_prob: None,
                },
            ]),
        };

        let merged = merge_chunk_responses(vec![chunk1, chunk2], vec![600.0, 500.0]);

        assert_eq!(merged.text, "hello there general kenobi");
        assert_eq!(merged.language.as_deref(), Some("en"));
        assert!((merged.duration.unwrap() - 1100.0).abs() < 1e-9);

        let segs = merged.segments.expect("no segments");
        assert_eq!(segs.len(), 4);
        // Global renumbering
        assert_eq!(segs[0].id, 0);
        assert_eq!(segs[1].id, 1);
        assert_eq!(segs[2].id, 2);
        assert_eq!(segs[3].id, 3);
        // Chunk 1 timestamps unchanged
        assert!((segs[0].start - 0.0).abs() < 1e-9);
        assert!((segs[0].end - 2.0).abs() < 1e-9);
        assert!((segs[1].start - 2.0).abs() < 1e-9);
        assert!((segs[1].end - 4.0).abs() < 1e-9);
        // Chunk 2 timestamps offset by 600.0
        assert!((segs[2].start - 600.0).abs() < 1e-9);
        assert!((segs[2].end - 601.5).abs() < 1e-9);
        assert!((segs[3].start - 601.5).abs() < 1e-9);
        assert!((segs[3].end - 603.0).abs() < 1e-9);
    }

    #[test]
    fn test_merge_chunk_responses_single_chunk() {
        let chunk = TranscriptionResponse {
            text: "only chunk".to_string(),
            language: Some("ja".to_string()),
            duration: Some(100.0),
            segments: Some(vec![TranscriptSegment {
                id: 0,
                start: 5.0,
                end: 10.0,
                text: "only".to_string(),
                tokens: None,
                temperature: None,
                avg_logprob: None,
                compression_ratio: None,
                no_speech_prob: None,
            }]),
        };
        let merged = merge_chunk_responses(vec![chunk.clone()], vec![100.0]);
        assert_eq!(merged.text, "only chunk");
        assert_eq!(merged.language.as_deref(), Some("ja"));
        assert!((merged.duration.unwrap() - 100.0).abs() < 1e-9);
        let segs = merged.segments.expect("no segments");
        assert_eq!(segs.len(), 1);
        // Offset 0 → unchanged
        assert!((segs[0].start - 5.0).abs() < 1e-9);
        assert!((segs[0].end - 10.0).abs() < 1e-9);
    }

    #[test]
    fn test_merge_chunk_responses_empty_segments() {
        let chunk1 = TranscriptionResponse {
            text: "no segments here".to_string(),
            language: None,
            duration: Some(50.0),
            segments: None,
        };
        let chunk2 = TranscriptionResponse {
            text: "neither here".to_string(),
            language: Some("en".to_string()),
            duration: Some(50.0),
            segments: None,
        };
        let merged = merge_chunk_responses(vec![chunk1, chunk2], vec![50.0, 50.0]);
        assert_eq!(merged.text, "no segments here neither here");
        assert_eq!(merged.language.as_deref(), Some("en"));
        assert!((merged.duration.unwrap() - 100.0).abs() < 1e-9);
        assert!(merged.segments.is_none());
    }
}
