//! Summary client for OpenAI-compatible chat completion APIs.

use crate::config::SummaryConfig;
use crate::models::{SummaryFormat, SummaryTemplate};
use crate::transcription::TranscriptionResponse;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const REFINE_CHUNK_CHARS: usize = 3000;

pub struct SummaryClient {
    client: reqwest::Client,
    config: SummaryConfig,
}

#[derive(Debug, Clone)]
pub struct SummarizeOptions {
    pub template: SummaryTemplate,
    pub format: SummaryFormat,
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
            options.format.clone(),
            &transcript_text,
            options
                .language
                .as_deref()
                .or(self.config.language.as_deref()),
        );

        let url = self.config.resolve_base_url();

        let request = ChatRequest {
            model: self.config.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt(options.template.clone(), options.format.clone()),
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
            "Sending summary request to {} (model: {}, template: {:?}, format: {:?})",
            url,
            self.config.model,
            options.template,
            options.format
        );

        let content = self.send_chat_request(&url, &request, "summary").await?;

        // Only parse sections for markdown format
        let (key_points, action_items, decisions) = match options.format {
            SummaryFormat::Markdown => parse_sections(&content, options.template.clone()),
            SummaryFormat::RawText => (Vec::new(), Vec::new(), Vec::new()),
        };

        Ok(SummarizeResult {
            content,
            key_points,
            action_items,
            decisions,
        })
    }

    /// Refine a raw transcript using LLM to improve formatting, punctuation, and readability.
    /// Returns the refined text as a String.
    pub async fn refine(&self, transcript: &TranscriptionResponse) -> Result<String> {
        let system_prompt = "You are a transcript refinement assistant. Your task is to improve raw transcripts by:\n\
            1. Adding proper punctuation and capitalization\n\
            2. Structuring the text into clear paragraphs based on topic shifts and speaker changes\n\
            3. Fixing transcription errors and making the text more readable\n\
            4. Preserving all original content and meaning\n\
            5. Keeping any speaker labels (like SPEAKER_00, SPEAKER_01) intact\n\
            6. Maintaining the original language(s) of the transcript\n\n\
            Output ONLY the refined transcript text without any additional commentary or metadata.";

        let url = self.config.resolve_base_url();
        let chunks = chunk_refinement_input(transcript, REFINE_CHUNK_CHARS);
        log::info!(
            "Sending refinement request to {} (model: {}, chunks: {})",
            url,
            self.config.model,
            chunks.len()
        );

        let mut refined_chunks = Vec::with_capacity(chunks.len());
        for (idx, chunk) in chunks.iter().enumerate() {
            let user_prompt = if chunks.len() == 1 {
                format!("Refine this transcript:\n\n{chunk}")
            } else {
                format!(
                    "Refine this transcript chunk ({}/{}). Do not summarize. Do not add chunk labels.\n\n{}",
                    idx + 1,
                    chunks.len(),
                    chunk
                )
            };

            let request = ChatRequest {
                model: self.config.model.clone(),
                messages: vec![
                    ChatMessage {
                        role: "system".to_string(),
                        content: system_prompt.to_string(),
                    },
                    ChatMessage {
                        role: "user".to_string(),
                        content: user_prompt,
                    },
                ],
                temperature: 0.3,
                max_tokens: self.config.max_tokens.max(1024),
            };

            log::info!(
                "Sending refinement chunk {}/{} ({} chars)",
                idx + 1,
                chunks.len(),
                chunk.len()
            );
            refined_chunks.push(
                self.send_chat_request(&url, &request, "refinement")
                    .await?
                    .trim()
                    .to_string(),
            );
        }

        Ok(refined_chunks.join("\n\n"))
    }

    async fn send_chat_request(
        &self,
        url: &str,
        request: &ChatRequest,
        operation: &str,
    ) -> Result<String> {
        let mut request_builder = self.client.post(url).json(request);

        if let Some(api_key) = &self.config.api_key {
            request_builder =
                request_builder.header("Authorization", format!("Bearer {}", api_key));
        }

        let response = request_builder
            .send()
            .await
            .with_context(|| format!("Failed to send {operation} request"))?;

        let status = response.status();
        log::info!("{} request completed with status: {}", operation, status);

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            log::error!(
                "{} API request failed with status {}: {}",
                operation,
                status,
                error_text
            );
            anyhow::bail!(
                "{} API request failed with status {}: {}",
                operation,
                status,
                error_text
            );
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .with_context(|| format!("Failed to parse {operation} response"))?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .with_context(|| format!("{operation} response contained no choices"))
    }
}

