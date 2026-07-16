//! Download speakrs models with correct HF cache + complete CUDA file set.
//!
//! speakrs `from_pretrained` has two issues we work around:
//! 1. Uses `hf_hub::Api::new()` → `Cache::default()` under `$HOME/.cache/...`,
//!    ignoring `HF_HOME` (so compose volumes never receive models).
//! 2. CUDA download list omits `wespeaker-voxceleb-resnet34-tail.onnx`, but
//!    embedding load enables the split backend when fbank+multimask exist and
//!    then requires that tail file — breaking GPU init and CPU fallback on the
//!    same snapshot.

use std::path::{Path, PathBuf};

use anyhow::Context;
use speakrs::ExecutionMode;

const HF_REPO: &str = "avencera/speakrs-models";

/// Files required for CPU and CUDA/MiGraphX paths, including split-tail ONNX
/// that speakrs load always opens when the split backend is detected.
const MODEL_FILES: &[&str] = &[
    // PLDA + meta
    "plda_lda.npy",
    "plda_tr.npy",
    "plda_mu.npy",
    "plda_psi.npy",
    "plda_mean1.npy",
    "plda_mean2.npy",
    "wespeaker-voxceleb-resnet34.min_num_samples.txt",
    // Base ONNX (CPU + all GPU modes)
    "segmentation-3.0.onnx",
    "wespeaker-voxceleb-resnet34.onnx",
    "wespeaker-voxceleb-resnet34.onnx.data",
    // CUDA / cuda-fast / migraphx extras (speakrs required_files)
    "wespeaker-fbank.onnx",
    "wespeaker-fbank-b32.onnx",
    "wespeaker-multimask-tail.onnx",
    "wespeaker-multimask-tail-b32.onnx",
    "segmentation-3.0-b32.onnx",
    "wespeaker-voxceleb-resnet34-b64.onnx",
    // Required by EmbeddingModel load when split backend is present
    "wespeaker-voxceleb-resnet34-tail.onnx",
    "wespeaker-voxceleb-resnet34-tail-b3.onnx",
    "wespeaker-voxceleb-resnet34-tail-b32.onnx",
];

/// Resolve the HuggingFace hub cache directory.
///
/// Matches hf-hub `Cache::from_env`: `HF_HOME/hub`, else
/// `$HOME/.cache/huggingface/hub`.
pub fn resolve_hf_hub_cache() -> PathBuf {
    if let Ok(hf_home) = std::env::var("HF_HOME") {
        if !hf_home.trim().is_empty() {
            return PathBuf::from(hf_home).join("hub");
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".cache")
        .join("huggingface")
        .join("hub")
}

/// Ensure all speakrs model files are present; return the snapshot directory
/// suitable for [`speakrs::OwnedDiarizationPipeline::from_dir`].
pub fn ensure_pretrained_models(_mode: ExecutionMode) -> anyhow::Result<PathBuf> {
    let hub_cache = resolve_hf_hub_cache();
    log::info!(
        "[diarize] ensuring speakrs models (repo={HF_REPO}, hub_cache={})",
        hub_cache.display()
    );

    std::fs::create_dir_all(&hub_cache)
        .with_context(|| format!("create HF hub cache {}", hub_cache.display()))?;

    // Prefer from_env so HF_HOME / HF_ENDPOINT are honored; then pin cache dir
    // explicitly so path matches resolve_hf_hub_cache even if env is empty.
    let api = hf_hub::api::sync::ApiBuilder::from_env()
        .with_cache_dir(hub_cache.clone())
        .with_progress(true)
        .build()
        .context("build hf-hub API for speakrs models")?;

    let repo = api.model(HF_REPO.to_string());
    let mut last_path: Option<PathBuf> = None;

    for file in MODEL_FILES {
        log::debug!("[diarize] ensuring model file {file}");
        let path = repo
            .get(file)
            .with_context(|| format!("download {HF_REPO}/{file}"))?;
        last_path = Some(path);
    }

    let snapshot = last_path
        .as_ref()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .context("no model files downloaded")?;

    log::info!(
        "[diarize] models ready at {} ({} files)",
        snapshot.display(),
        MODEL_FILES.len()
    );
    Ok(snapshot)
}
