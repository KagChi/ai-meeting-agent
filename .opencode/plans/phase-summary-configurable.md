# Phase: Configurable Summary Generation (OpenAI-compatible)

**Goal**: Generate meeting summaries via any OpenAI-compatible LLM endpoint. User picks template per request. Multiple summaries per meeting (one per template). Block summary while import job still running.

## Decisions locked

- **OpenAI-compatible only** — single HTTP path `/chat/completions`, no Anthropic branch.
- **Templates**: `key_points`, `action_items`, `decisions`, `full` (all three in one call). User picks per request.
- **Storage**: multiple summaries per meeting — `summaries/{template}.json` subdir.
- **Concurrency**: block summary if meeting import job still running (status != Ready).
- **No new deps** — reuse `reqwest`, `anyhow`, `serde`, `tokio`, `chrono`, `uuid`.
- **Summary GET route**: `GET /meetings/:id/summary` (list all) + `GET /meetings/:id/summary/:template` (specific).
- **Job routes**: hard rename `/import/:job_id/*` → `/jobs/:job_id/*` (generalized, works for both import+summary jobs).
- **SummaryJob module**: separate `crates/core/src/summary_job.rs` (parity with `import.rs`).
- **Section parsing**: tolerate missing sections (fill empty Vec).
- **Language**: per-request `CreateSummaryRequest.language` + `SUMMARY_LANGUAGE` env var default.

---

## Task 1: Config expansion (`crates/core/src/config.rs`)

Expand `SummaryConfig` (currently lines 22-26, missing `base_url`):

```rust
pub struct SummaryConfig {
    pub provider: String,            // "openai" | "groq" | "openrouter" | "ollama" | "custom"
    pub api_key: Option<String>,
    pub base_url: String,            // NEW
    pub model: String,
    pub temperature: Option<f64>,    // NEW, default 0.3
    pub max_tokens: Option<u32>,     // NEW, default 2000
}
```

Default impl (config.rs:44-49) update:
```rust
summary: SummaryConfig {
    provider: "openai".to_string(),
    api_key: None,
    base_url: "https://api.openai.com/v1".to_string(),
    model: "gpt-4o-mini".to_string(),
    temperature: Some(0.3),
    max_tokens: Some(2000),
}
```

Add env overrides to `Config::load` (after transcription overrides, config.rs:76-82), mirroring transcription pattern:
```rust
if let Ok(v) = std::env::var("SUMMARY_PROVIDER") { config.summary.provider = v; }
if let Ok(v) = std::env::var("SUMMARY_API_KEY") { config.summary.api_key = Some(v); }
if let Ok(v) = std::env::var("SUMMARY_BASE_URL") { config.summary.base_url = v; }
if let Ok(v) = std::env::var("SUMMARY_MODEL") { config.summary.model = v; }
if let Ok(v) = std::env::var("SUMMARY_TEMPERATURE") {
    config.summary.temperature = v.parse().ok();
}
if let Ok(v) = std::env::var("SUMMARY_MAX_TOKENS") {
    config.summary.max_tokens = v.parse().ok();
}
if let Ok(v) = std::env::var("SUMMARY_LANGUAGE") {
    config.summary.language = Some(v);
}
```

Add `language: Option<String>` field to `SummaryConfig` (env default, request overrides).

Provider preset resolution helper (used by `SummaryClient::new`):
```rust
fn resolve_base_url(provider: &str, explicit: &str) -> String {
    if !explicit.is_empty() { return explicit.to_string(); }  // explicit overrides
    match provider {
        "openai" => "https://api.openai.com/v1".to_string(),
        "groq" => "https://api.groq.com/openai/v1".to_string(),
        "openrouter" => "https://openrouter.ai/api/v1".to_string(),
        "ollama" => "http://localhost:11434/v1".to_string(),
        _ => String::new(),  // "custom" or unknown — require base_url
    }
}
```

---

## Task 2: Data model (`crates/core/src/models.rs`)

