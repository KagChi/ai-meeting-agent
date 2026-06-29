# Meeting Agent

A standalone meeting agent API & CLI for transcribing and summarizing meeting recordings.

## Features

- **HTTP API Server**: RESTful API for managing meetings, transcripts, and summaries
- **CLI Tool**: Command-line interface for local operations
- **File-Based Storage**: Simple, portable storage using `~/.meeting-agent/` directory
- **OpenAI-Compatible Transcription**: Works with OpenAI or any OpenAI-compatible STT API
- **AI-Powered Summaries**: Generate meeting summaries with key points and action items

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

## License

MIT

## Authors

BMW ECE NTUST
