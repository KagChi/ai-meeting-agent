# meeting-bot

Internal **meeting join + record** worker for [ai-meeting-agent](../../).

- **Runtime:** Bun + Elysia  
- **v1 platform:** Microsoft Teams (guest join via Playwright)  
- **Storage:** SQLite job metadata + local WAV under `DATA_DIR`  
- **Handoff:** `POST {MEETING_AGENT_URL}/import` after the call  

**Clients (Meetily) should not call this service.** Use the agent public API (`POST /bots` on meeting-agent-server), which proxies here.

## Quick start

```bash
cd services/meeting-bot
bun install
bunx playwright install chromium

export MEETING_AGENT_URL=http://127.0.0.1:8080
export MEETING_AGENT_API_KEY=   # if agent requires it
export BOT_API_KEY=dev-internal # optional; agent should send same key
export DATA_DIR=./data

bun run dev
# → http://127.0.0.1:8091
```

## Internal API

| Method | Path | Notes |
| --- | --- | --- |
| `GET` | `/health` | No auth |
| `GET` | `/platforms` | teams available; zoom/meet stubs |
| `POST` | `/bots` | body: `platform`, `meeting_url` or `native_meeting_id`, … → `202` |
| `GET` | `/bots` | list |
| `GET` | `/bots/:id` | status + `meeting_agent_job_id` when done |
| `DELETE` | `/bots/:id` | cancel / leave |

Auth: if `BOT_API_KEY` or `MEETING_BOT_INTERNAL_KEY` is set, require header `X-API-Key`.

## Env

| Variable | Default | Description |
| --- | --- | --- |
| `BOT_PORT` | `8091` | Listen port |
| `BOT_HOST` | `0.0.0.0` | Bind address |
| `BOT_API_KEY` | empty | Internal auth |
| `DATA_DIR` | `./data` | SQLite + recordings |
| `SQLITE_PATH` | `$DATA_DIR/meeting-bot.db` | DB path |
| `MEETING_AGENT_URL` | `http://127.0.0.1:8080` | Import target |
| `MEETING_AGENT_API_KEY` | empty | Agent API key |
| `BOT_NAME` | `BMW-Lab-Bot` | Guest display name |
| `JOIN_TIMEOUT_MS` | `900000` | Lobby admit timeout |
| `HEADLESS` | `false` | Chromium headless |
| `MAX_CONCURRENT_BOTS` | `1` | Parallel jobs |

## How this mirrors Vexa (Teams)

| Concern | Vexa | meeting-bot |
| --- | --- | --- |
| Join | Playwright + `msteams/join.ts` | `platforms/teams/adapter.ts` (same steps/selectors) |
| Record | **In-page MediaRecorder** (Web Audio mix) | `capture/media-recorder.ts` |
| Pulse/ffmpeg host | Zoom Web only | Optional fallback only (`capture/recorder.ts`) |
| After call | Chunks → MinIO | Local WAV/WebM → agent `POST /import` |

## Teams notes

1. Pass a full Teams **join URL** (`meeting_url`).  
2. Host must **admit** the guest from the lobby.  
3. Audio is captured **inside Chromium** (MediaRecorder), not host Pulse — same as Vexa for Teams.  
4. Selectors live in `src/platforms/teams/selectors.ts` — update when Teams UI changes.  
5. Camera uses blank/black fallback (no green Chromium test pattern).

## Docker

### Via lab deploy (recommended)

From the **repo root**:

```bash
# Builds server + diarize + meeting-bot
./deploy/docker-build.sh
# arm64: PLATFORM=linux/arm64 ./deploy/docker-build.sh

cp deploy/.env.example deploy/.env
docker compose -f deploy/docker-compose.yml --env-file deploy/.env up -d meeting-bot meeting-agent-server
```

GitHub Actions (`.github/workflows/docker-build.yml`) also builds/pushes `meeting-bot` to GHCR alongside server and diarize.

Compose service name: **`meeting-bot`**. Agent env:

- `MEETING_BOT_ENABLED=true`
- `MEETING_BOT_URL=http://meeting-bot:8091`

Meetily / clients still use **only** `http://…:8080` (`POST /bots`).

### Standalone image

```bash
# From services/meeting-bot
docker build -t meeting-bot:local .

# Or from repo root (same as docker-build.sh)
docker build -f services/meeting-bot/Dockerfile \
  -t ghcr.io/bmw-ece-ntust/ai-meeting-agent/meeting-bot:latest \
  services/meeting-bot

docker run --rm -p 127.0.0.1:8091:8091 \
  -e MEETING_AGENT_URL=http://host.docker.internal:8080 \
  -e HEADLESS=true \
  -e BOT_API_KEY=dev \
  -v meeting-bot-data:/data \
  --shm-size=1g \
  meeting-bot:local
```

## Adding a platform

1. Implement `PlatformAdapter` under `src/platforms/<name>/`.  
2. Register in `src/platforms/registry.ts`.  
3. No change to public agent routes if they already proxy `/bots`.
