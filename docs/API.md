# Meeting Agent API Specification

**Version:** 0.1.0  
**Base URL:** `http://{host}:{port}` (default `http://127.0.0.1:8080`)  
**OpenAPI Spec:** `GET /api-docs/openapi.json`  
**Swagger UI:** `GET /docs`

---

## Authentication

All endpoints (except `/health`, `/version`, `/docs`, `/api-docs/openapi.json`) require authentication when `server.api_key` is configured.

| Header | Description |
|--------|-------------|
| `X-API-Key: <key>` | API key matching `server.api_key` config |
| `Authorization: <key>` | Alternative header (same value, no `Bearer` prefix) |

If no API key is configured, the server runs in open-access mode (logs a warning on first request).

**Responses:**
- `401 Unauthorized` — missing or invalid API key

---

## Health & Info

### `GET /health`

Health check endpoint. No authentication required.

**Response:** `200 OK`
```json
{
  "status": "ok"
}
```

### `GET /version`

Version information. No authentication required.

**Response:** `200 OK`
```json
{
  "version": "0.1.0",
  "name": "meeting-agent-server"
}
```

---

## Meetings

### `GET /meetings`

List all meetings with optional pagination.

**Query Parameters:**
| Name | Type | Default | Description |
|------|------|---------|-------------|
| `limit` | integer | 20 | Number of results (1-100) |
| `offset` | integer | 0 | Number of results to skip |

**Response:** `200 OK`
```json
{
  "meetings": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "title": "Q3 Planning",
      "date": "2026-07-01T03:40:00Z",
      "duration_seconds": 3600,
      "status": "ready",
      "audio_file": "http://example.com/meetings/550e8400-e29b-41d4-a716-446655440000/recording",
      "transcription": {
        "provider": "openai",
        "model": "whisper-1",
        "completed_at": "2026-07-01T03:50:00Z",
        "version": 1
      },
      "created_at": "2026-07-01T03:40:00Z",
      "updated_at": "2026-07-01T03:50:00Z"
    }
  ],
  "total": 42,
  "limit": 20,
  "offset": 0
}
```

### `POST /meetings`

Create a new meeting manually (without audio import).

**Request Body:**
```json
{
  "title": "Q3 Planning",
  "date": "2026-07-01T03:40:00Z"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `title` | string | Yes | Meeting title (non-empty) |
| `date` | string (ISO 8601) | No | Meeting date/time (defaults to now) |

**Response:** `201 Created`
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "title": "Q3 Planning",
  "date": "2026-07-01T03:40:00Z",
  "duration_seconds": null,
  "status": "importing",
  "transcription": null,
  "created_at": "2026-07-01T03:40:00Z",
  "updated_at": "2026-07-01T03:40:00Z"
}
```

**Errors:**
- `400 Bad Request` — empty title

### `GET /meetings/{id}`

Get a specific meeting by ID or ID prefix (8-char minimum).

**Path Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `id` | string | Meeting UUID or prefix |

**Response:** `200 OK` — single meeting object (same shape as `POST /meetings` response)

**Errors:**
- `404 Not Found` — meeting not found
- `400 Bad Request` — invalid UUID format

### `PATCH /meetings/{id}`

Update meeting metadata. At least one field must be provided.

