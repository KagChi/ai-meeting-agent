# Deployment — AI Meeting Agent (self-hosted, DGX Spark)

This directory holds the **lab-intelligence stack**. It runs alongside **Vexa**
(the meeting-bot spine). Together they deliver the PRD vision: a bot joins
Teams/Zoom/Meet, transcription + speaker identification run on the DGX Spark, and
SOP minutes / daily-log / calendar updates are produced automatically.

> **Status:** this is a *bring-up blueprint*. The Rust services in this repo are
> real; the DGX model services are pulled images; the **orchestrator** (post-meeting
> glue) and the **/v1/identify** voiceprint endpoints are still to be implemented
> (see the repo `TODO.md`). Nothing here has been run end-to-end yet — it needs the
> DGX hardware, a Rust toolchain to build the images, and Google credentials.

---

## Which open-source repos to clone?

**Yes — one, and only one, external repo is required: [Vexa](https://github.com/Vexa-ai/vexa)** (Apache-2.0).
Do **not** vendor it into this repo; run it as a sibling stack.

| Repo | Clone? | Why | How we use it |
| --- | --- | --- | --- |
| **Vexa-ai/vexa** | **Yes (required)** | The bot spine: joins Teams/Zoom/Meet, records, realtime transcription, per-speaker audio, webhooks, MCP. Building this ourselves is the wrong bet. | Run its own compose (`make all`); we attach to its `vexa` network and call `POST /bots` + consume transcripts. |
| attendee-labs/attendee | Optional (fallback) | Alternative bot spine if Vexa gaps on a platform. | Not needed unless Vexa fails on Teams/Zoom for us. |
| Zackriya-Solutions/meetily | No (reference only) | Rust, self-hosted, close to our post-processing half — but bot-free. | Read for patterns; don't depend on it. |
| ASR / diarization / embedding models | Pulled as images/weights, not repos | WhisperX, pyannote/3D-Speaker, Qwen | Pulled by the compose / Ollama at runtime. |

We already keep the **voiceprint** models in this repo's `Dockerfile.diarize`
(pyannote segmentation + 3D-Speaker embedding), so no extra clone for identity.

---

## Topology

```
[Teams/Zoom/Meet] ── bot ──► Vexa stack (own compose, network: vexa)
                              api-gateway :8056 · meeting-api · vexa-bot · MinIO(recordings)
                                   │  realtime transcription  ▲ per-speaker audio + transcript
                                   ▼  (TRANSCRIPTION_SERVICE_URL override)
                              ┌──────────────── this stack (network: vexa + meeting-agent) ─────────────┐
                              │ whisperx :8010  (DGX, OpenAI STT)                                        │
                              │ minutes-llm :11434 (DGX, Qwen)                                           │
                              │ meeting-agent-server :8080 (in-process CPU diarization via speakrs)      │
                              │ MCP: CLI only (meeting-agent-mcp), not containerized                     │
                              │ orchestrator (Phase 3-4): Vexa→SOP minutes→daily-log→GCal               │
                              └────────────────────────────────────────────────────────────────────────┘
```

The key wiring: **Vexa's realtime transcription is pointed at our DGX WhisperX**
via `TRANSCRIPTION_SERVICE_URL`, so no meeting audio and no transcription leaves
the lab.

**Diarization**: Separate GPU service (`diarize-service`, Ubuntu 24.04 + CUDA)
using `speakrs`. When `DIARIZE_ENABLED=true`, meeting-agent-server calls it via
`DIARIZE_SERVICE_URL` (default `http://diarize-service:8001`). Models (~200MB)
are auto-downloaded into the `diarize-models` volume on first run.

---

## Bring-up runbook

### 0. Prerequisites (on the DGX Spark)
- Docker + Docker Compose with the NVIDIA container runtime (GPU passthrough).
- The box is **aarch64** — ensure images are arm64 (build our two Rust images
  locally; `speaches`/`ollama` publish arm64). Reserve GPU in compose (already set).

### 1. Clone + start Vexa
```bash
git clone https://github.com/Vexa-ai/vexa.git ~/Documents/GitHub/vexa
cd ~/Documents/GitHub/vexa
cp deploy/env-example .env
# Point Vexa's transcription at our DGX WhisperX (edit .env):
#   TRANSCRIPTION_SERVICE_URL=http://whisperx:8000/v1/audio/transcriptions
make all            # starts the vexa stack + creates the `vexa` docker network
```
Mint a self-hosted API key (via the admin-api with `ADMIN_TOKEN`) — see Vexa's
`deploy/README.md`. Put it in our `deploy/.env` as `VEXA_API_KEY`.

### 2. Build and start our stack
```bash
cd ~/Documents/GitHub/ai-meeting-agent
cp deploy/.env.example deploy/.env      # then fill it in

# Option A: Build locally using the helper script
./deploy/docker-build.sh

# Option B: Build with docker compose
docker compose -f deploy/docker-compose.yml --env-file deploy/.env up -d --build

# Pull the minutes LLM once:
docker compose -f deploy/docker-compose.yml exec minutes-llm ollama pull qwen2.5:32b
```

**To enable diarization**, edit `deploy/.env`:
```bash
DIARIZE_ENABLED=true
DIARIZE_SERVICE_URL=http://diarize-service:8001
DIARIZE_EXECUTION_MODE=auto
```
Then restart: `docker compose -f deploy/docker-compose.yml up -d diarize-service meeting-agent-server`

### 3. Smoke test
```bash
curl http://localhost:8080/health                          # meeting-agent-server
curl http://localhost:8001/health                          # diarize-service
curl http://localhost:8010/v1/models                       # whisperx (OpenAI STT)
# Send a bot to a live meeting (via Vexa):
curl -X POST "http://localhost:8056/bots" -H "X-API-Key: $VEXA_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"platform":"teams","native_meeting_id":"<share-link-id>"}'
```

---

## What still needs building before this is "done" (see repo TODO.md)
- **Phase 3** — SOP-format minutes generation wired to the DGX LLM.
- **Phase 4** — the **orchestrator** service (Vexa webhook → minutes →
  daily-log → Google Calendar). Uncomment its block in `docker-compose.yml` once it exists.

### Open decision (orchestrator language)
The orchestrator is a new service. Rust keeps one language; Python matches the
Vexa/DGX ecosystem and its Google/LLM client libraries. Pick before Phase 4.
