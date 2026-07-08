# Meeting Agent - Todo List

## Overview

Building **ai-meeting-agent** per `PRD.md` v3.1.
`PRD.md` is the source of truth.

**PRD Source of Truth**: [`PRD.md`](../PRD.md)

Scope tracked here: **Week 2-8 deliverables**.

**Sync Strategy:** Bidirectional sync with extern repo
**Extern Source:** `/Users/kagchi/Documents/projects/bmw-ntust-internship/docs/daily-logs/08_MeetingAgent.md`

---

## Week 1 / Pre-existing Baseline (Completed Context)

- [x] Rust workspace: core, server, cli
- [x] File-based storage under `~/.meeting-agent/`
- [x] Axum HTTP server with health/version endpoints
- [x] Meeting CRUD API
- [x] Import API with background jobs, SSE events, cancel/status
- [x] OpenAI-compatible transcription client
- [x] FFmpeg conversion and long-audio chunking
- [x] Summary generation API + CLI
- [x] Config API: `/config`, `/config/transcription`, `/config/summary`
- [x] API key auth middleware
- [x] Swagger/OpenAPI docs at `/docs`
- [x] CLI: import/list/show/summarize/export/config/server/config edit
- [x] In-process speaker diarization via speakrs
- [x] Automatic GPU detection with CPU fallback
- [x] Transcript segments support optional `speaker`

---

## Week 2: Canonical Ingest + Metadata

**Goal:** File upload/import becomes PRD canonical ingest stage for audio/video.

**Deliverables:**

- [ ] Audio/video upload support for `.mp4`, `.mkv`, `.m4a`, `.mp3`, `.wav`
- [ ] Video demux path extracts audio only and discards frames
- [ ] Normalized 16 kHz mono WAV artifact saved per meeting
- [ ] ffprobe file info stored per meeting
- [ ] Filename date/time parser for patterns like `2026-07-01_lab-meeting.mp4`
- [ ] Metadata source precedence implemented: user edit > calendar/bot > filename > ffprobe
- [ ] `meeting.json` extended with `starts_at`, `metadata_source`, `platform`
- [ ] HTTP upload and CLI import use same canonical runner
- [ ] Tests for video upload, filename metadata, ffprobe fallback

**PRD mapping:** FR-3, FR-4, FR-10, FR-15, G-2

---

## Week 3: Canonical ASR + Diarization

**Goal:** Downstream pipeline produces transcript + diarized speaker turns.

**Deliverables:**

- [x] speakrs integration using pyannote community-1
- [x] In-process `OwnedDiarizationPipeline`
- [x] Lazy initialization via `OnceLock`
- [x] Auto GPU detection with CPU fallback
- [x] `DIARIZE_ENABLED`, `DIARIZE_EXECUTION_MODE`, `DIARIZE_MODEL_DIR`
- [x] `TranscriptSegment.speaker`
- [ ] WhisperX large-v3 endpoint config documented as canonical ASR
- [ ] Word-aligned transcript artifact support
- [ ] Per-segment language tags for EN/ZH/ID
- [ ] Code-switching test fixture
- [ ] `diarization.json` artifact written separately from transcript
- [ ] Pipeline integration test: normalize -> transcribe -> diarize

**PRD mapping:** FR-5, FR-6, NFR-1, NFR-2, G-3

---

## Week 4: Voiceprint Identity

**Goal:** Diarized speakers become named people or `Guest-N`.

**Deliverables:**

- [ ] `Person` model
- [ ] `Voiceprint` model
- [ ] `~/.meeting-agent/voiceprints/{person_id}/person.json`
- [ ] Centroid embedding persistence under `VOICEPRINT_DIR`
- [ ] `IDENTIFY_THRESHOLD` config
- [ ] `ENROLL_MIN_SPEECH` config, default 30 seconds
- [ ] Standalone embedding extraction from audio span
- [ ] 3D-Speaker / WeSpeaker / TitaNet backend selected and wired
- [ ] `POST /v1/voiceprints`
- [ ] `GET /v1/voiceprints`
- [ ] `DELETE /v1/voiceprints/{person_id}`
- [ ] `POST /v1/identify`
- [ ] Cosine similarity matching against enrolled centroids
- [ ] Unknown speakers returned as `Guest-N`
- [ ] Unknown embeddings retained for later enrollment
- [ ] CLI voiceprint commands: enroll/list/delete/merge
- [ ] Consent tracking field for enrolled people
- [ ] Shared-login replay test

**PRD mapping:** FR-7, FR-8, NFR-4, G-4

---

## Week 5: SOP Minutes + Review Gate

**Goal:** Generate BMW-Lab SOP minutes, not generic summaries.

