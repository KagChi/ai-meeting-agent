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

## Teams notes

1. Pass a full Teams **join URL** (`meeting_url`).  
2. Host must **admit** the guest from the lobby.  
3. System audio capture uses **ffmpeg + Pulse** (`default` source). On macOS/desktop without Pulse, install/configure audio or run in the Docker image.  
4. Selectors live in `src/platforms/teams/selectors.ts` — update when Teams UI changes.

## Docker

```bash
docker build -t meeting-bot:local .
docker run --rm -p 8091:8091 \
  -e MEETING_AGENT_URL=http://host.docker.internal:8080 \
  -e HEADLESS=true \
  -v meeting-bot-data:/data \
  meeting-bot:local
```

## Adding a platform

1. Implement `PlatformAdapter` under `src/platforms/<name>/`.  
2. Register in `src/platforms/registry.ts`.  
3. No change to public agent routes if they already proxy `/bots`.
