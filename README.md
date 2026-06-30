# Meeting Agent

A standalone meeting agent API & CLI for transcribing and summarizing meeting recordings.

## Features

- **HTTP API Server**: RESTful API for managing meetings, transcripts, and summaries
- **CLI Tool**: Command-line interface for local operations
- **Interactive Config Wizard**: Guided setup via `meeting-agent config edit`
- **Live Config API**: Update server config at runtime via `PUT /config` endpoints
- **OpenAPI / Swagger UI**: Interactive API docs at `/docs`, spec at `/api-docs/openapi.json`
- **File-Based Storage**: Simple, portable storage using `~/.meeting-agent/` directory
- **OpenAI-Compatible Transcription**: Works with OpenAI or any OpenAI-compatible STT API
- **Chunked Transcription**: Automatically splits long audio into chunks for parallel transcription
- **Speaker Diarization**: Optional speaker labeling via standalone sherpa-onnx microservice
- **AI-Powered Summaries**: Generate meeting summaries with key points, action items, and decisions

## Architecture

The project uses a workspace structure with four crates:

- **`meeting-agent-core`**: Shared business logic, models, and file system operations
- **`meeting-agent-server`**: Axum-based HTTP API server
- **`meeting-agent-cli`**: Command-line interface and API client
- **`meeting-agent-diarize`**: Standalone speaker diarization microservice (sherpa-onnx)

## Installation

### Prerequisites

