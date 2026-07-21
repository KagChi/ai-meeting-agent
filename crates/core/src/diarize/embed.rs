//! Standalone speaker embedding extraction for voiceprint enrollment.
//!
//! Two backends:
//! - **CAM++_LM** (default): WeSpeaker ONNX via `ort` + kaldi fbank (`knf-rs`).
//!   Input `feats` → output `embs`, 512-dim.
//! - **ResNet34**: speakrs `EmbeddingModel` (`wespeaker-voxceleb-resnet34.onnx`),
//!   waveform+weights, 256-dim (same weights as diarization).

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use anyhow::Context;
use ort::session::Session;
use ort::value::TensorRef;
use speakrs::inference::EmbeddingModel;
use speakrs::ExecutionMode;

use crate::config::DiarizeConfig;
use crate::diarize::audio::decode_audio_from_file;
use crate::diarize::models_download::{
    ensure_embedding_models, ensure_pretrained_models, CAMPPLUS_LOCAL_ONNX, CAMPPLUS_MODEL_ID,
    RESNET34_MODEL_ID, RESNET34_ONNX,
};
use crate::diarize::resolve_execution_mode;

/// Default model id (CAM++_LM). Prefer [`DiarizeConfig::embedding_model`].
pub const EMBED_MODEL_ID: &str = CAMPPLUS_MODEL_ID;
/// Default embedding dimensionality (CAM++_LM). Prefer [`DiarizeConfig::embedding_dim`].
pub const EMBED_DIM: u32 = 512;
/// ResNet34 / speakrs window (10 s @ 16 kHz).
const RESNET_WINDOW_SAMPLES: usize = 160_000;
/// CAM++: use up to 10 s windows when longer.
const CAMPPLUS_WINDOW_SAMPLES: usize = 160_000;
const SAMPLE_RATE: usize = 16_000;
/// Minimum speech for CAM++ fbank (~0.5 s).
const CAMPPLUS_MIN_SAMPLES: usize = 8_000;

enum Backend {
    CampPlus(Session),
    ResNet34(EmbeddingModel),
}

