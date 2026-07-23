# Deployment — AI Meeting Agent (self-hosted, DGX Spark)

This directory holds the **lab-intelligence stack** (ASR, diarize, minutes API).
**Vexa** is the optional meeting-bot spine: a bot joins Teams/Zoom/Meet and
**records audio to MinIO**. Live transcription is **not** done by Vexa — import
the recording into **ai-meeting-agent** and run the existing pipeline.

> **Status:** bring-up blueprint. Rust services here are real; DGX model images
> are pulled; orchestrator (auto MinIO → import → minutes) is still TODO (see
> repo `TODO.md`). End-to-end needs DGX + Docker + a Vexa checkout for bots.

---

## Division of work

| Layer | Owner |
| --- | --- |
| Join Teams / Zoom / Meet + capture → MinIO | **Vexa** (record only) |
| Live / Vexa STT | **Off** (`TRANSCRIBE_ENABLED=false`) |
| Import → transcribe → diarize → minutes | **ai-meeting-agent** |

---

## Which open-source repos to clone?

| Repo | Clone? | Why | How we use it |
| --- | --- | --- | --- |
| **[Vexa-ai/vexa](https://github.com/Vexa-ai/vexa)** | For live bots | Join + record (Apache-2.0). Do not vendor. | `docker-compose.bots.yml` **include**s its compose; capture-only env. |
| attendee-labs/attendee | Optional fallback | If Vexa gaps on a platform | Same webhook/import idea later |
| Zackriya-Solutions/meetily | No | Reference only | Patterns only |
| ASR / diarize / LLM models | Images/weights | WhisperX, speakrs, Qwen | This compose / Ollama |

---

## Topology

```
[Teams/Zoom/Meet] ── bot ──► Vexa (include via docker-compose.bots.yml)
                              gateway :18056 · meeting-api · runtime → vexa-bot
                              MinIO (recordings only; no Vexa STT)
                                    │
                                    │  (manual or Phase-4 orchestrator)
                                    ▼  download recording → import
                              ┌──── this stack (meeting-agent network) ─────────┐
                              │ whisperx :8010     (DGX ASR — import path)     │
                              │ minutes-llm :11434 (DGX Qwen)                  │
                              │ diarize-service :8001                          │
                              │ meeting-agent-server :8080                     │
                              └────────────────────────────────────────────────┘
```

Shared Docker network name: `vexa` (created by this compose; bots overlay attaches).

---

## Compose files

| File | Role |
| --- | --- |
| `docker-compose.yml` | Lab stack only (works without a Vexa clone) |
| `docker-compose.bots.yml` | Includes Vexa + forces **record-only** on `meeting-api` |
| `.env.example` → `.env` | Ports, models, `VEXA_DIR`, `VEXA_API_KEY` |

---

## Bring-up runbook

### 0. Prerequisites (DGX Spark)

- Docker Compose **v2.20+** (`include` support) + NVIDIA container runtime for GPU services.
- **aarch64**: build our Rust images locally; prefer arm64-capable model images.
- For bots: **~8 vCPU / 16 GB RAM**, host Docker socket (Vexa runtime spawns bot containers).

### 1. Lab stack only (no bots)

```bash
cd /path/to/ai-meeting-agent
cp deploy/.env.example deploy/.env   # edit as needed

./deploy/docker-build.sh             # optional: build Rust images
# or:
docker compose -f deploy/docker-compose.yml --env-file deploy/.env up -d --build

docker compose -f deploy/docker-compose.yml exec minutes-llm ollama pull qwen2.5:32b
```

### 2. Lab stack + Vexa bots (record only)

```bash
# Sibling of ai-meeting-agent (matches default VEXA_DIR=../../vexa from deploy/)
git clone https://github.com/Vexa-ai/vexa.git ../vexa

cp ../vexa/deploy/compose/.env.example ../vexa/deploy/compose/.env
# In Vexa's .env: leave TRANSCRIPTION_SERVICE_URL empty.
# Keep BROWSER_IMAGE=vexaai/vexa-bot:v012 (not :dev).
# Optional: make -C ../vexa/deploy/compose bot   # build bot from source

# Our env
cp deploy/.env.example deploy/.env
# Set VEXA_DIR if not ../../vexa (path relative to deploy/)
# After first Vexa up, mint API key via admin-api (ADMIN_TOKEN) → VEXA_API_KEY

docker compose -f deploy/docker-compose.yml \
  -f deploy/docker-compose.bots.yml \
  --env-file deploy/.env up -d --build
```

`docker-compose.bots.yml` sets on Vexa `meeting-api`:

- `TRANSCRIBE_ENABLED=false`
- `RECORDING_ENABLED=true`
- empty `TRANSCRIPTION_SERVICE_URL` / token / model

So bots **join + record** only; ASR stays in this repo after import.

### 3. Diarization (optional)

In `deploy/.env`:

```bash
DIARIZE_ENABLED=true
DIARIZE_SERVICE_URL=http://diarize-service:8001
DIARIZE_EXECUTION_MODE=cuda-fast
DIARIZE_EMBEDDING_MODEL=wespeaker-voxceleb-CAM++_LM
DIARIZE_EMBEDDING_DIM=512
```

Restart: `docker compose -f deploy/docker-compose.yml up -d diarize-service meeting-agent-server`

If a partial HF cache breaks models:

```bash
docker compose -f deploy/docker-compose.yml stop diarize-service
docker volume rm "$(docker volume ls -q | grep diarize-models)"
docker compose -f deploy/docker-compose.yml up -d --build diarize-service
```

### 4. Smoke test

```bash
curl http://127.0.0.1:8080/health          # meeting-agent-server
curl http://127.0.0.1:8001/health          # diarize-service
curl http://127.0.0.1:8010/v1/models       # whisperx (our ASR)

# With bots overlay up — dispatch a Teams bot (Vexa 0.12 gateway):
export VEXA_API_KEY=...   # minted from Vexa admin-api
curl -X POST "http://127.0.0.1:18056/bots" \
  -H "X-API-Key: $VEXA_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"platform":"teams","native_meeting_id":"<id-from-share-link>","bot_name":"BMW-Lab"}'
```

Admit the bot in the Teams lobby if required. After the call:

1. Fetch the recording from Vexa (`GET /recordings` or MinIO console `:9001`).
2. Transcribe with this stack:

```bash
meeting-agent import /path/to/recording.wav --title "Lab meeting"
# or POST multipart to meeting-agent-server /import
```

---

## Ports (defaults)

| Service | Host port |
| --- | --- |
| meeting-agent-server | 8080 |
| diarize-service | 8001 |
| whisperx | 8010 |
| minutes-llm (Ollama) | 11434 |
| Vexa gateway | **18056** (0.12; not 8056) |
| Vexa admin-api | 18057 |
| Vexa MinIO | 9000 / console 9001 |
| Vexa Terminal (optional UI) | 13000 |

---

## What still needs building (see repo TODO.md)

- **Phase 3** — SOP-format minutes on the DGX LLM.
- **Phase 4** — orchestrator: Vexa meeting-end → pull MinIO recording → meeting-agent import (ASR here) → minutes → daily-log → Google Calendar.

### Open decision (orchestrator language)

Rust keeps one language; Python matches Vexa/Google client libs. Pick before Phase 4.