fn chunk_refinement_input(transcript: &TranscriptionResponse, max_chars: usize) -> Vec<String> {
    let segments = transcript.segments.as_deref().unwrap_or(&[]);
    if !segments.is_empty() {
        let lines = segments.iter().map(|s| {
            let text = s.text.trim();
            match s.speaker.as_deref() {
                Some(speaker) if !speaker.trim().is_empty() => format!("{}: {}", speaker, text),
                _ => text.to_string(),
            }
        });
        return chunk_lines(lines, max_chars);
    }

    chunk_words(&transcript.text, max_chars)
}

fn chunk_lines<I>(lines: I, max_chars: usize) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }

        if line.len() > max_chars {
            if !current.trim().is_empty() {
                chunks.push(current.trim().to_string());
                current.clear();
            }
            chunks.extend(chunk_words(&line, max_chars));
            continue;
        }

        let separator_len = if current.is_empty() { 0 } else { 1 };
        if current.len() + separator_len + line.len() > max_chars && !current.is_empty() {
            chunks.push(current.trim().to_string());
            current.clear();
        }

        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line.trim());
    }

    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }

    if chunks.is_empty() {
        chunks.push(String::new());
    }

    chunks
}

fn chunk_words(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        let separator_len = if current.is_empty() { 0 } else { 1 };
        if current.len() + separator_len + word.len() > max_chars && !current.is_empty() {
            chunks.push(current.trim().to_string());
            current.clear();
        }

        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }

    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }

    if chunks.is_empty() {
        chunks.push(String::new());
    }

    chunks
}

fn system_prompt(template: SummaryTemplate, format: SummaryFormat) -> String {
    let section_desc = match template {
        SummaryTemplate::KeyPoints => "key points",
        SummaryTemplate::ActionItems => "action items",
        SummaryTemplate::Decisions => "decisions",
        SummaryTemplate::Full => "key points, action items, and decisions",
    };
    
    match format {
        SummaryFormat::Markdown => {
            format!(
                "You are a meeting notes assistant. Read the transcript and produce a structured summary in Markdown. \
                 Always wrap each section in a Markdown heading using exactly these names: \
                 '## Key Points', '## Action Items', '## Decisions'. Under each heading, list items as \
                 Markdown bullet points (one per line, starting with '- '). \
                 For this request, focus on: {section_desc}. If a section has no content, include the \
                 heading anyway with a single line: '- (none)'. Be concise and factual."
            )
        }
        SummaryFormat::RawText => {
            format!(
                "You are a meeting notes assistant. Read the transcript and produce a plain text summary WITHOUT any markdown formatting. \
                 Organize the summary with clear section labels: 'Key Points:', 'Action Items:', 'Decisions:'. \
                 List each item on a new line without bullet points or markdown syntax. \
                 For this request, focus on: {section_desc}. If a section has no content, write '(none)'. \
                 Use simple, readable plain text formatting only. Be concise and factual."
            )
        }
    }
}

fn build_prompt(template: SummaryTemplate, format: SummaryFormat, transcript: &str, language: Option<&str>) -> String {
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
    let format_note = match format {
        SummaryFormat::Markdown => " Use markdown formatting with headings and bullet points.",
        SummaryFormat::RawText => " Use plain text without any markdown syntax.",
    };
    format!(
        "Please summarize the following meeting transcript.\n\n\
         Sections to include: {template_name}.{format_note}{lang_note}\n\n\
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
            timestamp: None,
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
            refined_text: None,
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
            refined_text: None,
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
            refined_text: None,
        };
        let out = format_transcript(&t);
        assert_eq!(out, "Fallback text");
    }

    #[test]
    fn test_build_prompt_key_points() {
        let p = build_prompt(SummaryTemplate::KeyPoints, SummaryFormat::Markdown, "transcript text", None);
        assert!(p.contains("key points only"));
        assert!(p.contains("transcript text"));
    }

    #[test]
    fn test_build_prompt_full() {
        let p = build_prompt(SummaryTemplate::Full, SummaryFormat::Markdown, "t", Some("zh"));
        assert!(p.contains("all three sections"));
        assert!(p.contains("language: zh"));
    }

    #[test]
    fn test_build_prompt_raw_text() {
        let p = build_prompt(SummaryTemplate::Full, SummaryFormat::RawText, "t", None);
        assert!(p.contains("plain text"));
        assert!(p.contains("without any markdown syntax"));
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
        let p = system_prompt(SummaryTemplate::Full, SummaryFormat::Markdown);
        assert!(p.contains("key points, action items, and decisions"));
    }

    #[test]
    fn test_system_prompt_raw_text() {
        let p = system_prompt(SummaryTemplate::Full, SummaryFormat::RawText);
        assert!(p.contains("plain text"));
        assert!(!p.contains("Markdown"));
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
