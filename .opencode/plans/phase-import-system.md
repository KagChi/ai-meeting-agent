# Phase 5: Import System with Background Jobs

**Goal**: Implement HTTP-based audio import with background transcription jobs, progress tracking via SSE, and cancellation support.

## Architecture

```
Client POST /import (multipart audio)
    ↓
[Handler] saves upload to tempfile, creates Job, spawns tokio task
    ↓
[Background Task]
    ├─ Convert audio (ffmpeg → mp3)        → progress: "converting"
    ├─ Create Meeting (status=Importing)   → progress: "processing"
    ├─ Transcribe (TranscriptionClient)    → progress: "transcribing"
    ├─ Save audio + transcript             → progress: "saving"
    └─ mark_transcription_complete          → progress: "completed"
    ↓
[JobRegistry] holds Job {state, progress, meeting_id, error?, cancel_token}
    ↓
Client polls GET /import/{job_id}/status  OR  streams GET /import/{job_id}/events (SSE)
Client POST /import/{job_id}/cancel  →  CancellationToken fires → task aborts
```

## Components

### 1. Job Registry (`crates/core/src/jobs.rs`)

In-memory store for background import jobs. Thread-safe via `Arc<Mutex<HashMap>>`.

```rust
pub struct ImportJob {
    pub id: String,
    pub state: JobState,
    pub progress: Vec<ProgressEvent>,
    pub meeting_id: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub enum JobState {
    Pending,
    Processing,
    Completed,
    Failed,
    Cancelled,
}

pub struct ProgressEvent {
    pub stage: String,        // "uploading", "converting", "transcribing", "saving", "completed"
    pub message: String,
    pub timestamp: DateTime<Utc>,
    pub percent: Option<f64>, // 0.0 - 100.0
}

pub struct JobRegistry {
    jobs: Arc<Mutex<HashMap<String, ImportJob>>>,
    cancel_tokens: Arc<Mutex<HashMap<String, CancellationToken>>>,
    event_txs: Arc<Mutex<HashMap<String, broadcast::Sender<ProgressEvent>>>>,
}
```

Methods:
- `new() -> Self`
- `create_job() -> String` (returns job_id, creates Pending job + broadcast channel)
- `get_job(id) -> Option<ImportJob>`
- `get_job_state(id) -> Option<JobState>`
- `update_progress(id, ProgressEvent)`
- `set_meeting_id(id, meeting_id)`
- `complete_job(id)`
- `fail_job(id, error)`
- `cancel_job(id) -> bool` (triggers CancellationToken, sets Cancelled)
- `subscribe(id) -> Option<Receiver<ProgressEvent>>` (for SSE)
- `cancel_token(id) -> Option<CancellationToken>`
- `list_jobs() -> Vec<ImportJob>`

### 2. Import Processing (`crates/core/src/import.rs`)

Background task logic — `async fn run_import(...)`:

```rust
pub async fn run_import(
    job_id: String,
    audio_path: PathBuf,          // tempfile path of uploaded file
    original_filename: String,
    title: Option<String>,
    config: Config,
    storage: Arc<MeetingStorage>,
    registry: Arc<JobRegistry>,
    cancel_token: CancellationToken,
) -> Result<()>
```

Steps:
1. Emit "converting" progress. Run ffmpeg conversion if needed (`audio::needs_conversion` + `audio::convert_to_mp3`). Check `cancel_token.is_cancelled()` between steps.
2. Emit "processing" progress. Create Meeting (status=Importing). Set `meeting_id` on job.
3. Emit "transcribing" progress. Build `TranscriptionClient`, transcribe verbose_json. Handle cancellation — if cancelled mid-transcription, abort.
4. Emit "saving" progress. `save_audio`, `save_transcript`, `mark_transcription_complete`.
5. Emit "completed" progress. `complete_job`.

On error: `fail_job(id, error.to_string())`, `mark_transcription_failed(meeting_id)` if meeting was created.
On cancel: `cancel_job` already set state; if meeting created, `mark_transcription_failed`.

### 3. HTTP Handlers (`crates/server/src/import_handlers.rs`)

#### `POST /import` — multipart upload
- Extract `Multipart`, save `file` field to tempfile (`tempfile::NamedTempFile`).
- Extract optional `title` field.
- Create job via `registry.create_job()`.
- Spawn `tokio::spawn(run_import(...))`.
- Return 202 Accepted with `{ job_id, status: "pending" }`.

