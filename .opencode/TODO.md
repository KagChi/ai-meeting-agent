# Meeting Agent — Todo List

**Sync Strategy:** Bidirectional sync with extern repo  
**Extern Source:** `/Users/kagchi/Documents/projects/bmw-ntust-internship/docs/daily-logs/08_MeetingAgent.md`

---

## Phase 1: Repository Setup and Initial Architecture ✅

- [x] Create repository structure (3-crate workspace: core, server, cli)
- [x] Set up Cargo workspace with dependencies
- [x] Design file-based storage system (`~/.meeting-agent/`)
- [x] Create core data models (Meeting, Transcript, Summary, Config)
- [x] Implement config management (TranscriptionConfig, SummaryConfig, ServerConfig)
- [x] Set up basic HTTP server skeleton with Axum
- [x] Add health and version endpoints

## Phase 2: Core Transcription Client Implementation ✅

- [x] Create transcription module in meeting-agent-core
- [x] Implement TranscriptionClient with OpenAI-compatible API support
- [x] Add request/response models (TranscriptionRequest, TranscriptionResponse)
- [x] Implement multipart file upload for audio files
- [x] Add error handling and retry logic
- [x] Support multiple response formats (json, verbose_json, srt, vtt, text)
- [x] Add environment variable loading for TRANSCRIPTION_* vars
- [x] Update Config to load from both file and environment variables
- [x] Add FFmpeg integration for audio format conversion
- [x] Implement CLI import command with progress indicators

## Phase 3: File System and Meeting Management ✅

- [x] Implement meeting storage operations (create, read, update, delete)
- [x] Add meeting metadata management
- [x] Implement audio file handling (copy to meeting directory)
- [x] Add transcript storage (save verbose_json format)
- [x] Implement meeting listing and search
- [x] Add file cleanup on meeting deletion

## Phase 4: HTTP API Endpoints - Meetings & Transcripts ✅

- [x] Implement GET /meetings (list all meetings)
- [x] Implement GET /meetings/{id} (get meeting details)
- [x] Implement POST /meetings (create meeting)
- [x] Implement PATCH /meetings/{id} (update meeting metadata)
- [x] Implement DELETE /meetings/{id} (delete meeting)
- [x] Implement GET /meetings/{id}/transcript (get transcript)
- [x] Add request validation and error responses
- [x] Add authentication middleware with MEETING_AGENT_API_KEY

## Phase 5: Import System with Background Jobs

- [x] Design background job system for import processing
- [x] Implement POST /import (accept audio file, spawn background job)
- [x] Implement POST /import/validate (validate audio file format)
- [x] Implement GET /import/{job_id}/status (poll job status)
- [x] Implement GET /import/{job_id}/events (SSE stream of progress)
- [x] Implement POST /import/{job_id}/cancel (cancel import)
- [x] Add job state management (pending, processing, completed, failed)
- [x] Add progress tracking (upload, transcription, storage)

## Phase 6: Summary Generation System

- [x] Create summary module with OpenAI-compatible client (configurable via SUMMARY_* env / config)
- [x] Implement POST /meetings/{id}/summary (generate summary, spawns background job)
- [x] Implement GET /meetings/{id}/summary (list all summaries for meeting)
- [x] Implement GET /meetings/{id}/summary/{template} (fetch specific summary)
- [x] Generalize job routes: /jobs/{job_id}/{status,events,cancel} (shared by import + summary)
- [x] Add summary templates (key_points, action_items, decisions, full)
- [x] Add language preference support (per-request + SUMMARY_LANGUAGE env default)
- [x] Block summary generation if meeting status != Ready

## Phase 7: CLI Implementation

- [x] Implement `meeting-agent import` command (import audio file)
- [x] Implement `meeting-agent list` command (list all meetings)
- [x] Implement `meeting-agent show <id>` command (show meeting details, 8-char prefix OK)
- [x] Implement `meeting-agent summarize <id>` command (generate summary, synchronous)
- [x] Implement `meeting-agent export <id>` command (export transcript as srt/vtt/text/json)
- [x] Implement `meeting-agent config show` command (show current config, API keys masked)
- [x] Implement `meeting-agent config set` command (update config, dotted notation)
- [x] Implement `meeting-agent server` command (start API server, --port/--host overrides)
- [x] Add progress bars and colored output (indicatif spinners, colored status, comfy-table)
- [ ] Add interactive mode for configuration

## Phase 8: Configuration Management API

- [ ] Implement GET /config (get current config)
- [ ] Implement PUT /config (update config)
- [ ] Implement GET /config/transcription (get transcription config)
- [ ] Implement PUT /config/transcription (update transcription config)
- [ ] Implement GET /config/summary (get summary config)
- [ ] Implement PUT /config/summary (update summary config)
- [ ] Add config validation
- [ ] Add secure credential storage

## Phase 9: Testing and Documentation

- [ ] Write unit tests for core transcription client
- [ ] Write unit tests for file system operations
- [ ] Write integration tests for HTTP endpoints
- [ ] Write integration tests for CLI commands
- [ ] Add API documentation (OpenAPI/Swagger)
- [ ] Write usage examples in README
- [ ] Add troubleshooting guide
- [ ] Add deployment guide
