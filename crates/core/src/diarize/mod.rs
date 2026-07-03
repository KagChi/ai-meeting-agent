//! In-process speaker diarization via `speakrs`.
//!
//! Wraps `speakrs::OwnedDiarizationPipeline` in a lazily-initialized
//! `Mutex` so the model loads once per process. The pipeline is CPU-bound
//! and blocking, so [`Diarizer::diarize`] offloads work to a
//! `spawn_blocking` task.
//!
//! Resilience contract: [`Diarizer::diarize`] returns an error on any
//! failure (model load, decode, pipeline). Callers are expected to log
//! and proceed without speaker labels rather than fail the whole import.

pub mod audio;
pub mod error;
pub mod merge;
pub mod models;

pub use error::{DiarizeError, Result};
pub use merge::{merge as merge_segments, CleanedSegment};
pub use models::{WhisperSegment, WhisperTranscript};

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::Context;
use speakrs::{ExecutionMode, OwnedDiarizationPipeline};

use crate::diarize::audio::decode_audio_to_f32_mono_16k;
use crate::diarize::merge::merge;
use crate::transcription::TranscriptionResponse;

/// Configuration for the in-process diarizer. Built once, cached in the
/// process-wide [`OnceLock`] so the first call pays the model-load cost.
#[derive(Debug, Clone)]
pub struct DiarizerConfig {
    pub execution_mode: ExecutionMode,
    /// Optional local model directory. `None` = download via
    /// `speakrs` `online` feature on first use.
    pub model_dir: Option<PathBuf>,
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

impl Diarizer {
    /// Run diarization on an audio file + Whisper transcript, returning
    /// a clone of the transcript with `speaker` labels assigned to each
    /// segment.
    ///
    /// Blocks on a `spawn_blocking` task because `speakrs` inference is
    /// CPU/GPU-bound and not async-safe. Takes a reference so callers
    /// keep the original transcript on error.
    pub async fn diarize(
        audio_path: &Path,
        transcript: &TranscriptionResponse,
        cfg: &DiarizerConfig,
    ) -> anyhow::Result<TranscriptionResponse> {
        let cfg = cfg.clone();
        let audio_path = audio_path.to_path_buf();
        let transcript = transcript.clone();
        tokio::task::spawn_blocking(move || run_in_process(&audio_path, &transcript, &cfg))
            .await
            .context("diarize blocking task panicked")?
    }
}

fn run_in_process(
    audio_path: &Path,
    transcript: &TranscriptionResponse,
    cfg: &DiarizerConfig,
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
    let audio_bytes = std::fs::read(audio_path)
        .with_context(|| format!("Failed to read audio file: {}", audio_path.display()))?;
    let samples = decode_audio_to_f32_mono_16k(&audio_bytes)?;
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
    if let Some(segs) = out.segments.as_mut() {
        for (i, seg) in segs.iter_mut().enumerate() {
            if let Some(c) = cleaned.get(i) {
                seg.speaker = c.speaker.clone();
            }
        }
    }

    Ok(out)
}

/// Run `f` against the process-wide pipeline, initializing it on first use.
fn with_pipeline<T>(
    cfg: &DiarizerConfig,
    f: impl FnOnce(&mut OwnedDiarizationPipeline) -> Result<T>,
) -> anyhow::Result<T> {
    let mutex = DIARIZER.get_or_init(|| Mutex::new(None));
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
        let pipeline = match &cfg.model_dir {
            Some(dir) => OwnedDiarizationPipeline::from_dir(dir, cfg.execution_mode)
                .map_err(DiarizeError::from)
                .context("Failed to load speakrs pipeline from local model dir")?,
            None => OwnedDiarizationPipeline::from_pretrained(cfg.execution_mode)
                .map_err(DiarizeError::from)
                .context("Failed to load speakrs pipeline (pretrained download)")?,
        };
        log::info!(
            "[diarize] pipeline initialized in {:.2}s",
            init_start.elapsed().as_secs_f64()
        );
        *guard = Some(pipeline);
    }

    let pipeline = guard.as_mut().expect("pipeline must be initialized above");
    f(pipeline).map_err(Into::into)
}