**Request Body (partial update):**
```json
{
  "title": "Updated Title",
  "date": "2026-07-02T10:00:00Z"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `title` | string | No | New title |
| `date` | string (ISO 8601) | No | New date/time |

**Response:** `200 OK` — updated meeting object

**Errors:**
- `400 Bad Request` — no fields provided
- `404 Not Found` — meeting not found

### `DELETE /meetings/{id}`

Delete a meeting and all associated files (audio, transcript, summaries).

**Response:** `204 No Content`

**Errors:**
- `404 Not Found` — meeting not found

### `GET /meetings/{id}/metadata`

Get structured metadata for a meeting (participants, location, organizer, etc.).

**Response:** `200 OK`
```json
{
  "meeting_id": "550e8400-e29b-41d4-a716-446655440000",
  "participants": ["Alice", "Bob", "Charlie"],
  "location": "Conference Room A",
  "organizer": "Alice",
  "metadata_source": "manual",
  "recording_date": "2026-07-01T03:40:00Z",
  "platform": "zoom",
  "file_metadata": {
    "original_filename": "meeting.mp3",
    "size_bytes": 5242880,
    "duration_seconds": 3600
  }
}
```

**Errors:**
- `404 Not Found` — meeting not found

### `GET /meetings/{id}/recording`

Download the audio recording file for a meeting.

**Response:** `200 OK` with appropriate `Content-Type` header:
- `audio/mpeg` for `.mp3`
- `audio/wav` for `.wav`
- `audio/mp4` for `.m4a`
- `audio/flac` for `.flac`
- `audio/ogg` for `.ogg`/`.opus`
- `audio/webm` for `.webm`

**Headers:**
- `Content-Type`: Detected MIME type based on file extension
- `Content-Disposition`: `attachment; filename="<original-filename>"`

**Errors:**
- `404 Not Found` — meeting or recording file not found

---

## Transcripts

### `GET /transcripts/search`

Full-text search across **all ready meetings'** transcripts using SQLite FTS5.

Returns **meetings** that contain matching segments (not a flat segment list), ordered by relevance. Each meeting includes up to 10 top matching segments plus a total match count.

**Query Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `q` | string | Yes | — | Search query (FTS5 syntax, max 500 chars) |
| `limit` | integer | No | 50 | Max meetings to return (1–500) |
| `offset` | integer | No | 0 | Meetings to skip |

**Example:**
```
GET /transcripts/search?q=roadmap&limit=20
```

**Response:** `200 OK`
```json
{
  "query": "roadmap",
  "total_meetings": 3,
  "limit": 20,
  "offset": 0,
  "meetings": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "title": "Q3 Planning Meeting",
      "date": "2026-07-15T10:00:00Z",
      "duration_seconds": 1800,
      "status": "ready",
      "participants": ["Alice", "Bob"],
      "matched_segments": [
        {
          "segment_id": 5,
          "start": 12.5,
          "end": 18.2,
          "text": "The roadmap includes three key initiatives...",
          "timestamp": "00:12",
          "speaker": "Alice"
        }
      ],
      "match_count": 47,
      "relevance_score": -12.34
    }
  ]
}
```

**Notes:**
- Only `ready` meetings are searched
- Only the latest transcript version per meeting is searched
- `matched_segments` is capped at 10; use `match_count` for the full total
- Lower `relevance_score` = better FTS5 match
- FTS5 syntax: `word1 word2`, `"exact phrase"`, `word1 OR word2`, `word1 NOT word2`, `word*`

**Errors:**
- `400 Bad Request` — missing, empty, or too-long query

---

### `GET /meetings/{id}/transcript`

Get the transcript for a meeting with optional pagination.

**Query Parameters:**
| Name | Type | Default | Description |
|------|------|---------|-------------|
| `limit` | integer | 100 | Number of segments to return (1-1000) |
| `offset` | integer | 0 | Number of segments to skip |

**Response:** `200 OK`
```json
{
  "meeting_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "ready",
  "transcript": {
    "text": "Welcome everyone to the Q3 planning meeting...",
    "segments": [
      {
        "id": 0,
        "start": 0.0,
        "end": 5.32,
        "text": "Welcome everyone to the Q3 planning meeting."
      },
      {
        "id": 1,
        "start": 5.32,
        "end": 12.10,
        "text": "Today we'll discuss the roadmap."
      }
    ]
  },
  "total_segments": 150,
  "limit": 100,
  "offset": 0
}
```

If no transcript exists yet, `transcript` is `null`.

**Errors:**
- `404 Not Found` — meeting not found

### `GET /meetings/{id}/transcript/search`

Full-text search across transcript segments using SQLite FTS5.

**Query Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `q` | string | Yes | Search query (supports FTS5 syntax) |
| `limit` | integer | No | Max results (default 50, max 500) |
| `offset` | integer | No | Results to skip (default 0) |

**Response:** `200 OK`
```json
{
  "meeting_id": "550e8400-e29b-41d4-a716-446655440000",
  "query": "roadmap",
  "results": [
    {
      "segment_id": 1,
      "start": 5.32,
      "end": 12.10,
      "text": "Today we'll discuss the roadmap.",
      "rank": 0.85
    },
    {
      "segment_id": 42,
      "start": 120.5,
      "end": 128.3,
      "text": "The roadmap for Q4 includes three major features.",
      "rank": 0.72
    }
  ],
  "total": 2,
  "limit": 50,
  "offset": 0
}
```

**Errors:**
- `400 Bad Request` — missing or invalid query
- `404 Not Found` — meeting not found

### `POST /meetings/{id}/retranscribe`

Retranscribe a meeting with the current transcription configuration. Creates a new transcript version while preserving the previous one.

**Path Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `id` | string | Meeting UUID or prefix |

**Response:** `202 Accepted`
```json
{
  "job_id": "990e8400-e29b-41d4-a716-446655440004",
  "status": "pending"
}
```

**Errors:**
- `404 Not Found` — meeting not found
- `409 Conflict` — no audio file available for retranscription

**Notes:**
- Uses the current `transcription.provider` and `transcription.model` from config
- Previous transcript versions are preserved and accessible via `/meetings/{id}/transcript/versions`
- Poll job status at `GET /jobs/{job_id}/status` or stream events at `GET /jobs/{job_id}/events`

### `GET /meetings/{id}/transcript/versions`

List all transcript versions for a meeting, ordered by version number (newest first).

**Path Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `id` | string | Meeting UUID or prefix |

**Response:** `200 OK`
```json
{
  "meeting_id": "550e8400-e29b-41d4-a716-446655440000",
  "versions": [
    {
      "id": 2,
      "meeting_id": "550e8400-e29b-41d4-a716-446655440000",
      "version": 2,
      "provider": "openai",
      "model": "whisper-1",
      "language": "en",
      "segment_count": 150,
      "created_at": "2026-07-20T04:30:00Z"
    },
    {
      "id": 1,
      "meeting_id": "550e8400-e29b-41d4-a716-446655440000",
      "version": 1,
      "provider": "openai",
      "model": "whisper-large-v3",
      "language": "en",
      "segment_count": 148,
      "created_at": "2026-07-20T03:50:00Z"
    }
  ]
}
```

**Errors:**
- `404 Not Found` — meeting not found

**Notes:**
- Each retranscription creates a new version entry
- The current active transcript is always the latest version
- Version metadata includes provider, model, language, and segment count for comparison

---

## Summaries

### `GET /meetings/{id}/summary`

List all summaries for a meeting.

**Response:** `200 OK`
```json
{
  "meeting_id": "550e8400-e29b-41d4-a716-446655440000",
  "summaries": [
    {
      "id": "660e8400-e29b-41d4-a716-446655440001",
      "meeting_id": "550e8400-e29b-41d4-a716-446655440000",
      "template": "key_points",
      "language": "en",
      "status": "completed",
      "content": "## Key Points\n\n- Q3 roadmap focuses on...",
      "key_points": ["Q3 roadmap focus", "Budget approved"],
      "action_items": [],
      "decisions": [],
      "provider": "openai",
      "model": "gpt-4o-mini",
      "created_at": "2026-07-01T04:00:00Z",
      "updated_at": "2026-07-01T04:01:00Z"
    }
  ]
}
```

### `POST /meetings/{id}/summary`

Generate a summary for a meeting. Returns a job ID for async tracking.

**Request Body:**
```json
{
  "template": "full",
  "format": "markdown",
  "language": "en"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `template` | enum | Yes | `key_points`, `action_items`, `decisions`, or `full` |
| `format` | enum | No | `markdown` (default) or `rawtext` - output format |
| `language` | string | No | Override summary language (defaults to config `summary.language`) |

**Response:** `202 Accepted`
```json
{
  "job_id": "770e8400-e29b-41d4-a716-446655440002",
  "status": "pending"
}
```

**Errors:**
- `404 Not Found` — meeting not found
- `409 Conflict` — meeting not ready (status != `ready`)

### `GET /meetings/{id}/summary/{template}`

Get a specific summary by template and format.

**Path Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `id` | string | Meeting ID or prefix |
| `template` | string | `key_points`, `action_items`, `decisions`, or `full` |

**Query Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `format` | string | No | `markdown` (default) or `rawtext` - output format |

**Response:** `200 OK` — single summary object

**Example:**
```
GET /meetings/550e8400/summary/full?format=rawtext
```

**Errors:**
- `404 Not Found` — meeting or summary not found
- `400 Bad Request` — invalid template name or format

---

## Import & Jobs

### `POST /import`

Upload an audio file and start background transcription. Returns a job ID immediately.

**Request:** `multipart/form-data`
| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `file` | binary | Yes | Audio file (mp3, wav, m4a, flac, webm, ogg, opus, aac, wma) |
| `title` | string | No | Meeting title (defaults to filename) |

**Response:** `202 Accepted`
```json
{
  "job_id": "880e8400-e29b-41d4-a716-446655440003",
  "status": "pending"
}
```

**Errors:**
- `400 Bad Request` — missing file, unsupported format, or no extension

### `POST /import/validate`

Validate an audio file without importing. Checks file extension only.

**Request:** `multipart/form-data`
| Field | Type | Required |
|-------|------|----------|
| `file` | binary | Yes |

**Response:** `200 OK`
```json
{
  "valid": true,
  "format": "mp3",
  "size": 5242880
}
```

### `GET /jobs/{job_id}/status`

Poll the status of a background job (import or summary).

**Response:** `200 OK`
```json
{
  "job_id": "880e8400-e29b-41d4-a716-446655440003",
  "job_type": "import",
  "state": "processing",
  "progress": [
    {
      "stage": "chunking",
      "message": "Splitting audio into 600s chunks",
      "timestamp": "2026-07-01T03:41:00Z",
      "percent": null
    },
    {
      "stage": "transcribing",
      "message": "Transcribing chunk 1/3",
      "timestamp": "2026-07-01T03:42:00Z",
      "percent": 33.3
    }
  ],
  "meeting_id": "550e8400-e29b-41d4-a716-446655440000",
  "template": null,
  "error": null,
  "created_at": "2026-07-01T03:40:30Z",
  "updated_at": "2026-07-01T03:42:00Z"
}
```

**Job States:** `pending`, `processing`, `completed`, `failed`, `cancelled`

**Job Types:** `import`, `summary`, `retranscribe`

**Errors:**
- `404 Not Found` — job not found

### `GET /jobs/{job_id}/events`

Server-Sent Events stream of progress updates for a job. Replays all existing progress events first, then streams live events until the job reaches a terminal state.

**Response:** `200 OK` (`text/event-stream`)

Each event:
```
data:{"stage":"transcribing","message":"Chunk 1/3 done","timestamp":"2026-07-01T03:42:30Z","percent":33.3}
```

**Errors:**
- `404 Not Found` — job not found

### `POST /jobs/{job_id}/cancel`

Cancel a running job.

**Response:** `200 OK`
```json
{
  "job_id": "880e8400-e29b-41d4-a716-446655440003",
  "cancelled": true
}
```

**Errors:**
- `404 Not Found` — job not found
- `409 Conflict` — job already in terminal state

---

## Configuration

### `GET /config`

Get current configuration. API keys are masked as `"****"`.

**Response:** `200 OK`
```json
{
  "transcription": {
    "provider": "openai",
    "api_key": "****",
    "base_url": "https://api.openai.com/v1",
    "model": "whisper-1",
    "chunk_seconds": 600.0,
    "chunk_concurrency": 2
  },
  "summary": {
    "provider": "openai",
    "api_key": "****",
    "base_url": "https://api.openai.com/v1",
    "model": "gpt-4o-mini",
    "temperature": 0.3,
    "max_tokens": 1024,
    "language": "en"
  }
}
```

### `PUT /config`

Update full configuration. Validates before saving.

**Request Body:** Same shape as `GET /config` response (with `UpdateConfigRequest` wrapper).

To keep an existing API key unchanged, send `"****"`. To replace, send the new key value.

**Response:** `200 OK`
```json
{
  "message": "Configuration updated successfully"
}
```

**Errors:**
- `400 Bad Request` — validation failed

### `GET /config/transcription`

Get transcription configuration only.

**Response:** `200 OK`
```json
{
  "provider": "openai",
  "api_key": "****",
  "base_url": "https://api.openai.com/v1",
  "model": "whisper-1",
  "chunk_seconds": 600.0,
  "chunk_concurrency": 2
}
```

### `PUT /config/transcription`

Update transcription configuration.

**Request Body:**
```json
{
  "provider": "openai",
  "api_key": "sk-...",
  "base_url": "https://api.openai.com/v1",
  "model": "whisper-1",
  "chunk_seconds": 600.0,
  "chunk_concurrency": 2
}
```

| Field | Type | Description |
|-------|------|-------------|
| `provider` | string | STT provider name |
| `api_key` | string? | API key (`"****"` = keep existing) |
| `base_url` | string | API base URL |
| `model` | string | Model name |
| `chunk_seconds` | float | Audio chunk duration in seconds |
| `chunk_concurrency` | int | Parallel chunk transcription count |

**Response:** `200 OK`
```json
{
  "message": "Transcription configuration updated successfully"
}
```

### `GET /config/summary`

Get summary configuration only.

**Response:** `200 OK`
```json
{
  "provider": "openai",
  "api_key": "****",
  "base_url": "https://api.openai.com/v1",
  "model": "gpt-4o-mini",
  "temperature": 0.3,
  "max_tokens": 1024,
  "language": "en"
}
```

### `PUT /config/summary`

Update summary configuration.

**Request Body:**
```json
{
  "provider": "openai",
  "api_key": "sk-...",
  "base_url": "https://api.openai.com/v1",
  "model": "gpt-4o-mini",
  "temperature": 0.3,
  "max_tokens": 1024,
  "language": "en"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `provider` | string | LLM provider name |
| `api_key` | string? | API key (`"****"` = keep existing) |
| `base_url` | string | API base URL |
| `model` | string | Model name |
| `temperature` | float | Sampling temperature (0.0–2.0) |
| `max_tokens` | int | Max response tokens |
| `language` | string? | Default summary language |

**Response:** `200 OK`
```json
{
  "message": "Summary configuration updated successfully"
}
```

---

## Data Models

### Meeting

```rust
pub struct Meeting {
    pub id: String,                          // UUID v4
    pub title: String,
    pub date: DateTime<Utc>,
    pub duration_seconds: Option<u64>,
    pub status: MeetingStatus,               // importing | ready | failed
    pub transcription: Option<TranscriptionInfo>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

### Transcript

```rust
pub struct Transcript {
    pub segments: Vec<TranscriptSegment>,
}

pub struct TranscriptSegment {
    pub id: u32,
    pub start: f64,       // seconds
    pub end: f64,         // seconds
    pub text: String,
}
```

### Summary

```rust
pub struct Summary {
    pub id: String,
    pub meeting_id: String,
    pub template: SummaryTemplate,           // key_points | action_items | decisions | full
    pub language: Option<String>,
    pub status: SummaryStatus,               // pending | processing | completed | failed
    pub content: String,                     // raw LLM markdown output
    pub key_points: Vec<String>,             // parsed (full or key_points template)
    pub action_items: Vec<String>,           // parsed (full or action_items template)
    pub decisions: Vec<String>,              // parsed (full or decisions template)
    pub provider: String,
    pub model: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

### Job

```rust
pub struct Job {
    pub id: String,
    pub job_type: JobType,                   // import | summary | retranscribe
    pub state: JobState,                     // pending | processing | completed | failed | cancelled
    pub progress: Vec<ProgressEvent>,
    pub meeting_id: Option<String>,
    pub template: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct ProgressEvent {
    pub stage: String,
    pub message: String,
    pub timestamp: DateTime<Utc>,
    pub percent: Option<f64>,
}
```

---

## File Storage Structure

All data stored in `~/.meeting-agent/`:

```
~/.meeting-agent/
├── config.json                          # Global configuration
└── meetings/{id}/
    ├── meeting.json                     # Meeting metadata
    ├── audio/
    │   └── {original-filename}          # Uploaded audio file
    ├── transcript.json                  # Whisper transcription result
    └── summaries/
        ├── key_points.json              # Summary (key_points template)
        ├── action_items.json            # Summary (action_items template)
        ├── decisions.json               # Summary (decisions template)
        └── full.json                    # Summary (full template)
```

---

## OpenAPI / Swagger UI Integration

The server uses [`utoipa`](https://docs.rs/utoipa) for compile-time OpenAPI 3.0 spec generation:

- **Source:** `crates/server/src/openapi.rs` — `#[derive(OpenApi)]` with `#[openapi(paths(...), components(...))]`
- **Spec endpoint:** `GET /api-docs/openapi.json` — machine-readable OpenAPI 3.0 JSON
- **Swagger UI:** `GET /docs` — interactive API explorer (no auth required)
- **Schemas:** All request/response types derive `utoipa::ToSchema` via `#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]`
- **Tags:** `meetings`, `transcripts`, `summaries`, `imports`, `jobs`, `config`

The OpenAPI spec is generated at compile time from Rust handler signatures and type definitions — it is always in sync with the code.

---

## Self-Hosted vLLM Configuration

For self-hosted Whisper and Gemma 4 via vLLM:

```bash
# Transcription (Whisper via vLLM)
TRANSCRIPTION_PROVIDER=openai
TRANSCRIPTION_API_KEY=token-placeholder
TRANSCRIPTION_BASE_URL=http://140.118.122.126:8080/v1
TRANSCRIPTION_MODEL=whisper-large-v3

# Summary (Gemma 4 via vLLM)
SUMMARY_PROVIDER=openai
SUMMARY_API_KEY=token-placeholder
SUMMARY_BASE_URL=http://140.118.122.126:8001/v1
SUMMARY_MODEL=gemma-3-4b-it
SUMMARY_TEMPERATURE=0.3
SUMMARY_MAX_TOKENS=1024
SUMMARY_LANGUAGE=en
```

Both endpoints use the OpenAI-compatible API format (`/v1/audio/transcriptions` for Whisper, `/v1/chat/completions` for Gemma 4).

---

## Key Request/Response Patterns

1. **Async jobs:** Import and summary generation are async. `POST /import` and `POST /meetings/{id}/summary` return `202 Accepted` with a `job_id`. Poll `GET /jobs/{job_id}/status` or stream `GET /jobs/{job_id}/events` (SSE).

2. **ID prefix matching:** All `{id}` path parameters accept a full UUID or an 8-character prefix. The server resolves the prefix to the full UUID automatically.

3. **Secret masking:** API keys in GET responses are masked as `"****"`. To keep an existing key during PUT, send `"****"`. To replace, send the new value.

4. **Multipart upload:** Audio import uses `multipart/form-data` with `file` (binary) and optional `title` (text) fields.

5. **Terminal job states:** `completed`, `failed`, and `cancelled` are terminal. Cancel requests on terminal jobs return `409 Conflict`. SSE streams close naturally on terminal state.
