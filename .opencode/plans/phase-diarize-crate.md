# Phase: diarize crate (standalone speaker diarization microservice)

## Status
**Complete.** Pre-commit checks pass (fmt, clippy, test). Awaiting commit permission.

## Goal
New standalone `meeting-agent-diarize` crate: HTTP microservice wrapping
sherpa-onnx `OfflineSpeakerDiarization`. Takes audio (mp3/wav) + Whisper
transcript JSON, returns cleaned transcript with speaker labels merged in via
max-timestamp-overlap. No ASR. No disk I/O. Decoupled from `core`/`server`/`cli`.

## Deliverables (all done)
- `crates/diarize/Cargo.toml` — workspace-inherited pkg metadata, sherpa-onnx + symphonia deps, `diarize-server` bin, `diarize_file` example.
- `crates/diarize/src/error.rs` — `DiarizeError` enum, `IntoResponse` mapping (400/422/500), `From<serde_json::Error>`.
- `crates/diarize/src/config.rs` — `DiarizeConfig::from_env()` validates both model paths exist at boot (fail-fast).
- `crates/diarize/src/models.rs` — `SpeakerSegment`, `WhisperSegment` (serde, ignores unknown fields), `CleanedSegment`, `DiarizeResponse`, `WhisperTranscript` wrapper.
- `crates/diarize/src/audio.rs` — `decode_audio_to_f32_mono_16k()`: symphonia probe + decode, `SampleBuffer<f32>::copy_interleaved_ref`, downmix to mono by mean, linear resample to 16kHz.
- `crates/diarize/src/validate.rs` — `validate_whisper_segments()`: drops empty-text, NaN, inverted (start>=end) segments.
- `crates/diarize/src/merge.rs` — `merge()`: max-overlap speaker assignment, no-overlap → speaker=-1 sentinel. 3 unit tests pass.
- `crates/diarize/src/lib.rs` — `SpeakerDiarizer { inner: OfflineSpeakerDiarization }` Send+Sync wrapper; `new()`, `process()` → `(num_speakers, Vec<SpeakerSegment>)`, `sample_rate()`.
- `crates/diarize/src/bin/diarize-server.rs` — axum `POST /v1/diarize` (multipart: file+transcript+optional num_speakers), `GET /health`. Format sniff (RIFF/WAVE magic, ID3/MPEG sync). num_speakers override rebuilds diarizer per-request.
- `crates/diarize/examples/diarize_file.rs` — CLI: `<audio> <transcript.json>` → cleaned JSON stdout.

## Workspace changes
- Root `Cargo.toml`: added `crates/diarize` member; `[workspace.dependencies]` += `sherpa-onnx = "1.13"`, `symphonia = { version = "0.5", features = ["mp3", "wav"] }`.

## API contract
```
POST /v1/diarize  multipart: file (mp3/wav), transcript (Whisper JSON), [num_speakers i32]
→ 200 { num_speakers, segments: [{start,end,speaker,text}] }
→ 400 missing field / bad num_speakers
→ 422 bad audio format / unparseable transcript
→ 500 model/diarization failure
GET /health → 200 {"status":"ok"}
```

## Env vars
- `DIARIZE_SEGMENTATION_MODEL` (required) — pyannote-segmentation-3.0.onnx path
- `DIARIZE_EMBEDDING_MODEL` (required) — 3dspeaker_eres2net.onnx path
- `DIARIZE_NUM_SPEAKERS` (optional, default 0=auto)
- `DIARIZE_CLUSTERING_THRESHOLD` (optional, default 0.5)
- `DIARIZE_HOST` (default 0.0.0.0)
- `DIARIZE_PORT` (default 8002)

## Verification
- `cargo fmt --all -- --check` — clean
- `cargo clippy --all --all-targets -- -D warnings` — clean
- `cargo test --all` — 28 passed (3 in diarize merge.rs: max-overlap, no-overlap sentinel, order/text preservation)

## Notes / out of scope
- ONNX model files NOT bundled (~140MB). Manual download + env vars. Fail-fast at boot.
- onnxruntime bundled statically by sherpa-onnx build script (~50MB first build).
- Caller (existing server) writes cleaned `transcript.json` to disk — documented, not in crate.
- README with download links + caller integration snippet: not yet written (deferred).
- Server smoke test: deferred (needs model files).
