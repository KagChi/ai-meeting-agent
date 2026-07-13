//! Summary client for OpenAI-compatible chat completion APIs.

use crate::config::SummaryConfig;
use crate::models::SummaryTemplate;
use crate::transcription::TranscriptionResponse;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub struct SummaryClient {
    client: reqwest::Client,
    config: SummaryConfig,
}

#[derive(Debug, Clone)]
pub struct SummarizeOptions {
    pub template: SummaryTemplate,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummarizeResult {
    pub content: String,
    pub key_points: Vec<String>,
    pub action_items: Vec<String>,
    pub decisions: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    content: String,
}

impl SummaryClient {
    pub fn new(config: SummaryConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("Failed to build HTTP client")?;
        Ok(Self { client, config })
    }

    pub async fn summarize(
        &self,
        transcript: &TranscriptionResponse,
        options: &SummarizeOptions,
    ) -> Result<SummarizeResult> {
        let transcript_text = format_transcript(transcript);
        let prompt = build_prompt(
            options.template.clone(),
            &transcript_text,
            options
                .language
                .as_deref()
                .or(self.config.language.as_deref()),
        );

        let api_key = self
            .config
            .api_key
            .as_ref()
            .context("SUMMARY_API_KEY is required but not set")?;

        let url = self.config.resolve_base_url();

        let request = ChatRequest {
            model: self.config.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt(options.template.clone()),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: prompt,
                },
            ],
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
        };

        log::info!(
            "Sending summary request to {} (model: {}, template: {:?})",
            url,
            self.config.model,
            options.template
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&request)
            .send()
            .await
            .context("Failed to send summary request")?;

        let status = response.status();
        log::info!("Summary request completed with status: {}", status);

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            log::error!(
                "Summary API request failed with status {}: {}",
                status,
                error_text
            );
            anyhow::bail!(
                "Summary API request failed with status {}: {}",
                status,
                error_text
            );
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .context("Failed to parse summary response")?;

        let content = chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .context("Summary response contained no choices")?;

        let (key_points, action_items, decisions) =
            parse_sections(&content, options.template.clone());

        Ok(SummarizeResult {
            content,
            key_points,
            action_items,
            decisions,
        })
    }
}

fn system_prompt(template: SummaryTemplate) -> String {
    let section_desc = match template {
        SummaryTemplate::KeyPoints => "key points",
        SummaryTemplate::ActionItems => "action items",
        SummaryTemplate::Decisions => "decisions",
        SummaryTemplate::Full => "key points, action items, and decisions",
    };
    format!(
        "You are a meeting notes assistant. Read the transcript and produce a structured summary in Markdown. \
         Always wrap each section in a Markdown heading using exactly these names: \
         '## Key Points', '## Action Items', '## Decisions'. Under each heading, list items as \
         Markdown bullet points (one per line, starting with '- '). \
         For this request, focus on: {section_desc}. If a section has no content, include the \
         heading anyway with a single line: '- (none)'. Be concise and factual."
    )
}

fn build_prompt(template: SummaryTemplate, transcript: &str, language: Option<&str>) -> String {
    let template_name = match template {
        SummaryTemplate::KeyPoints => "key points only",
        SummaryTemplate::ActionItems => "action items only",
        SummaryTemplate::Decisions => "decisions only",
        SummaryTemplate::Full => "key points, action items, and decisions (all three sections)",
    };
    let lang_note = match language {
        Some(l) => format!("\n\nWrite the summary in this language: {l}."),
        None => String::new(),
    };
    format!(
        "Please summarize the following meeting transcript.\n\n\
         Sections to include: {template_name}.{lang_note}\n\n\
         --- TRANSCRIPT START ---\n{transcript}\n--- TRANSCRIPT END ---"
    )
}

