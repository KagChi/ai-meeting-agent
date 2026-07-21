//! Standalone WeSpeaker embedding extraction for voiceprint enrollment.
//!
//! Uses the same ONNX weights as diarization (`wespeaker-voxceleb-resnet34.onnx`)
//! via [`speakrs::inference::EmbeddingModel`]. Embeddings are L2-normalized
//! for cosine matching.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::Context;
use speakrs::inference::EmbeddingModel;
use speakrs::ExecutionMode;

use crate::config::DiarizeConfig;
use crate::diarize::audio::decode_audio_from_file;
use crate::diarize::models_download::ensure_pretrained_models;
use crate::diarize::resolve_execution_mode;

/// Model id stored on `voiceprints.model`.
pub const EMBED_MODEL_ID: &str = "wespeaker-voxceleb-resnet34";
/// Embedding dimensionality.
pub const EMBED_DIM: u32 = 256;
/// WeSpeaker single-window length (10 s @ 16 kHz).
const WINDOW_SAMPLES: usize = 160_000;
const SAMPLE_RATE: usize = 16_000;

/// Cached embedder + the [`ExecutionMode`] it was loaded with so config
/// changes (or first-load CPU vs later GPU) can reload instead of sticky CPU.
struct CachedEmbedder {
    model: EmbeddingModel,
    mode: ExecutionMode,
}

static EMBEDDER: OnceLock<Mutex<Option<CachedEmbedder>>> = OnceLock::new();

/// L2-normalize a vector in place. Zero vector left unchanged.
pub fn l2_normalize(v: &mut [f32]) {
    let mut sum_sq = 0.0f32;
    for x in v.iter() {
        sum_sq += x * x;
    }
    let norm = sum_sq.sqrt();
    if norm > 1e-12 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Cosine similarity of two equal-length vectors (prefer L2-normalized inputs).
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom < 1e-12 {
        0.0
    } else {
        (dot / denom).clamp(-1.0, 1.0)
    }
}

/// Mean-pool then L2-normalize a list of embeddings.
pub fn mean_pool_l2(vectors: &[Vec<f32>]) -> anyhow::Result<Vec<f32>> {
    if vectors.is_empty() {
        anyhow::bail!("cannot mean-pool empty embedding list");
    }
    let dim = vectors[0].len();
    if dim == 0 {
        anyhow::bail!("embedding dimension is 0");
    }
    for v in vectors {
        if v.len() != dim {
            anyhow::bail!("embedding dim mismatch: {} vs {}", v.len(), dim);
        }
    }
    let mut mean = vec![0.0f32; dim];
    for v in vectors {
        for (i, x) in v.iter().enumerate() {
            mean[i] += x;
        }
    }
    let n = vectors.len() as f32;
    for x in mean.iter_mut() {
        *x /= n;
    }
    l2_normalize(&mut mean);
    Ok(mean)
}

fn resolve_model_dir(model_dir: Option<&Path>, mode: ExecutionMode) -> anyhow::Result<PathBuf> {
    match model_dir {
        Some(dir) => Ok(dir.to_path_buf()),
        None => ensure_pretrained_models(mode)
            .context("Failed to download speakrs models for embedding"),
    }
}

fn embedding_onnx_path(model_dir: &Path) -> PathBuf {
    model_dir.join("wespeaker-voxceleb-resnet34.onnx")
}

fn load_embedder(
    model_dir: Option<&Path>,
    mode: ExecutionMode,
) -> anyhow::Result<EmbeddingModel> {
    let dir = resolve_model_dir(model_dir, mode)?;
    let onnx = embedding_onnx_path(&dir);
    if !onnx.exists() {
        anyhow::bail!(
            "WeSpeaker ONNX not found at {} (set DIARIZE_MODEL_DIR or allow HF download)",
            onnx.display()
        );
    }
    EmbeddingModel::with_mode(&onnx, mode).with_context(|| {
        format!(
            "Failed to load EmbeddingModel from {} (mode={mode})",
            onnx.display()
        )
    })
}

fn load_embedder_with_fallback(
    model_dir: Option<&Path>,
    mode: ExecutionMode,
) -> anyhow::Result<(EmbeddingModel, ExecutionMode)> {
    match load_embedder(model_dir, mode) {
        Ok(emb) => Ok((emb, mode)),
        Err(e) if !matches!(mode, ExecutionMode::Cpu) => {
            log::warn!("[embed] GPU load failed (mode={mode}): {e:#}; falling back to CPU");
            let emb = load_embedder(model_dir, ExecutionMode::Cpu)?;
            Ok((emb, ExecutionMode::Cpu))
        }
        Err(e) => Err(e),
    }
}