**Deliverables:**

- [ ] SOP minutes generator using Appendix A template verbatim
- [ ] Slot mapping implemented: source name, date/time, attendees, recording, topic
- [ ] Topic inference when title is missing or ambiguous
- [ ] `minutes/sop.md` artifact
- [ ] `Pending from last meeting` section
- [ ] Per-attendee `Action Items` with `- [ ]` checkboxes
- [ ] Structured `action_items[]` JSON side-call
- [ ] `reviewed_by` field added to meeting state
- [ ] Draft minutes blocked from publishing while `Reviewed by` empty
- [ ] Certification path to set `Reviewed by`
- [ ] Regenerate minutes without retranscribing
- [ ] SOP template conformance test

**PRD mapping:** FR-9, FR-10, FR-11, FR-14, G-5

---

## Week 6: Vexa Bot Integration

**Goal:** Live Teams/Zoom/Meet capture enters same pipeline as upload.

**Deliverables:**

- [ ] Self-host Vexa v0.10.6 stack
- [ ] Configure Vexa `TRANSCRIPTION_SERVICE_URL` to DGX WhisperX
- [ ] `POST /bots` integration for Teams/Zoom/Meet links
- [ ] Bot status/webhook contract file
- [ ] Recorded webhook fixtures
- [ ] Meeting-ended webhook receiver
- [ ] Fetch raw recording from Vexa MinIO
- [ ] Fetch participant/timing metadata from Vexa
- [ ] Store `bot_id`, `calendar_event_id`, live `platform`
- [ ] Live path starts same normalize -> metadata -> pipeline runner as upload
- [ ] Bot failure/stuck-state handling
- [ ] Test call matrix for Teams/Zoom/Meet

**PRD mapping:** FR-1, FR-2, G-1, G-2, Appendix C row 1, Appendix D patterns

---

## Week 7: Publish Automation

**Goal:** Certified minutes publish to GitHub daily-log and Google Calendar.

**Deliverables:**

- [ ] Publishing blocked unless meeting is certified
- [ ] Daily-log target config
- [ ] GitHub daily-log publisher
- [ ] Minutes link posted to configured target
- [ ] Per-person action items included in daily-log output
- [ ] Google Calendar auth/config
- [ ] Calendar event matching by time window + attendees
- [ ] Calendar description PATCH with minutes link
- [ ] No-match Calendar case logs warning, not error
- [ ] `PublishRecord` stored with `daily_log_url` and `calendar_updated`
- [ ] End-to-end test: certify -> GitHub + Calendar publish

**PRD mapping:** FR-12, FR-13, G-6

---

## Week 8: Hardening, Docs, Evaluation

**Goal:** Prove privacy, performance, rerun behavior, and docs.

**Deliverables:**

- [ ] Idempotent per-stage rerun support
- [ ] Meeting state machine implemented: Requested/Capturing/Normalizing/MetadataResolved/Transcribing/Identifying/GeneratingMinutes/Draft/Certified/Published/Failed
- [ ] Raw audio retention config `RETENTION_RAW_AUDIO`
- [ ] Retention cleanup job
- [ ] Egress audit proving no audio/transcript/minutes leave lab services
- [ ] DGX memory/headroom measurement
- [ ] 1-hour recording benchmark target <= 30 min
- [ ] Voiceprint consent + retention policy documented
- [ ] README updated for PRD workflow
- [ ] `.env.example` updated with all PRD env vars
- [ ] Deployment guide updated for Vexa + WhisperX + LLM + diarize
- [ ] End-to-end demo script
- [ ] Final acceptance checklist against PRD FR-1 through FR-15

**PRD mapping:** NFR-1 through NFR-6, FR-15, §9 state machines, §12 risks

---

## Out of Scope for Week 2-8

- [ ] Mobile one-tap recorder (PRD FR-16)
- [ ] Realtime captions / MCP hooks (PRD FR-17)
- [ ] Ask-the-archive local RAG
- [ ] Talk-time and participation analytics
- [ ] Action-item tracker from GitHub checkboxes
- [ ] Soundbite links using word timestamps
- [ ] Cross-meeting person page
- [ ] Lab custom vocabulary management UI/API

---

## Quick Reference

- **PRD**: `PRD.md` in the ai-meeting-agent repo
- **PRD File**: [`PRD.md`](../PRD.md)
- **Extern Source**: `/Users/kagchi/Documents/projects/bmw-ntust-internship/docs/daily-logs/08_MeetingAgent.md`
- **Repository**: https://github.com/bmw-ece-ntust/ai-meeting-agent
- **Plan files**: `.opencode/plans/phase-*.md` in this repo
