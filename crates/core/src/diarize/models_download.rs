//! Download speakrs + voiceprint embedding models with HF_HOME-aware cache.
//!
//! speakrs `from_pretrained` has two issues we work around:
//! 1. Uses `hf_hub::Api::new()` → `Cache::default()` under `$HOME/.cache/...`,
//!    ignoring `HF_HOME` (so compose volumes never receive models).
//! 2. CUDA download list omits `wespeaker-voxceleb-resnet34-tail.onnx`, but
//!    embedding load enables the split backend when fbank+multimask exist and
//!    then requires that tail file — breaking GPU init and CPU fallback on the
//!    same snapshot.
//!
//! Voiceprint CAM++_LM is a separate WeSpeaker ONNX (feats→embs) from
//! `Wespeaker/wespeaker-voxceleb-campplus-LM`.

use std::path::{Path, PathBuf};

use anyhow::Context;
use speakrs::ExecutionMode;

const HF_REPO_SPEAKRS: &str = "avencera/speakrs-models";
const HF_REPO_CAMPPLUS: &str = "Wespeaker/wespeaker-voxceleb-campplus-LM";

/// Remote filename in the Wespeaker CAM++_LM repo.
const CAMPPLUS_REMOTE_ONNX: &str = "voxceleb_CAM++_LM.onnx";
/// Local filename we standardize on (matches `embedding_model` id).
pub const CAMPPLUS_LOCAL_ONNX: &str = "wespeaker-voxceleb-CAM++_LM.onnx";
pub const CAMPPLUS_MODEL_ID: &str = "wespeaker-voxceleb-CAM++_LM";
pub const RESNET34_MODEL_ID: &str = "wespeaker-voxceleb-resnet34";
pub const RESNET34_ONNX: &str = "wespeaker-voxceleb-resnet34.onnx";

/// Files required for CPU and CUDA/MiGraphX paths, including split-tail ONNX
/// that speakrs load always opens when the split backend is detected.
const SPEAKRS_MODEL_FILES: &[&str] = &[
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

fn build_hf_api(hub_cache: &Path) -> anyhow::Result<hf_hub::api::sync::Api> {
    std::fs::create_dir_all(hub_cache)
        .with_context(|| format!("create HF hub cache {}", hub_cache.display()))?;
    hf_hub::api::sync::ApiBuilder::from_env()
        .with_cache_dir(hub_cache.to_path_buf())
        .with_progress(true)
        .build()
        .context("build hf-hub API")
}

/// Ensure all speakrs model files are present; return the snapshot directory
/// suitable for [`speakrs::OwnedDiarizationPipeline::from_dir`].
pub fn ensure_pretrained_models(_mode: ExecutionMode) -> anyhow::Result<PathBuf> {
    let hub_cache = resolve_hf_hub_cache();
    log::info!(
        "[diarize] ensuring speakrs models (repo={HF_REPO_SPEAKRS}, hub_cache={})",
        hub_cache.display()
    );

    let api = build_hf_api(&hub_cache)?;
    let repo = api.model(HF_REPO_SPEAKRS.to_string());
    let mut last_path: Option<PathBuf> = None;

    for file in SPEAKRS_MODEL_FILES {
        log::debug!("[diarize] ensuring model file {file}");
        let path = repo
            .get(file)
            .with_context(|| format!("download {HF_REPO_SPEAKRS}/{file}"))?;
        last_path = Some(path);
    }

    let snapshot = last_path
        .as_ref()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .context("no model files downloaded")?;

    log::info!(
        "[diarize] models ready at {} ({} files)",
        snapshot.display(),
        SPEAKRS_MODEL_FILES.len()
    );
    Ok(snapshot)
}

/// Ensure CAM++_LM ONNX is present; return directory containing
/// [`CAMPPLUS_LOCAL_ONNX`].
///
/// Downloads `voxceleb_CAM++_LM.onnx` from HuggingFace and copies/links it as
/// `wespeaker-voxceleb-CAM++_LM.onnx` in the same snapshot (or into
/// `model_dir` when provided).
pub fn ensure_campplus_model(model_dir: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(dir) = model_dir {
        let local = dir.join(CAMPPLUS_LOCAL_ONNX);
        if local.exists() {
            return Ok(dir.to_path_buf());
        }
        // Also accept the upstream filename in a local dir.
        let remote_name = dir.join(CAMPPLUS_REMOTE_ONNX);
        if remote_name.exists() {
            std::fs::copy(&remote_name, &local).with_context(|| {
                format!(
                    "copy {} → {}",
                    remote_name.display(),
                    local.display()
                )
            })?;
            return Ok(dir.to_path_buf());
        }
        anyhow::bail!(
            "CAM++ ONNX not found at {} or {} (set DIARIZE_MODEL_DIR or allow HF download)",
            local.display(),
            remote_name.display()
        );
    }

    let hub_cache = resolve_hf_hub_cache();
    log::info!(
        "[embed] ensuring CAM++_LM (repo={HF_REPO_CAMPPLUS}, hub_cache={})",
        hub_cache.display()
    );

    let api = build_hf_api(&hub_cache)?;
    let repo = api.model(HF_REPO_CAMPPLUS.to_string());
    let remote_path = repo
        .get(CAMPPLUS_REMOTE_ONNX)
        .with_context(|| format!("download {HF_REPO_CAMPPLUS}/{CAMPPLUS_REMOTE_ONNX}"))?;

    let snapshot = remote_path
        .parent()
        .map(Path::to_path_buf)
        .context("CAM++ download has no parent dir")?;

    let local = snapshot.join(CAMPPLUS_LOCAL_ONNX);
    if !local.exists() {
        // Prefer hardlink/copy so path is stable for EmbeddingModel load.
        if std::fs::hard_link(&remote_path, &local).is_err() {
            std::fs::copy(&remote_path, &local).with_context(|| {
                format!(
                    "copy {} → {}",
                    remote_path.display(),
                    local.display()
                )
            })?;
        }
    }

    log::info!("[embed] CAM++_LM ready at {}", local.display());
    Ok(snapshot)
}

/// Resolve model directory for the active embedding backend.
pub fn ensure_embedding_models(
    mode: ExecutionMode,
    embedding_model: &str,
    model_dir: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    let is_campplus = {
        let m = embedding_model.to_ascii_lowercase();
        m.contains("cam++") || m.contains("campplus")
    };
    if is_campplus {
        ensure_campplus_model(model_dir)
    } else if let Some(dir) = model_dir {
        Ok(dir.to_path_buf())
    } else {
        ensure_pretrained_models(mode)
    }
}
