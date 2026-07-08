# Plan: File Upload/Import PRD Canonical Ingest Stage

**Status**: Planning (Worktree-based parallel development)
**Phase**: PRD Stage A′ (Ingest — file upload) + Stage B (Normalize)
**Goal**: File upload/import becomes PRD canonical ingest stage for audio/video
**Strategy**: Use git worktrees with parallel Task agents, then sequential merge + PR

## Deliverables

- [ ] Audio/video upload support for `.mp4`, `.mkv`, `.m4a`, `.mp3`, `.wav`
- [ ] Video demux path extracts audio only and discards frames
- [ ] Normalized 16 kHz mono WAV artifact saved per meeting

## Current State

### ✅ Already Working

1. **File upload API** - `POST /import` accepts multipart uploads (crates/server/src/import_handlers.rs)
2. **Audio format support** - Handles: mp3, wav, m4a, flac, webm, ogg, opus, aac, wma
3. **In-memory processing** - No temp file proliferation (convert_to_mp3_memory)
4. **Video demux capability** - FFmpeg automatically extracts audio from video containers
5. **16 kHz mono normalization** - Currently outputs MP3 format
6. **Background job queue** - With SSE progress streaming
7. **CLI import command** - `meeting-agent import <file>` uses same pipeline

### ❌ Gaps

1. **Video format validation** - `.mp4`, `.mkv` rejected at upload (not in AUDIO_EXTENSIONS list)
2. **Output format** - Currently MP3, PRD requires WAV
3. **Video frames discarded** - Works but not explicit (FFmpeg `-f mp3` drops video implicitly)

## Implementation Plan

### Task 1: Add video format support to validation

**File**: `crates/server/src/import_handlers.rs`

**Change**: Expand `AUDIO_EXTENSIONS` constant to include video formats

```rust
// Before (line 31-33):
const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "wav", "m4a", "flac", "webm", "ogg", "opus", "aac", "wma",
];

// After:
const AUDIO_VIDEO_EXTENSIONS: &[&str] = &[
    // Audio formats
    "mp3", "wav", "m4a", "flac", "webm", "ogg", "opus", "aac", "wma",
    // Video formats (audio will be extracted)
    "mp4", "mkv", "avi", "mov",
];
```

**Update references**:
- `validate_audio_extension()` function (line 314)
- `validate_import()` handler (line 176)
- Error messages to clarify "audio/video" instead of just "audio"

**Verification**: Upload test files for each format (.mp4, .mkv) and confirm acceptance.

---

### Task 2: Change output format from MP3 to WAV

**File**: `crates/core/src/audio.rs`

**Function**: `convert_to_mp3_memory()` (line 152)

**Changes**:

1. **Rename function**:
   - `convert_to_mp3_memory()` → `convert_to_wav_memory()`
   - `convert_file_to_mp3_memory()` → `convert_file_to_wav_memory()`

2. **Update FFmpeg args** (line 159-163):

```rust
// Before:
.args(["-f", "mp3"])
.args(["-ac", "1"])
.args(["-ar", "16000"])
.args(["-codec:a", "libmp3lame"])
.args(["-b:a", "64k"])

// After:
.args(["-f", "wav"])
.args(["-ac", "1"])          // mono
.args(["-ar", "16000"])      // 16 kHz sample rate
.args(["-acodec", "pcm_s16le"])  // PCM signed 16-bit little-endian
```

3. **Update output file extension** in `convert_to_mp3()` function (line 22):
   - Change from `.mp3` to `.wav`

4. **Update function comments/docs** to reflect WAV output

**Files to update for function renames**:
- `crates/core/src/runners.rs` - calls `convert_to_mp3_memory()` / `convert_file_to_mp3_memory()`
- `crates/cli/src/commands/import.rs` - may reference these functions
- `crates/core/src/transcription.rs` - chunked transcription paths

**Verification**: 
- Run `cargo build --all` - ensure no compilation errors
- Check audio artifact is WAV format using `file` command
- Verify WAV is 16 kHz mono using `ffprobe`

---

### Task 3: Explicit video demux (audio-only extraction)

**File**: `crates/core/src/audio.rs`

**Function**: `convert_to_wav_memory()` (after Task 2 rename)

**Change**: Add explicit video stream filtering to make audio-only extraction clear

```rust
// Add after line 158 (inside convert_to_wav_memory):
.args(["-vn"])  // Explicitly disable video - no frames processed/written
```

