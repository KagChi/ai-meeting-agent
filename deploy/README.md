# Deployment — AI Meeting Agent (self-hosted, DGX Spark)

Lab stack: **ASR + diarize + minutes API + meeting-bot** (Teams join/record).

Clients (e.g. **Meetily**) talk only to **meeting-agent-server :8080**.  
Live join is handled by **`services/meeting-bot`** (internal); the server proxies `POST /bots`.

> **Status:** compose + images are a bring-up blueprint. End-to-end Teams join needs a
> working Playwright/audio environment (Docker recommended). See repo `TODO.md`.

---

## Division of work

| Layer | Owner |
| --- | --- |
| Join Teams (v1) + record → local disk | **meeting-bot** (Bun, Playwright) |
| Public API / import / ASR / diarize / minutes | **meeting-agent-server** + WhisperX / diarize / LLM |
| UI | **Meetily** → agent API only (`POST /bots`, then poll jobs) |

Optional legacy: Vexa as an external capture spine (`docker-compose.bots.yml`) — not required if you use meeting-bot.

---

## Topology

```
[Teams] ── Playwright ──► meeting-bot :8091 (internal)
                              │  local WAV + SQLite
                              │  POST /import
                              ▼
                         meeting-agent-server :8080  ◄── Meetily / clients
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
         whisperx :8010  diarize :8001  minutes-llm :11434
```

---

## Images

| Image | Build |
| --- | --- |
| `meeting-agent-server` | `deploy/Dockerfile.server` |
| `meeting-agent-diarize-service` | `deploy/Dockerfile.diarize` |
| **`meeting-bot`** | `services/meeting-bot/Dockerfile` |

```bash
# All three (default platform linux/amd64)
./deploy/docker-build.sh

# DGX Spark / aarch64
PLATFORM=linux/arm64 ./deploy/docker-build.sh

# meeting-bot only
docker build -f services/meeting-bot/Dockerfile \
  -t ghcr.io/bmw-ece-ntust/ai-meeting-agent/meeting-bot:latest \
  services/meeting-bot
```

CI (`.github/workflows/docker-build.yml`) builds and pushes all three images to GHCR on `main`/`dev` and tags.

---

## Bring-up

```bash
cd /path/to/ai-meeting-agent
cp deploy/.env.example deploy/.env
# Edit secrets if needed. Defaults enable meeting-bot proxy:
#   MEETING_BOT_ENABLED=true
#   MEETING_BOT_URL=http://meeting-bot:8091

./deploy/docker-build.sh
docker compose -f deploy/docker-compose.yml --env-file deploy/.env up -d

# Minutes model (once)
docker compose -f deploy/docker-compose.yml exec minutes-llm ollama pull qwen2.5:32b
```

### Smoke

```bash
curl -s http://127.0.0.1:8080/health
curl -s http://127.0.0.1:8091/health          # bot (localhost bind only)
curl -s -H "X-API-Key: $KEY" http://127.0.0.1:8080/bots/platforms

# Start Teams bot (admit guest in lobby)
curl -s -X POST http://127.0.0.1:8080/bots \
  -H "Content-Type: application/json" \
  -H "X-API-Key: $KEY" \
  -d '{"platform":"teams","meeting_url":"https://teams.microsoft.com/l/meetup-join/...","title":"Lab"}'
```

Poll `GET /bots/{id}` until `completed`, then `GET /jobs/{meeting_agent_job_id}/status`.

---

## Env (meeting-bot related)

| Variable | Role |
| --- | --- |
| `MEETING_BOT_ENABLED` | Server exposes `/bots` proxy (`true` in compose default) |
| `MEETING_BOT_URL` | Internal URL (`http://meeting-bot:8091` in compose) |
| `MEETING_BOT_INTERNAL_KEY` | Shared secret agent → bot (`BOT_API_KEY` on worker) |
| `MEETING_BOT_IMAGE` | Image tag for compose |
| `MEETING_BOT_HEADLESS` | Chromium headless in container (default `true`) |
| `MEETING_AGENT_URL` | Set on **bot** to `http://meeting-agent-server:8080` for `/import` |

Full template: [`.env.example`](.env.example).

---

## Compose services

| Service | Port (host) | Notes |
| --- | --- | --- |
| meeting-agent-server | 8080 | Public API |
| meeting-bot | 127.0.0.1:8091 | Internal worker; volume `meeting-bot-data` |
| whisperx | 8010 | ASR |
| diarize-service | 8001 | Optional GPU diarize |
| minutes-llm | 11434 | Ollama |

---

## Diarization

Set in `deploy/.env`:

```bash
DIARIZE_ENABLED=true
DIARIZE_SERVICE_URL=http://diarize-service:8001
DIARIZE_EXECUTION_MODE=cuda-fast
```

Restart: `docker compose -f deploy/docker-compose.yml up -d diarize-service meeting-agent-server`

---

## Optional: Vexa overlay

Legacy capture spine: `docker-compose.bots.yml` includes a Vexa checkout. Prefer **meeting-bot** for lab Teams join. See comments in that file and repo `TODO.md`.

---

## What still needs building

- Meetily UI for “Join meeting” (agent `/bots` only)  
- SOP minutes + daily-log / GCal (Phase 4 remainder)  
- Zoom / Google Meet adapters in `services/meeting-bot`