fn format_transcript(transcript: &TranscriptionResponse) -> String {
    let segments = transcript.segments.as_deref().unwrap_or(&[]);
    if !segments.is_empty() {
        segments
            .iter()
            .map(|s| {
                let mm = (s.start as u64) / 60;
                let ss = (s.start as u64) % 60;
                format!("[{mm:02}:{ss:02}] {}", s.text.trim())
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        transcript.text.clone()
    }
}

fn parse_sections(
    content: &str,
    template: SummaryTemplate,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut key_points = Vec::new();
    let mut action_items = Vec::new();
    let mut decisions = Vec::new();

    let include_kp = matches!(template, SummaryTemplate::KeyPoints | SummaryTemplate::Full);
    let include_ai = matches!(
        template,
        SummaryTemplate::ActionItems | SummaryTemplate::Full
    );
    let include_dec = matches!(template, SummaryTemplate::Decisions | SummaryTemplate::Full);

    if include_kp {
        key_points = extract_section(content, &["## Key Points", "# Key Points", "Key Points:"]);
    }
    if include_ai {
        action_items = extract_section(
            content,
            &["## Action Items", "# Action Items", "Action Items:"],
        );
    }
    if include_dec {
        decisions = extract_section(content, &["## Decisions", "# Decisions", "Decisions:"]);
    }

    (key_points, action_items, decisions)
}

fn extract_section(content: &str, headers: &[&str]) -> Vec<String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut start: Option<usize> = None;
    let mut header_len = 0;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        for h in headers {
            if trimmed.eq_ignore_ascii_case(h) {
                start = Some(i + 1);
                header_len = h.len();
                break;
            }
        }
        if start.is_some() {
            break;
        }
    }

    let start = match start {
        Some(s) => s,
        None => return Vec::new(),
    };

    let mut items = Vec::new();
    let _ = header_len;
    for line in &lines[start..] {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if (trimmed.starts_with('#') || is_section_header(trimmed)) && !trimmed.starts_with('-') {
            break;
        }
        if trimmed == "- (none)" {
            continue;
        }
        if let Some(item) = trimmed.strip_prefix("- ") {
            let cleaned = item.trim().to_string();
            if !cleaned.is_empty() {
                items.push(cleaned);
            }
        } else if let Some(item) = trimmed.strip_prefix("* ") {
            let cleaned = item.trim().to_string();
            if !cleaned.is_empty() {
                items.push(cleaned);
            }
        } else if !trimmed.starts_with('-') && !trimmed.starts_with('*') {
            let cleaned = trimmed.to_string();
            if !cleaned.is_empty() {
                items.push(cleaned);
            }
        }
    }

    items
}

