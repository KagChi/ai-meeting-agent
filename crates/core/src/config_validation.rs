//! Configuration validation shared by the HTTP API and CLI.
//!
//! Each validator returns `Ok(())` on success or a `Vec<String>` of
//! human-readable errors joined with `; ` when wrapped by [`validate_config`].

use crate::config::{Config, DiarizeConfig, ServerConfig, SummaryConfig, TranscriptionConfig};

/// Validate the full config, returning all errors found across sections.
pub fn validate_config(c: &Config) -> Result<(), Vec<String>> {
    let mut errs = Vec::new();
    if let Err(e) = validate_transcription(&c.transcription) {
        errs.extend(e);
    }
    if let Err(e) = validate_summary(&c.summary) {
        errs.extend(e);
    }
    if let Err(e) = validate_server(&c.server) {
        errs.extend(e);
    }
    if let Err(e) = validate_diarize(&c.diarize) {
        errs.extend(e);
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

pub fn validate_transcription(t: &TranscriptionConfig) -> Result<(), Vec<String>> {
    let mut errs = Vec::new();
    if t.provider.trim().is_empty() {
        errs.push("transcription.provider must not be empty".into());
    }
    if let Err(e) = validate_url(&t.base_url) {
        errs.push(format!("transcription.base_url: {e}"));
    }
    if t.model.trim().is_empty() {
        errs.push("transcription.model must not be empty".into());
    }
    if t.chunk_seconds < 0.0 {
        errs.push("transcription.chunk_seconds must be >= 0".into());
    }
    if t.chunk_concurrency == 0 {
        errs.push("transcription.chunk_concurrency must be >= 1".into());
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

pub fn validate_summary(s: &SummaryConfig) -> Result<(), Vec<String>> {
    let mut errs = Vec::new();
    if s.provider.trim().is_empty() {
        errs.push("summary.provider must not be empty".into());
    }
    if let Err(e) = validate_url(&s.base_url) {
        errs.push(format!("summary.base_url: {e}"));
    }
    if s.model.trim().is_empty() {
        errs.push("summary.model must not be empty".into());
    }
    if !(0.0..=2.0).contains(&s.temperature) {
        errs.push("summary.temperature must be between 0.0 and 2.0".into());
    }
    if s.max_tokens < 32 || s.max_tokens > 8192 {
        errs.push("summary.max_tokens must be between 32 and 8192".into());
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

pub fn validate_server(s: &ServerConfig) -> Result<(), Vec<String>> {
    let mut errs = Vec::new();
    if s.port == 0 {
        errs.push("server.port must be between 1 and 65535".into());
    }
    if s.host.trim().is_empty() {
        errs.push("server.host must not be empty".into());
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

pub fn validate_diarize(d: &DiarizeConfig) -> Result<(), Vec<String>> {
    if !d.enabled {
        return Ok(());
    }
    let mut errs = Vec::new();
    let valid_modes = [
        "cpu",
        "coreml",
        "coreml-fast",
        "cuda",
        "cuda-fast",
        "migraphx",
    ];
    if !valid_modes.contains(&d.execution_mode.to_lowercase().as_str()) {
        errs.push(format!(
            "diarize.execution_mode must be one of {:?} (got {})",
            valid_modes, d.execution_mode
        ));
    }
    if let Some(dir) = &d.model_dir {
        if !dir.exists() {
            errs.push(format!("diarize.model_dir not found: {}", dir.display()));
        }
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

/// Validate a URL. Empty string is rejected. Must parse as http(s).
fn validate_url(url: &str) -> Result<(), String> {
    if url.trim().is_empty() {
        return Err("must not be empty".into());
    }
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("invalid URL: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!(
            "scheme must be http or https, got {}",
            parsed.scheme()
        ));
    }
    Ok(())
}

/// Sentinel used by the HTTP API and CLI to mean "keep the existing key".
pub const MASK_SENTINEL: &str = "****";

/// Resolve an incoming api_key value against the existing one.
/// - `None` (omitted or null) → clear (None)
/// - `Some(MASK_SENTINEL)` → keep existing
/// - `Some(other)` → replace
pub fn resolve_secret(incoming: &Option<String>, existing: &Option<String>) -> Option<String> {
    match incoming {
        None => None,
        Some(v) if v == MASK_SENTINEL => existing.clone(),
        Some(v) => Some(v.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good_transcription() -> TranscriptionConfig {
        TranscriptionConfig {
            provider: "openai".into(),
            api_key: None,
            base_url: "https://api.openai.com/v1".into(),
            model: "whisper-1".into(),
            chunk_seconds: 600.0,
            chunk_concurrency: 2,
        }
    }

    fn good_summary() -> SummaryConfig {
        SummaryConfig {
            provider: "openai".into(),
            api_key: None,
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-4o-mini".into(),
            temperature: 0.3,
            max_tokens: 1024,
            language: None,
        }
    }

    fn good_server() -> ServerConfig {
        ServerConfig {
            port: 8080,
            host: "127.0.0.1".into(),
            api_key: None,
        }
    }

    fn good_diarize() -> DiarizeConfig {
        DiarizeConfig {
            enabled: false,
            execution_mode: "cpu".into(),
            model_dir: None,
        }
    }

    #[test]
    fn valid_config_passes() {
        let c = Config {
            transcription: good_transcription(),
            summary: good_summary(),
            server: good_server(),
            diarize: good_diarize(),
        };
        assert!(validate_config(&c).is_ok());
    }

    #[test]
    fn empty_provider_rejected() {
        let mut t = good_transcription();
        t.provider = "".into();
        let errs = validate_transcription(&t).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("provider")));
    }

    #[test]
    fn bad_url_rejected() {
        let mut t = good_transcription();
        t.base_url = "not a url".into();
        assert!(validate_transcription(&t).is_err());
    }

    #[test]
    fn ftp_scheme_rejected() {
        let mut t = good_transcription();
        t.base_url = "ftp://example.com".into();
        let errs = validate_transcription(&t).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("scheme")));
    }

    #[test]
    fn temperature_out_of_range() {
        let mut s = good_summary();
        s.temperature = 3.0;
        let errs = validate_summary(&s).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("temperature")));
    }

    #[test]
    fn max_tokens_boundary() {
        let mut s = good_summary();
        s.max_tokens = 31;
        assert!(validate_summary(&s).is_err());
        s.max_tokens = 32;
        assert!(validate_summary(&s).is_ok());
        s.max_tokens = 8192;
        assert!(validate_summary(&s).is_ok());
        s.max_tokens = 8193;
        assert!(validate_summary(&s).is_err());
    }

    #[test]
    fn diarize_disabled_skips_validation() {
        let mut d = good_diarize();
        d.execution_mode = "garbage".into();
        d.enabled = false;
        assert!(validate_diarize(&d).is_ok());
    }

    #[test]
    fn diarize_enabled_validates() {
        let mut d = good_diarize();
        d.enabled = true;
        d.execution_mode = "garbage".into();
        assert!(validate_diarize(&d).is_err());
        d.execution_mode = "cpu".into();
        assert!(validate_diarize(&d).is_ok());
    }

    #[test]
    fn resolve_secret_keep() {
        assert_eq!(
            resolve_secret(&Some(MASK_SENTINEL.into()), &Some("real".into())),
            Some("real".into())
        );
    }

    #[test]
    fn resolve_secret_replace() {
        assert_eq!(
            resolve_secret(&Some("new".into()), &Some("old".into())),
            Some("new".into())
        );
    }

    #[test]
    fn resolve_secret_clear() {
        assert_eq!(resolve_secret(&None, &Some("old".into())), None);
    }

    #[test]
    fn resolve_secret_clear_when_no_existing() {
        assert_eq!(resolve_secret(&None, &None), None);
    }

    #[test]
    fn chunk_concurrency_zero_rejected() {
        let mut t = good_transcription();
        t.chunk_concurrency = 0;
        assert!(validate_transcription(&t).is_err());
    }
}