**Rationale**: 
- Currently, `-f wav` implicitly drops video (WAV container has no video support)
- Adding `-vn` makes the "discard frames" behavior explicit and documented
- Clearer intent for reviewers and PRD compliance verification

**Documentation**: Add comment explaining video frame handling:

```rust
/// Convert audio bytes to WAV format in memory
///
/// Takes raw audio bytes (audio or video file) as input and returns WAV-encoded bytes.
/// For video files, only the audio track is extracted; video frames are discarded (-vn).
/// Uses FFmpeg with pipe:0 (stdin) and pipe:1 (stdout) for in-memory processing.
/// No temporary files are created.
///
/// Output format: 16 kHz mono WAV (PCM signed 16-bit little-endian)
```

**Verification**: 
- Upload a video file (.mp4) and confirm:
  - No video frames in output artifact
  - Output is pure WAV audio
  - File size matches audio-only expectation

---

### Task 4: Update chunking functions for WAV

**File**: `crates/core/src/audio.rs`

**Functions**: `chunk_audio()` (line 103), `chunk_audio_memory()` (line 251)

**Changes**:

1. Update output format in `chunk_audio()` (line 112-113):
```rust
// Before:
.args(["-c:a", "libmp3lame"])
.args(["-b:a", "128k"])

// After:
.args(["-c:a", "pcm_s16le"])
.args(["-f", "segment"])
```

2. Update file extension pattern (line 106, 132):
```rust
// Before: .mp3
// After: .wav
```

3. Update `chunk_audio_memory()` similarly (line 251+)

**Verification**:
- Chunked transcription still works
- Each chunk is valid WAV format
- Timestamps align correctly after chunking

---

### Task 5: Update storage and naming conventions

**Goal**: Update all callers of renamed functions (convert_to_mp3 → convert_to_wav)

**Files to update**:

#### 5a. `crates/core/src/runners.rs`

**Line 92**:
```rust
// Current:
move || audio::convert_to_mp3(&path)

// New:
move || audio::convert_to_wav(&path)
```

**Line 351-352** (log message):
```rust
// Current:
log::info!(
    "[import_memory] converting {} bytes to MP3 in memory",
    cfg.audio_bytes.len()
);

// New:
log::info!(
    "[import_memory] converting {} bytes to WAV in memory",
    cfg.audio_bytes.len()
);
```

**Line 356**:
```rust
// Current:
move || audio::convert_to_mp3_memory(&bytes)

// New:
move || audio::convert_to_wav_memory(&bytes)
```

**Line 433** (temp file for diarization):
```rust
// Current:
"meeting-agent-diarize-{}.mp3",

// New:
"meeting-agent-diarize-{}.wav",
```

#### 5b. `crates/cli/src/commands/import.rs`

**Line 59**:
```rust
// Current:
let converted = meeting_agent_core::audio::convert_to_mp3(&file_path)?;

// New:
let converted = meeting_agent_core::audio::convert_to_wav(&file_path)?;
```

#### 5c. `crates/core/src/transcription.rs`

**Line 590** (chunk filename hint):
```rust
// Current:
let chunk_name = format!("chunk-{:03}.mp3", i);

// New:
let chunk_name = format!("chunk-{:03}.wav", i);
```

**Line 216** (default filename):
```rust
// Current:
.unwrap_or("audio.m4a")

// New:
.unwrap_or("audio.wav")
```

**Line 429** (function comment):
```rust
// Current:
/// Takes MP3-encoded audio bytes and a filename hint for the multipart request.

// New:
/// Takes WAV-encoded audio bytes and a filename hint for the multipart request.
```

**Line 488** (function comment):
```rust
// Current:
/// Takes MP3-encoded audio bytes, splits them into chunks in memory,

// New:
/// Takes WAV-encoded audio bytes, splits them into chunks in memory,
```

#### 5d. `crates/core/src/storage.rs`

Check for any hardcoded `.mp3` extensions in audio file paths. Update `save_audio_from_bytes()` if extension is hardcoded.

**Verification**:
- `cargo build --all` passes with no compilation errors
- Check storage directory after import: `~/.meeting-agent/meetings/{id}/audio/{original-filename}` exists
- Confirm normalized artifact uses `.wav` extension

---

### Task 6: Update tests

**Files to update**:
- `crates/core/tests/audio_test.rs` - audio processing tests
- `crates/core/tests/whisper_integration_test.rs` - integration tests (if exists)

**Changes needed in `audio_test.rs`**:

**Line 2** (import statement):
```rust
// Current:
chunk_audio_memory, convert_to_mp3_memory, probe_duration_from_bytes,

// New:
chunk_audio_memory, convert_to_wav_memory, probe_duration_from_bytes,
```

**Line 20** (test function name):
```rust
// Current:
fn test_convert_to_mp3_memory_basic() {

// New:
fn test_convert_to_wav_memory_basic() {
```

**Line 22** (function call):
```rust
// Current:
let result = convert_to_mp3_memory(&input);

// New:
let result = convert_to_wav_memory(&input);
```

**Line 74** (function call):
```rust
// Current:
let result = convert_to_mp3_memory(&empty);

// New:
let result = convert_to_wav_memory(&empty);
```

**Additional test cases to add**:
1. Test video file upload (.mp4) - verify audio extraction
2. Test WAV format validation (16 kHz mono, pcm_s16le)
3. Test chunk output format is WAV

**Verification**: `cargo test --all` passes

---

## Pre-Commit Verification Checklist

Before committing, run in order:

1. **Format**: `cargo fmt --all -- --check` (fix with `cargo fmt --all`)
2. **Lint**: `cargo clippy --all --all-targets -- -D warnings`
3. **Test**: `cargo test --all`
4. **Integration test**: 
   - Upload `.mp4` file → verify WAV output, 16 kHz mono
   - Upload `.mkv` file → verify WAV output, 16 kHz mono
   - Upload `.mp3` file → verify WAV output, 16 kHz mono
   - Check no video frames in output: `ffprobe normalized.wav` shows audio stream only

---

## PRD Compliance Verification

After implementation, verify against PRD requirements:

| Requirement | Verification | Status |
|------------|--------------|--------|
| FR-3: Accept `.mp4`, `.mkv`, `.m4a`, `.mp3`, `.wav` | Upload test file for each format, confirm 202 Accepted | ⏳ |
| FR-3: Extract/convert to 16 kHz mono WAV | `ffprobe normalized.wav` shows 16000 Hz, 1 channel, pcm_s16le codec | ⏳ |
| FR-3: Video frames not retained | `ffprobe normalized.wav` shows 0 video streams, audio stream only | ⏳ |
| NFR-6: Idempotent re-run | Re-import same file → artifact overwrites cleanly | ⏳ |

---

## Worktree-Based Parallel Development Strategy

**Approach**: Use git worktrees to develop each commit in isolation with parallel Task agents, then merge sequentially to avoid conflicts.

### Overview

1. **Create 4 feature branches** from current `dev` branch
2. **Create 4 worktrees** (one per branch)
3. **Spawn 4 Task agents in parallel** (one per worktree)
4. **Each agent develops independently** in its worktree
5. **Merge sequentially** back to main integration branch (resolve conflicts if any)
6. **Create PR** after all commits merged and verified

### Branch Structure

```
dev (base: d75c9e4)
  ├── feat/video-format-validation         (Task 1 - Commit 1)
  ├── feat/wav-normalization               (Task 2+3 - Commit 2)
  ├── feat/wav-callers-update              (Task 4+5 - Commit 3)
  └── feat/wav-tests                       (Task 6 - Commit 4)
       ↓
  feat/file-upload-ingest                  (merge target, then PR to dev)
```

### Worktree Layout

```
/Users/kagchi/Documents/projects/@bmw-ece-ntust/ai-meeting-agent  (main workspace, dev branch)
/var/folders/.../opencode/worktree-video-validation/              (Task 1)
/var/folders/.../opencode/worktree-wav-normalization/             (Task 2+3)
/var/folders/.../opencode/worktree-wav-callers/                   (Task 4+5)
/var/folders/.../opencode/worktree-wav-tests/                     (Task 6)
```

### Execution Plan

#### Phase 1: Setup Branches and Worktrees

**Commands** (executed in main workspace):

```bash
# Create integration branch from dev
git checkout -b feat/file-upload-ingest

# Create feature branches (conventional feat/ prefix)
git branch feat/video-format-validation dev
git branch feat/wav-normalization dev
git branch feat/wav-callers-update dev
git branch feat/wav-tests dev

# Create worktrees in /var/folders/.../opencode/ (temp directory)
git worktree add /var/folders/.../opencode/worktree-video-validation feat/video-format-validation
git worktree add /var/folders/.../opencode/worktree-wav-normalization feat/wav-normalization
git worktree add /var/folders/.../opencode/worktree-wav-callers feat/wav-callers-update
git worktree add /var/folders/.../opencode/worktree-wav-tests feat/wav-tests
```