Validation: file field present, filename has audio extension (mp3/wav/m4a/flac/webm/ogg).

#### `POST /import/validate` — validate audio without importing
- Multipart upload, check extension + magic bytes (optional).
- Return `{ valid: bool, format: String, size: u64 }`.

#### `GET /import/:job_id/status` — poll status
- Lookup job. If not found → 404.
- Return `{ job_id, state, progress: [...], meeting_id?, error? }`.

#### `GET /import/:job_id/events` — SSE stream
- Lookup job. If not found → 404.
- Subscribe to broadcast channel.
- Stream `text/event-stream`: `data: {json}\n\n` per ProgressEvent.
- On job terminal state, send final event + close.
- Use `axum::response::sse::{Sse, Event, KeepAlive}`.

#### `POST /import/:job_id/cancel` — cancel import
- Lookup job. If not found → 404.
- If terminal state → 409 Conflict.
- Call `registry.cancel_job(id)`. Return `{ cancelled: true }`.

### 4. AppState Update (`crates/server/src/state.rs`)

```rust
#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub storage: Arc<MeetingStorage>,
    pub jobs: Arc<JobRegistry>,  // NEW
}
```

### 5. Route Wiring (`crates/server/src/main.rs`)

```rust
.route("/import", axum::routing::post(import_handlers::create_import))
.route("/import/validate", axum::routing::post(import_handlers::validate_import))
.route("/import/:job_id/status", axum::routing::get(import_handlers::get_import_status))
.route("/import/:job_id/events", axum::routing::get(import_handlers::get_import_events))
.route("/import/:job_id/cancel", axum::routing::post(import_handlers::cancel_import))
```

## Dependencies to Add

### `crates/server/Cargo.toml`
```toml
axum = { version = "0.7", features = ["multipart"] }
tokio-util = { version = "0.7", features = ["rt"] }  # CancellationToken
tempfile = "3"
```

### `crates/core/Cargo.toml`
```toml
tokio-util = { version = "0.7", features = ["rt"] }  # CancellationToken
```

(tokio already has `full` features including `sync`.)

## Tasks

- [ ] Add dependencies (axum multipart, tokio-util, tempfile)
- [ ] Create `crates/core/src/jobs.rs` — JobRegistry, ImportJob, JobState, ProgressEvent
- [ ] Create `crates/core/src/import.rs` — run_import async fn
- [ ] Export new modules in `crates/core/src/lib.rs`
- [ ] Add `jobs_dir()` to `crates/core/src/fs.rs` (for persisting upload tempfiles if needed)
- [ ] Create `crates/server/src/import_handlers.rs` — 5 handlers
- [ ] Update `crates/server/src/state.rs` — add `jobs: Arc<JobRegistry>` field
- [ ] Update `crates/server/src/main.rs` — wire import routes, init JobRegistry in AppState
- [ ] Add request/response types to `crates/server/src/types.rs` (ImportResponse, ImportStatusResponse, etc.)
- [ ] Write unit tests for JobRegistry
- [ ] Write integration test for import flow
- [ ] Run pre-commit: fmt, clippy, test

## Design Decisions

1. **In-memory jobs** (no DB persistence): Jobs live in `Arc<Mutex<HashMap>>`. If server restarts, pending jobs are lost. Acceptable for v1 — meeting records persist on disk, only in-flight job state is volatile.

2. **tokio::CancellationToken** (not bool flag): Clean async cancellation. Checked between stages. Transcription HTTP call can't be interrupted mid-request, but we check before/after and abort if cancelled.

3. **broadcast::channel for SSE**: One sender per job, multiple subscribers (poll + SSE can coexist). Buffer size 16 — progress events are small; if subscriber lags, old events dropped (acceptable for progress UI).

4. **Tempfile for upload**: Multipart body saved to `tempfile::NamedTempFile` before spawning task. File lives until task completes or is cancelled. NamedTempFile auto-deletes on drop.

5. **Meeting created as Importing**: Meeting record created early (after audio conversion) so user can see it in `GET /meetings` even if transcription is still running. Status flips to Ready/Failed at end.

6. **Auth**: Import endpoints under same auth middleware as `/meetings`.

## Success Criteria

- POST /import returns 202 with job_id
- Background transcription runs without blocking server
- GET /status reflects current stage
- SSE /events streams live progress updates
- POST /cancel stops a running job
- Cancelled/failed jobs mark meeting as Failed
- Completed jobs mark meeting as Ready with transcript saved
- All pre-commit checks pass (fmt, clippy, test)
