# Context — ai-meeting-agent

Self-hosted meeting-intelligence system (Fireflies.ai-style, lab-owned). Full product
spec in [PRD.md](PRD.md); deployment in [deploy/README.md](deploy/README.md).

## Architecture (hybrid)

Two cooperating stacks, all models on the **DGX Spark**, no meeting data leaves the lab:

1. **Bot spine — Vexa** (external, `Vexa-ai/vexa`, Apache-2.0; run as a sibling stack).
   Joins Teams/Zoom/Meet, records, realtime transcription, per-speaker audio (MinIO),
   webhooks, MCP. We attach to its `vexa` Docker network and call `POST /bots` /
   consume `GET /transcripts/...`. Its `TRANSCRIPTION_SERVICE_URL` is pointed at our DGX
   WhisperX so ASR runs locally.
2. **Lab-intelligence layer — this repo** (Rust workspace + DGX model services):
   - `crates/core` — models, storage (`~/.meeting-agent/`), transcription/summary
     clients, jobs. File-import path retained as an ingest source.
   - `crates/server` — Axum HTTP API + OpenAPI/Swagger (`/docs`).
   - `crates/cli` — operator CLI / API client.
   - `crates/diarize` — sherpa-onnx diarization microservice (pyannote seg + 3D-Speaker
     embeddings). **Being extended into speaker identification** (voiceprint enroll +
     cosine match) — the differentiator that resolves the "several people on one Teams
     account" case (PRD §13). Serves `/v1/diarize` (+ planned `/v1/identify`,
     `/v1/voiceprints`).
   - `deploy/` — compose stack (WhisperX + minutes-LLM + our services), Dockerfile.server,
     env template, bring-up runbook.
   - `orchestrator` (Phase 4, not yet built) — Vexa webhook → identify → SOP minutes →
     daily-log → Google Calendar.

## Models (DGX Spark, GB10, 128 GB unified, aarch64)
- ASR: Whisper large-v3 via WhisperX (EN/ZH/ID + code-switch). OpenAI-compatible server.
- Diarization: pyannote 3.1 / sherpa-onnx (NeMo Sortformer to evaluate).
- Identification: 3D-Speaker / WeSpeaker / TitaNet embeddings + voiceprint DB.
- Minutes LLM: Qwen2.5 (or GPT-OSS) via Ollama/vLLM, OpenAI-compatible.

## External services / integrations
- Vexa api-gateway `:8056`; our services `:8080` (server), `:8002` (diarize), `:8010`
  (whisperx), `:11434` (llm).
- BMW-Lab SOP minutes template: `bmw-ece-ntust/SOP` → `logistics/meeting.md`.
- Google Calendar API (event link-back); lab daily-log / progress-plan (action items).

## Build / verify
- Rust workspace: `cargo fmt/clippy/test` (mandatory pre-commit, per AGENTS.md).
  **Not available on the current dev laptop** (no cargo/docker) — build on the DGX or a
  Rust-equipped host. ffmpeg + ffprobe required on PATH for audio.

## Governance notes
- `AGENTS.md` = strict working agreement (no commit/push without explicit per-action
  permission) + the Samuel/`bmw-ntust-internship` #812 daily-log sync rules.
- `.opencode/` = the internship-track workflow files (ACTIVE/TODO/NOTES).