**Verification**: `git worktree list` shows 5 worktrees (main + 4 feature)

---

#### Phase 2: Parallel Development with Task Agents

**Spawn 4 Task agents in parallel**, each with its own worktree working directory:

##### Agent 1: Video Format Validation

**Task agent prompt**:
```
You are developing in an isolated git worktree for feat/video-format-validation.

Working directory: /var/folders/.../opencode/worktree-video-validation

Task: Implement video format support to file upload validation (Task 1)

Changes required:
1. Edit crates/server/src/import_handlers.rs
2. Rename AUDIO_EXTENSIONS → AUDIO_VIDEO_EXTENSIONS
3. Add video formats: "mp4", "mkv", "avi", "mov"
4. Update 3 references to the constant (lines 32, 176, 321)
5. Update error messages to say "audio/video"

Verification steps:
- Run: cargo fmt --all
- Run: cargo clippy --all --all-targets -- -D warnings
- Run: cargo build --all (may fail - expected, other tasks not complete)

Commit:
- Message: "feat(server): add video format support to file upload validation"
- Stage only: crates/server/src/import_handlers.rs
- DO NOT push, commit locally only

Report back: commit SHA and verification results
```

##### Agent 2: WAV Normalization

**Task agent prompt**:
```
You are developing in an isolated git worktree for feat/wav-normalization.

Working directory: /var/folders/.../opencode/worktree-wav-normalization

Task: Change audio normalization from MP3 to WAV format (Task 2 + Task 3)

Changes required:
1. Edit crates/core/src/audio.rs
2. Rename functions: convert_to_mp3_memory → convert_to_wav_memory, convert_to_mp3 → convert_to_wav, convert_file_to_mp3_memory → convert_file_to_wav_memory
3. Update FFmpeg args: -f wav -acodec pcm_s16le (remove -b:a)
4. Add -vn flag to explicitly disable video
5. Change temp file extensions .mp3 → .wav (line 22)
6. Update function documentation

Verification steps:
- Run: cargo fmt --all
- Run: cargo clippy --all --all-targets -- -D warnings
- Run: cargo build --all (will fail - callers not updated yet, expected)

Commit:
- Message: "feat(core): change audio normalization from MP3 to WAV format"
- Stage only: crates/core/src/audio.rs
- DO NOT push, commit locally only

Report back: commit SHA and verification results
```

##### Agent 3: Update Callers for WAV

**Task agent prompt**:
```
You are developing in an isolated git worktree for feat/wav-callers-update.

Working directory: /var/folders/.../opencode/worktree-wav-callers

Task: Update audio processing callers and chunking for WAV format (Task 4 + Task 5)

Changes required:

1. crates/core/src/audio.rs (chunking):
   - Line 106: output pattern .mp3 → .wav
   - Lines 112-115: codec libmp3lame → pcm_s16le
   - Line 132: filter .mp3 → .wav
   - chunk_audio_memory: similar changes

2. crates/core/src/runners.rs:
   - Line 92: convert_to_mp3 → convert_to_wav
   - Line 351-352: log "MP3" → "WAV"
   - Line 356: convert_to_mp3_memory → convert_to_wav_memory
   - Line 433: temp file .mp3 → .wav

3. crates/cli/src/commands/import.rs:
   - Line 59: convert_to_mp3 → convert_to_wav

4. crates/core/src/transcription.rs:
   - Line 216: audio.m4a → audio.wav
   - Line 429: comment "MP3-encoded" → "WAV-encoded"
   - Line 488: comment "MP3-encoded" → "WAV-encoded"
   - Line 590: chunk .mp3 → .wav

5. crates/core/src/storage.rs: check for hardcoded .mp3 extensions

Verification steps:
- Run: cargo fmt --all
- Run: cargo clippy --all --all-targets -- -D warnings
- Run: cargo build --all (should pass)
- Run: cargo test --all (may fail - tests not updated yet)

Commit:
- Message: "refactor(core): update audio processing callers for WAV format"
- Stage all modified files
- DO NOT push, commit locally only

Report back: commit SHA and verification results
```

##### Agent 4: Update Tests

