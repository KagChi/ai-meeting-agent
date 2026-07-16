# Meeting Agent MCP

MCP tools for the Meeting Agent REST API.

## Run HTTP Server

Start the Meeting Agent API first:

```bash
cargo run --bin meeting-agent-server
```

Start MCP HTTP server:

```bash
MEETING_AGENT_BASE_URL=http://localhost:8080 cargo run --bin meeting-agent-mcp-server
```

Default MCP endpoint:

```text
http://localhost:9080/mcp
```

Health endpoint:

```text
http://localhost:9080/health
```

## Configuration

```bash
MCP_HOST=0.0.0.0
MCP_PORT=9080
MEETING_AGENT_BASE_URL=http://localhost:8080
MEETING_AGENT_API_KEY=

# OpenClaw inbound attachments (importFromFile with media://inbound/…)
# Default: ~/.openclaw/media/inbound
# OPENCLAW_MEDIA_INBOUND_DIR=/path/to/openclaw/media/inbound
```

No MCP-side auth enforced for now. CORS is permissive for local/dev clients.

### OpenClaw `media://inbound/` paths

OpenClaw attaches files as pseudo-URIs like `media://inbound/<name>.mp3`.
`importFromFile` maps that prefix to a real directory:

| `file_path` | Resolved path |
| --- | --- |
| `media://inbound/clip.mp3` | `$OPENCLAW_MEDIA_INBOUND_DIR/clip.mp3` |
| `/tmp/meeting.wav` | unchanged (plain filesystem path) |

Default base dir: `~/.openclaw/media/inbound`. Override with
`OPENCLAW_MEDIA_INBOUND_DIR` when OpenClaw stores media elsewhere or when the
MCP process runs under a different user/home than OpenClaw.

The MCP process must share the same filesystem view as OpenClaw (same host, or
a bind-mount of that inbound directory).

## Run Stdio CLI

Use `meeting-agent-mcp` as a local MCP server command for clients that launch MCP over stdio:

```bash
MEETING_AGENT_BASE_URL=http://localhost:8080 cargo run --bin meeting-agent-mcp
```

Logs are written to stderr so stdout stays reserved for MCP JSON-RPC.

## Tools

- `importMeetingAudio`: multipart upload via REST API, returns `job_id`
- `getJobStatus`: poll import/summary job
- `streamJobEvents`: read meeting-agent SSE events for any job
- `listMeetings`: list meetings
- `getMeeting`: get meeting by id or prefix
- `getTranscript`: get transcript
- `generateSummary`: create summary job; `template` defaults to `full`
- `getSummary`: get summary; `template` defaults to `full`
- `updateMeeting`: update title/date
- `deleteMeeting`: delete meeting and files
- `cancelJob`: cancel import/summary job
- `exportTranscript`: export transcript as `srt`, `vtt`, `text`, or `json`

`importFromFile` accepts `file_path` (real path or `media://inbound/…`); file must be readable by MCP server process.
