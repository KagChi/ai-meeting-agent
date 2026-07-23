//! Vexa HTTP + recording download client.

use super::config::OrchestratorConfig;
use super::models::MeetingEndedEvent;
use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::Client;

#[derive(Debug, Clone)]
pub struct DownloadedRecording {
    pub bytes: Vec<u8>,
    pub filename: String,
}

#[derive(Clone)]
pub struct VexaClient {
    http: Client,
    config: OrchestratorConfig,
}

impl VexaClient {
    pub fn new(config: OrchestratorConfig) -> Result<Self> {
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .context("Failed to build HTTP client for Vexa")?;
        Ok(Self { http, config })
    }

    fn auth_headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        if let Some(key) = &self.config.vexa_api_key {
            if !key.is_empty() {
                headers.insert(
                    "X-API-Key",
                    HeaderValue::from_str(key).context("Invalid VEXA_API_KEY header value")?,
                );
                // Some gateways accept Bearer as well
                if let Ok(v) = HeaderValue::from_str(&format!("Bearer {key}")) {
                    headers.insert(AUTHORIZATION, v);
                }
            }
        }
        Ok(headers)
    }

    /// Resolve recording bytes for a meeting-ended event.
    pub async fn download_recording(&self, event: &MeetingEndedEvent) -> Result<DownloadedRecording> {
        if let Some(url) = event.recording_url.as_deref() {
            if !url.is_empty() {
                return self.download_url(url, event.filename.as_deref()).await;
            }
        }

        // Try Vexa API: GET /recordings?platform=&native_meeting_id=
        if let Some(base) = &self.config.vexa_api_base {
            if let (Some(platform), Some(native)) =
                (event.platform.as_deref(), event.native_meeting_id.as_deref())
            {
                if let Some(rec) = self
                    .try_fetch_via_recordings_api(base, platform, native, event.filename.as_deref())
                    .await?
                {
                    return Ok(rec);
                }
            }

            // Path-style: GET /recordings/{platform}/{native_meeting_id}
            if let (Some(platform), Some(native)) =
                (event.platform.as_deref(), event.native_meeting_id.as_deref())
            {
                let url = format!(
                    "{}/recordings/{}/{}",
                    base.trim_end_matches('/'),
                    urlencoding_lite(platform),
                    urlencoding_lite(native)
                );
                if let Ok(rec) = self.download_url(&url, event.filename.as_deref()).await {
                    return Ok(rec);
                }
            }
        }

        // MinIO path-style URL construction (unsigned GET — works when bucket is public
        // or when endpoint is an internal pre-signed URL already provided as recording_url).
        if let Some(key) = event.recording_key.as_deref() {
            if let Some(url) = self.minio_object_url(key) {
                return self.download_url(&url, event.filename.as_deref()).await;
            }
        }

        bail!(
            "Cannot resolve recording: set recording_url, or VEXA_API_BASE + platform/native_meeting_id, or MINIO_* + recording_key"
        );
    }

    async fn try_fetch_via_recordings_api(
        &self,
        base: &str,
        platform: &str,
        native: &str,
        filename_hint: Option<&str>,
    ) -> Result<Option<DownloadedRecording>> {
        let list_url = format!(
            "{}/recordings?platform={}&native_meeting_id={}",
            base.trim_end_matches('/'),
            urlencoding_lite(platform),
            urlencoding_lite(native)
        );
        let headers = self.auth_headers()?;
        let resp = self.http.get(&list_url).headers(headers).send().await;

        let Ok(resp) = resp else {
            return Ok(None);
        };
        if !resp.status().is_success() {
            log::debug!(
                "Vexa GET /recordings query returned {}",
                resp.status()
            );
            return Ok(None);
        }

        let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
        let url = extract_recording_url(&body);
        if let Some(url) = url {
            return Ok(Some(self.download_url(&url, filename_hint).await?));
        }
        Ok(None)
    }

    pub async fn download_url(
        &self,
        url: &str,
        filename_hint: Option<&str>,
    ) -> Result<DownloadedRecording> {
        log::info!("Downloading recording from {url}");
        let headers = self.auth_headers()?;
        let resp = self
            .http
            .get(url)
            .headers(headers)
            .send()
            .await
            .with_context(|| format!("Failed to GET recording URL: {url}"))?;

        if !resp.status().is_success() {
            bail!(
                "Recording download failed: HTTP {} from {url}",
                resp.status()
            );
        }

        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let bytes = resp
            .bytes()
            .await
            .context("Failed to read recording body")?
            .to_vec();

        if bytes.is_empty() {
            bail!("Downloaded recording is empty from {url}");
        }

        let filename = filename_hint
            .map(|s| s.to_string())
            .or_else(|| filename_from_url(url))
            .or_else(|| extension_from_content_type(&content_type))
            .unwrap_or_else(|| "recording.webm".to_string());

        Ok(DownloadedRecording { bytes, filename })
    }

    fn minio_object_url(&self, key: &str) -> Option<String> {
        let endpoint = self.config.minio_endpoint.as_ref()?;
        let bucket = self.config.minio_bucket.as_ref()?;
        let scheme = if self.config.minio_secure {
            "https"
        } else {
            "http"
        };
        let endpoint = endpoint
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/');
        let key = key.trim_start_matches('/');
        Some(format!("{scheme}://{endpoint}/{bucket}/{key}"))
    }
}