struct CachedEmbedder {
    backend: Backend,
    model_id: String,
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

fn ort_err(e: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}

fn build_ort_session(model_path: &Path, mode: ExecutionMode) -> anyhow::Result<Session> {
    use ort::ep;
    use ort::session::builder::GraphOptimizationLevel;

    let builder = Session::builder()
        .map_err(ort_err)?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(ort_err)?
        .with_intra_threads(1)
        .map_err(ort_err)?
        .with_inter_threads(1)
        .map_err(ort_err)?;

    let mut builder = match mode {
        ExecutionMode::Cuda | ExecutionMode::CudaFast => {
            #[cfg(all(feature = "diarization", target_os = "linux"))]
            {
                builder
                    .with_execution_providers([ep::CUDA::default()
                        .with_device_id(0)
                        .build()])
                    .map_err(ort_err)?
            }
            #[cfg(not(all(feature = "diarization", target_os = "linux")))]
            {
                log::warn!(
                    "[embed] CUDA requested but not available on this build; using CPU for CAM++"
                );
                builder
                    .with_execution_providers([ep::CPU::default().build()])
                    .map_err(ort_err)?
            }
        }
        _ => builder
            .with_execution_providers([ep::CPU::default().build()])
            .map_err(ort_err)?,
    };

    builder
        .commit_from_file(model_path)
        .map_err(|e| anyhow::anyhow!("load ONNX session from {}: {e}", model_path.display()))
}

fn load_campplus(
    model_dir: Option<&Path>,
    mode: ExecutionMode,
) -> anyhow::Result<(Session, ExecutionMode)> {
    let dir = ensure_embedding_models(mode, CAMPPLUS_MODEL_ID, model_dir)?;
    let onnx = dir.join(CAMPPLUS_LOCAL_ONNX);
    if !onnx.exists() {
        anyhow::bail!("CAM++ ONNX not found at {}", onnx.display());
    }
    match build_ort_session(&onnx, mode) {
        Ok(s) => Ok((s, mode)),
        Err(e) if !matches!(mode, ExecutionMode::Cpu) => {
            log::warn!("[embed] CAM++ GPU load failed (mode={mode}): {e:#}; falling back to CPU");
            let s = build_ort_session(&onnx, ExecutionMode::Cpu)?;
            Ok((s, ExecutionMode::Cpu))
        }
        Err(e) => Err(e),
    }
}

fn load_resnet34(
    model_dir: Option<&Path>,
    mode: ExecutionMode,
) -> anyhow::Result<(EmbeddingModel, ExecutionMode)> {
    let dir = match model_dir {
        Some(d) => d.to_path_buf(),
        None => ensure_pretrained_models(mode)
            .context("Failed to download speakrs models for ResNet34 embedding")?,
    };
    let onnx = dir.join(RESNET34_ONNX);
    if !onnx.exists() {
        anyhow::bail!(
            "WeSpeaker ResNet34 ONNX not found at {} (set DIARIZE_MODEL_DIR or allow HF download)",
            onnx.display()
        );
    }
    match EmbeddingModel::with_mode(&onnx, mode) {
        Ok(emb) => Ok((emb, mode)),
        Err(e) if !matches!(mode, ExecutionMode::Cpu) => {
            log::warn!("[embed] ResNet34 GPU load failed (mode={mode}): {e:#}; falling back to CPU");
            let emb = EmbeddingModel::with_mode(&onnx, ExecutionMode::Cpu)
                .context("Failed to load ResNet34 EmbeddingModel on CPU")?;
            Ok((emb, ExecutionMode::Cpu))
        }
        Err(e) => Err(anyhow::anyhow!("Failed to load ResNet34 EmbeddingModel: {e}")),
    }
}

fn load_backend(
    cfg: &DiarizeConfig,
    mode: ExecutionMode,
) -> anyhow::Result<(Backend, ExecutionMode, String)> {
    let model_dir = cfg.model_dir.as_deref();
    if cfg.uses_campplus_embedding() {
        let (session, loaded) = load_campplus(model_dir, mode)?;
        Ok((
            Backend::CampPlus(session),
            loaded,
            cfg.embedding_model.clone(),
        ))
    } else {
        let (emb, loaded) = load_resnet34(model_dir, mode)?;
        Ok((
            Backend::ResNet34(emb),
            loaded,
            if cfg.embedding_model.is_empty() {
                RESNET34_MODEL_ID.to_string()
            } else {
                cfg.embedding_model.clone()
            },
        ))
    }
}

fn with_embedder<T>(
    cfg: &DiarizeConfig,
    mode: ExecutionMode,
    f: impl FnOnce(&mut Backend) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let mutex = EMBEDDER.get_or_init(|| Mutex::new(None));
    let mut cached = {
        let mut guard = mutex
            .lock()
            .map_err(|e| anyhow::anyhow!("embedder mutex poisoned: {e}"))?;
        let need_load = match guard.as_ref() {
            None => true,
            Some(c) if c.mode != mode || c.model_id != cfg.embedding_model => {
                log::info!(
                    "[embed] reload (mode {}→{}, model {}→{})",
                    c.mode,
                    mode,
                    c.model_id,
                    cfg.embedding_model
                );
                true
            }
            Some(_) => false,
        };
        if need_load {
            log::info!(
                "[embed] loading embedding model id={} mode={} model_dir={:?}",
                cfg.embedding_model,
                mode,
                cfg.model_dir
            );
            let (backend, loaded_mode, model_id) = load_backend(cfg, mode)?;
            log::info!("[embed] model ready id={model_id} mode={loaded_mode}");
            *guard = Some(CachedEmbedder {
                backend,
                model_id,
                mode: loaded_mode,
            });
        }
        guard.take().expect("embedder initialized")
    };
    let result = f(&mut cached.backend);
    {
        let mut guard = mutex
            .lock()
            .map_err(|e| anyhow::anyhow!("embedder mutex poisoned: {e}"))?;
        *guard = Some(cached);
    }
    result
}

/// Kaldi fbank (80-dim) with CMN, matching WeSpeaker `infer_onnx.py`.
///
/// Returns `(shape [1, T, F], flat row-major data)`. Waveform is scaled by
/// 2^15 before fbank (WeSpeaker convention). knf-rs ndarray is converted to a
/// plain `Vec` so we can feed ort without mixing ndarray major versions.
fn compute_campplus_feats(samples: &[f32]) -> anyhow::Result<(Vec<usize>, Vec<f32>)> {
    let scaled: Vec<f32> = samples.iter().map(|s| s * 32768.0).collect();
    let feats2d = knf_rs::compute_fbank(&scaled).map_err(|e| anyhow::anyhow!("fbank: {e:#}"))?;
    let (frames, bins) = feats2d.dim();
    if frames == 0 || bins == 0 {
        anyhow::bail!("empty fbank features frames={frames} bins={bins}");
    }
    // Contiguous [1, T, F] layout for ORT.
    let mut data = Vec::with_capacity(frames * bins);
    for t in 0..frames {
        for b in 0..bins {
            data.push(feats2d[[t, b]]);
        }
    }
    Ok((vec![1usize, frames, bins], data))
}

fn embed_campplus_window(session: &mut Session, samples: &[f32]) -> anyhow::Result<Vec<f32>> {
    let (shape, data) = compute_campplus_feats(samples)?;
    let feats_tensor = TensorRef::from_array_view((shape.as_slice(), data.as_slice()))
        .map_err(|e| anyhow::anyhow!("feats tensor: {e}"))?;
    let outputs = session
        .run(ort::inputs!["feats" => feats_tensor])
        .map_err(|e| anyhow::anyhow!("CAM++ ort run: {e}"))?;
    // Prefer named output `embs`, else first output.
    let output = outputs.get("embs").unwrap_or(&outputs[0]);
    let (_shape, emb_data) = output
        .try_extract_tensor::<f32>()
        .map_err(|e| anyhow::anyhow!("CAM++ extract emb: {e}"))?;
    let mut emb = emb_data.to_vec();
    // Flatten [1, D] → D when needed.
    if emb.len() > 512 && emb.len() % 512 == 0 {
        emb.truncate(512);
    }
    l2_normalize(&mut emb);
    Ok(emb)
}

fn embed_resnet_window(model: &mut EmbeddingModel, samples: &[f32]) -> anyhow::Result<Vec<f32>> {
    let arr = model.embed(samples).context("WeSpeaker ResNet34 embed failed")?;
    let mut emb = arr.to_vec();
    l2_normalize(&mut emb);
    Ok(emb)
}

fn embed_samples_with_backend(
    backend: &mut Backend,
    samples: &[f32],
    expected_dim: usize,
) -> anyhow::Result<Vec<f32>> {
    match backend {
        Backend::CampPlus(session) => {
            if samples.len() < CAMPPLUS_MIN_SAMPLES {
                anyhow::bail!(
                    "audio too short for CAM++ embedding: {} samples ({:.2}s), need at least {} ({:.2}s)",
                    samples.len(),
                    samples.len() as f64 / SAMPLE_RATE as f64,
                    CAMPPLUS_MIN_SAMPLES,
                    CAMPPLUS_MIN_SAMPLES as f64 / SAMPLE_RATE as f64
                );
            }
            let window = CAMPPLUS_WINDOW_SAMPLES;
            let mut windows: Vec<Vec<f32>> = Vec::new();
            if samples.len() <= window {
                windows.push(embed_campplus_window(session, samples)?);
            } else {
                let mut start = 0usize;
                while start < samples.len() {
                    let end = (start + window).min(samples.len());
                    let chunk = &samples[start..end];
                    if chunk.len() < CAMPPLUS_MIN_SAMPLES {
                        break;
                    }
                    windows.push(embed_campplus_window(session, chunk)?);
                    if end == samples.len() {
                        break;
                    }
                    start = end;
                }
            }
            let emb = mean_pool_l2(&windows)?;
            if emb.len() != expected_dim {
                anyhow::bail!(
                    "CAM++ embed dim {} != expected {expected_dim}",
                    emb.len()
                );
            }
            Ok(emb)
        }
        Backend::ResNet34(model) => {
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
            let window = RESNET_WINDOW_SAMPLES.max(min_samples);
            let mut windows: Vec<Vec<f32>> = Vec::new();
            if samples.len() <= window {
                windows.push(embed_resnet_window(model, samples)?);
            } else {
                let mut start = 0usize;
                while start < samples.len() {
                    let end = (start + window).min(samples.len());
                    let chunk = &samples[start..end];
                    if chunk.len() < min_samples {
                        break;
                    }
                    windows.push(embed_resnet_window(model, chunk)?);
                    if end == samples.len() {
                        break;
                    }
                    start = end;
                }
            }
            let emb = mean_pool_l2(&windows)?;
            if emb.len() != expected_dim {
                anyhow::bail!(
                    "ResNet34 embed dim {} != expected {expected_dim}",
                    emb.len()
                );
            }
            Ok(emb)
        }
    }
}

/// Embed mono f32 @ 16 kHz samples. Windows of 10 s are mean-pooled when longer.
///
/// Uses [`DiarizeConfig::embedding_model`] / [`DiarizeConfig::embedding_dim`] and
/// the same [`DiarizeConfig::execution_mode`] as diarization.
pub fn embed_samples(samples: &[f32], cfg: &DiarizeConfig) -> anyhow::Result<Vec<f32>> {
    if samples.is_empty() {
        anyhow::bail!("cannot embed empty audio");
    }
    let mode = resolve_execution_mode(&cfg.execution_mode);
    log::debug!(
        "[embed] model={} dim={} execution_mode={:?} → resolved={}",
        cfg.embedding_model,
        cfg.embedding_dim,
        cfg.execution_mode,
        mode
    );

    let samples = samples.to_vec();
    let cfg = cfg.clone();
    let expected_dim = cfg.embedding_dim as usize;
    let handle = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            with_embedder(&cfg, mode, |backend| {
                embed_samples_with_backend(backend, &samples, expected_dim)
            })
        })
        .context("failed to spawn embed thread")?;
    handle
        .join()
        .map_err(|_| anyhow::anyhow!("embed thread panicked"))?
}

