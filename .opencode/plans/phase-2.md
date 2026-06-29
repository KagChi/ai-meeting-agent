# Phase 2: Core Transcription Client Implementation

**Goal**: Implement transcription client that can send audio files to OpenAI-compatible APIs and receive transcripts.

## Tasks

- [ ] Create `crates/core/src/transcription.rs` module
- [ ] Implement `TranscriptionClient` struct with HTTP client (reqwest)
- [ ] Add `TranscriptionRequest` and `TranscriptionResponse` models
- [ ] Implement `transcribe()` method with multipart file upload
- [ ] Add error types and error handling
- [ ] Support multiple response formats (json, verbose_json, srt, vtt, text)
- [ ] Add retry logic for transient failures
- [ ] Update `crates/core/src/lib.rs` to export transcription module
- [ ] Add required dependencies to `crates/core/Cargo.toml` (reqwest with multipart)

## Dependencies to Add

```toml
reqwest = { version = "0.11", features = ["json", "multipart"] }
tokio = { version = "1", features = ["full"] }
```

## API Endpoint

```
POST {base_url}/audio/transcriptions
Content-Type: multipart/form-data

Fields:
- file: audio file (required)
- model: model name (required)
- response_format: json|verbose_json|text|srt|vtt (optional, default: json)
- language: ISO-639-1 code (optional)
- prompt: context/spelling guide (optional)
- temperature: 0.0-1.0 (optional)
```

## Success Criteria

- Can transcribe an audio file using OpenAI-compatible API
- Handles API errors gracefully
- Returns structured transcript data
- Config system properly loads API credentials