Replace existing basic `Summary` (models.rs:63-69) with expanded version + add supporting types:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SummaryTemplate {
    KeyPoints,
    ActionItems,
    Decisions,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SummaryStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub meeting_id: String,
    pub template: SummaryTemplate,
    pub language: Option<String>,
    pub status: SummaryStatus,
    pub content: String,           // raw LLM output (markdown)
    pub key_points: Vec<String>,   // parsed sections (empty if not Full/key_points)
    pub action_items: Vec<String>,
    pub decisions: Vec<String>,
    pub provider: String,
    pub model: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

**Note on `key_points`/`action_items`/`decisions` fields**: For `Full` template, parse the LLM markdown output into the three Vec fields. For single-template requests, populate only the matching field and leave others empty. Content field always holds raw LLM output. Parsing logic lives in `summary.rs`.

`MeetingStatus` enum unchanged (existing `Importing`/`Ready`/`Failed` sufficient; block rule checks `status == Ready`).

---

## Task 3: Summary client (`crates/core/src/summary.rs`) — NEW FILE

Mirrors `transcription.rs` structure (transcription.rs:11-75):

```rust
pub struct SummaryClient {
    client: reqwest::Client,
    config: SummaryConfig,
    base_url: String,  // resolved
}

pub struct SummarizeOptions {
    pub template: SummaryTemplate,
    pub language: Option<String>,
    pub custom_prompt: Option<String>,
}

impl SummaryClient {
    pub fn new(config: SummaryConfig) -> Result<Self> {
        let base_url = resolve_base_url(&config.provider, &config.base_url);
        if base_url.is_empty() {
            anyhow::bail!("Summary base_url required for provider '{}' — set SUMMARY_BASE_URL", config.provider);
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;
        Ok(Self { client, config, base_url })
    }

    pub async fn summarize(
        &self,
        transcript: &TranscriptionResponse,
        opts: &SummarizeOptions,
    ) -> Result<Summary> { ... }
}
```

**`summarize` flow:**
1. Build system prompt from template (helper `system_prompt(template) -> String`).
2. Build user message: transcript text. If `segments` present, format as `[00:00:03] text\n...`; else flat `transcript.text`.
3. POST `{base_url}/chat/completions`, header `Authorization: Bearer {api_key}` (skip if None for local ollama), body:
```json
{
  "model": "{config.model}",
  "messages": [
    {"role": "system", "content": "..."},
    {"role": "user", "content": "..."}
  ],
  "temperature": 0.3,
  "max_tokens": 2000
}
```
4. Parse response: `choices[0].message.content`.
5. Build `Summary` — parse sections if `Full`, else single-field.
6. Return.

**Prompt templates** (system prompts):
- `KeyPoints`: "Extract the key points discussed in this meeting transcript. Return as a markdown bullet list."
- `ActionItems`: "Extract action items from this meeting transcript. Format: '- [ ] task (owner if mentioned)'. Return as markdown."
- `Decisions`: "Extract decisions made in this meeting transcript. Return as markdown bullet list."
- `Full`: "Analyze this meeting transcript and return three sections:\n## Key Points\n- ...\n## Action Items\n- ...\n## Decisions\n- ..."

Language override: if `opts.language` set (or config.language), append to system prompt: "Respond in {language}." Request value overrides config.

**Section parsing** (Full only): split content on `## ` headers, collect bullets per section into the three Vec fields. Tolerant of missing sections — fill empty Vec.

**Request/response structs** (private, serde):
```rust
#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<Message<'a>>,
    temperature: f64,
    max_tokens: u32,
}
#[derive(Serialize)]
struct Message<'a> { role: &'a str, content: String }
#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}
#[derive(Deserialize)]
struct Choice { message: ChoiceMessage }
#[derive(Deserialize)]
struct ChoiceMessage { content: String }
```

Export from `lib.rs`: add `pub mod summary;` + `pub use summary::{SummaryClient, SummarizeOptions};`.

---

## Task 4: Storage methods (`crates/core/src/storage.rs`)

Add to `MeetingStorage` impl (after `get_transcript`, storage.rs:144):

```rust
/// Save a summary for a meeting (one per template)
pub fn save_summary(&self, meeting_id: &str, summary: &Summary) -> Result<()> {
    let meeting_path = fs::meeting_dir(meeting_id)?;
    if !meeting_path.exists() { anyhow::bail!("Meeting not found: {}", meeting_id); }
    let summaries_dir = meeting_path.join("summaries");
    std::fs::create_dir_all(&summaries_dir)?;
    let file = summaries_dir.join(format!("{}.json", template_filename(&summary.template)));
    std::fs::write(&file, serde_json::to_string_pretty(summary)?)?;
    Ok(())
}

/// Get a specific template's summary
pub fn get_summary(&self, meeting_id: &str, template: &SummaryTemplate) -> Result<Summary> {
    let path = fs::meeting_dir(meeting_id)?.join("summaries").join(format!("{}.json", template_filename(template)));
    if !path.exists() { anyhow::bail!("Summary not found for meeting {} template {:?}", meeting_id, template); }
    let json = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&json)?)
}

/// List all summaries for a meeting
pub fn list_summaries(&self, meeting_id: &str) -> Result<Vec<Summary>> {
    let dir = fs::meeting_dir(meeting_id)?.join("summaries");
    if !dir.exists() { return Ok(vec![]); }
    let mut summaries = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Ok(json) = std::fs::read_to_string(&path) {
                if let Ok(s) = serde_json::from_str::<Summary>(&json) {
                    summaries.push(s);
                }
            }
        }
    }
    summaries.sort_by_key(|s| s.created_at);
    Ok(summaries)
}

/// Delete a specific summary
pub fn delete_summary(&self, meeting_id: &str, template: &SummaryTemplate) -> Result<()> { ... }
```

Helper:
```rust
fn template_filename(t: &SummaryTemplate) -> String {
    match t {
        SummaryTemplate::KeyPoints => "key_points",
        SummaryTemplate::ActionItems => "action_items",
        SummaryTemplate::Decisions => "decisions",
        SummaryTemplate::Full => "full",
    }.to_string()
}
```

Need `use crate::models::{Summary, SummaryTemplate};` at top of storage.rs (currently imports only `Meeting, MeetingStatus, TranscriptionInfo` at storage.rs:6).

---

## Task 5: Job system — generalize `JobRegistry` (`crates/core/src/jobs.rs`)

**Problem**: `JobRegistry` is import-only — `ImportJob` struct hardcoded (jobs.rs:54-65), all methods typed to it. `JobState`/`ProgressEvent`/cancel/subscribe are reusable; the job struct isn't.

**Approach (Option A)**: Add `job_type: JobType` field + template field to existing struct, rename `ImportJob` → `Job`:
```rust
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum JobType { Import, Summary }

pub struct Job {
    pub id: String,
    pub job_type: JobType,
    pub state: JobState,
    pub progress: Vec<ProgressEvent>,
    pub meeting_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,  // summary only
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```
Methods `create_job(job_type)` / `get_job` / etc. stay generic. `complete_job`/`fail_job` messages become generic ("Job completed" vs "Import completed"). `types.rs` `ImportStatusResponse` rename → `JobStatusResponse` with `job_type: JobType` field.

Changes to jobs.rs:
- Add `JobType` enum.
- Rename `ImportJob` → `Job`, add `job_type` + `template` fields.
- `Job::new(id, job_type)` takes type param.
- `JobRegistry::create_job(&self, job_type: JobType) -> String`.
- `set_template(&self, job_id, template: String)` — new method for summary jobs.
- `complete_job` message: match on job_type for message text.
- Update all tests (jobs.rs:272-426) — mechanical.

---

## Task 6: Summary background job (`crates/core/src/summary_job.rs`) — NEW FILE

Mirror `import.rs` `run_import` pattern (import.rs:28-69):

```rust
pub async fn run_summary(
    job_id: String,
    meeting_id: String,
    opts: SummarizeOptions,
    config: Config,
    storage: Arc<MeetingStorage>,
    registry: Arc<JobRegistry>,
    cancel_token: CancellationToken,
) {
    let result = run_summary_inner(...).await;
    match result {
        Ok(()) => registry.complete_job(&job_id),
        Err(e) => {
            if cancel_token.is_cancelled() {
                if registry.get_job_state(&job_id) != Some(JobState::Cancelled) {
                    registry.cancel_job(&job_id);
                }
            } else {
                registry.fail_job(&job_id, e.to_string());
            }
        }
    }
}

async fn run_summary_inner(...) -> Result<()> {
    // Step 1: check meeting status == Ready (block rule)
    let meeting = storage.get_meeting(&meeting_id)?;
    if meeting.status != MeetingStatus::Ready {
        anyhow::bail!("Meeting {} is not ready (status: {:?}) — cannot summarize until import completes", meeting_id, meeting.status);
    }
    check_cancelled(&cancel_token)?;

    // Step 2: load transcript
    registry.update_progress(&job_id, ProgressEvent::new("fetching_transcript", "Loading transcript"));
    let transcript = storage.get_transcript(&meeting_id)?;
    check_cancelled(&cancel_token)?;

    // Step 3: call LLM
    registry.update_progress(&job_id, ProgressEvent::new("calling_llm", "Generating summary").with_percent(50.0));
    let client = SummaryClient::new(config.summary.clone())?;
    let summary = client.summarize(&transcript, &opts).await?;
    check_cancelled(&cancel_token)?;

    // Step 4: save
    registry.update_progress(&job_id, ProgressEvent::new("saving", "Saving summary").with_percent(90.0));
    storage.save_summary(&meeting_id, &summary)?;

    Ok(())
}
```

Separate file `summary_job.rs` for parity with `import.rs`.

---

## Task 7: HTTP API — server types (`crates/server/src/types.rs`)

Add request/response types (after import types, types.rs:99):

```rust
// === Summary Types ===

#[derive(Debug, Deserialize)]
pub struct CreateSummaryRequest {
    pub template: SummaryTemplate,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub custom_prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SaveSummaryRequest {
    pub template: SummaryTemplate,
    pub content: String,
    #[serde(default)]
    pub language: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SummaryResponse {
    pub job_id: String,
    pub status: JobState,
}

#[derive(Debug, Serialize)]
pub struct SummaryStatusResponse {
    pub job_id: String,
    pub state: JobState,
    pub job_type: JobType,
    pub progress: Vec<ProgressEvent>,
    pub meeting_id: String,
    pub template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct SummaryDataResponse {
    pub meeting_id: String,
    pub summary: Option<Summary>,
}

#[derive(Debug, Serialize)]
pub struct ListSummariesResponse {
    pub meeting_id: String,
    pub summaries: Vec<Summary>,
}

#[derive(Debug, Serialize)]
pub struct CancelSummaryResponse {
    pub job_id: String,
    pub cancelled: bool,
}
```

Rename `ImportStatusResponse` (types.rs:60-70) → `JobStatusResponse` with `job_type: JobType` field (generalized, works for both import+summary jobs).

Need `use meeting_agent_core::models::{Summary, SummaryTemplate};` + `use meeting_agent_core::jobs::JobType;` imports.

---

## Task 8: HTTP handlers (`crates/server/src/summary_handlers.rs`) — NEW FILE

Mirror `import_handlers.rs` (293 lines). Six handlers:

```rust
/// POST /meetings/:id/summary
/// Body: CreateSummaryRequest. Spawns background summary job. Returns 202.
pub async fn create_summary(
    State(state): State<AppState>,
    Path(meeting_id): Path<String>,
    Json(req): Json<CreateSummaryRequest>,
) -> Result<axum::response::Response, ApiError> {
    // Validate meeting exists + Ready
    let meeting = state.storage.get_meeting(&meeting_id)
        .map_err(|_| ApiError::NotFound("Meeting not found".into()))?;
    if meeting.status != MeetingStatus::Ready {
        return Err(ApiError::Conflict(format!(
            "Meeting not ready (status: {:?}). Wait for import to complete.", meeting.status
        )));
    }

    // Create job
    let job_id = state.jobs.create_job(JobType::Summary);
    state.jobs.set_meeting_id(&job_id, meeting_id.clone());
    state.jobs.set_template(&job_id, template_str(&req.template));
    let cancel_token = state.jobs.cancel_token(&job_id)
        .ok_or_else(|| ApiError::InternalServerError("Failed to get cancel token".into()))?;

    let opts = SummarizeOptions { template: req.template, language: req.language, custom_prompt: req.custom_prompt };

    tokio::spawn(async move {
        run_summary(job_id, meeting_id, opts, config, storage, registry, cancel_token).await;
    });

    Ok((StatusCode::ACCEPTED, Json(SummaryResponse { job_id, status: JobState::Pending })).into_response())
}

/// GET /meetings/:id/summary
/// List all saved summaries for a meeting.
pub async fn list_summaries(...) { ... }

/// GET /meetings/:id/summary/:template
/// Returns specific saved summary.
pub async fn get_summary(...) { ... }

/// PUT /meetings/:id/summary/:template
/// Body: SaveSummaryRequest. Manual save/overwrite (no LLM call).
pub async fn save_summary(...) { ... }

/// POST /meetings/:id/summary/cancel
pub async fn cancel_summary(...) { ... }

/// GET /meetings/:id/summary/events?job_id=...
/// SSE stream — same pattern as get_import_events (import_handlers.rs:207-250).
pub async fn get_summary_events(...) { ... }
```

---

## Task 9: Routes (`crates/server/src/main.rs`)

Add to router (main.rs:55-70), after transcript route:

```rust
// Summary endpoints
.route(
    "/meetings/:id/summary",
    get(summary_handlers::list_summaries).post(summary_handlers::create_summary),
)
.route(
    "/meetings/:id/summary/:template",
    get(summary_handlers::get_summary).put(summary_handlers::save_summary),
)
.route(
    "/meetings/:id/summary/events",
    get(summary_handlers::get_summary_events),
)
// Generalized job endpoints (rename from /import/*)
.route("/jobs/:job_id/status", get(handlers::get_job_status))  // renamed from /import/:job_id/status
.route("/jobs/:job_id/events", get(handlers::get_job_events))  // renamed
.route("/jobs/:job_id/cancel", post(handlers::cancel_job))     // renamed
```

Add `mod summary_handlers;` (main.rs:16).

**Hard rename** `/import/:job_id/*` → `/jobs/:job_id/*`. Move `get_import_status`/`get_import_events`/`cancel_import` from `import_handlers.rs` to `handlers.rs` (or keep in import_handlers but rename functions to `get_job_status` etc.). Remove old `/import/:job_id/*` routes. Keep `/import` (POST create) + `/import/validate` (those are upload-specific, not job-status).

---

## Task 10: Tests

### Unit tests (`crates/core/src/summary.rs`)
- `test_system_prompt_key_points` — prompt contains "key points"
- `test_system_prompt_full` — prompt contains all three section headers
- `test_parse_full_sections` — sample LLM markdown → correct Vec splits
- `test_parse_full_missing_section` — tolerant of missing `## Decisions`
- `test_template_filename` — enum → filename mapping
- `test_resolve_base_url_explicit_overrides` — explicit base_url wins
- `test_resolve_base_url_presets` — each provider → correct URL
- `test_resolve_base_url_custom_requires_explicit` — empty + custom → empty

### Storage tests (`crates/core/src/storage.rs`)
- `test_save_and_get_summary` — round-trip one template
- `test_list_summaries_multiple` — save 3 templates, list returns 3
- `test_get_summary_missing_template` — error
- `test_delete_summary`
- `test_save_summary_missing_meeting` — error

### Job tests (`crates/core/src/jobs.rs`)
- Update existing tests for `JobType` param
- `test_create_summary_job_has_type` — `create_job(JobType::Summary)` → job_type Summary
- `test_set_template` — template field set

### Server tests (`crates/server/src/summary_handlers.rs` or `tests/`)
- `test_create_summary_meeting_not_ready` — 409
- `test_create_summary_missing_meeting` — 404
- `test_get_summary_not_found` — 404
- `test_list_summaries_empty` — empty array

### Integration test
- End-to-end: import → poll until Ready → POST summary → poll until complete → GET summary. Requires mock transcription + LLM, or skip if no API key (gated test).

---

## Task 11: Documentation

- Update `CLAUDE.md` if it lists endpoints.
- Add summary config example to default `config.json` (auto-generated on first run).
- Document env vars: `SUMMARY_*`.
- Note provider presets table.

---

## File change summary

| File | Action |
|------|--------|
| `crates/core/src/config.rs` | Edit — expand `SummaryConfig`, add env overrides, `resolve_base_url`, `language` field |
| `crates/core/src/models.rs` | Edit — replace `Summary`, add `SummaryTemplate`, `SummaryStatus` |
| `crates/core/src/summary.rs` | **New** — `SummaryClient`, `SummarizeOptions`, prompts, parsing |
| `crates/core/src/summary_job.rs` | **New** — `run_summary` background task |
| `crates/core/src/storage.rs` | Edit — add summary CRUD methods |
| `crates/core/src/jobs.rs` | Edit — generalize to `Job` + `JobType` |
| `crates/core/src/lib.rs` | Edit — export new modules |
| `crates/server/src/summary_handlers.rs` | **New** — 6 handlers |
| `crates/server/src/types.rs` | Edit — add summary request/response types, rename `ImportStatusResponse` → `JobStatusResponse` |
| `crates/server/src/main.rs` | Edit — add routes, module decl |
| `crates/server/src/import_handlers.rs` | Edit — move/rename job status handlers to `handlers.rs` |

---

## Implementation order

1. Task 2: models.rs (SummaryTemplate, SummaryStatus, expanded Summary)
2. Task 1: config.rs (SummaryConfig expansion + env overrides + resolve_base_url)
3. Task 3: summary.rs (SummaryClient + prompts + parsing + unit tests)
4. Task 4: storage.rs (summary CRUD + tests)
5. Task 5: jobs.rs (generalize to Job + JobType + tests)
6. Task 6: summary_job.rs (run_summary background task)
7. Task 7: types.rs (summary request/response types)
8. Task 8: summary_handlers.rs (6 handlers)
9. Task 9: main.rs (routes + module decl)
10. Task 10: server tests
11. Task 11: docs
12. Verify: `cargo fmt --all -- --check` && `cargo clippy --all --all-targets -- -D warnings` && `cargo test --all`

## Success criteria

- ✓ Can configure any OpenAI-compatible provider (openai, groq, openrouter, ollama, custom) via config or env vars
- ✓ User picks template per request (key_points, action_items, decisions, full)
- ✓ Multiple summaries stored per meeting (one per template)
- ✓ Summary blocked if meeting import still running
- ✓ Background job with SSE progress stream
- ✓ Cancellation support
- ✓ Manual save/overwrite via PUT
- ✓ All pre-commit checks pass (fmt, clippy, test)

## References

- OpenAI Chat Completions API: https://platform.openai.com/docs/api-reference/chat
- Existing transcription client pattern: `crates/core/src/transcription.rs`
- Existing import job pattern: `crates/core/src/import.rs`
- Existing job registry: `crates/core/src/jobs.rs`
- Todo source: `/Users/kagchi/Documents/projects/bmw-ntust-internship/docs/daily-logs/08_MeetingAgent.md` lines 69-76
