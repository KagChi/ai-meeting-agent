use crate::error::{ClientError, Result};
use std::env;
use std::path::{Component, Path, PathBuf};

/// OpenClaw inbound attachment pseudo-URI prefix.
/// Only this literal prefix is emitted for inbound media; no other `media://` form.
const OPENCLAW_INBOUND_PREFIX: &str = "media://inbound/";

/// Env override for the on-disk directory that backs `media://inbound/…` URIs.
pub const OPENCLAW_MEDIA_INBOUND_DIR_ENV: &str = "OPENCLAW_MEDIA_INBOUND_DIR";

const DEFAULT_OPENCLAW_INBOUND_REL: &str = ".openclaw/media/inbound";

/// Resolve an `importFromFile` path.
///
/// - `media://inbound/<rel>` → `$OPENCLAW_MEDIA_INBOUND_DIR/<rel>`
///   (default base: `~/.openclaw/media/inbound`)
/// - anything else → plain filesystem path
///
/// Rejects path traversal (`..`) under the OpenClaw media root.
pub fn resolve_import_path(input: &str) -> Result<PathBuf> {
    let path = if let Some(rel) = input.strip_prefix(OPENCLAW_INBOUND_PREFIX) {
        resolve_openclaw_inbound(rel)?
    } else {
        PathBuf::from(input)
    };

    if !path.exists() {
        return Err(ClientError::InvalidInput(format!(
            "file not found: {}",
            path.display()
        )));
    }
    if !path.is_file() {
        return Err(ClientError::InvalidInput(format!(
            "file_path must point to a file: {}",
            path.display()
        )));
    }
    Ok(path)
}

fn resolve_openclaw_inbound(rel: &str) -> Result<PathBuf> {
    if rel.is_empty() {
        return Err(ClientError::InvalidInput(
            "media://inbound/ URI is missing a filename".to_string(),
        ));
    }
    if has_parent_or_root_component(Path::new(rel)) {
        return Err(ClientError::InvalidInput(format!(
            "path escapes media root: media://inbound/{rel}"
        )));
    }

    let base = openclaw_inbound_base()?;
    let resolved = base.join(rel);

    // Defense in depth: after join, ensure we still sit under base (handles odd separators).
    if !path_is_under(&base, &resolved) {
        return Err(ClientError::InvalidInput(format!(
            "path escapes media root: media://inbound/{rel}"
        )));
    }
    Ok(resolved)
}

fn openclaw_inbound_base() -> Result<PathBuf> {
    match env::var(OPENCLAW_MEDIA_INBOUND_DIR_ENV) {
        Ok(value) if !value.trim().is_empty() => Ok(PathBuf::from(value)),
        _ => {
            let home = dirs::home_dir().ok_or_else(|| {
                ClientError::InvalidInput(
                    "cannot resolve OpenClaw media path: home directory unknown; set OPENCLAW_MEDIA_INBOUND_DIR"
                        .to_string(),
                )
            })?;
            Ok(home.join(DEFAULT_OPENCLAW_INBOUND_REL))
        }
    }
}

fn has_parent_or_root_component(path: &Path) -> bool {
    path.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    })
}

fn path_is_under(base: &Path, candidate: &Path) -> bool {
    let mut base_components = base.components();
    let mut candidate_components = candidate.components();
    loop {
        match (base_components.next(), candidate_components.next()) {
            (None, _) => return true,
            (Some(_), None) => return false,
            (Some(b), Some(c)) if b == c => continue,
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Mutex, OnceLock};

    /// Serialize env-mutating tests so parallel nextest/cargo test stays deterministic.
    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = env::var(key).ok();
            // SAFETY: tests hold `env_lock`; only this suite mutates this key.
            env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => env::set_var(self.key, value),
                None => env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn resolves_openclaw_inbound_under_env_base() {
        let _lock = env_lock().lock().expect("env lock");
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("clip.mp3");
        fs::write(&file, b"audio").expect("write");
        let _env = EnvGuard::set(OPENCLAW_MEDIA_INBOUND_DIR_ENV, dir.path().to_str().unwrap());

        let resolved = resolve_import_path("media://inbound/clip.mp3").expect("resolve");
        assert_eq!(resolved, file);
    }

    #[test]
    fn rejects_parent_dir_escape() {
        let _lock = env_lock().lock().expect("env lock");
        let dir = tempfile::tempdir().expect("tempdir");
        let _env = EnvGuard::set(OPENCLAW_MEDIA_INBOUND_DIR_ENV, dir.path().to_str().unwrap());

        let err = resolve_import_path("media://inbound/../../etc/passwd")
            .expect_err("must reject escape");
        assert!(
            err.to_string().contains("escapes media root"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn plain_path_unchanged_when_file_exists() {
        let _lock = env_lock().lock().expect("env lock");
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("local.wav");
        fs::write(&file, b"wav").expect("write");

        let resolved = resolve_import_path(file.to_str().unwrap()).expect("resolve");
        assert_eq!(resolved, file);
    }

    #[test]
    fn missing_file_is_invalid_input() {
        let _lock = env_lock().lock().expect("env lock");
        let dir = tempfile::tempdir().expect("tempdir");
        let _env = EnvGuard::set(OPENCLAW_MEDIA_INBOUND_DIR_ENV, dir.path().to_str().unwrap());

        let err = resolve_import_path("media://inbound/missing.mp3").expect_err("missing");
        assert!(
            err.to_string().contains("file not found"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn empty_inbound_filename_rejected() {
        let _lock = env_lock().lock().expect("env lock");
        let dir = tempfile::tempdir().expect("tempdir");
        let _env = EnvGuard::set(OPENCLAW_MEDIA_INBOUND_DIR_ENV, dir.path().to_str().unwrap());

        let err = resolve_import_path("media://inbound/").expect_err("empty");
        assert!(
            err.to_string().contains("missing a filename"),
            "unexpected error: {err}"
        );
    }
}