/// Decode file (optionally slice `[start_s, end_s)`) and embed.
///
/// When [`DiarizeConfig::service_url`] is set (API server), posts audio to
/// `{service_url}/v1/embed` on diarize-service. When unset (diarize-service
/// itself / local), runs models in-process.
pub fn embed_audio_file(
    audio_path: &Path,
    start_s: Option<f64>,
    end_s: Option<f64>,
    cfg: &DiarizeConfig,
) -> anyhow::Result<Vec<f32>> {
    if let Some(url) = cfg.service_url.as_deref() {
        if !url.trim().is_empty() {
            return embed_audio_file_via_http(audio_path, start_s, end_s, url, cfg);
        }
    }
    embed_audio_file_local(audio_path, start_s, end_s, cfg)
}

/// In-process embed only (no HTTP). Used by diarize-service and local tools.
pub fn embed_audio_file_local(
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

#[derive(Debug, serde::Deserialize)]
struct EmbedServiceResponse {
    success: bool,
    #[serde(default)]
    embedding: Option<Vec<f32>>,
    #[serde(default)]
    dim: Option<u32>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

fn embed_http_client() -> &'static reqwest::Client {
    use std::sync::OnceLock;
    use std::time::Duration;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .expect("failed to build embed reqwest client")
    })
}