**Task agent prompt**:
```
You are developing in an isolated git worktree for feat/wav-tests.

Working directory: /var/folders/.../opencode/worktree-wav-tests

Task: Update tests for WAV format and video upload support (Task 6)

Changes required:

1. crates/core/tests/audio_test.rs:
   - Line 2: import convert_to_mp3_memory → convert_to_wav_memory
   - Line 20: test name test_convert_to_mp3_memory_basic → test_convert_to_wav_memory_basic
   - Line 22: function call updated
   - Line 74: function call updated

2. Optional: Add test cases for video upload, WAV validation, chunk format

Verification steps:
- Run: cargo fmt --all
- Run: cargo clippy --all --all-targets -- -D warnings
- Run: cargo test --all (must pass)

Commit:
- Message: "test: update tests for WAV format and video upload support"
- Stage all modified test files
- DO NOT push, commit locally only

Report back: commit SHA and test results
```

**How to spawn agents in parallel**:

Use a single message with 4 Task tool calls (one per agent), each with its own `workdir` parameter pointing to the respective worktree path.

---

#### Phase 3: Sequential Merge to Integration Branch

After all 4 agents report success, merge commits sequentially in the main workspace:

**Commands** (in main workspace, on integration/file-upload-ingest branch):

```bash
# Verify we're on integration branch
git checkout feat/file-upload-ingest

# Merge Commit 1 (video validation) with fast-forward
git merge --ff feat/video-format-validation
cargo fmt --all
cargo clippy --all --all-targets -- -D warnings
cargo build --all

# Merge Commit 2 (WAV normalization) with fast-forward
git merge --ff feat/wav-normalization
# Resolve conflicts if any (unlikely since files don't overlap)
cargo fmt --all
cargo clippy --all --all-targets -- -D warnings
cargo build --all

# Merge Commit 3 (callers update) with fast-forward
git merge --ff feat/wav-callers-update
# Resolve conflicts if any
cargo fmt --all
cargo clippy --all --all-targets -- -D warnings
cargo build --all

# Merge Commit 4 (tests) with fast-forward
git merge --ff feat/wav-tests
# Resolve conflicts if any
cargo fmt --all
cargo clippy --all --all-targets -- -D warnings
cargo test --all  # MUST PASS
```

**Conflict resolution**:
- Most likely conflicts: audio.rs if multiple tasks touch same lines
- Resolve by keeping all changes (both are needed)
- Re-run verification after each merge

---

#### Phase 4: Final Verification and PR

After all merges complete on integration branch:

**Final integration testing**:

```bash
# On feat/file-upload-ingest branch

# 1. Full build
cargo build --all --release

# 2. All tests
cargo test --all

# 3. Manual integration tests (if possible without DGX):
# - Upload test files (create mock .mp4, .mkv, .mp3)
# - Verify WAV output format
# - Check ffprobe output
```

**Create PR**:

```bash
# Push integration branch
git push origin feat/file-upload-ingest

# Create PR using gh CLI
gh pr create \
  --base dev \
  --head feat/file-upload-ingest \
  --title "feat: add video format support and normalize to WAV per PRD" \
  --body "$(cat <<EOF
## Summary
Implements PRD Stage A′ (Ingest) canonical pipeline for file upload:
- ✅ Audio/video upload support for .mp4, .mkv, .m4a, .mp3, .wav
- ✅ Video demux path extracts audio only and discards frames
- ✅ Normalized 16 kHz mono WAV artifact saved per meeting

## Changes
- **Commit 1**: Add video format validation (.mp4, .mkv, .avi, .mov)
- **Commit 2**: Change audio normalization from MP3 to WAV (16 kHz mono PCM)
- **Commit 3**: Update all callers and chunking functions for WAV format
- **Commit 4**: Update tests for WAV format

## Verification
- ✅ cargo fmt
- ✅ cargo clippy (0 warnings)
- ✅ cargo test --all (all pass)
- ✅ PRD FR-3 compliance verified

## Breaking Changes
- Output format changed from MP3 to WAV (10x larger files, expected per PRD)
- Existing meetings with MP3 artifacts unaffected (backward compatible)

## References
- PRD.md §6 Stage A′
- PRD.md FR-3
- Plan: .opencode/plans/file-upload-ingest.md
EOF
)"
```

---

#### Phase 5: Cleanup Worktrees

After PR is merged:

```bash
# Remove worktrees
git worktree remove /var/folders/.../opencode/worktree-video-validation
git worktree remove /var/folders/.../opencode/worktree-wav-normalization
git worktree remove /var/folders/.../opencode/worktree-wav-callers
git worktree remove /var/folders/.../opencode/worktree-wav-tests

# Delete feature branches
git branch -d feat/video-format-validation
git branch -d feat/wav-normalization
git branch -d feat/wav-callers-update
git branch -d feat/wav-tests

# Delete integration branch (after PR merged to dev)
git branch -d feat/file-upload-ingest
```

