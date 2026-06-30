//! HTTP client for the standalone `diarize-server` microservice.
//!
//! The diarize service (crate `meeting-agent-diarize`) performs speaker
//! diarization on an audio file and merges the resulting speaker labels
//! with a Whisper transcript. This module is the caller-side client that
//! the import pipeline uses when `Config.diarize.enabled` is true.
//!
//! Resilience contract: [`DiarizeClient::diarize`] returns an error on any
//! failure (server down, non-2xx, decode error). Callers are expected to
//! log and proceed without speaker labels rather than fail the whole import.

use crate::transcription::TranscriptionResponse;
use anyhow::{Context, Result};
use reqwest::multipart;
use serde::Deserialize;
use std::path::Path;

/// Diarize-service response shape (mirrors `DiarizeResponse` in the diarize crate).
#[derive(Debug, Deserialize)]
pub struct DiarizeResponse {
    pub num_speakers: i32,
    pub segments: Vec<DiarizeSegment>,
}

#[derive(Debug, Deserialize)]
pub struct DiarizeSegment {
    pub start: f64,
    pub end: f64,
    pub speaker: i32,
    pub text: String,
}

/// HTTP client for the diarize-server.
pub struct DiarizeClient {
    client: reqwest::Client,
    base_url: String,
}

impl DiarizeClient {
    pub fn new(base_url: String, timeout: std::time::Duration) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .context("Failed to build HTTP client")?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    /// POST audio + Whisper transcript to `/v1/diarize`.
    ///
    /// `audio_path` must be an mp3 or wav file readable from disk.
    /// `transcript` is serialized as JSON and sent as the `transcript` part.
    /// `num_speakers` (if `Some`) overrides the server's auto-detection.
    pub async fn diarize(
        &self,
        audio_path: &Path,
        transcript: &TranscriptionResponse,
        num_speakers: Option<i32>,
    ) -> Result<DiarizeResponse> {
        log::debug!(
            "[diarize-client] preparing request for {}",
            audio_path.display()
        );

        let audio_bytes = tokio::fs::read(audio_path)
            .await
            .with_context(|| format!("Failed to read audio file: {}", audio_path.display()))?;

        let filename = audio_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("audio.mp3")
            .to_string();
        let mime = mime_for_extension(audio_path);

        let transcript_bytes =
            serde_json::to_vec(transcript).context("Failed to serialize transcript")?;
        let transcript_len = transcript_bytes.len();

        let segment_count = transcript.segments.as_ref().map(|s| s.len()).unwrap_or(0);
        log::debug!(
            "[diarize-client] transcript: {} segments, {} bytes JSON",
            segment_count,
            transcript_len
        );

        let form = multipart::Form::new()
            .part(
                "file",
                multipart::Part::bytes(audio_bytes)
                    .file_name(filename.clone())
                    .mime_str(mime)
                    .context("Invalid mime string")?,
            )
            .part(
                "transcript",
                multipart::Part::bytes(transcript_bytes)
                    .file_name("transcript.json")
                    .mime_str("application/json")
                    .context("Invalid mime string")?,
            );

        let form = if let Some(n) = num_speakers {
            log::debug!("[diarize-client] num_speakers override: {}", n);
            form.text("num_speakers", n.to_string())
        } else {
            form
        };

        let url = format!("{}/v1/diarize", self.base_url);
        log::info!(
            "[diarize-client] POST {} (audio={}, transcript bytes={})",
            url,
            filename,
            transcript_len
        );

        let request_start = std::time::Instant::now();
        let resp = self
            .client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .with_context(|| format!("Failed to send diarize request to {url}"))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            log::warn!("[diarize-client] server returned {}: {}", status, body);
            anyhow::bail!("diarize-server returned {}: {}", status, body);
        }

        let parsed: DiarizeResponse = resp
            .json()
            .await
            .context("Failed to parse diarize response")?;

        let request_time = request_start.elapsed().as_secs_f64();
        log::info!(
            "[diarize-client] response received: {} speakers, {} segments, took {:.2}s",
            parsed.num_speakers,
            parsed.segments.len(),
            request_time
        );

        Ok(parsed)
    }
}

