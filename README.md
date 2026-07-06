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
- **Speaker Diarization**: Optional in-process speaker labeling via speakrs (pyannote pipeline)
- **AI-Powered Summaries**: Generate meeting summaries with key points, action items, and decisions

## Architecture

The project uses a workspace structure with three crates:

- **`meeting-agent-core`**: Shared business logic, models, and file system operations
- **`meeting-agent-server`**: Axum-based HTTP API server
- **`meeting-agent-cli`**: Command-line interface and API client

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

#### Health & Info (public — no auth required)
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

## Workspace Structure

```
ai-meeting-agent/
├── Cargo.toml              # Workspace root
├── crates/
│   ├── core/               # meeting-agent-core: business logic, models, storage, diarization
│   ├── server/             # meeting-agent-server: Axum HTTP API + OpenAPI
│   └── cli/                # meeting-agent-cli: command-line interface
├── docs/
│   └── API.md              # API specification document
├── .env.example            # Configuration template
└── README.md
```

### Crate Dependencies

| Crate | Key Dependencies |
|-------|-----------------|
| `meeting-agent-core` | axum, tokio, serde, reqwest, uuid, chrono, anyhow, thiserror, dirs, ffmpeg-sidecar, speakrs, symphonia |
| `meeting-agent-server` | axum, tower-http (cors, trace, compression-gzip), utoipa, utoipa-swagger-ui |
| `meeting-agent-cli` | clap, colored, indicatif, comfy-table, dialoguer |

### Workspace Verification

```bash
$ cargo check --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.46s
```

All four crates compile cleanly with no warnings or errors.

## Development

```bash
# Run tests
cargo test

# Check code
cargo check --workspace

# Format code
cargo fmt

# Lint
cargo clippy
```

## Diarization

Speaker diarization runs **in-process** via [`speakrs`](https://crates.io/crates/speakrs),
a Rust-native pyannote `community-1` style pipeline (segmentation + embedding +
VBx clustering). No separate server or Python runtime is required — the first
import with `diarize.enabled=true` loads the model once and caches it for the
process lifetime.

### Enable

```bash
meeting-agent config set diarize.enabled true
```

### Automatic GPU Detection

By default, `meeting-agent` automatically detects and uses GPU acceleration for
speaker diarization when available, with graceful fallback to CPU:

- **macOS**: Tries CoreML (fast) → CoreML (standard) → CPU
- **Linux/Windows**: Tries CUDA (NVIDIA) → MIGraphX (AMD) → CPU

If GPU initialization fails (missing drivers, insufficient memory, etc.), the
system logs a warning and automatically falls back to CPU mode. No manual
configuration is required.

### Execution modes

| Mode | Backend | Use it for |
| --- | --- | --- |
| `auto` (default) | Platform-specific GPU priority | Automatic GPU detection with CPU fallback |
| `cpu` | ONNX Runtime CPU | Portable, widest compatibility |
| `coreml` | Native CoreML | macOS with CoreML acceleration |
| `coreml-fast` | Native CoreML (2s step) | macOS, faster on long meetings |
| `cuda` | ONNX Runtime CUDA | NVIDIA GPU |
| `cuda-fast` | ONNX Runtime CUDA (2s step) | NVIDIA GPU, faster on long meetings |
| `migraphx` | ONNX Runtime MIGraphX | AMD GPU |

To override automatic detection and force a specific mode:

```bash
meeting-agent config set diarize.execution_mode cpu
```

### Models

With the default `online` feature, `speakrs` downloads models on first use
from [avencera/speakrs-models](https://huggingface.co/avencera/speakrs-models)
to a local cache. Set `DIARIZE_MODEL_DIR` to point at a pre-bundled model
directory for offline/airgapped setups:

```bash
meeting-agent config set diarize.model_dir /opt/speakrs-models
```

### Environment variables

| Variable | Default | Description |
| --- | --- | --- |
| `DIARIZE_ENABLED` | `false` | Enable speaker diarization during import |
| `DIARIZE_EXECUTION_MODE` | `auto` | `auto` \| `cpu` \| `coreml` \| `coreml-fast` \| `cuda` \| `cuda-fast` \| `migraphx` |
| `DIARIZE_MODEL_DIR` | (blank) | Local model dir; blank = download on first use |

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

Ensure `diarize.enabled` is `true` and the execution mode is valid for
your platform:

```bash
meeting-agent config set diarize.enabled true
meeting-agent config show
```

The first import with diarization enabled downloads the speakrs models
(~hundreds of MB) on first use; subsequent imports reuse the cached
pipeline. For offline setups, point `diarize.model_dir` at a pre-bundled
model directory.

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
