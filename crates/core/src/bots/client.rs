//! HTTP client for internal meeting-bot service.

use super::config::MeetingBotConfig;
use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client;
use serde_json::Value;

#[derive(Clone)]
pub struct MeetingBotClient {
    http: Client,
    base: String,
    api_key: Option<String>,
}

impl MeetingBotClient {
    pub fn from_config(cfg: &MeetingBotConfig) -> Result<Self> {
        let base = cfg
            .base_url()
            .context("MEETING_BOT_URL is not set")?
            .trim_end_matches('/')
            .to_string();
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .context("Failed to build meeting-bot HTTP client")?;
        Ok(Self {
            http,
            base,
            api_key: cfg.api_key.clone(),
        })
    }

    fn headers(&self) -> Result<HeaderMap> {
        let mut h = HeaderMap::new();
        if let Some(key) = &self.api_key {
            if !key.is_empty() {
                h.insert(
                    "X-API-Key",
                    HeaderValue::from_str(key).context("invalid MEETING_BOT_INTERNAL_KEY")?,
                );
            }
        }
        h.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        Ok(h)
    }

    pub async fn health(&self) -> Result<Value> {
        self.get_json("/health").await
    }

    pub async fn platforms(&self) -> Result<Value> {
        self.get_json("/platforms").await
    }

    pub async fn list_bots(&self, query: &str) -> Result<Value> {
        let path = if query.is_empty() {
            "/bots".to_string()
        } else {
            format!("/bots?{query}")
        };
        self.get_json(&path).await
    }

    pub async fn get_bot(&self, id: &str) -> Result<(u16, Value)> {
        self.request_json(reqwest::Method::GET, &format!("/bots/{id}"), None)
            .await
    }

    pub async fn create_bot(&self, body: &Value) -> Result<(u16, Value)> {
        self.request_json(reqwest::Method::POST, "/bots", Some(body))
            .await
    }

    pub async fn delete_bot(&self, id: &str) -> Result<(u16, Value)> {
        self.request_json(reqwest::Method::DELETE, &format!("/bots/{id}"), None)
            .await
    }

    async fn get_json(&self, path: &str) -> Result<Value> {
        let (status, v) = self.request_json(reqwest::Method::GET, path, None).await?;
        if !(200..300).contains(&status) {
            bail!("meeting-bot GET {path} → HTTP {status}: {v}");
        }
        Ok(v)
    }

    async fn request_json(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value)> {
        let url = format!("{}{}", self.base, path);
        let mut req = self.http.request(method, &url).headers(self.headers()?);
        if let Some(b) = body {
            req = req.json(b);
        }
        let res = req.send().await.with_context(|| format!("request {url}"))?;
        let status = res.status().as_u16();
        let text = res.text().await.unwrap_or_default();
        let v = if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        };
        Ok((status, v))
    }
}
