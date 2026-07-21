//! Speaker diarization via HTTP service or in-process `speakrs`.
//!
//! Two modes:
//! - **HTTP mode**: When `service_url` is set, sends audio + transcript to
//!   remote diarization service via multipart POST.
//! - **In-process mode**: When `service_url` is None, wraps
//!   `speakrs::OwnedDiarizationPipeline` in a lazily-initialized `Mutex`
//!   so the model loads once per process. The pipeline is CPU-bound and
//!   blocking, so [`Diarizer::diarize`] offloads work to a `spawn_blocking`
//!   task.
//!
//! Resilience contract: [`Diarizer::diarize`] returns an error on any
//! failure (model load, decode, pipeline, or HTTP). Callers are expected
//! to log and proceed without speaker labels rather than fail the whole
//! import.

pub mod audio;
pub mod embed;
pub mod error;
pub mod merge;
pub mod models;
pub mod models_download;

pub use error::{DiarizeError, Result};
pub use merge::{merge as merge_segments, CleanedSegment};
pub use models::{WhisperSegment, WhisperTranscript};
pub use models_download::{ensure_pretrained_models, resolve_hf_hub_cache};
pub use embed::{
    cosine_similarity, embed_audio_file, embed_samples, l2_normalize, mean_pool_l2, EMBED_DIM,
    EMBED_MODEL_ID,
};

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::Context;
use speakrs::{ExecutionMode, OwnedDiarizationPipeline};

use crate::config::DiarizeConfig;
use crate::diarize::audio::decode_audio_from_file;
use crate::diarize::merge::merge;
use crate::transcription::TranscriptionResponse;

/// Local configuration for the in-process diarizer.
///
/// Not to be confused with `crate::config::DiarizeConfig`, which includes
/// HTTP service URL and is the public API.
#[derive(Debug, Clone)]
struct InProcessConfig {
    execution_mode: ExecutionMode,
    /// Optional local model directory. `None` = download via
    /// `speakrs` `online` feature on first use.
    model_dir: Option<PathBuf>,
}

/// HTTP service response structure
#[derive(Debug, serde::Deserialize)]
struct DiarizeServiceResponse {
    success: bool,
    transcript: TranscriptionResponse,
    error: Option<String>,
}

/// Shared HTTP client for the diarization service.
///
/// Built once with a 2h timeout (long meetings up to 7200s) and a connection
/// pool that keeps the diarize-service link warm across imports.
fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(7200))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .expect("failed to build diarize reqwest client")
    })
}

/// Call remote diarization service via HTTP multipart POST.
///
/// Streams the audio file from disk (no full byte buffer in RAM) and ships the
/// transcript JSON alongside it. The server writes the stream to a temp file
/// and runs speakrs over it.
async fn call_service(
    audio_path: &Path,
    transcript: &TranscriptionResponse,
    service_url: &str,
) -> anyhow::Result<TranscriptionResponse> {
    let transcript_json = serde_json::to_string(transcript)
        .context("Failed to serialize transcript for HTTP diarization")?;

    let file = tokio::fs::File::open(audio_path)
        .await
        .with_context(|| format!("Failed to open audio file: {}", audio_path.display()))?;

    let file_name = audio_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.wav")
        .to_string();

    let audio_len = file.metadata().await.map(|m| m.len()).unwrap_or(0);

    // reqwest::Body implements From<tokio::fs::File> directly, so the file is
    // streamed to the server without buffering the full audio in RAM.
    let audio_part = reqwest::multipart::Part::stream_with_length(file, audio_len)
        .file_name(file_name)
        .mime_str("audio/wav")?;

    let form = reqwest::multipart::Form::new()
        .part("audio", audio_part)
        .part(
            "transcript",
            reqwest::multipart::Part::text(transcript_json),
        );

    let url = format!("{}/v1/diarize", service_url.trim_end_matches('/'));
    let response = http_client()
        .post(&url)
        .multipart(form)
        .send()
        .await
        .context("Failed to send request to diarization service")?;

    if !response.status().is_success() {
        anyhow::bail!(
            "Diarization service returned status {}: {}",
            response.status(),
            response.text().await.unwrap_or_default()
        );
    }

    let result: DiarizeServiceResponse = response
        .json()
        .await
        .context("Failed to parse diarization service response")?;

    if !result.success {
        anyhow::bail!(
            "Diarization service reported failure: {}",
            result.error.unwrap_or_else(|| "Unknown error".to_string())
        );
    }

    Ok(result.transcript)
}

/// Lazily-initialized, process-wide diarization pipeline.
/// `Mutex` is required because (a) `run()` needs `&mut` and (b) the
/// underlying ONNX Runtime `RunOptions` is not `Sync`.
static DIARIZER: OnceLock<Mutex<Option<OwnedDiarizationPipeline>>> = OnceLock::new();

/// Entry point for in-process speaker diarization.
pub struct Diarizer;