fn with_embedder<T>(
    model_dir: Option<&Path>,
    mode: ExecutionMode,
    f: impl FnOnce(&mut EmbeddingModel) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let mutex = EMBEDDER.get_or_init(|| Mutex::new(None));
    let mut cached = {
        let mut guard = mutex
            .lock()
            .map_err(|e| anyhow::anyhow!("embedder mutex poisoned: {e}"))?;
        let need_load = match guard.as_ref() {
            None => true,
            Some(c) if c.mode != mode => {
                log::info!(
                    "[embed] execution_mode changed ({} → {}); reloading WeSpeaker",
                    c.mode,
                    mode
                );
                true
            }
            Some(_) => false,
        };
        if need_load {
            log::info!(
                "[embed] loading WeSpeaker model (mode={mode}, model_dir={:?})",
                model_dir
            );
            let (emb, loaded_mode) = load_embedder_with_fallback(model_dir, mode)?;
            log::info!(
                "[embed] model ready (mode={loaded_mode}, sample_rate={}, min_samples={})",
                emb.sample_rate(),
                emb.min_num_samples()
            );
            *guard = Some(CachedEmbedder {
                model: emb,
                mode: loaded_mode,
            });
        }
        guard.take().expect("embedder initialized")
    };
    let result = f(&mut cached.model);
    {
        let mut guard = mutex
            .lock()
            .map_err(|e| anyhow::anyhow!("embedder mutex poisoned: {e}"))?;
        *guard = Some(cached);
    }
    result
}

/// Embed mono f32 @ 16 kHz samples. Windows of 10 s are mean-pooled when longer.
///
/// Uses the same [`DiarizeConfig::execution_mode`] as in-process diarization
/// (`auto` / `cpu` / `cuda` / `cuda-fast` / `coreml` / …).
pub fn embed_samples(samples: &[f32], cfg: &DiarizeConfig) -> anyhow::Result<Vec<f32>> {
    if samples.is_empty() {
        anyhow::bail!("cannot embed empty audio");
    }
    let mode = resolve_execution_mode(&cfg.execution_mode);
    log::debug!(
        "[embed] diarize.execution_mode={:?} → resolved={}",
        cfg.execution_mode,
        mode
    );
    let model_dir = cfg.model_dir.as_deref();

    // Dedicated stack like diarize (ORT can be deep).
    let samples = samples.to_vec();
    let model_dir_owned = model_dir.map(|p| p.to_path_buf());
    let handle = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            with_embedder(model_dir_owned.as_deref(), mode, |model| {
                embed_samples_with_model(model, &samples)
            })
        })
        .context("failed to spawn embed thread")?;
    handle
        .join()
        .map_err(|_| anyhow::anyhow!("embed thread panicked"))?
}

fn embed_samples_with_model(
    model: &mut EmbeddingModel,
    samples: &[f32],
) -> anyhow::Result<Vec<f32>> {
    let min_samples = model.min_num_samples();
    if samples.len() < min_samples {
        anyhow::bail!(
            "audio too short for embedding: {} samples ({:.2}s), need at least {} ({:.2}s)",
            samples.len(),
            samples.len() as f64 / SAMPLE_RATE as f64,
            min_samples,
            min_samples as f64 / SAMPLE_RATE as f64
        );
    }

    let window = WINDOW_SAMPLES.max(min_samples);
    let mut windows: Vec<Vec<f32>> = Vec::new();

    if samples.len() <= window {
        let arr = model
            .embed(samples)
            .context("WeSpeaker embed failed")?;
        windows.push(arr.to_vec());
    } else {
        // Non-overlapping 10 s windows; last partial window if ≥ min_samples.
        let mut start = 0usize;
        while start < samples.len() {
            let end = (start + window).min(samples.len());
            let chunk = &samples[start..end];
            if chunk.len() < min_samples {
                break;
            }
            let arr = model
                .embed(chunk)
                .context("WeSpeaker embed failed on window")?;
            windows.push(arr.to_vec());
            if end == samples.len() {
                break;
            }
            start = end;
        }
    }

    mean_pool_l2(&windows)
}

/// Decode file (optionally slice `[start_s, end_s)`) and embed.
pub fn embed_audio_file(
    audio_path: &Path,
    start_s: Option<f64>,
    end_s: Option<f64>,
    cfg: &DiarizeConfig,
) -> anyhow::Result<Vec<f32>> {
    let samples = decode_audio_from_file(audio_path)
        .with_context(|| format!("Failed to decode audio: {}", audio_path.display()))?;

    let start_idx = start_s
        .map(|s| ((s.max(0.0) * SAMPLE_RATE as f64) as usize).min(samples.len()))
        .unwrap_or(0);
    let end_idx = end_s
        .map(|s| ((s.max(0.0) * SAMPLE_RATE as f64) as usize).min(samples.len()))
        .unwrap_or(samples.len());
    if end_idx <= start_idx {
        anyhow::bail!(
            "invalid audio span: start_s={:?} end_s={:?}",
            start_s,
            end_s
        );
    }
    embed_samples(&samples[start_idx..end_idx], cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l2_normalize_unit() {
        let mut v = vec![3.0f32, 4.0];
        l2_normalize(&mut v);
        assert!((v[0] - 0.6).abs() < 1e-5);
        assert!((v[1] - 0.8).abs() < 1e-5);
    }

    #[test]
    fn cosine_identical_is_one() {
        let a = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &a) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn cosine_orthogonal_is_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-5);
    }

    #[test]
    fn mean_pool_l2_works() {
        let vectors = vec![vec![1.0f32, 0.0], vec![0.0, 1.0]];
        let m = mean_pool_l2(&vectors).unwrap();
        assert_eq!(m.len(), 2);
        // mean (0.5, 0.5) → L2
        let expected = 0.5f32 / (0.5f32 * 0.5 * 2.0).sqrt();
        assert!((m[0] - expected).abs() < 1e-5);
        assert!((m[1] - expected).abs() < 1e-5);
    }
}