async fn call_embed_service(
    audio_path: &Path,
    start_s: Option<f64>,
    end_s: Option<f64>,
    service_url: &str,
    cfg: &DiarizeConfig,
) -> anyhow::Result<Vec<f32>> {
    let file = tokio::fs::File::open(audio_path)
        .await
        .with_context(|| format!("Failed to open audio for embed: {}", audio_path.display()))?;
    let audio_len = file.metadata().await.map(|m| m.len()).unwrap_or(0);
    let file_name = audio_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.wav")
        .to_string();

    let audio_part = reqwest::multipart::Part::stream_with_length(file, audio_len)
        .file_name(file_name)
        .mime_str("audio/wav")?;

    let mut form = reqwest::multipart::Form::new().part("audio", audio_part);
    if let Some(s) = start_s {
        form = form.text("start_s", s.to_string());
    }
    if let Some(s) = end_s {
        form = form.text("end_s", s.to_string());
    }

    let url = format!("{}/v1/embed", service_url.trim_end_matches('/'));
    log::debug!(
        "[embed] remote POST {url} path={} start={:?} end={:?}",
        audio_path.display(),
        start_s,
        end_s
    );

    let response = embed_http_client()
        .post(&url)
        .multipart(form)
        .send()
        .await
        .context("Failed to send embed request to diarize-service")?;

    if !response.status().is_success() {
        anyhow::bail!(
            "embed service status {}: {}",
            response.status(),
            response.text().await.unwrap_or_default()
        );
    }

    let result: EmbedServiceResponse = response
        .json()
        .await
        .context("Failed to parse embed service response")?;

    if !result.success {
        anyhow::bail!(
            "embed service failed: {}",
            result.error.unwrap_or_else(|| "unknown".into())
        );
    }

    let emb = result
        .embedding
        .context("embed service returned no embedding")?;
    if emb.is_empty() {
        anyhow::bail!("embed service returned empty embedding");
    }
    if let Some(dim) = result.dim {
        if dim as usize != emb.len() {
            anyhow::bail!(
                "embed service dim mismatch: header={dim} vec={}",
                emb.len()
            );
        }
    }
    if emb.len() != cfg.embedding_dim as usize {
        anyhow::bail!(
            "embed dim {} != configured embedding_dim {}",
            emb.len(),
            cfg.embedding_dim
        );
    }
    if let Some(ref model) = result.model {
        if model != &cfg.embedding_model {
            log::warn!(
                "[embed] remote model={model} local config={}",
                cfg.embedding_model
            );
        }
    }
    Ok(emb)
}

fn embed_audio_file_via_http(
    audio_path: &Path,
    start_s: Option<f64>,
    end_s: Option<f64>,
    service_url: &str,
    cfg: &DiarizeConfig,
) -> anyhow::Result<Vec<f32>> {
    let path = audio_path.to_path_buf();
    let url = service_url.to_string();
    let cfg = cfg.clone();
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| {
            handle.block_on(call_embed_service(&path, start_s, end_s, &url, &cfg))
        }),
        Err(_) => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("build tokio runtime for embed HTTP")?;
            rt.block_on(call_embed_service(&path, start_s, end_s, &url, &cfg))
        }
    }
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
        let expected = 0.5f32 / (0.5f32 * 0.5 * 2.0).sqrt();
        assert!((m[0] - expected).abs() < 1e-5);
        assert!((m[1] - expected).abs() < 1e-5);
    }
}