/// Merge speaker labels from a [`DiarizeResponse`] back into a
/// [`TranscriptionResponse`].
///
/// The diarize service already performs max-overlap assignment and returns
/// one `DiarizeSegment` per Whisper segment (same start/end/text). We match
/// by index order — the service preserves Whisper segment ordering — and
/// copy the `speaker` field onto each `TranscriptSegment`. Any unmatched
/// Whisper segment keeps `speaker = None`.
pub fn merge_speakers(
    mut transcript: TranscriptionResponse,
    diarize: DiarizeResponse,
) -> TranscriptionResponse {
    log::debug!(
        "[diarize-client] merging {} diarize segments into transcript",
        diarize.segments.len()
    );

    let mut assigned = 0;
    let mut unmatched = 0;

    if let Some(segs) = transcript.segments.as_mut() {
        for (i, seg) in segs.iter_mut().enumerate() {
            if let Some(dseg) = diarize.segments.get(i) {
                // Sanity check: timestamps should match (within float slack).
                // We assign regardless; the service guarantees alignment by index.
                seg.speaker = Some(dseg.speaker);
                assigned += 1;
                log::debug!(
                    "[diarize-client] segment {}: assigned speaker {}",
                    i,
                    dseg.speaker
                );
            } else {
                unmatched += 1;
                log::debug!(
                    "[diarize-client] segment {}: no matching diarize segment",
                    i
                );
            }
        }
    }

    log::debug!(
        "[diarize-client] merge complete: {} assigned, {} unmatched",
        assigned,
        unmatched
    );

    transcript
}

/// Helper: resolve a mime type string from the audio file extension.
fn mime_for_extension(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .as_deref()
    {
        Some("wav") => "audio/wav",
        Some("mp3") => "audio/mpeg",
        // Fallback: the diarize server sniffs bytes anyway, but reqwest
        // requires a mime string on the part. mp3 is the post-conversion
        // format produced by `audio::convert_to_mp3`.
        _ => "audio/mpeg",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::TranscriptSegment;

    #[test]
    fn merge_assigns_speaker_by_index() {
        let transcript = TranscriptionResponse {
            text: "hello there".to_string(),
            language: None,
            duration: None,
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
                    speaker: None,
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
                    speaker: None,
                },
            ]),
        };
        let diarize = DiarizeResponse {
            num_speakers: 2,
            segments: vec![
                DiarizeSegment {
                    start: 0.0,
                    end: 2.0,
                    speaker: 0,
                    text: "hello".to_string(),
                },
                DiarizeSegment {
                    start: 2.0,
                    end: 4.0,
                    speaker: 1,
                    text: "there".to_string(),
                },
            ],
        };
        let merged = merge_speakers(transcript, diarize);
        let segs = merged.segments.unwrap();
        assert_eq!(segs[0].speaker, Some(0));
        assert_eq!(segs[1].speaker, Some(1));
    }

    #[test]
    fn merge_no_segments_leaves_transcript_unchanged() {
        let transcript = TranscriptionResponse {
            text: "hello".to_string(),
            language: None,
            duration: None,
            segments: None,
        };
        let diarize = DiarizeResponse {
            num_speakers: 0,
            segments: vec![],
        };
        let merged = merge_speakers(transcript, diarize);
        assert!(merged.segments.is_none());
    }

    #[test]
    fn merge_fewer_diarize_segments_leaves_rest_none() {
        let transcript = TranscriptionResponse {
            text: "a b".to_string(),
            language: None,
            duration: None,
            segments: Some(vec![
                TranscriptSegment {
                    id: 0,
                    start: 0.0,
                    end: 1.0,
                    text: "a".to_string(),
                    tokens: None,
                    temperature: None,
                    avg_logprob: None,
                    compression_ratio: None,
                    no_speech_prob: None,
                    speaker: None,
                },
                TranscriptSegment {
                    id: 1,
                    start: 1.0,
                    end: 2.0,
                    text: "b".to_string(),
                    tokens: None,
                    temperature: None,
                    avg_logprob: None,
                    compression_ratio: None,
                    no_speech_prob: None,
                    speaker: None,
                },
            ]),
        };
        let diarize = DiarizeResponse {
            num_speakers: 1,
            segments: vec![DiarizeSegment {
                start: 0.0,
                end: 1.0,
                speaker: 0,
                text: "a".to_string(),
            }],
        };
        let merged = merge_speakers(transcript, diarize);
        let segs = merged.segments.unwrap();
        assert_eq!(segs[0].speaker, Some(0));
        assert_eq!(segs[1].speaker, None);
    }

    #[test]
    fn mime_for_mp3_and_wav() {
        assert_eq!(
            mime_for_extension(std::path::Path::new("a.mp3")),
            "audio/mpeg"
        );
        assert_eq!(
            mime_for_extension(std::path::Path::new("a.wav")),
            "audio/wav"
        );
        assert_eq!(mime_for_extension(std::path::Path::new("a")), "audio/mpeg");
    }
}
