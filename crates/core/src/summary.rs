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

/// Result of LLM transcript refinement.
#[derive(Debug, Clone)]
pub struct RefineResult {
    /// Joined document-level refined text (space-separated segment refinements).
    pub refined_text: String,
    /// Per-segment refined text, same length/order as input segments (empty if no segments).
    pub segment_refined: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct MeetingContext {
    pub title: Option<String>,
    pub date: Option<String>,
    pub participants: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct SummarizeOptions {
    pub template: SummaryTemplate,
    pub format: SummaryFormat,
    pub language: Option<String>,
    /// Known meeting metadata injected into the LLM prompt.
    pub meeting: MeetingContext,
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
    /// Always false: prefer a single JSON completion. Some proxies still stream;
    /// response parsing handles both modes.
    stream: bool,
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

#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: StreamDelta,
}

#[derive(Debug, Default, Deserialize)]
struct StreamDelta {
    content: Option<String>,
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
            &options.meeting,
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
            stream: false,
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

    /// Refine a raw transcript using LLM.
    ///
    /// When segments are present, returns refined text **per segment** (same length/order)
    /// plus a joined document-level string. Without segments, only the document string is set.
    pub async fn refine(&self, transcript: &TranscriptionResponse) -> Result<RefineResult> {
        let system_prompt = "You are a transcript refinement assistant. Your task is to improve raw transcripts by:\n\
            1. Adding proper punctuation and capitalization\n\
            2. Fixing transcription errors and making the text more readable\n\
            3. Preserving all original content and meaning\n\
            4. Maintaining the original language(s) of the transcript\n\
            5. Keeping each input line as exactly one output line with the same [N] index prefix\n\
            6. Do NOT merge, split, reorder, or drop lines\n\
            7. Do NOT add commentary, metadata, or speaker labels\n\
            8. Do NOT invent or copy speaker names (e.g. Guest-1, SPEAKER_00) into the refined text\n\n\
            Output format: one line per input line, each starting with the same [N] prefix followed by refined speech text only.";

        let url = self.config.resolve_base_url();
        let segments = transcript.segments.as_deref().unwrap_or(&[]);

        if segments.is_empty() {
            let chunks = chunk_words(&transcript.text, REFINE_CHUNK_CHARS);
            log::info!(
                "Sending refinement request to {} (model: {}, text chunks: {})",
                url,
                self.config.model,
                chunks.len()
            );
            let mut refined_chunks = Vec::with_capacity(chunks.len());
            for (idx, chunk) in chunks.iter().enumerate() {
                let user_prompt = if chunks.len() == 1 {
                    format!("Refine this transcript text. Output only the refined text:\n\n{chunk}")
                } else {
                    format!(
                        "Refine this transcript chunk ({}/{}). Do not summarize.\n\n{}",
                        idx + 1,
                        chunks.len(),
                        chunk
                    )
                };
                refined_chunks.push(
                    self.send_refine_request(&url, system_prompt, &user_prompt)
                        .await?,
                );
            }
            return Ok(RefineResult {
                refined_text: refined_chunks.join("\n\n"),
                segment_refined: Vec::new(),
            });
        }

        // Speaker labels stay on segments for UI/identify; refine input is text only so
        // refined_text does not get polluted with "Guest-1: ..." prefixes.
        let indexed_lines: Vec<(usize, String)> = segments
            .iter()
            .enumerate()
            .map(|(i, s)| (i, format!("[{i}] {}", s.text.trim())))
            .collect();

        let line_chunks = chunk_indexed_lines(&indexed_lines, REFINE_CHUNK_CHARS);
        log::info!(
            "Sending refinement request to {} (model: {}, segment chunks: {}, segments: {})",
            url,
            self.config.model,
            line_chunks.len(),
            segments.len()
        );

        let mut segment_refined = vec![None; segments.len()];
        for (chunk_idx, chunk_lines) in line_chunks.iter().enumerate() {
            let body = chunk_lines
                .iter()
                .map(|(_, line)| line.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let user_prompt = format!(
                "Refine this transcript chunk ({}/{}). Keep every [N] line; one refined line per input line.\n\n{}",
                chunk_idx + 1,
                line_chunks.len(),
                body
            );

            log::info!(
                "Sending refinement chunk {}/{} ({} lines, {} chars)",
                chunk_idx + 1,
                line_chunks.len(),
                chunk_lines.len(),
                body.len()
            );

            let response = self
                .send_refine_request(&url, system_prompt, &user_prompt)
                .await?;
            let parsed = parse_indexed_refined_lines(&response);
            for (i, _) in chunk_lines {
                if let Some(text) = parsed.get(i) {
                    segment_refined[*i] = Some(text.clone());
                }
            }
        }

        // Fallback: keep raw text for any segment the model dropped
        for (i, slot) in segment_refined.iter_mut().enumerate() {
            if slot.is_none() {
                log::warn!("[refine] segment {i} missing from LLM output; keeping raw");
                *slot = Some(segments[i].text.trim().to_string());
            }
        }

        let refined_joined = segment_refined
            .iter()
            .filter_map(|s| s.as_ref())
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        Ok(RefineResult {
            refined_text: refined_joined,
            segment_refined: segment_refined
                .into_iter()
                .map(|s| s.unwrap_or_default())
                .collect(),
        })
    }

    async fn send_refine_request(
        &self,
        url: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String> {
        let request = ChatRequest {
            model: self.config.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user_prompt.to_string(),
                },
            ],
            temperature: 0.3,
            max_tokens: self.config.max_tokens.max(1024),
            stream: false,
        };
        Ok(self
            .send_chat_request(url, &request, "refinement")
            .await?
            .trim()
            .to_string())
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

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let body = response
            .text()
            .await
            .with_context(|| format!("Failed to read {operation} response body"))?;

        parse_chat_completion_body(&body, &content_type)
            .with_context(|| format!("Failed to parse {operation} response"))
    }
}

/// Parse OpenAI-compatible chat completion body in either non-stream JSON or SSE form.
fn parse_chat_completion_body(body: &str, content_type: &str) -> Result<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        anyhow::bail!("chat completion response body was empty");
    }