---

## Commit Strategy

**User requirement**: Break into multiple logical commits, not one large commit.

### Commit 1: `feat(server): add video format support to file upload validation`

**Files changed**: `crates/server/src/import_handlers.rs`

**Tasks included**: Task 1 only

**Verification**:
- `cargo fmt --all`
- `cargo clippy --all --all-targets -- -D warnings`
- `cargo build --all` (may fail until Task 2-3 complete)

---

### Commit 2: `feat(core): change audio normalization from MP3 to WAV format`

**Files changed**: `crates/core/src/audio.rs`

**Tasks included**: Task 2 + Task 3 (rename functions, update FFmpeg args, add `-vn` flag)

**Verification**:
- `cargo fmt --all`
- `cargo clippy --all --all-targets -- -D warnings`
- `cargo build --all` (will fail - callers still reference old function names)

---

### Commit 3: `refactor(core): update audio processing callers for WAV format`

**Files changed**: 
- `crates/core/src/runners.rs`
- `crates/core/src/transcription.rs`
- `crates/core/src/storage.rs` (if needed)
- `crates/cli/src/commands/import.rs`

**Tasks included**: Task 4 (chunking) + Task 5 (update callers)

**Verification**:
- `cargo fmt --all`
- `cargo clippy --all --all-targets -- -D warnings`
- `cargo build --all` (should pass)
- `cargo test --all` (may have test failures until Task 6)

---

### Commit 4: `test: update tests for WAV format and video upload support`

**Files changed**: 
- `crates/core/tests/audio_test.rs`
- `crates/core/tests/whisper_integration_test.rs`

**Tasks included**: Task 6

**Verification**:
- `cargo fmt --all`
- `cargo clippy --all --all-targets -- -D warnings`
- `cargo test --all` (must pass)
- Manual integration tests with .mp4, .mkv, .wav files

---

### Pre-Commit Verification (per commit)

Run before each commit:

1. **Format**: `cargo fmt --all`
2. **Lint**: `cargo clippy --all --all-targets -- -D warnings`
3. **Build**: `cargo build --all` (expected to pass after Commit 3)
4. **Test**: `cargo test --all` (expected to pass after Commit 4)

### Final Integration Testing (after Commit 4)

1. Upload `.mp4` file → verify WAV output, 16 kHz mono
2. Upload `.mkv` file → verify WAV output, 16 kHz mono
3. Upload `.mp3` file → verify WAV output, 16 kHz mono
4. Check no video frames: `ffprobe normalized.wav` shows audio stream only
5. Run full pipeline: upload → transcribe → verify transcript exists

---

## Push Strategy

**Do NOT push** until user explicitly approves with "push", "ship it", or equivalent.

After all 4 commits are complete and verified:
1. Show commit summary: `git log --oneline -4`
2. Show diff from base: `git diff main...HEAD` (or current branch base)
3. Wait for explicit push permission

---

## File Change Summary

| File | Change Type | Lines Changed (est.) |
|------|-------------|---------------------|
| `crates/server/src/import_handlers.rs` | Modify | ~15 |
| `crates/core/src/audio.rs` | Modify | ~50 |
| `crates/core/src/runners.rs` | Modify | ~10 |
| `crates/core/src/storage.rs` | Modify | ~5 |
| `crates/cli/src/commands/import.rs` | Modify | ~5 |
| `crates/core/tests/audio_test.rs` | Modify | ~20 |

**Total estimated**: ~105 lines changed across 6 files

---

## Notes

- **Backward compatibility**: Existing stored meetings with MP3 artifacts will remain MP3. Only new imports will use WAV. No migration needed.
- **Whisper API compatibility**: Whisper accepts both MP3 and WAV, so transcription will work with WAV chunks.
- **File size increase**: WAV is uncompressed, so artifacts will be ~10x larger than MP3. For 1-hour meeting: MP3 ~30 MB → WAV ~300 MB. PRD accepts this tradeoff for canonical format.
- **FFmpeg dependency**: Already required, no new dependencies.

---

## References

- PRD.md §6 Stage A′: "Audio/video file → ffmpeg demux/re-encode → Normalized 16 kHz mono WAV"
- PRD.md FR-3: File upload acceptance criteria
- Cargo.toml: `ffmpeg-sidecar = "2.5"`
- Current implementation: crates/core/src/audio.rs:152 (`convert_to_mp3_memory`)