/// Parse a string into an [`ExecutionMode`]. Unknown values fall back
/// to `Cpu` (portable) and are logged.
pub fn parse_execution_mode(s: &str) -> ExecutionMode {
    match s.to_lowercase().as_str() {
        "cpu" => ExecutionMode::Cpu,
        "coreml" => ExecutionMode::CoreMl,
        "coreml-fast" => ExecutionMode::CoreMlFast,
        "cuda" => ExecutionMode::Cuda,
        "cuda-fast" => ExecutionMode::CudaFast,
        "migraphx" => ExecutionMode::MiGraphX,
        other => {
            log::warn!("[diarize] unknown execution_mode '{other}', falling back to cpu");
            ExecutionMode::Cpu
        }
    }
}

/// Detect the best execution mode for the current platform, prioritizing
/// GPU acceleration when available.
///
/// Platform-specific priority order:
/// - macOS: `CoreMlFast` → `CoreMl` → `Cpu`
/// - Linux/Windows: `CudaFast` → `Cuda` → `MiGraphX` → `Cpu`
///
/// Returns the first mode in the priority list. Actual availability is
/// validated during pipeline initialization in [`with_pipeline`], which
/// will attempt CPU fallback if the GPU mode fails.
fn detect_best_execution_mode() -> ExecutionMode {
    #[cfg(target_os = "macos")]
    {
        log::info!("[diarize] auto-detecting execution mode (macOS): trying CoreML first");
        ExecutionMode::CoreMlFast
    }

    #[cfg(not(target_os = "macos"))]
    {
        log::info!("[diarize] auto-detecting execution mode (non-macOS): trying CUDA first");
        ExecutionMode::CudaFast
    }
}

/// Resolve the execution mode from a config string.
///
/// - `"auto"` → platform-specific GPU priority via [`detect_best_execution_mode`]
/// - Explicit mode string → parsed directly via [`parse_execution_mode`]
pub fn resolve_execution_mode(config_string: &str) -> ExecutionMode {
    if config_string.to_lowercase() == "auto" {
        detect_best_execution_mode()
    } else {
        parse_execution_mode(config_string)
    }
}

impl Diarizer {
    /// Run diarization on an audio file + Whisper transcript, returning
    /// a clone of the transcript with `speaker` labels assigned to each
    /// segment.
    ///
    /// Routes by `cfg.service_url`:
    /// - `Some(url)` → HTTP multipart POST to remote service
    /// - `None` → in-process speakrs (via `spawn_blocking`)
    ///
    /// Callers keep the original transcript on error.
    pub async fn diarize(
        audio_path: &Path,
        transcript: &TranscriptionResponse,
        cfg: &DiarizeConfig,
    ) -> anyhow::Result<TranscriptionResponse> {
        if let Some(service_url) = cfg.service_url.as_deref() {
            log::info!("[diarize] using HTTP service at {service_url}");
            return call_service(audio_path, transcript, service_url).await;
        }

        let in_process = InProcessConfig {
            execution_mode: resolve_execution_mode(&cfg.execution_mode),
            model_dir: cfg.model_dir.clone(),
        };
        let audio_path = audio_path.to_path_buf();
        let transcript = transcript.clone();

        // speakrs ONNX/CUDA inference uses deep C call stacks (cuDNN conv
        // kernels + ndarray Array3 stack allocs) that exceed tokio's default
        // 2 MB blocking-thread stack. Run on a dedicated thread with 8 MB.
        let handle = std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(move || run_in_process(&audio_path, &transcript, &in_process))
            .context("failed to spawn diarize thread")?;
        handle
            .join()
            .map_err(|_| anyhow::anyhow!("diarize thread panicked"))?
    }
}