    let looks_like_sse = content_type
        .to_ascii_lowercase()
        .contains("text/event-stream")
        || trimmed.lines().any(|line| {
            let t = line.trim_start();
            t.starts_with("data:") || t.starts_with("data: ")
        });

    if looks_like_sse {
        log::info!("chat completion response mode: stream (SSE)");
        let content = parse_sse_chat_completion(trimmed)?;
        if content.is_empty() {
            anyhow::bail!(
                "SSE chat completion produced empty content (body preview: {})",
                preview_body(trimmed)
            );
        }
        return Ok(content);
    }

    log::info!("chat completion response mode: json");
    let chat_response: ChatResponse = serde_json::from_str(trimmed).with_context(|| {
        format!(
            "invalid non-stream chat completion JSON (body preview: {})",
            preview_body(trimmed)
        )
    })?;

    chat_response
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .filter(|c| !c.is_empty())
        .with_context(|| {
            format!(
                "chat completion response contained no content (body preview: {})",
                preview_body(trimmed)
            )
        })
}

/// Accumulate `choices[0].delta.content` from OpenAI-style SSE chunks.
fn parse_sse_chat_completion(body: &str) -> Result<String> {
    let mut out = String::new();

    for raw_line in body.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(':') {
            continue;
        }

        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim_start();
        if data.is_empty() || data == "[DONE]" {
            if data == "[DONE]" {
                break;
            }
            continue;
        }

        match serde_json::from_str::<StreamChunk>(data) {
            Ok(chunk) => {
                for choice in chunk.choices {
                    if let Some(piece) = choice.delta.content {
                        out.push_str(&piece);
                    }
                }
            }
            Err(err) => {
                // Some proxies emit non-chunk JSON (e.g. error objects) mid-stream.
                log::warn!(
                    "skipping unparseable SSE data line: {} (err: {})",
                    preview_body(data),
                    err
                );
            }
        }
    }

    Ok(out)
}