fn is_section_header(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.starts_with("key points")
        || lower.starts_with("action items")
        || lower.starts_with("decisions")
        || lower.starts_with("summary")
        || lower.starts_with("notes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::TranscriptSegment;

    fn make_segment(start: f64, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            id: 0,
            start,
            end: start + 1.0,
            text: text.to_string(),
            tokens: None,
            temperature: None,
            avg_logprob: None,
            compression_ratio: None,
            no_speech_prob: None,
            speaker: None,
        }
    }

    #[test]
    fn test_format_transcript() {
        let t = TranscriptionResponse {
            text: String::new(),
            language: None,
            duration: None,
            segments: Some(vec![
                make_segment(0.0, "Hello world"),
                make_segment(65.0, "Second line"),
            ]),
        };
        let out = format_transcript(&t);
        assert!(out.contains("[00:00] Hello world"));
        assert!(out.contains("[01:05] Second line"));
    }

    #[test]
    fn test_format_transcript_no_segments_fallback() {
        let t = TranscriptionResponse {
            text: "Hello world from text".to_string(),
            language: None,
            duration: None,
            segments: None,
        };
        let out = format_transcript(&t);
        assert_eq!(out, "Hello world from text");
    }

    #[test]
    fn test_format_transcript_empty_segments_fallback() {
        let t = TranscriptionResponse {
            text: "Fallback text".to_string(),
            language: None,
            duration: None,
            segments: Some(vec![]),
        };
        let out = format_transcript(&t);
        assert_eq!(out, "Fallback text");
    }

    #[test]
    fn test_build_prompt_key_points() {
        let p = build_prompt(SummaryTemplate::KeyPoints, "transcript text", None);
        assert!(p.contains("key points only"));
        assert!(p.contains("transcript text"));
    }

    #[test]
    fn test_build_prompt_full() {
        let p = build_prompt(SummaryTemplate::Full, "t", Some("zh"));
        assert!(p.contains("all three sections"));
        assert!(p.contains("language: zh"));
    }

    #[test]
    fn test_parse_sections_full() {
        let content = "## Key Points\n- Point one\n- Point two\n\n## Action Items\n- Do X\n\n## Decisions\n- Decided Y\n";
        let (kp, ai, dec) = parse_sections(content, SummaryTemplate::Full);
        assert_eq!(kp, vec!["Point one".to_string(), "Point two".to_string()]);
        assert_eq!(ai, vec!["Do X".to_string()]);
        assert_eq!(dec, vec!["Decided Y".to_string()]);
    }

    #[test]
    fn test_parse_sections_single_template() {
        let content = "## Key Points\n- Only point\n\n## Action Items\n- Should be ignored\n";
        let (kp, _ai, _dec) = parse_sections(content, SummaryTemplate::KeyPoints);
        assert_eq!(kp, vec!["Only point".to_string()]);
    }

    #[test]
    fn test_parse_sections_missing_section() {
        let content = "Some intro text without any headers.";
        let (kp, ai, dec) = parse_sections(content, SummaryTemplate::Full);
        assert!(kp.is_empty());
        assert!(ai.is_empty());
        assert!(dec.is_empty());
    }

    #[test]
    fn test_parse_sections_none_marker() {
        let content = "## Key Points\n- (none)\n";
        let (kp, _ai, _dec) = parse_sections(content, SummaryTemplate::KeyPoints);
        assert!(kp.is_empty());
    }

    #[test]
    fn test_extract_section_alt_headers() {
        let content = "# Key Points\n- A\n\n# Action Items\n- B\n";
        let kp = extract_section(content, &["## Key Points", "# Key Points", "Key Points:"]);
        assert_eq!(kp, vec!["A".to_string()]);
    }

    #[test]
    fn test_system_prompt_full() {
        let p = system_prompt(SummaryTemplate::Full);
        assert!(p.contains("key points, action items, and decisions"));
    }

    #[test]
    fn test_resolve_base_url_openai() {
        let cfg = SummaryConfig {
            provider: "openai".to_string(),
            api_key: None,
            base_url: String::new(),
            model: "gpt-4o-mini".to_string(),
            temperature: 0.3,
            max_tokens: 1024,
            language: None,
        };
        let url = cfg.resolve_base_url();
        assert_eq!(url, "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn test_resolve_base_url_groq() {
        let cfg = SummaryConfig {
            provider: "groq".to_string(),
            api_key: None,
            base_url: String::new(),
            model: "llama-3.1-70b".to_string(),
            temperature: 0.3,
            max_tokens: 1024,
            language: None,
        };
        let url = cfg.resolve_base_url();
        assert_eq!(url, "https://api.groq.com/openai/v1/chat/completions");
    }

    #[test]
    fn test_resolve_base_url_custom() {
        let cfg = SummaryConfig {
            provider: "custom".to_string(),
            api_key: None,
            base_url: "https://my.endpoint/v1".to_string(),
            model: "m".to_string(),
            temperature: 0.3,
            max_tokens: 1024,
            language: None,
        };
        let url = cfg.resolve_base_url();
        assert_eq!(url, "https://my.endpoint/v1/chat/completions");
    }

    #[test]
    fn test_resolve_base_url_already_full() {
        let cfg = SummaryConfig {
            provider: "custom".to_string(),
            api_key: None,
            base_url: "https://my.endpoint/v1/chat/completions".to_string(),
            model: "m".to_string(),
            temperature: 0.3,
            max_tokens: 1024,
            language: None,
        };
        let url = cfg.resolve_base_url();
        assert_eq!(url, "https://my.endpoint/v1/chat/completions");
    }
}