fn run_in_process(
    audio_path: &Path,
    transcript: &TranscriptionResponse,
    cfg: &InProcessConfig,
) -> anyhow::Result<TranscriptionResponse> {
    // Build WhisperSegment vec from the transcript for the merge step.
    let whisper_segments: Vec<WhisperSegment> = transcript
        .segments
        .as_ref()
        .map(|segs| {
            segs.iter()
                .map(|s| WhisperSegment {
                    start: s.start,
                    end: s.end,
                    text: s.text.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    log::info!("[diarize] decoding audio: {}", audio_path.display());
    let samples = decode_audio_from_file(audio_path)
        .with_context(|| format!("Failed to decode audio file: {}", audio_path.display()))?;
    log::info!(
        "[diarize] decoded {} samples ({:.2}s)",
        samples.len(),
        samples.len() as f64 / 16000.0
    );

    let result = with_pipeline(cfg, |pipeline| {
        log::info!(
            "[diarize] running speakrs pipeline (mode={})",
            cfg.execution_mode
        );
        let process_start = std::time::Instant::now();
        let result = pipeline.run(&samples).map_err(DiarizeError::from)?;
        log::info!(
            "[diarize] pipeline complete: {} segments, took {:.2}s",
            result.segments.len(),
            process_start.elapsed().as_secs_f64()
        );
        Ok(result)
    })?;

    let cleaned = merge(whisper_segments, &result.segments);

    // Copy speaker labels onto a clone of the transcript.
    let mut out = transcript.clone();
    let mut labeled = 0usize;
    let mut total = 0usize;
    if let Some(segs) = out.segments.as_mut() {
        total = segs.len();
        for (i, seg) in segs.iter_mut().enumerate() {
            if let Some(c) = cleaned.get(i) {
                seg.speaker = c.speaker.clone();
                if seg.speaker.is_some() {
                    labeled += 1;
                }
            }
        }
    }

    if total == 0 {
        log::warn!("[diarize] no transcript segments to label");
    } else if labeled == 0 {
        log::warn!(
            "[diarize] labeled 0/{} transcript segments (no time overlap with {} speaker turns)",
            total,
            result.segments.len()
        );
    } else {
        log::info!(
            "[diarize] labeled {}/{} transcript segments ({} speaker turns)",
            labeled,
            total,
            result.segments.len()
        );
    }

    Ok(out)
}

/// Try to initialize the pipeline with the given mode, with optional CPU
/// fallback if the mode is GPU-based and initialization fails.
///
/// - If `mode` is `Cpu`, attempts initialization and returns the result.
/// - If `mode` is a GPU mode (CoreML, CUDA, MIGraphX) and initialization
///   fails, logs a warning and retries with `Cpu`.
fn try_init_pipeline_with_fallback(
    model_dir: &Option<PathBuf>,
    mode: ExecutionMode,
) -> anyhow::Result<OwnedDiarizationPipeline> {
    let is_gpu_mode = !matches!(mode, ExecutionMode::Cpu);

    // Resolve models once: local dir or HF download that honors HF_HOME and
    // includes CUDA split-tail files speakrs from_pretrained omits.
    let resolved_dir: PathBuf = match model_dir {
        Some(dir) => dir.clone(),
        None => ensure_pretrained_models(mode)
            .context("Failed to download speakrs models (HF_HOME-aware)")?,
    };

    let init_result = OwnedDiarizationPipeline::from_dir(&resolved_dir, mode)
        .map_err(DiarizeError::from)
        .with_context(|| {
            format!(
                "Failed to load speakrs pipeline from {} (mode={mode})",
                resolved_dir.display()
            )
        });

    match init_result {
        Ok(pipeline) => Ok(pipeline),
        Err(e) if is_gpu_mode => {
            log::warn!("[diarize] GPU initialization failed (mode={mode}): {e:#}");
            log::warn!("[diarize] falling back to CPU mode");

            OwnedDiarizationPipeline::from_dir(&resolved_dir, ExecutionMode::Cpu)
                .map_err(DiarizeError::from)
                .with_context(|| {
                    format!(
                        "Failed to load speakrs pipeline from {} (CPU fallback)",
                        resolved_dir.display()
                    )
                })
                .map_err(|cpu_err| {
                    log::error!("[diarize] CPU fallback also failed: {cpu_err:#}");
                    cpu_err
                })
        }
        Err(e) => Err(e),
    }
}

/// Run `f` against the process-wide pipeline, initializing it on first use.
///
/// The mutex is only held during initialization. The pipeline is moved out
/// temporarily, used, then put back to allow concurrent diarization operations.
fn with_pipeline<T>(
    cfg: &InProcessConfig,
    f: impl FnOnce(&mut OwnedDiarizationPipeline) -> Result<T>,
) -> anyhow::Result<T> {
    let mutex = DIARIZER.get_or_init(|| Mutex::new(None));

    // Take the pipeline out of the mutex temporarily
    let mut pipeline = {
        let mut guard = mutex
            .lock()
            .map_err(|e| anyhow::anyhow!("diarizer mutex poisoned: {e}"))?;

        if guard.is_none() {
            log::info!(
                "[diarize] initializing speakrs pipeline (mode={}, model_dir={:?})",
                cfg.execution_mode,
                cfg.model_dir
            );
            let init_start = std::time::Instant::now();
            let pipeline = try_init_pipeline_with_fallback(&cfg.model_dir, cfg.execution_mode)?;
            log::info!(
                "[diarize] pipeline initialized in {:.2}s",
                init_start.elapsed().as_secs_f64()
            );
            *guard = Some(pipeline);
        }

        // Take ownership of the pipeline, releasing the mutex
        guard.take().expect("pipeline must be initialized above")
    }; // Mutex guard is dropped here, allowing other threads to proceed

    // Run the closure with the pipeline outside the mutex
    let result = f(&mut pipeline);

    // Put the pipeline back
    {
        let mut guard = mutex
            .lock()
            .map_err(|e| anyhow::anyhow!("diarizer mutex poisoned: {e}"))?;
        *guard = Some(pipeline);
    }

    result.map_err(Into::into)
}
