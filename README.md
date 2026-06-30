# Meeting Agent

A standalone meeting agent API & CLI for transcribing and summarizing meeting recordings.

## Features

- **HTTP API Server**: RESTful API for managing meetings, transcripts, and summaries
- **CLI Tool**: Command-line interface for local operations
- **File-Based Storage**: Simple, portable storage using `~/.meeting-agent/` directory
- **OpenAI-Compatible Transcription**: Works with OpenAI or any OpenAI-compatible STT API
- **AI-Powered Summaries**: Generate meeting summaries with key points and action items

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
# Using the CLI
meeting-agent server --port 8080

# Or run the server binary directly
meeting-agent-server
```

### CLI Commands

```bash
# Import a meeting recording
meeting-agent import meeting.wav --title "Q3 Planning"

# List all meetings
meeting-agent list

# Show meeting details
meeting-agent show <meeting-id>

# Generate summary
meeting-agent summarize <meeting-id>

# Export transcript
meeting-agent export <meeting-id> --format srt

# Manage configuration
meeting-agent config show
meeting-agent config set transcription.provider openai
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

#### Configuration
- `GET /config` - Get current config
- `PUT /config` - Update config

## Data Storage

All data is stored in `~/.meeting-agent/`:

```
~/.meeting-agent/
├── config.json
└── meetings/{id}/
    ├── metadata.json
    ├── audio.wav
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

## License

MIT

## Authors

BMW ECE NTUST
