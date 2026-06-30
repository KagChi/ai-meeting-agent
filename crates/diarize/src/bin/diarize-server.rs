use std::net::SocketAddr;

use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;

use meeting_agent_diarize::{
    audio::decode_audio_to_f32_mono_16k, merge, validate_whisper_segments, DiarizeConfig,
    DiarizeError, DiarizeResponse, Result, SpeakerDiarizer, WhisperTranscript,
};

#[derive(Clone)]
struct AppState {
    diarizer: std::sync::Arc<SpeakerDiarizer>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env from CWD if present (ignored if absent). Errors (malformed
    // file) are logged but non-fatal so explicit env vars still work.
    if let Err(e) = dotenv::dotenv() {
        if !e.not_found() {
            eprintln!("warning: .env not loaded: {e}");
        }
    }

    let log_level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into());
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level)).init();

    let cfg = DiarizeConfig::from_env()?;
    log::info!(
        "loading diarizer: segmentation={}, embedding={}, num_clusters={}, threshold={}",
        cfg.segmentation_model.display(),
        cfg.embedding_model.display(),
        cfg.num_clusters,
        cfg.clustering_threshold
    );
    let diarizer = SpeakerDiarizer::new(&cfg)?;
    log::info!("diarizer loaded; sample_rate={}", diarizer.sample_rate());

    let state = AppState {
        diarizer: std::sync::Arc::new(diarizer),
    };

    let host = std::env::var("DIARIZE_HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port: u16 = std::env::var("DIARIZE_PORT")
        .ok()
        .map(|s| s.parse())
        .transpose()
        .map_err(|e: std::num::ParseIntError| {
            DiarizeError::ConfigError(format!("DIARIZE_PORT: {e}"))
        })?
        .unwrap_or(8002);
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| DiarizeError::ConfigError(format!("bad addr: {e}")))?;

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/diarize", post(diarize))
        .with_state(state);

    log::info!("diarize-server listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| DiarizeError::ConfigError(format!("bind: {e}")))?;
    axum::serve(listener, app)
        .await
        .map_err(|e| DiarizeError::ConfigError(format!("serve: {e}")))?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({ "status": "ok" })))
}

async fn diarize(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<DiarizeResponse>> {
    let mut audio_bytes: Option<Vec<u8>> = None;
    let mut transcript_bytes: Option<Vec<u8>> = None;
    let mut num_speakers_override: Option<i32> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| DiarizeError::TranscriptParseError(format!("multipart: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        let data = field
            .bytes()
            .await
            .map_err(|e| DiarizeError::TranscriptParseError(format!("read field: {e}")))?;
        match name.as_str() {
            "file" => audio_bytes = Some(data.to_vec()),
            "transcript" => transcript_bytes = Some(data.to_vec()),
            "num_speakers" => {
                let s = String::from_utf8_lossy(&data);
                let n: i32 = s
                    .trim()
                    .parse()
                    .map_err(|_| DiarizeError::InvalidNumSpeakers(s.to_string()))?;
                if n < 0 {
                    return Err(DiarizeError::InvalidNumSpeakers(s.to_string()));
                }
                num_speakers_override = Some(n);
            }
            _ => {}
        }
    }

    let audio_bytes = audio_bytes.ok_or(DiarizeError::MissingField("file"))?;
    let transcript_bytes = transcript_bytes.ok_or(DiarizeError::MissingField("transcript"))?;

    if !is_mp3(&audio_bytes) && !is_wav(&audio_bytes) {
        return Err(DiarizeError::UnsupportedAudioFormat(sniff_format(
            &audio_bytes,
        )));
    }

    let transcript: WhisperTranscript = serde_json::from_slice(&transcript_bytes)
        .map_err(|e| DiarizeError::TranscriptParseError(e.to_string()))?;
    let segments = validate_whisper_segments(transcript.segments);

    let samples = decode_audio_to_f32_mono_16k(&audio_bytes)?;

    if let Some(n) = num_speakers_override {
        if n > 0 {
            log::info!("num_speakers override={n}; rebuilding diarizer config");
            // Override requires new diarizer instance per request; build fresh.
            let mut cfg = DiarizeConfig::from_env()?;
            cfg.num_clusters = n;
            let diarizer = SpeakerDiarizer::new(&cfg)?;
            let (num_spk, spk_segments) = diarizer.process(&samples)?;
            let cleaned = merge(segments, spk_segments);
            return Ok(Json(DiarizeResponse {
                num_speakers: num_spk,
                segments: cleaned,
            }));
        }
    }

    let (num_speakers, speaker_segments) = state.diarizer.process(&samples)?;
    let cleaned = merge(segments, speaker_segments);
    Ok(Json(DiarizeResponse {
        num_speakers,
        segments: cleaned,
    }))
}

fn is_wav(b: &[u8]) -> bool {
    b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WAVE"
}

fn is_mp3(b: &[u8]) -> bool {
    // ID3 tag or MPEG frame sync
    (b.len() >= 3 && &b[0..3] == b"ID3") || (b.len() >= 2 && b[0] == 0xFF && (b[1] & 0xE0) == 0xE0)
}

fn sniff_format(b: &[u8]) -> String {
    if is_wav(b) {
        "wav".into()
    } else if is_mp3(b) {
        "mp3".into()
    } else if b.len() >= 4 {
        format!("unknown (magic={:02x?})", &b[0..4])
    } else {
        "unknown".into()
    }
}