fn preview_body(body: &str) -> String {
    const MAX: usize = 300;
    let collapsed: String = body.chars().take(MAX).collect();
    if body.chars().count() > MAX {
        format!("{collapsed}…")
    } else {
        collapsed
    }
}

/// Group indexed segment lines into character-budgeted chunks (never split a line).
fn chunk_indexed_lines(lines: &[(usize, String)], max_chars: usize) -> Vec<Vec<(usize, String)>> {
    let mut chunks: Vec<Vec<(usize, String)>> = Vec::new();
    let mut current: Vec<(usize, String)> = Vec::new();
    let mut current_len = 0usize;

    for (idx, line) in lines {
        let line_len = line.len();
        let sep = if current.is_empty() { 0 } else { 1 };
        if !current.is_empty() && current_len + sep + line_len > max_chars {
            chunks.push(std::mem::take(&mut current));
            current_len = 0;
        }
        current_len += if current.is_empty() { 0 } else { 1 } + line_len;
        current.push((*idx, line.clone()));
    }

    if !current.is_empty() {
        chunks.push(current);
    }
    if chunks.is_empty() {
        chunks.push(Vec::new());
    }
    chunks
}

/// Parse lines like `[3] refined text here` into a map of index → text.
fn parse_indexed_refined_lines(response: &str) -> std::collections::HashMap<usize, String> {
    let mut out = std::collections::HashMap::new();
    for raw in response.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some(close) = rest.find(']') else {
            continue;
        };
        let Ok(idx) = rest[..close].trim().parse::<usize>() else {
            continue;
        };
        let mut text = rest[close + 1..].trim().to_string();
        // Drop optional "SPEAKER_XX: " prefix the model may echo
        if let Some(colon) = text.find(':') {
            let prefix = text[..colon].trim();
            if prefix.starts_with("SPEAKER") || prefix.chars().all(|c| c.is_ascii_digit()) {
                text = text[colon + 1..].trim().to_string();
            }
        }
        if !text.is_empty() {
            out.insert(idx, text);
        }
    }
    out
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
    if matches!(template, SummaryTemplate::MeetingNotes) {
        return meeting_notes_system_prompt(format);
    }

    let section_desc = match template {
        SummaryTemplate::KeyPoints => "key points",
        SummaryTemplate::ActionItems => "action items",
        SummaryTemplate::Decisions => "decisions",
        SummaryTemplate::Full => "key points, action items, and decisions",
        SummaryTemplate::MeetingNotes => unreachable!(),
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

fn meeting_notes_system_prompt(format: SummaryFormat) -> String {
    let format_note = match format {
        SummaryFormat::Markdown => "Output valid GitHub-flavored Markdown only.",
        SummaryFormat::RawText => "Output plain text following the same section order (no markdown tables if avoidable; use simple lists).",
    };
    format!(
        "You are a meeting notes assistant for an internship program. \
         Produce meeting notes that match this exact document structure and section order. \
         {format_note} \
         Be detailed, factual, and comprehensive. \
         When Meeting Context supplies title, date, or participants, use those values and do not invent replacements. \
         Only infer date, participants, or topic from the transcript when the corresponding context field is missing. \
         Action items must be measurable deliverables with owners when possible."
    )
}

fn format_meeting_context(meeting: &MeetingContext) -> String {
    let mut lines = Vec::new();
    if let Some(title) = meeting
        .title
        .as_ref()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
    {
        lines.push(format!("- Title: {title}"));
    }
    if let Some(date) = meeting
        .date
        .as_ref()
        .map(|d| d.trim())
        .filter(|d| !d.is_empty())
    {
        lines.push(format!("- Date: {date}"));
    }
    if let Some(participants) = &meeting.participants {
        let names: Vec<&str> = participants
            .iter()
            .map(|n| n.trim())
            .filter(|n| !n.is_empty())
            .collect();
        if !names.is_empty() {
            lines.push(format!("- Participants: {}", names.join(", ")));
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    format!(
        "Meeting Context:\n{}\n\
         Use Title/Date/Participants above when filling those fields. \
         Do not invent participant names that conflict with the list; \
         the transcript may mention additional speakers not listed.\n\n",
        lines.join("\n")
    )
}

fn build_prompt(
    template: SummaryTemplate,
    format: SummaryFormat,
    transcript: &str,
    language: Option<&str>,
    meeting: &MeetingContext,
) -> String {
    let lang_note = match language {
        Some(l) => format!("\n\nWrite the notes in this language: {l}."),
        None => String::new(),
    };
    let context_block = format_meeting_context(meeting);

    let participants_cell = meeting
        .participants
        .as_ref()
        .map(|p| {
            p.iter()
                .map(|n| n.trim())
                .filter(|n| !n.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "<comma-separated names>".to_string());

    let date_cell = meeting
        .date
        .as_ref()
        .map(|d| d.trim())
        .filter(|d| !d.is_empty())
        .unwrap_or("<full date with weekday if known>")
        .to_string();

    let topic_hint = meeting
        .title
        .as_ref()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .unwrap_or("<short meeting topic>");

    if matches!(template, SummaryTemplate::MeetingNotes) {
        return format!(
            "Generate internship meeting notes from the transcript below.\n\n\
             {context_block}\
             Use this exact Markdown structure (fill every section; use 'N/A' or '- (none)' when empty):\n\n\
             # Meeting Notes\n\n\
             ## Meeting Information\n\n\
             | Item | Description |\n\
             |------|-------------|\n\
             | Date | {date_cell} |\n\
             | Participants | {participants_cell} |\n\
             | Topic | {topic_hint} |\n\n\
             ---\n\n\
             ## 1. Review Pending Tasks\n\n\
             | Pending Task | Owner | Status | Remarks |\n\
             |--------------|-------|--------|---------|\n\
             | ... | ... | ... | ... |\n\n\
             ---\n\n\
             ## 2. Discussion Topics\n\n\
             ### Topic 1: <title>\n\n\
             **Reference:** <optional context>\n\n\
             - bullet points covering discussion\n\n\
             ### Topic N: ...\n\n\
             ---\n\n\
             ## 3. New Action Items\n\n\
             | Task | Owner | Measurable Deliverable | Due Date | Evidence |\n\
             |------|-------|------------------------|----------|----------|\n\
             | ... | ... | ... | ... | ... |\n\n\
             Do not wrap the whole document in a code fence. Output only the notes document.\n\
             {lang_note}\n\n\
             --- TRANSCRIPT START ---\n{transcript}\n--- TRANSCRIPT END ---"
        );
    }

    let template_name = match template {
        SummaryTemplate::KeyPoints => "key points only",
        SummaryTemplate::ActionItems => "action items only",
        SummaryTemplate::Decisions => "decisions only",
        SummaryTemplate::Full => "key points, action items, and decisions (all three sections)",
        SummaryTemplate::MeetingNotes => unreachable!(),
    };
    let format_note = match format {
        SummaryFormat::Markdown => " Use markdown formatting with headings and bullet points.",
        SummaryFormat::RawText => " Use plain text without any markdown syntax.",
    };
    format!(
        "{context_block}\
         Please summarize the following meeting transcript.\n\n\
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

/// Parse markdown section bullets into structured lists (public for manual summary updates).
pub fn parse_sections(
    content: &str,
    template: SummaryTemplate,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    // Meeting notes content is the full document; structured vecs are best-effort only.
    if matches!(template, SummaryTemplate::MeetingNotes) {
        return (Vec::new(), Vec::new(), Vec::new());
    }

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
            display_name: None,
            person_id: None,
            identify_confidence: None,
            refined_text: None,
        }
    }

    #[test]
    fn test_parse_indexed_refined_lines() {
        let map = parse_indexed_refined_lines(
            "[0] Hello world.\n[1] SPEAKER_00: Second line.\nnoise\n[2] Third.",
        );
        assert_eq!(map.get(&0).map(String::as_str), Some("Hello world."));
        assert_eq!(map.get(&1).map(String::as_str), Some("Second line."));
        assert_eq!(map.get(&2).map(String::as_str), Some("Third."));
    }

    #[test]
    fn test_parse_chat_completion_non_stream_json() {
        let body = r#"{"id":"x","choices":[{"index":0,"message":{"role":"assistant","content":"Hello world"},"finish_reason":"stop"}]}"#;
        let content = parse_chat_completion_body(body, "application/json").unwrap();
        assert_eq!(content, "Hello world");
    }

    #[test]
    fn test_parse_chat_completion_sse_stream() {
        let body = "\
data: {\"id\":\"c\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"## Key\"},\"finish_reason\":null}]}\n\
\n\
data: {\"id\":\"c\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" Points\"},\"finish_reason\":null}]}\n\
\n\
data: {\"id\":\"c\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\
\n\
data: [DONE]\n";
        let content = parse_chat_completion_body(body, "text/event-stream").unwrap();
        assert_eq!(content, "## Key Points");
    }

    #[test]
    fn test_parse_chat_completion_sse_detected_from_body() {
        // No event-stream content-type; body still has data: lines (9router-style).
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\ndata: [DONE]\n";
        let content = parse_chat_completion_body(body, "application/json").unwrap();
        assert_eq!(content, "Hi");
    }

    #[test]
    fn test_parse_chat_completion_empty_sse_errors() {
        let body = "data: {\"choices\":[{\"delta\":{}}]}\n\ndata: [DONE]\n";
        let err = parse_chat_completion_body(body, "text/event-stream").unwrap_err();
        assert!(err.to_string().contains("empty content"));
    }

    #[test]
    fn test_chat_request_serializes_stream_false() {
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
            temperature: 0.3,
            max_tokens: 64,
            stream: false,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["stream"], false);
    }

    #[test]
    fn test_chunk_indexed_lines() {
        let lines = vec![
            (0, "[0] a".to_string()),
            (1, "[1] bbb".to_string()),
            (2, "[2] c".to_string()),
        ];
        let chunks = chunk_indexed_lines(&lines, 12);
        assert!(chunks.len() >= 2);
        assert_eq!(chunks.iter().map(|c| c.len()).sum::<usize>(), 3);
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
        let p = build_prompt(
            SummaryTemplate::KeyPoints,
            SummaryFormat::Markdown,
            "transcript text",
            None,
            &MeetingContext::default(),
        );
        assert!(p.contains("key points only"));
        assert!(p.contains("transcript text"));
        assert!(!p.contains("Meeting Context:"));
    }

    #[test]
    fn test_build_prompt_full() {
        let p = build_prompt(
            SummaryTemplate::Full,
            SummaryFormat::Markdown,
            "t",
            Some("zh"),
            &MeetingContext::default(),
        );
        assert!(p.contains("all three sections"));
        assert!(p.contains("language: zh"));
    }

    #[test]
    fn test_build_prompt_raw_text() {
        let p = build_prompt(
            SummaryTemplate::Full,
            SummaryFormat::RawText,
            "t",
            None,
            &MeetingContext::default(),
        );
        assert!(p.contains("plain text"));
        assert!(p.contains("without any markdown syntax"));
    }

    #[test]
    fn test_build_prompt_with_participants() {
        let ctx = MeetingContext {
            title: Some("Weekly Sync".to_string()),
            date: Some("2026-07-21T10:00:00Z".to_string()),
            participants: Some(vec!["Alice".to_string(), "Bob".to_string()]),
        };
        let p = build_prompt(
            SummaryTemplate::MeetingNotes,
            SummaryFormat::Markdown,
            "hello transcript",
            None,
            &ctx,
        );
        assert!(p.contains("Meeting Context:"));
        assert!(p.contains("Participants: Alice, Bob"));
        assert!(p.contains("| Participants | Alice, Bob |"));
        assert!(p.contains("| Date | 2026-07-21T10:00:00Z |"));
        assert!(p.contains("Weekly Sync"));
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