fn extract_recording_url(body: &serde_json::Value) -> Option<String> {
    // Array of recordings
    if let Some(arr) = body.as_array() {
        for item in arr {
            if let Some(u) = item
                .get("url")
                .or_else(|| item.get("download_url"))
                .or_else(|| item.get("recording_url"))
                .and_then(|v| v.as_str())
            {
                return Some(u.to_string());
            }
        }
    }
    // Object with items / recordings
    for key in ["recordings", "items", "data", "results"] {
        if let Some(arr) = body.get(key).and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(u) = item
                    .get("url")
                    .or_else(|| item.get("download_url"))
                    .or_else(|| item.get("recording_url"))
                    .and_then(|v| v.as_str())
                {
                    return Some(u.to_string());
                }
            }
        }
    }
    body.get("url")
        .or_else(|| body.get("download_url"))
        .or_else(|| body.get("recording_url"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn filename_from_url(url: &str) -> Option<String> {
    let path = url.split('?').next().unwrap_or(url);
    let name = path.rsplit('/').next()?;
    if name.is_empty() || !name.contains('.') {
        return None;
    }
    Some(name.to_string())
}

fn extension_from_content_type(ct: &str) -> Option<String> {
    let ct = ct.split(';').next()?.trim().to_ascii_lowercase();
    let ext = match ct.as_str() {
        "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" => "m4a",
        "audio/webm" | "video/webm" => "webm",
        "audio/ogg" => "ogg",
        "video/mp4" => "mp4",
        _ => return None,
    };
    Some(format!("recording.{ext}"))
}

/// Minimal query escaping (path segments / query values).
fn urlencoding_lite(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_url_from_list() {
        let v = serde_json::json!([
            {"id": "1", "url": "http://example.com/a.webm"}
        ]);
        assert_eq!(
            extract_recording_url(&v).as_deref(),
            Some("http://example.com/a.webm")
        );
    }

    #[test]
    fn minio_url_builds() {
        let mut cfg = OrchestratorConfig::default();
        cfg.minio_endpoint = Some("127.0.0.1:9000".into());
        cfg.minio_bucket = Some("vexa".into());
        let c = VexaClient::new(cfg).unwrap();
        assert_eq!(
            c.minio_object_url("meetings/x.webm").as_deref(),
            Some("http://127.0.0.1:9000/vexa/meetings/x.webm")
        );
    }
}
