use crate::error::{ClientError, Result};
use reqwest::multipart;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Clone)]
pub struct MeetingAgentClient {
    base_url: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

impl MeetingAgentClient {
    pub fn new(base_url: String, api_key: Option<String>) -> Self {
        Self {
            base_url,
            api_key,
            client: reqwest::Client::new(),
        }
    }

    pub async fn import_meeting_audio(
        &self,
        file_path: &str,
        title: Option<&str>,
    ) -> Result<Value> {
        let path = Path::new(file_path);
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| ClientError::InvalidInput("file_path must point to a file".to_string()))?
            .to_string();

        let bytes = tokio::fs::read(path).await?;
        let part = multipart::Part::bytes(bytes).file_name(filename);
        let mut form = multipart::Form::new().part("file", part);
        if let Some(title) = title.filter(|value| !value.trim().is_empty()) {
            form = form.text("title", title.to_string());
        }

        let request = self
            .authed(self.client.post(self.url("/import")))
            .multipart(form);
        self.send_json(request).await
    }

    pub async fn get_job_status(&self, job_id: &str) -> Result<Value> {
        self.get_json(&format!("/jobs/{job_id}/status")).await
    }

    pub async fn list_meetings(&self) -> Result<Value> {
        self.get_json("/meetings").await
    }

    pub async fn get_meeting(&self, meeting_id: &str) -> Result<Value> {
        self.get_json(&format!("/meetings/{meeting_id}")).await
    }

    pub async fn get_transcript(&self, meeting_id: &str) -> Result<Value> {
        self.get_json(&format!("/meetings/{meeting_id}/transcript"))
            .await
    }

    pub async fn generate_summary(
        &self,
        meeting_id: &str,
        template: &str,
        language: Option<&str>,
    ) -> Result<Value> {
        let request = self
            .authed(
                self.client
                    .post(self.url(&format!("/meetings/{meeting_id}/summary"))),
            )
            .json(&json!({
                "template": template,
                "language": language,
            }));
        self.send_json(request).await
    }

    pub async fn get_summary(&self, meeting_id: &str, template: &str) -> Result<Value> {
        self.get_json(&format!("/meetings/{meeting_id}/summary/{template}"))
            .await
    }

    pub async fn update_meeting(
        &self,
        meeting_id: &str,
        title: Option<String>,
        date: Option<String>,
    ) -> Result<Value> {
        if title.is_none() && date.is_none() {
            return Err(ClientError::InvalidInput(
                "title or date must be provided".to_string(),
            ));
        }

        let request = self
            .authed(
                self.client
                    .patch(self.url(&format!("/meetings/{meeting_id}"))),
            )
            .json(&json!({
                "title": title,
                "date": date,
            }));
        self.send_json(request).await
    }

    pub async fn delete_meeting(&self, meeting_id: &str) -> Result<Value> {
        let request = self.authed(
            self.client
                .delete(self.url(&format!("/meetings/{meeting_id}"))),
        );
        let response = request.send().await?;
        self.ensure_success(response).await?;
        Ok(json!({ "meeting_id": meeting_id, "deleted": true }))
    }

    pub async fn cancel_job(&self, job_id: &str) -> Result<Value> {
        let request = self.authed(
            self.client
                .post(self.url(&format!("/jobs/{job_id}/cancel"))),
        );
        self.send_json(request).await
    }

    pub async fn stream_job_events(&self, job_id: &str) -> Result<String> {
        let request = self.authed(self.client.get(self.url(&format!("/jobs/{job_id}/events"))));
        let response = request.send().await?;
        let response = self.ensure_success(response).await?;
        Ok(response.text().await?)
    }

    async fn get_json(&self, path: &str) -> Result<Value> {
        let request = self.authed(self.client.get(self.url(path)));
        self.send_json(request).await
    }

    async fn send_json(&self, request: reqwest::RequestBuilder) -> Result<Value> {
        let response = request.send().await?;
        let response = self.ensure_success(response).await?;
        Ok(response.json::<Value>().await?)
    }

    async fn ensure_success(&self, response: reqwest::Response) -> Result<reqwest::Response> {
        if response.status().is_success() {
            return Ok(response);
        }

        let status = response.status();
        let text = response
            .text()
            .await
            .unwrap_or_else(|_| "<empty body>".to_string());
        let message = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| {
                        value
                            .get("details")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
            })
            .unwrap_or(text);
        Err(ClientError::Api { status, message })
    }

    fn authed(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(api_key) => request.header("X-API-Key", api_key),
            None => request,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}
