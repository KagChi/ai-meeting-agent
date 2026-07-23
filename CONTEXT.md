# Context — ai-meeting-agent

Self-hosted meeting-intelligence system (Fireflies.ai-style, lab-owned). Full product
spec in [PRD.md](PRD.md); deployment in [deploy/README.md](deploy/README.md).

## Architecture (hybrid)

Two cooperating stacks, all models on the **DGX Spark**, no meeting data leaves the lab:

1. **Bot spine — Vexa** (external, `Vexa-ai/vexa`, Apache-2.0; sibling checkout).
   Joins Teams/Zoom/Meet and **records to MinIO only** (no Vexa live STT:
   `TRANSCRIBE_ENABLED=false`). Include via `deploy/docker-compose.bots.yml`.
   Call `POST /bots` on gateway `:18056`; pull recordings from MinIO / `GET /recordings`.
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
    - `deploy/` — lab compose + optional bots overlay (Vexa include, record-only),
      Dockerfile.server, env template, bring-up runbook.
    - `orchestrator` (Phase 4 v1 in `crates/core/src/orchestrator` + server routes) —
      Vexa meeting-end → download recording → import (ASR here). Publish (daily-log /
      GCal) still later.

## Models (DGX Spark, GB10, 128 GB unified, aarch64)
- ASR: Whisper large-v3 via WhisperX (EN/ZH/ID + code-switch). OpenAI-compatible server.
- Diarization: pyannote 3.1 / sherpa-onnx (NeMo Sortformer to evaluate).
- Identification: 3D-Speaker / WeSpeaker / TitaNet embeddings + voiceprint DB.
- Minutes LLM: Qwen2.5 (or GPT-OSS) via Ollama/vLLM, OpenAI-compatible.

## External services / integrations
- Vexa gateway `:18056` (0.12); our services `:8080` (server), `:8001` (diarize), `:8010`
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