- Rust 1.70+ (install via [rustup](https://rustup.rs/))
- OpenAI API key or compatible STT service

### Build from Source

```bash
# Clone the repository
git clone https://github.com/bmw-ece-ntust/ai-meeting-agent.git
cd ai-meeting-agent

# Build all binaries
cargo build --release

# Binaries will be in target/release/
# - meeting-agent-server
# - meeting-agent
```

## Configuration

Copy `.env.example` to `.env` and configure:

```bash
cp .env.example .env
```

Key environment variables:

```bash
# Server
MEETING_AGENT_PORT=8080
MEETING_AGENT_HOST=127.0.0.1

# Transcription (choose one provider)
TRANSCRIPTION_PROVIDER=openai
TRANSCRIPTION_API_KEY=your-api-key-here
TRANSCRIPTION_BASE_URL=https://api.openai.com/v1
TRANSCRIPTION_MODEL=whisper-1

# Summary
SUMMARY_PROVIDER=openai
SUMMARY_API_KEY=your-api-key-here
SUMMARY_BASE_URL=https://api.openai.com/v1
SUMMARY_MODEL=gpt-4o-mini
SUMMARY_TEMPERATURE=0.3
SUMMARY_MAX_TOKENS=1024
SUMMARY_LANGUAGE=en
```

## Usage

### Start the Server

```bash
# Using the CLI (default port 8080, host 127.0.0.1)
meeting-agent server

# Custom port and host
meeting-agent server --port 3000 --host 0.0.0.0

# Or run the server binary directly
meeting-agent-server
```

### API Documentation (Swagger UI)

Once the server is running, open:

- **Swagger UI**: `http://127.0.0.1:8080/docs`
- **OpenAPI JSON spec**: `http://127.0.0.1:8080/api-docs/openapi.json`

### CLI Commands

```bash
# Import a meeting recording (with optional title)
meeting-agent import meeting.wav --title "Q3 Planning"
meeting-agent import recording.mp3

# List all meetings
meeting-agent list

# Show meeting details (8-char ID prefix supported)
meeting-agent show abc12345

# Generate summary (templates: full, key-points, action-items, decisions)
meeting-agent summarize abc12345 --template key-points
meeting-agent summarize abc12345 --template action-items --language en

# Export transcript (formats: srt, vtt, text, json)
meeting-agent export abc12345 --format srt
meeting-agent export abc12345 --format json --output transcript.json

# Manage configuration
meeting-agent config show
meeting-agent config set transcription.provider openai
meeting-agent config set server.port 3000
meeting-agent config set diarize.enabled true

# Interactive config wizard (guided setup)
meeting-agent config edit
```

### curl Examples

```bash
# Health check
curl http://127.0.0.1:8080/health

# List meetings
curl -H "X-API-Key: your-key" http://127.0.0.1:8080/meetings

# Import audio file
curl -X POST -H "X-API-Key: your-key" \
  -F "file=@meeting.mp3" -F "title=Q3 Planning" \
  http://127.0.0.1:8080/import

# Check job status
curl -H "X-API-Key: your-key" http://127.0.0.1:8080/jobs/{job_id}/status

# Generate summary
curl -X POST -H "X-API-Key: your-key" \
  http://127.0.0.1:8080/meetings/{id}/summary

# Get current config (secrets masked)
curl -H "X-API-Key: your-key" http://127.0.0.1:8080/config

# Update transcription config
curl -X PUT -H "X-API-Key: your-key" -H "Content-Type: application/json" \
  -d '{"provider":"groq","base_url":"https://api.groq.com/openai/v1","model":"whisper-large-v3","chunk_seconds":600,"chunk_concurrency":2}' \
  http://127.0.0.1:8080/config/transcription
```

### API Endpoints

#### Health & Info
- `GET /health` - Health check
- `GET /version` - Version info

#### Meetings
- `GET /meetings` - List all meetings
- `GET /meetings/{id}` - Get meeting details
- `POST /meetings` - Create meeting
- `PATCH /meetings/{id}` - Update meeting
- `DELETE /meetings/{id}` - Delete meeting

#### Transcripts & Summaries
- `GET /meetings/{id}/transcript` - Get transcript
- `GET /meetings/{id}/summary` - List all summaries for a meeting
- `POST /meetings/{id}/summary` - Generate summary (templates: key_points, action_items, decisions, full)
- `GET /meetings/{id}/summary/{template}` - Get specific summary

#### Jobs (shared by import & summary)
- `POST /import` - Import audio file
- `GET /jobs/{job_id}/status` - Check job status
- `GET /jobs/{job_id}/events` - SSE stream of job progress
- `POST /jobs/{job_id}/cancel` - Cancel a running job

#### Configuration (Live API)
- `GET /config` - Get current config (secrets masked as `****`)
- `PUT /config` - Update full config (validates before saving)
- `GET /config/transcription` - Get transcription config
- `PUT /config/transcription` - Update transcription config
- `GET /config/summary` - Get summary config
- `PUT /config/summary` - Update summary config

> **Secret handling**: API keys are masked (`****`) in GET responses. To keep
> an existing key unchanged, send `"****"` in PUT requests. To replace, send
> the new key value.

#### API Documentation
- `GET /docs` - Swagger UI (interactive API docs)
- `GET /api-docs/openapi.json` - OpenAPI 3.0 spec

## Data Storage

All data is stored in `~/.meeting-agent/`:

```
~/.meeting-agent/
├── config.json
└── meetings/{id}/
    ├── meeting.json
    ├── audio/
    │   └── {original-filename}
    ├── transcript.json
    └── summaries/
        ├── key_points.json
        ├── action_items.json
        ├── decisions.json
        └── full.json
```

## Development

```bash
# Run tests
cargo test

# Check code
cargo check

# Format code
cargo fmt

# Lint
cargo clippy
```

## Diarization Service

`meeting-agent-diarize` is a standalone HTTP microservice that performs
speaker diarization on an audio file using a Whisper transcript. It wraps
`sherpa-onnx`'s `OfflineSpeakerDiarization` (pyannote segmentation +
3D-Speaker embedding models) and merges speaker labels into the transcript
via max-timestamp-overlap.

### API

```
POST /v1/diarize   multipart: file (mp3/wav), transcript (Whisper JSON), [num_speakers]
                  → 200 {"num_speakers": N, "segments": [{"start","end","speaker","text"}]}
GET  /health       → 200 {"status":"ok"}
```

### Run via Docker (recommended)

Prebuilt multi-arch image (linux/amd64, linux/arm64) with models baked in:

```bash
docker pull ghcr.io/bmw-ece-ntust/ai-meeting-agent/diarize-server:latest
docker run --rm -p 8002:8002 ghcr.io/bmw-ece-ntust/ai-meeting-agent/diarize-server:latest
```

The image ships the segmentation + embedding ONNX models at `/models/` and
sets the `DIARIZE_*` env defaults — no volume mounts required.

### Run via binary tarball

Download `diarize-server-linux-{amd64,arm64}.tar.gz` from the latest
[release](https://github.com/bmw-ece-ntust/ai-meeting-agent/releases),
extract, and run:

```bash
tar xzf diarize-server-linux-amd64.tar.gz
cd diarize-server-linux-amd64
./run.sh
```

The tarball ships the binary + a `run.sh` wrapper that sets
`LD_LIBRARY_PATH`. Models are **not** bundled — download them and point
the `DIARIZE_*_MODEL` env vars at them:

```bash
# Segmentation model (extracts to sherpa-onnx-pyannote-segmentation-3-0/)
curl -SL -o seg.tar.bz2 https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2
tar xjf seg.tar.bz2

# Embedding model (bare .onnx)
curl -SL -o 3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx \
  https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx
```

### Environment variables

| Variable | Default | Description |
| --- | --- | --- |
| `DIARIZE_HOST` | `0.0.0.0` | Bind address |
| `DIARIZE_PORT` | `8002` | Listen port |
| `DIARIZE_SEGMENTATION_MODEL` | (required) | Path to pyannote-segmentation-3.0 `model.onnx` |
| `DIARIZE_EMBEDDING_MODEL` | (required) | Path to 3D-Speaker ERes2Net `.onnx` |
| `DIARIZE_NUM_SPEAKERS` | `0` | Override speaker count (`0` = auto-detect) |
| `DIARIZE_CLUSTERING_THRESHOLD` | `0.5` | Agglomerative clustering threshold |
| `DIARIZE_MAX_BODY_MB` | `512` | Max request body size accepted by diarize-server (MB) |
| `DIARIZE_TIMEOUT_SECS` | `900` | Client request timeout for diarize calls (seconds) |

## Troubleshooting

### `ffmpeg` not found

Audio conversion and chunking require `ffmpeg` + `ffprobe` on your `PATH`:

```bash
# macOS
brew install ffmpeg

# Ubuntu/Debian
sudo apt install ffmpeg
```

### Transcription fails with 401/403

Check `TRANSCRIPTION_API_KEY` is set and valid:

```bash
meeting-agent config show
# Verify api_key field is not "(not set)"
```

### Audio file too large / transcription timeout

Long audio is auto-chunked. Adjust chunk settings:

```bash
meeting-agent config set transcription.chunk_seconds 300
meeting-agent config set transcription.chunk_concurrency 4
```

### Diarization not working

Ensure the diarize microservice is running and `diarize.enabled` is `true`:

```bash
meeting-agent config set diarize.enabled true
meeting-agent config set diarize.base_url http://localhost:8002
```

Verify the service is up:

```bash
curl http://localhost:8002/health
```

### Config file permissions

The config file (`~/.meeting-agent/config.json`) is created with `chmod 600`
(owner read/write only). If permissions are wrong:

```bash
chmod 600 ~/.meeting-agent/config.json
```

### Reset to defaults

Delete the config file and run any command — a fresh default config is
auto-created:

```bash
rm ~/.meeting-agent/config.json
meeting-agent config show
```

## Deployment

### Systemd Service (Linux)

Create `/etc/systemd/system/meeting-agent.service`:

```ini
[Unit]
Description=Meeting Agent API Server
After=network.target

[Service]
Type=simple
User=meeting
WorkingDirectory=/opt/meeting-agent
EnvironmentFile=/opt/meeting-agent/.env
ExecStart=/opt/meeting-agent/meeting-agent-server
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now meeting-agent
```

### Docker

```dockerfile
FROM rust:1.70-slim as builder
WORKDIR /app
COPY . .
RUN apt-get update && apt-get install -y ffmpeg && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ffmpeg && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/meeting-agent-server /usr/local/bin/
EXPOSE 8080
CMD ["meeting-agent-server"]
```

```bash
docker build -t meeting-agent .
docker run -p 8080:8080 -v ~/.meeting-agent:/root/.meeting-agent meeting-agent
```

### Reverse Proxy (nginx)

```nginx
server {
    listen 80;
    server_name meetings.example.com;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # SSE support (for /jobs/{id}/events)
        proxy_buffering off;
        proxy_cache off;
        proxy_read_timeout 86400;
    }
}
```

## License

MIT

## Authors

BMW ECE NTUST
