# Plan: Week 2 Metadata Resolution

**Status**: Ready for Execution
**Phase**: PRD Stage C (Metadata resolution)
**Goal**: Complete Week 2 by implementing metadata extraction and resolution for uploaded files
**PRD Mapping**: FR-4, FR-10, FR-15, G-2
**Strategy**: Stacked branches with parallel worktree execution
**Total PRs**: 4 (each targets previous branch)
**Total Commits**: 26 fine-grained commits

## Overview

This plan implements the metadata resolution layer that sits between file upload and the canonical pipeline. It extracts file information, parses filenames for dates, and implements precedence logic for metadata sources.

**Week 2 Remaining Tasks:**
- [ ] ffprobe file info stored per meeting
- [ ] Filename date/time parser for patterns like `2026-07-01_lab-meeting.mp4`
- [ ] Metadata source precedence implemented: user edit > calendar/bot > filename > ffprobe
- [ ] `meeting.json` extended with `starts_at`, `metadata_source`, `platform`
- [ ] HTTP upload and CLI import use same canonical runner
- [ ] Tests for video upload, filename metadata, ffprobe fallback

---

## Current State Analysis

### ✅ Already Working

1. **Audio/video upload** - Accepts all required formats (mp4, mkv, m4a, mp3, wav)
2. **Video demux** - Extracts audio, discards video frames (`-vn` flag)
3. **WAV normalization** - 16 kHz mono WAV artifacts saved
4. **In-memory processing** - No temp file proliferation (except for diarizer)
5. **Background jobs** - Job queue with SSE progress streaming
6. **Basic meeting model** - `Meeting` struct with title, date, status, transcription
7. **ffprobe utilities** - `probe_duration()` and `probe_duration_from_bytes()` exist

### ❌ Gaps

1. **No file metadata extraction** - ffprobe can return container metadata (creation time, format, bitrate, etc.) but we only use it for duration
2. **No filename parsing** - Filenames like `2026-07-01_lab-meeting.mp4` are not parsed for date/time
3. **Limited Meeting model** - Missing `starts_at`, `metadata_source`, `platform`, `bot_id`, `calendar_event_id`, `reviewed_by`
4. **No metadata precedence** - No logic to choose between user edit, calendar, filename, or ffprobe sources
5. **Title always from filename stem** - No topic inference or ambiguity detection
6. **Two separate runners** - `run_import()` (file path) and `run_import_memory()` (bytes) share logic but aren't unified

---

## Implementation Plan

### Task 1: Extend Meeting Model

**File**: `crates/core/src/models.rs`

**Goal**: Add PRD-required fields to `Meeting` struct per §9 class diagram

**Changes**:

```rust
// Add new types for metadata source tracking
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum MetadataSource {
    /// User manually edited metadata
    User,
    /// From calendar event (for live meetings)
    Calendar,
    /// From bot metadata (for live meetings)
    Bot,
    /// Parsed from filename pattern
    Filename,
    /// Extracted from file container metadata (ffprobe)
    Probe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Teams,
    Zoom,
    GoogleMeet,
    Upload,
}

// Extend Meeting struct
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub date: DateTime<Utc>,  // Keep for backward compat (list sorting)
    
    // NEW: PRD-required fields
    pub starts_at: Option<DateTime<Utc>>,
    pub metadata_source: MetadataSource,
    pub platform: Platform,
    pub bot_id: Option<String>,
    pub calendar_event_id: Option<String>,
    pub reviewed_by: Option<String>,
    
    pub duration_seconds: Option<u64>,
    pub status: MeetingStatus,
    pub transcription: Option<TranscriptionInfo>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

**Update `Meeting::new()`**:
```rust
impl Meeting {
    pub fn new(title: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            title,
            date: now,
            starts_at: None,
            metadata_source: MetadataSource::User,  // Default for manual creation
            platform: Platform::Upload,  // Default for file uploads
            bot_id: None,
            calendar_event_id: None,
            reviewed_by: None,
            duration_seconds: None,
            status: MeetingStatus::Importing,
            transcription: None,
            created_at: now,
            updated_at: now,
        }
    }
}
```

**Verification**:
- `cargo build --all` should pass (existing code uses `Meeting::new()` unchanged)
- New fields are `Option` or have defaults, so no breaking changes

---

### Task 2: Create File Metadata Extractor

**File**: `crates/core/src/metadata.rs` (NEW)

**Goal**: Extract comprehensive file metadata using ffprobe, parse filenames for dates

**Create new module**:

```rust
//! File metadata extraction and resolution
//!
//! Implements PRD Stage C: Metadata resolution with precedence logic.

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use ffmpeg_sidecar::ffprobe;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// File metadata extracted from ffprobe
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    /// Duration in seconds
    pub duration: Option<f64>,
    /// Container format (mp4, mkv, wav, etc.)
    pub format: Option<String>,
    /// Bitrate in bits/s
    pub bitrate: Option<u64>,
    /// File creation time from container metadata
    pub creation_time: Option<DateTime<Utc>>,
    /// File size in bytes
    pub size: Option<u64>,
}

/// Parsed filename metadata
#[derive(Debug, Clone)]
pub struct FilenameMetadata {
    /// Parsed date/time from filename
    pub datetime: Option<DateTime<Utc>>,
    /// Extracted title (filename without date prefix and extension)
    pub title: Option<String>,
}

/// Extract comprehensive metadata from file using ffprobe
pub fn probe_file_metadata(path: &Path) -> Result<FileMetadata> {
    let output = std::process::Command::new(ffprobe::ffprobe_path())
        .arg("-v")
        .arg("error")
        .arg("-show_format")
        .arg("-of")
        .arg("json")
        .arg(path)
        .output()
        .context("Failed to spawn ffprobe")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffprobe failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .context("Failed to parse ffprobe JSON output")?;

    let format = json.get("format");
    
    Ok(FileMetadata {
        duration: format
            .and_then(|f| f.get("duration"))
            .and_then(|d| d.as_str())
            .and_then(|s| s.parse::<f64>().ok()),
        format: format
            .and_then(|f| f.get("format_name"))
            .and_then(|n| n.as_str())
            .map(String::from),
        bitrate: format
            .and_then(|f| f.get("bit_rate"))
            .and_then(|b| b.as_str())
            .and_then(|s| s.parse::<u64>().ok()),
        creation_time: format
            .and_then(|f| f.get("tags"))
            .and_then(|t| t.get("creation_time"))
            .and_then(|c| c.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc)),
        size: format
            .and_then(|f| f.get("size"))
            .and_then(|s| s.as_str())
            .and_then(|s| s.parse::<u64>().ok()),
    })
}

/// Extract metadata from file bytes using ffprobe with pipe:0
pub fn probe_file_metadata_from_bytes(audio_bytes: &[u8]) -> Result<FileMetadata> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new(ffprobe::ffprobe_path())
        .arg("-v")
        .arg("error")
        .arg("-show_format")
        .arg("-of")
        .arg("json")
        .arg("pipe:0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn ffprobe")?;

    // Write audio bytes to stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(audio_bytes)
            .context("Failed to write to ffprobe stdin")?;
        drop(stdin);
    }

    let output = child
        .wait_with_output()
        .context("Failed to wait for ffprobe")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffprobe failed: {}", stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .context("Failed to parse ffprobe JSON output")?;

    let format = json.get("format");
    
    Ok(FileMetadata {
        duration: format
            .and_then(|f| f.get("duration"))
            .and_then(|d| d.as_str())
            .and_then(|s| s.parse::<f64>().ok()),
        format: format
            .and_then(|f| f.get("format_name"))
            .and_then(|n| n.as_str())
            .map(String::from),
        bitrate: format
            .and_then(|f| f.get("bit_rate"))
            .and_then(|b| b.as_str())
            .and_then(|s| s.parse::<u64>().ok()),
        creation_time: None,  // Not available from pipe input
        size: format
            .and_then(|f| f.get("size"))
            .and_then(|s| s.as_str())
            .and_then(|s| s.parse::<u64>().ok()),
    })
}

/// Parse filename for date/time patterns
///
/// Supported patterns:
/// - `YYYY-MM-DD_title.ext` → date only, time defaults to 00:00
/// - `YYYY-MM-DD-HH.MM_title.ext` → date and time
/// - `YYYYMMDD_title.ext` → compact date
/// - `YYYYMMDD-HHMM_title.ext` → compact date and time
///
/// Returns both the parsed datetime and the extracted title (filename without date prefix).
pub fn parse_filename(filename: &str) -> FilenameMetadata {
    // Remove extension
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);

    // Pattern 1: YYYY-MM-DD_title or YYYY-MM-DD-HH.MM_title
    if let Some(caps) = regex::Regex::new(r"^(\d{4})-(\d{2})-(\d{2})(?:-(\d{2})\.(\d{2}))?_(.+)$")
        .ok()
        .and_then(|re| re.captures(stem))
    {
        let year = caps.get(1).and_then(|m| m.as_str().parse::<i32>().ok());
        let month = caps.get(2).and_then(|m| m.as_str().parse::<u32>().ok());
        let day = caps.get(3).and_then(|m| m.as_str().parse::<u32>().ok());
        let hour = caps.get(4).and_then(|m| m.as_str().parse::<u32>().ok()).unwrap_or(0);
        let minute = caps.get(5).and_then(|m| m.as_str().parse::<u32>().ok()).unwrap_or(0);
        let title = caps.get(6).map(|m| m.as_str().to_string());

        if let (Some(y), Some(m), Some(d)) = (year, month, day) {
            if let Some(naive_date) = NaiveDate::from_ymd_opt(y, m, d) {
                if let Some(naive_time) = NaiveTime::from_hms_opt(hour, minute, 0) {
                    let naive_dt = NaiveDateTime::new(naive_date, naive_time);
                    return FilenameMetadata {
                        datetime: Some(DateTime::from_naive_utc_and_offset(naive_dt, Utc)),
                        title,
                    };
                }
            }
        }
    }

    // Pattern 2: YYYYMMDD_title or YYYYMMDD-HHMM_title
    if let Some(caps) = regex::Regex::new(r"^(\d{4})(\d{2})(\d{2})(?:-(\d{2})(\d{2}))?_(.+)$")
        .ok()
        .and_then(|re| re.captures(stem))
    {
        let year = caps.get(1).and_then(|m| m.as_str().parse::<i32>().ok());
        let month = caps.get(2).and_then(|m| m.as_str().parse::<u32>().ok());
        let day = caps.get(3).and_then(|m| m.as_str().parse::<u32>().ok());
        let hour = caps.get(4).and_then(|m| m.as_str().parse::<u32>().ok()).unwrap_or(0);
        let minute = caps.get(5).and_then(|m| m.as_str().parse::<u32>().ok()).unwrap_or(0);
        let title = caps.get(6).map(|m| m.as_str().to_string());

        if let (Some(y), Some(m), Some(d)) = (year, month, day) {
            if let Some(naive_date) = NaiveDate::from_ymd_opt(y, m, d) {
                if let Some(naive_time) = NaiveTime::from_hms_opt(hour, minute, 0) {
                    let naive_dt = NaiveDateTime::new(naive_date, naive_time);
                    return FilenameMetadata {
                        datetime: Some(DateTime::from_naive_utc_and_offset(naive_dt, Utc)),
                        title,
                    };
                }
            }
        }
    }

    // No pattern matched - return None for datetime, full stem as title
    FilenameMetadata {
        datetime: None,
        title: Some(stem.to_string()),
    }
}
```

**Add to `crates/core/src/lib.rs`**:
```rust
pub mod metadata;
```

**Add dependency** to `crates/core/Cargo.toml`:
```toml
regex = "1"
```

**Verification**:
- `cargo build --all` passes
- Test with `probe_file_metadata()` on a sample file
- Test `parse_filename()` with various patterns

---

### Task 3: Implement Metadata Resolution Logic

**File**: `crates/core/src/metadata.rs` (extend)

**Goal**: Implement precedence logic for metadata sources per PRD Stage C

**Add to the module**:

```rust
use crate::models::{MetadataSource, Platform};

/// Resolved metadata for a meeting
#[derive(Debug, Clone)]
pub struct ResolvedMetadata {
    /// Meeting title
    pub title: String,
    /// Meeting start time
    pub starts_at: Option<DateTime<Utc>>,
    /// Source of the metadata
    pub source: MetadataSource,
    /// Platform (Upload for file uploads)
    pub platform: Platform,
}

/// Resolve metadata with precedence: user edit > calendar/bot > filename > ffprobe
///
/// For file uploads (Week 2), only filename and ffprobe sources are relevant.
/// Calendar and bot sources will be used in Week 6 for live meetings.
pub fn resolve_metadata_for_upload(
    filename: &str,
    file_metadata: &FileMetadata,
    user_title: Option<String>,
    user_starts_at: Option<DateTime<Utc>>,
) -> ResolvedMetadata {
    // Parse filename for date/time patterns
    let filename_meta = parse_filename(filename);

    // Precedence: user edit > filename > ffprobe
    let (title, title_source) = if let Some(user_title) = user_title {
        // User explicitly provided title
        (user_title, MetadataSource::User)
    } else if let Some(filename_title) = filename_meta.title {
        // Title extracted from filename pattern
        (filename_title, MetadataSource::Filename)
    } else {
        // Fallback to full filename stem
        (
            std::path::Path::new(filename)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled Meeting")
                .to_string(),
            MetadataSource::Filename,
        )
    };

    let (starts_at, time_source) = if let Some(user_time) = user_starts_at {
        // User explicitly provided start time
        (Some(user_time), MetadataSource::User)
    } else if let Some(filename_time) = filename_meta.datetime {
        // Time parsed from filename pattern
        (Some(filename_time), MetadataSource::Filename)
    } else if let Some(probe_time) = file_metadata.creation_time {
        // Fallback to file container creation time
        (Some(probe_time), MetadataSource::Probe)
    } else {
        // No time available
        (None, MetadataSource::Probe)
    };

    // Final source is the highest precedence source used
    let source = if user_title.is_some() || user_starts_at.is_some() {
        MetadataSource::User
    } else if filename_meta.datetime.is_some() || filename_meta.title.is_some() {
        MetadataSource::Filename
    } else {
        MetadataSource::Probe
    };

    ResolvedMetadata {
        title,
        starts_at,
        source,
        platform: Platform::Upload,
    }
}
```

**Verification**:
- Test precedence: user > filename > ffprobe
- Test with various combinations of available metadata
- `cargo build --all` passes

---

### Task 4: Store File Metadata in Meeting

**File**: `crates/core/src/models.rs` (extend)

**Goal**: Add file metadata storage to Meeting struct

**Add new struct**:

```rust
/// File metadata stored with meeting (from ffprobe)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StoredFileMetadata {
    /// Duration in seconds
    pub duration: Option<f64>,
    /// Container format (mp4, mkv, wav, etc.)
    pub format: Option<String>,
    /// Bitrate in bits/s
    pub bitrate: Option<u64>,
    /// File creation time from container metadata
    pub creation_time: Option<DateTime<Utc>>,
    /// File size in bytes
    pub size: Option<u64>,
}
```

**Extend Meeting struct**:

```rust
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub date: DateTime<Utc>,
    pub starts_at: Option<DateTime<Utc>>,
    pub metadata_source: MetadataSource,
    pub platform: Platform,
    pub bot_id: Option<String>,
    pub calendar_event_id: Option<String>,
    pub reviewed_by: Option<String>,
    pub duration_seconds: Option<u64>,
    
    // NEW: File metadata
    pub file_metadata: Option<StoredFileMetadata>,
    
    pub status: MeetingStatus,
    pub transcription: Option<TranscriptionInfo>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

**Update Meeting::new()**:

```rust
impl Meeting {
    pub fn new(title: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            title,
            date: now,
            starts_at: None,
            metadata_source: MetadataSource::User,
            platform: Platform::Upload,
            bot_id: None,
            calendar_event_id: None,
            reviewed_by: None,
            duration_seconds: None,
            file_metadata: None,
            status: MeetingStatus::Importing,
            transcription: None,
            created_at: now,
            updated_at: now,
        }
    }
    
    /// Create meeting with resolved metadata (for file uploads)
    pub fn from_resolved_metadata(resolved: crate::metadata::ResolvedMetadata, file_meta: crate::metadata::FileMetadata) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            title: resolved.title,
            date: resolved.starts_at.unwrap_or(now),
            starts_at: resolved.starts_at,
            metadata_source: resolved.source,
            platform: resolved.platform,
            bot_id: None,
            calendar_event_id: None,
            reviewed_by: None,
            duration_seconds: file_meta.duration.map(|d| d.round() as u64),
            file_metadata: Some(StoredFileMetadata {
                duration: file_meta.duration,
                format: file_meta.format,
                bitrate: file_meta.bitrate,
                creation_time: file_meta.creation_time,
                size: file_meta.size,
            }),
            status: MeetingStatus::Importing,
            transcription: None,
            created_at: now,
            updated_at: now,
        }
    }
}
```

**Verification**:
- `cargo build --all` passes
- `Meeting::new()` unchanged for backward compatibility
- New `from_resolved_metadata()` constructor available

---

### Task 5: Integrate Metadata Resolution into Import Runner

**File**: `crates/core/src/runners.rs`

**Goal**: Use metadata resolution in `run_import_memory_inner()` when creating meetings

**Changes**:

```rust
// Around line 341, in run_import_memory_inner()

async fn run_import_memory_inner(cfg: &ImportMemoryConfig) -> Result<()> {
    cfg.registry.update_progress(
        &cfg.job_id,
        ProgressEvent::new("converting", "Preparing audio file"),
    );

    // Check if conversion is needed based on filename
    let working_audio = if audio::needs_conversion_by_filename(&cfg.audio_filename) {
        check_cancelled(&cfg.cancel_token)?;
        log::info!(
            "[import_memory] converting {} bytes to WAV in memory",
            cfg.audio_bytes.len()
        );
        let converted = tokio::task::spawn_blocking({
            let bytes = cfg.audio_bytes.clone();
            move || audio::convert_to_wav_memory(&bytes)
        })
        .await??;
        log::info!(
            "[import_memory] conversion complete: {} bytes",
            converted.len()
        );
        converted
    } else {
        log::info!(
            "[import_memory] no conversion needed, using original {} bytes",
            cfg.audio_bytes.len()
        );
        cfg.audio_bytes.clone()
    };

    check_cancelled(&cfg.cancel_token)?;

    cfg.registry.update_progress(
        &cfg.job_id,
        ProgressEvent::new("metadata", "Extracting file metadata"),
    );

    // NEW: Extract file metadata
    let file_metadata = tokio::task::spawn_blocking({
        let bytes = cfg.audio_bytes.clone();
        move || crate::metadata::probe_file_metadata_from_bytes(&bytes)
    })
    .await??;

    // NEW: Resolve metadata with precedence
    let resolved = crate::metadata::resolve_metadata_for_upload(
        &cfg.audio_filename,
        &file_metadata,
        cfg.title.clone(),
        None,  // user_starts_at - will be exposed in API later
    );

    cfg.registry.update_progress(
        &cfg.job_id,
        ProgressEvent::new("processing", "Creating meeting record"),
    );

    // OLD CODE (remove):
    // let meeting_title = cfg.title.clone().unwrap_or_else(|| {
    //     std::path::Path::new(&cfg.audio_filename)
    //         .file_stem()
    //         .and_then(|s| s.to_str())
    //         .unwrap_or("Untitled Meeting")
    //         .to_string()
    // });
    // let meeting = Meeting::new(meeting_title);

    // NEW CODE:
    let meeting = Meeting::from_resolved_metadata(resolved, file_metadata);
    
    cfg.storage.create_meeting(&meeting)?;
    cfg.registry.set_meeting_id(&cfg.job_id, meeting.id.clone());

    // Rest of function unchanged...
    check_cancelled(&cfg.cancel_token)?;
    // ... (transcription, diarization, etc.)
}
```

**Verification**:
- `cargo build --all` passes
- Upload a file named `2026-07-01_lab-meeting.mp4`
- Check `meeting.json`: `starts_at` should be `2026-07-01T00:00:00Z`, `metadata_source` should be `"filename"`
- Upload a file named `recording.wav`
- Check `meeting.json`: `starts_at` should be from ffprobe creation time, `metadata_source` should be `"probe"`

---

### Task 6: Update CLI Import Command

**File**: `crates/cli/src/commands/import.rs`

**Goal**: Use metadata resolution in CLI import path

**Changes**:

```rust
// Around line 59, in execute()

pub async fn execute(&self, config: Config, storage: Arc<MeetingStorage>) -> Result<()> {
    let file_path = self.file.canonicalize()?;

    if !file_path.exists() {
        anyhow::bail!("File not found: {:?}", file_path);
    }

    println!("Importing meeting from file: {:?}", file_path);

    // Extract file metadata
    let file_metadata = meeting_agent_core::metadata::probe_file_metadata(&file_path)?;
    
    // Get filename
    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Resolve metadata
    let resolved = meeting_agent_core::metadata::resolve_metadata_for_upload(
        &filename,
        &file_metadata,
        self.title.clone(),
        None,  // user_starts_at - could be added as CLI flag later
    );

    // Create meeting with resolved metadata
    let meeting = Meeting::from_resolved_metadata(resolved, file_metadata);
    storage.create_meeting(&meeting)?;

    println!("Created meeting: {}", meeting.id);
    println!("  Title: {}", meeting.title);
    if let Some(starts_at) = meeting.starts_at {
        println!("  Starts at: {}", starts_at);
    }
    println!("  Metadata source: {:?}", meeting.metadata_source);

    // Rest of function unchanged (convert audio, transcribe, etc.)
    // ...
}
```

**Verification**:
- CLI import uses same metadata resolution as HTTP API
- `cargo build --all` passes
- Test: `meeting-agent import 2026-07-01_lab-meeting.mp4`

---

### Task 7: Add Tests

**File**: `crates/core/tests/metadata_test.rs` (NEW)

**Goal**: Test metadata extraction, filename parsing, and resolution precedence

**Create test file**:

```rust
use meeting_agent_core::metadata::{
    parse_filename, probe_file_metadata_from_bytes, resolve_metadata_for_upload, FileMetadata,
};
use chrono::{DateTime, Utc, TimeZone};

#[test]
fn test_parse_filename_date_only() {
    let meta = parse_filename("2026-07-01_lab-meeting.mp4");
    
    assert!(meta.datetime.is_some());
    let dt = meta.datetime.unwrap();
    assert_eq!(dt.year(), 2026);
    assert_eq!(dt.month(), 7);
    assert_eq!(dt.day(), 1);
    assert_eq!(dt.hour(), 0);
    assert_eq!(dt.minute(), 0);
    
    assert_eq!(meta.title, Some("lab-meeting".to_string()));
}

#[test]
fn test_parse_filename_date_and_time() {
    let meta = parse_filename("2026-07-01-14.30_weekly-sync.mp4");
    
    assert!(meta.datetime.is_some());
    let dt = meta.datetime.unwrap();
    assert_eq!(dt.year(), 2026);
    assert_eq!(dt.month(), 7);
    assert_eq!(dt.day(), 1);
    assert_eq!(dt.hour(), 14);
    assert_eq!(dt.minute(), 30);
    
    assert_eq!(meta.title, Some("weekly-sync".to_string()));
}

#[test]
fn test_parse_filename_compact_date() {
    let meta = parse_filename("20260701_meeting.wav");
    
    assert!(meta.datetime.is_some());
    let dt = meta.datetime.unwrap();
    assert_eq!(dt.year(), 2026);
    assert_eq!(dt.month(), 7);
    assert_eq!(dt.day(), 1);
    
    assert_eq!(meta.title, Some("meeting".to_string()));
}

#[test]
fn test_parse_filename_compact_date_time() {
    let meta = parse_filename("20260701-1430_standup.mp3");
    
    assert!(meta.datetime.is_some());
    let dt = meta.datetime.unwrap();
    assert_eq!(dt.year(), 2026);
    assert_eq!(dt.month(), 7);
    assert_eq!(dt.day(), 1);
    assert_eq!(dt.hour(), 14);
    assert_eq!(dt.minute(), 30);
    
    assert_eq!(meta.title, Some("standup".to_string()));
}

#[test]
fn test_parse_filename_no_pattern() {
    let meta = parse_filename("recording-001.wav");
    
    assert!(meta.datetime.is_none());
    assert_eq!(meta.title, Some("recording-001".to_string()));
}

#[test]
fn test_resolve_metadata_user_precedence() {
    let file_meta = FileMetadata {
        duration: Some(120.5),
        format: Some("wav".to_string()),
        bitrate: None,
        creation_time: Some(Utc.with_ymd_and_hms(2026, 6, 30, 10, 0, 0).unwrap()),
        size: Some(1024000),
    };
    
    let resolved = resolve_metadata_for_upload(
        "2026-07-01_old-title.wav",
        &file_meta,
        Some("User Override Title".to_string()),
        Some(Utc.with_ymd_and_hms(2026, 7, 2, 15, 0, 0).unwrap()),
    );
    
    assert_eq!(resolved.title, "User Override Title");
    assert_eq!(resolved.starts_at.unwrap().day(), 2);
    assert_eq!(resolved.source, meeting_agent_core::models::MetadataSource::User);
}

#[test]
fn test_resolve_metadata_filename_precedence() {
    let file_meta = FileMetadata {
        duration: Some(120.5),
        format: Some("wav".to_string()),
        bitrate: None,
        creation_time: Some(Utc.with_ymd_and_hms(2026, 6, 30, 10, 0, 0).unwrap()),
        size: Some(1024000),
    };
    
    let resolved = resolve_metadata_for_upload(
        "2026-07-01_filename-title.wav",
        &file_meta,
        None,
        None,
    );
    
    assert_eq!(resolved.title, "filename-title");
    assert_eq!(resolved.starts_at.unwrap().day(), 1);
    assert_eq!(resolved.source, meeting_agent_core::models::MetadataSource::Filename);
}

#[test]
fn test_resolve_metadata_probe_fallback() {
    let file_meta = FileMetadata {
        duration: Some(120.5),
        format: Some("wav".to_string()),
        bitrate: None,
        creation_time: Some(Utc.with_ymd_and_hms(2026, 6, 30, 10, 0, 0).unwrap()),
        size: Some(1024000),
    };
    
    let resolved = resolve_metadata_for_upload(
        "recording.wav",  // No pattern
        &file_meta,
        None,
        None,
    );
    
    assert_eq!(resolved.title, "recording");
    assert_eq!(resolved.starts_at.unwrap().day(), 30);  // From probe
    assert_eq!(resolved.source, meeting_agent_core::models::MetadataSource::Probe);
}
```

**Verification**:
- `cargo test --all` passes
- All filename patterns parse correctly
- Precedence logic verified

---

### Task 8: Update API Response Types

**File**: `crates/server/src/types.rs`

**Goal**: Expose new metadata fields in API responses

**Changes**:

```rust
// Extend MeetingResponse to include new fields
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MeetingResponse {
    pub id: String,
    pub title: String,
    pub date: DateTime<Utc>,
    
    // NEW: PRD fields
    pub starts_at: Option<DateTime<Utc>>,
    pub metadata_source: String,  // Serialized enum
    pub platform: String,         // Serialized enum
    
    pub duration_seconds: Option<u64>,
    pub status: String,
    pub transcription: Option<TranscriptionInfoResponse>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    
    // NEW: File metadata
    pub file_metadata: Option<FileMetadataResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FileMetadataResponse {
    pub duration: Option<f64>,
    pub format: Option<String>,
    pub bitrate: Option<u64>,
    pub creation_time: Option<DateTime<Utc>>,
    pub size: Option<u64>,
}

impl From<Meeting> for MeetingResponse {
    fn from(meeting: Meeting) -> Self {
        Self {
            id: meeting.id,
            title: meeting.title,
            date: meeting.date,
            starts_at: meeting.starts_at,
            metadata_source: format!("{:?}", meeting.metadata_source).to_lowercase(),
            platform: format!("{:?}", meeting.platform).to_lowercase(),
            duration_seconds: meeting.duration_seconds,
            status: format!("{:?}", meeting.status).to_lowercase(),
            transcription: meeting.transcription.map(Into::into),
            created_at: meeting.created_at,
            updated_at: meeting.updated_at,
            file_metadata: meeting.file_metadata.map(|fm| FileMetadataResponse {
                duration: fm.duration,
                format: fm.format,
                bitrate: fm.bitrate,
                creation_time: fm.creation_time,
                size: fm.size,
            }),
        }
    }
}
```

**Verification**:
- `cargo build --all` passes
- API responses include new fields
- OpenAPI docs at `/docs` show updated schema

---

## Execution Strategy: Stacked Branches with Parallel Worktrees

### Overview

**Strategy**: 4 waves of development in parallel, using stacked branches to handle dependencies.

**Branch Structure** (stacked):
- `feat/model-extension` ← from `dev`
- `feat/metadata-extraction` ← from `feat/model-extension`
- `feat/pipeline-integration` ← from `feat/metadata-extraction`
- `feat/tests-api` ← from `feat/pipeline-integration`

**PR Structure** (each targets parent):
- PR #1: `feat/model-extension` → `dev`
- PR #2: `feat/metadata-extraction` → `feat/model-extension`
- PR #3: `feat/pipeline-integration` → `feat/metadata-extraction`
- PR #4: `feat/tests-api` → `feat/pipeline-integration`

**Merge Flow**:
1. Review & merge PR #1 → GitHub auto-changes PR #2 base to `dev`
2. Review & merge PR #2 → GitHub auto-changes PR #3 base to `dev`
3. Review & merge PR #3 → GitHub auto-changes PR #4 base to `dev`
4. Review & merge PR #4 → Week 2 complete!

**Why stacked branches?**
- All 4 worktrees can work in parallel without build failures
- Each branch has its dependencies available
- GitHub automatically rebases after each merge
- Clean, linear history

**Execution Timeline**:
- **T=0**: Create all 4 worktrees + branches simultaneously
- **T=0**: Spawn 5 agents in parallel (Wave 1: 1, Wave 2: 1, Wave 3: 2, Wave 4: 2)
- **T+30min**: All agents complete commits + verification
- **T+30min**: Push all 4 branches + create all 4 PRs
- **T+31min**: Cleanup all 4 worktrees
- **T+31min**: Report results (commit SHAs, PR URLs)

---

## Wave 1: Model Extension

**Branch**: `feat/model-extension`  
**Base**: `dev`  
**Worktree**: `/var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-model-extension/`  
**Tasks**: 1 (extend Meeting) + 4 (add file metadata storage)  
**Agent count**: 1 (single file edited)
**Commits**: 5 fine-grained

### Setup Commands

```bash
cd /Users/kagchi/Documents/projects/@bmw-ece-ntust/ai-meeting-agent

# Create feature branch from dev
git checkout dev
git pull origin dev
git checkout -b feat/model-extension

# Create worktree
git worktree add /var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-model-extension feat/model-extension
```

### Commit Breakdown (Fine-Grained)

**Commit 1**: `feat(core): add MetadataSource enum`
- Add `MetadataSource` enum with User, Calendar, Bot, Filename, Probe variants
- Add serde derives and openapi schema

**Commit 2**: `feat(core): add Platform enum`
- Add `Platform` enum with Teams, Zoom, GoogleMeet, Upload variants
- Add serde derives and openapi schema

**Commit 3**: `feat(core): add StoredFileMetadata struct`
- Add struct with duration, format, bitrate, creation_time, size fields
- Add derives

**Commit 4**: `feat(core): extend Meeting struct with metadata fields`
- Add `starts_at`, `metadata_source`, `platform`, `bot_id`, `calendar_event_id`, `reviewed_by`, `file_metadata`
- Update `Meeting::new()` to initialize new fields with defaults

**Commit 5**: `feat(core): add Meeting::from_resolved_metadata constructor`
- Add new constructor that accepts `ResolvedMetadata` and `FileMetadata`
- Set all metadata fields from resolved data

### Verification

```bash
cd /var/folders/.../opencode/worktree-model-extension/
cargo fmt --all
cargo clippy --all --all-targets -- -D warnings
cargo build --all
```

### PR Creation

```bash
# Push branch
git push origin feat/model-extension

# Create PR targeting dev
gh pr create \
  --base dev \
  --head feat/model-extension \
  --title "feat: extend Meeting model with PRD metadata fields" \
  --body "$(cat <<EOF
## Summary
Extends the Meeting model with PRD-required metadata fields for Week 2 metadata resolution.

## Changes
- Add \`MetadataSource\` enum (user, calendar, bot, filename, probe)
- Add \`Platform\` enum (teams, zoom, googlemeet, upload)
- Add \`StoredFileMetadata\` struct for ffprobe data
- Extend \`Meeting\` with: \`starts_at\`, \`metadata_source\`, \`platform\`, \`bot_id\`, \`calendar_event_id\`, \`reviewed_by\`, \`file_metadata\`
- Add \`Meeting::from_resolved_metadata()\` constructor

## Commits (5 fine-grained)
1. Add MetadataSource enum
2. Add Platform enum
3. Add StoredFileMetadata struct
4. Extend Meeting struct with new fields
5. Add from_resolved_metadata constructor

## Verification
- ✅ cargo fmt
- ✅ cargo clippy (0 warnings)
- ✅ cargo build --all

## PRD Compliance
- PRD §9 Class Diagram: Meeting model extended
- PRD FR-4: Metadata source tracking
- PRD FR-10: starts_at field for resolved timestamps

## Breaking Changes
None - all new fields are Option or have defaults

## Merge Order
**Merge first** - Wave 2 depends on this branch
EOF
)"
```

### Cleanup

```bash
cd /Users/kagchi/Documents/projects/@bmw-ece-ntust/ai-meeting-agent
git worktree remove /var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-model-extension
```

---

## Wave 2: Metadata Extraction

**Branch**: `feat/metadata-extraction`  
**Base**: `feat/model-extension` (stacked on Wave 1)  
**Worktree**: `/var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-metadata-extraction/`  
**Tasks**: 2 (file metadata extractor) + 3 (resolution logic)  
**Agent count**: 1 (new module creation)
**Commits**: 6 fine-grained

### Setup Commands

```bash
cd /Users/kagchi/Documents/projects/@bmw-ece-ntust/ai-meeting-agent

# Create feature branch from feat/model-extension (stacked)
git checkout feat/model-extension
git checkout -b feat/metadata-extraction

# Create worktree
git worktree add /var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-metadata-extraction feat/metadata-extraction
```

### Commit Breakdown (Fine-Grained)

**Commit 1**: `feat(core): create metadata module with structs`
- Create `crates/core/src/metadata.rs`
- Add `FileMetadata`, `FilenameMetadata`, `ResolvedMetadata` structs
- Add to `lib.rs`

**Commit 2**: `feat(core): add ffprobe file metadata extraction`
- Implement `probe_file_metadata()` for file paths
- Implement `probe_file_metadata_from_bytes()` for in-memory

**Commit 3**: `feat(core): add filename date parsing (pattern 1)`
- Implement `parse_filename()` skeleton
- Add pattern: `YYYY-MM-DD_title` and `YYYY-MM-DD-HH.MM_title`

**Commit 4**: `feat(core): add filename date parsing (pattern 2)`
- Add pattern: `YYYYMMDD_title` and `YYYYMMDD-HHMM_title`

**Commit 5**: `feat(core): implement metadata resolution with precedence`
- Implement `resolve_metadata_for_upload()`
- Apply precedence: user > filename > ffprobe

**Commit 6**: `build(core): add regex dependency`
- Add `regex = "1"` to `Cargo.toml`

### Verification

```bash
cd /var/folders/.../opencode/worktree-metadata-extraction/
cargo fmt --all
cargo clippy --all --all-targets -- -D warnings
cargo build --all
```

### PR Creation

```bash
# Push branch
git push origin feat/metadata-extraction

# Create PR targeting feat/model-extension (stacked)
gh pr create \
  --base feat/model-extension \
  --head feat/metadata-extraction \
  --title "feat: add file metadata extraction and resolution" \
  --body "$(cat <<EOF
## Summary
Implements metadata extraction from files using ffprobe and filename parsing with precedence resolution.

## Changes
- New \`metadata.rs\` module
- \`probe_file_metadata()\` - extract duration, format, bitrate, creation_time, size
- \`probe_file_metadata_from_bytes()\` - in-memory variant
- \`parse_filename()\` - parse date/time from filenames (4 patterns)
- \`resolve_metadata_for_upload()\` - precedence logic (user > filename > ffprobe)
- Add \`regex\` dependency

## Commits (6 fine-grained)
1. Create metadata module with structs
2. Add ffprobe extraction
3. Add filename parsing pattern 1 (YYYY-MM-DD)
4. Add filename parsing pattern 2 (YYYYMMDD)
5. Implement resolution precedence
6. Add regex dependency

## Verification
- ✅ cargo fmt
- ✅ cargo clippy (0 warnings)
- ✅ cargo build --all

## PRD Compliance
- PRD FR-4: Extract file info, parse filename date/time
- PRD FR-10: Metadata resolution with source tracking
- PRD §6 Stage C: Metadata resolution

## Dependencies
Stacked on: \`feat/model-extension\` (Wave 1)

## Merge Order
**Merge second** - After PR #1 merges, GitHub will auto-rebase this to \`dev\`
EOF
)"
```

### Cleanup

```bash
cd /Users/kagchi/Documents/projects/@bmw-ece-ntust/ai-meeting-agent
git worktree remove /var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-metadata-extraction
```

---

## Wave 3: Pipeline Integration

**Branch**: `feat/pipeline-integration`  
**Base**: `feat/metadata-extraction` (stacked on Wave 2)  
**Worktrees**: 2 separate worktrees (agents work in parallel on same branch)
  - `/var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-runners-integration/`
  - `/var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-cli-integration/`  
**Tasks**: 5 (API runners) + 6 (CLI import)  
**Agent count**: 2 (parallel - different files)
**Commits**: 6 fine-grained

### Setup Commands

```bash
cd /Users/kagchi/Documents/projects/@bmw-ece-ntust/ai-meeting-agent

# Create feature branch from feat/metadata-extraction (stacked)
git checkout feat/metadata-extraction
git checkout -b feat/pipeline-integration

# Create two worktrees for parallel work on same branch
git worktree add /var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-runners-integration feat/pipeline-integration
git worktree add /var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-cli-integration feat/pipeline-integration
```

### Commit Breakdown (Fine-Grained)

**Agent 1 (runners.rs) - Commit 1**: `feat(core): add metadata extraction to import pipeline`
- Add metadata extraction step in `run_import_memory_inner()`
- Call `probe_file_metadata_from_bytes()`

**Agent 1 - Commit 2**: `feat(core): add metadata resolution to import pipeline`
- Call `resolve_metadata_for_upload()` after extraction
- Replace `Meeting::new()` with `Meeting::from_resolved_metadata()`

**Agent 1 - Commit 3**: `feat(core): add metadata progress event`
- Add progress event "metadata" / "Extracting file metadata"

**Agent 2 (import.rs) - Commit 4**: `feat(cli): add metadata extraction to CLI import`
- Call `probe_file_metadata()` in CLI import
- Call `resolve_metadata_for_upload()`

**Agent 2 - Commit 5**: `feat(cli): use from_resolved_metadata in CLI`
- Replace `Meeting::new()` with `Meeting::from_resolved_metadata()`

**Agent 2 - Commit 6**: `feat(cli): print resolved metadata to console`
- Add console output for title, starts_at, metadata_source

### Verification

```bash
# Agent 1
cd /var/folders/.../opencode/worktree-runners-integration/
cargo fmt --all
cargo clippy --all --all-targets -- -D warnings
cargo build --all

# Agent 2
cd /var/folders/.../opencode/worktree-cli-integration/
cargo fmt --all
cargo clippy --all --all-targets -- -D warnings
cargo build --all
```

### PR Creation

```bash
# Push branch
git push origin feat/pipeline-integration

# Create PR targeting feat/metadata-extraction (stacked)
gh pr create \
  --base feat/metadata-extraction \
  --head feat/pipeline-integration \
  --title "feat: integrate metadata resolution into import pipeline" \
  --body "$(cat <<EOF
## Summary
Integrates metadata extraction and resolution into both HTTP API and CLI import paths.

## Changes
- **API path** (\`runners.rs\`): Extract and resolve metadata in \`run_import_memory_inner()\`
- **CLI path** (\`import.rs\`): Extract and resolve metadata in CLI import command
- Both paths now use \`Meeting::from_resolved_metadata()\`
- Add progress event for metadata extraction

## Commits (6 fine-grained, parallel work)
1. Add metadata extraction to import pipeline
2. Add metadata resolution to import pipeline
3. Add metadata progress event
4. Add metadata extraction to CLI import
5. Use from_resolved_metadata in CLI
6. Print resolved metadata to console

## Verification
- ✅ cargo fmt
- ✅ cargo clippy (0 warnings)
- ✅ cargo build --all

## PRD Compliance
- PRD FR-15: CLI and API share same pipeline
- PRD G-2: One canonical pipeline
- PRD §6 Stage C: Metadata resolution integrated

## Dependencies
Stacked on: \`feat/metadata-extraction\` (Wave 2)

## Merge Order
**Merge third** - After PR #2 merges, GitHub will auto-rebase this to \`dev\`
EOF
)"
```

### Cleanup

```bash
cd /Users/kagchi/Documents/projects/@bmw-ece-ntust/ai-meeting-agent
git worktree remove /var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-runners-integration
git worktree remove /var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-cli-integration
```

---

## Wave 4: Tests & API

**Branch**: `feat/tests-api`  
**Base**: `feat/pipeline-integration` (stacked on Wave 3)  
**Worktrees**: 2 separate worktrees (agents work in parallel on same branch)
  - `/var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-tests/`
  - `/var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-api-types/`  
**Tasks**: 7 (tests) + 8 (API response types)  
**Agent count**: 2 (parallel - different files)
**Commits**: 7 fine-grained

### Setup Commands

```bash
cd /Users/kagchi/Documents/projects/@bmw-ece-ntust/ai-meeting-agent

# Create feature branch from feat/pipeline-integration (stacked)
git checkout feat/pipeline-integration
git checkout -b feat/tests-api

# Create two worktrees for parallel work on same branch
git worktree add /var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-tests feat/tests-api
git worktree add /var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-api-types feat/tests-api
```

### Commit Breakdown (Fine-Grained)

**Agent 1 (metadata_test.rs) - Commit 1**: `test(core): add filename parsing tests (pattern 1)`
- Create `crates/core/tests/metadata_test.rs`
- Test `YYYY-MM-DD_title` and `YYYY-MM-DD-HH.MM_title`

**Agent 1 - Commit 2**: `test(core): add filename parsing tests (pattern 2)`
- Test `YYYYMMDD_title` and `YYYYMMDD-HHMM_title`

**Agent 1 - Commit 3**: `test(core): add filename parsing test (no pattern)`
- Test fallback when no pattern matches

**Agent 1 - Commit 4**: `test(core): add metadata resolution precedence tests`
- Test user > filename precedence
- Test filename > probe precedence
- Test probe fallback

**Agent 2 (types.rs) - Commit 5**: `feat(server): add FileMetadataResponse struct`
- Add struct matching `StoredFileMetadata`

**Agent 2 - Commit 6**: `feat(server): extend MeetingResponse with metadata fields`
- Add `starts_at`, `metadata_source`, `platform`, `file_metadata`

**Agent 2 - Commit 7**: `feat(server): update From<Meeting> implementation`
- Map new Meeting fields to MeetingResponse

### Verification

```bash
# Agent 1
cd /var/folders/.../opencode/worktree-tests/
cargo fmt --all
cargo clippy --all --all-targets -- -D warnings
cargo test --all  # MUST PASS

# Agent 2
cd /var/folders/.../opencode/worktree-api-types/
cargo fmt --all
cargo clippy --all --all-targets -- -D warnings
cargo build --all
```

### PR Creation

```bash
# Push branch
git push origin feat/tests-api

# Create PR targeting feat/pipeline-integration (stacked)
gh pr create \
  --base feat/pipeline-integration \
  --head feat/tests-api \
  --title "feat: add metadata tests and API response fields" \
  --body "$(cat <<EOF
## Summary
Adds comprehensive tests for metadata extraction/resolution and exposes new fields in API responses.

## Changes
- **Tests**: Filename parsing (4 patterns + no-pattern fallback), precedence logic
- **API types**: Extend \`MeetingResponse\` with \`starts_at\`, \`metadata_source\`, \`platform\`, \`file_metadata\`
- Add \`FileMetadataResponse\` struct

## Commits (7 fine-grained, parallel work)
1. Add filename parsing tests (pattern 1)
2. Add filename parsing tests (pattern 2)
3. Add filename parsing test (no pattern)
4. Add precedence tests
5. Add FileMetadataResponse struct
6. Extend MeetingResponse with metadata fields
7. Update From<Meeting> implementation

## Verification
- ✅ cargo fmt
- ✅ cargo clippy (0 warnings)
- ✅ cargo test --all (ALL PASS)
- ✅ cargo build --all

## PRD Compliance
- PRD Week 2: Tests for filename metadata, ffprobe fallback
- API responses expose all new metadata fields

## Dependencies
Stacked on: \`feat/pipeline-integration\` (Wave 3)

## Merge Order
**Merge fourth (final)** - After PR #3 merges, GitHub will auto-rebase this to \`dev\`

## Completion
✅ Week 2 deliverables complete after this PR merges.
EOF
)"
```

### Cleanup

```bash
cd /Users/kagchi/Documents/projects/@bmw-ece-ntust/ai-meeting-agent
git worktree remove /var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-tests
git worktree remove /var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-api-types
```

---

## Agent Spawn Prompts

Each wave spawns agents with specific prompts. Copy-paste these when executing.

### Wave 1: Single Agent (Model Extension)

**Working directory**: `/var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-model-extension`

**Prompt**:
```
You are working in an isolated git worktree for Week 2 Wave 1: Model Extension.

Working directory: /var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-model-extension
Branch: feat/model-extension

Your task: Extend the Meeting model with PRD-required metadata fields.

Changes required (5 fine-grained commits):

Commit 1: feat(core): add MetadataSource enum
- File: crates/core/src/models.rs
- Add enum with variants: User, Calendar, Bot, Filename, Probe
- Add derives: Debug, Clone, PartialEq, Eq, Serialize, Deserialize
- Add #[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
- Add #[serde(rename_all = "lowercase")]
- Stage only this change, commit

Commit 2: feat(core): add Platform enum
- File: crates/core/src/models.rs
- Add enum with variants: Teams, Zoom, GoogleMeet, Upload
- Add same derives as MetadataSource
- Stage only this change, commit

Commit 3: feat(core): add StoredFileMetadata struct
- File: crates/core/src/models.rs
- Add struct with fields: duration (Option<f64>), format (Option<String>), bitrate (Option<u64>), creation_time (Option<DateTime<Utc>>), size (Option<u64>)
- Add derives: Debug, Clone, Serialize, Deserialize
- Add #[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
- Stage only this change, commit

Commit 4: feat(core): extend Meeting struct with metadata fields
- File: crates/core/src/models.rs
- Add fields to Meeting struct:
  - pub starts_at: Option<DateTime<Utc>>
  - pub metadata_source: MetadataSource
  - pub platform: Platform
  - pub bot_id: Option<String>
  - pub calendar_event_id: Option<String>
  - pub reviewed_by: Option<String>
  - pub file_metadata: Option<StoredFileMetadata>
- Update Meeting::new() to initialize these fields with defaults:
  - starts_at: None
  - metadata_source: MetadataSource::User
  - platform: Platform::Upload
  - bot_id: None
  - calendar_event_id: None
  - reviewed_by: None
  - file_metadata: None
- Stage only this change, commit

Commit 5: feat(core): add Meeting::from_resolved_metadata constructor
- File: crates/core/src/models.rs
- Add new method to impl Meeting block
- Signature: pub fn from_resolved_metadata(resolved: crate::metadata::ResolvedMetadata, file_meta: crate::metadata::FileMetadata) -> Self
- Create Meeting with:
  - id: Uuid::new_v4().to_string()
  - title: resolved.title
  - date: resolved.starts_at.unwrap_or(now)
  - starts_at: resolved.starts_at
  - metadata_source: resolved.source
  - platform: resolved.platform
  - bot_id: None
  - calendar_event_id: None
  - reviewed_by: None
  - duration_seconds: file_meta.duration.map(|d| d.round() as u64)
  - file_metadata: Some(StoredFileMetadata { ... })
  - status: MeetingStatus::Importing
  - transcription: None
  - created_at: now
  - updated_at: now
- Stage only this change, commit

Verification after all commits:
- cargo fmt --all
- cargo clippy --all --all-targets -- -D warnings
- cargo build --all

Report back: All 5 commit SHAs and verification results.

DO NOT push or create PR - orchestrator will handle that.
```

---

### Wave 2: Single Agent (Metadata Extraction)

**Working directory**: `/var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-metadata-extraction`

**Prompt**:
```
You are working in an isolated git worktree for Week 2 Wave 2: Metadata Extraction.

Working directory: /var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-metadata-extraction
Branch: feat/metadata-extraction

Your task: Create metadata extraction module with ffprobe integration and filename parsing.

Changes required (6 fine-grained commits):

Commit 1: feat(core): create metadata module with structs
- Create file: crates/core/src/metadata.rs
- Add module header comment: "//! File metadata extraction and resolution\n//!\n//! Implements PRD Stage C: Metadata resolution with precedence logic."
- Add imports: anyhow::{Context, Result}, chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc}, ffmpeg_sidecar::ffprobe, serde::{Deserialize, Serialize}, std::path::Path
- Add 3 structs:
  - FileMetadata: duration (Option<f64>), format (Option<String>), bitrate (Option<u64>), creation_time (Option<DateTime<Utc>>), size (Option<u64>)
  - FilenameMetadata: datetime (Option<DateTime<Utc>>), title (Option<String>)
  - ResolvedMetadata: title (String), starts_at (Option<DateTime<Utc>>), source (MetadataSource), platform (Platform)
- Add to crates/core/src/lib.rs: pub mod metadata;
- Stage both files, commit

Commit 2: feat(core): add ffprobe file metadata extraction
- File: crates/core/src/metadata.rs
- Add function: pub fn probe_file_metadata(path: &Path) -> Result<FileMetadata>
  - Use ffprobe with -show_format -of json
  - Parse JSON, extract duration, format_name, bit_rate, tags.creation_time, size
- Add function: pub fn probe_file_metadata_from_bytes(audio_bytes: &[u8]) -> Result<FileMetadata>
  - Same logic but with pipe:0 stdin
  - creation_time will be None (not available from pipe)
- Stage, commit

Commit 3: feat(core): add filename date parsing (pattern 1)
- File: crates/core/src/metadata.rs
- Add function skeleton: pub fn parse_filename(filename: &str) -> FilenameMetadata
- Implement pattern: YYYY-MM-DD_title and YYYY-MM-DD-HH.MM_title
  - Regex: r"^(\d{4})-(\d{2})-(\d{2})(?:-(\d{2})\.(\d{2}))?_(.+)$"
  - Parse year, month, day, hour (default 0), minute (default 0), title
  - Return FilenameMetadata with datetime and title
- Stage, commit

Commit 4: feat(core): add filename date parsing (pattern 2)
- File: crates/core/src/metadata.rs
- Add second pattern in parse_filename(): YYYYMMDD_title and YYYYMMDD-HHMM_title
  - Regex: r"^(\d{4})(\d{2})(\d{2})(?:-(\d{2})(\d{2}))?_(.+)$"
  - Same parsing logic
- Add fallback: if no pattern matches, return FilenameMetadata { datetime: None, title: Some(stem) }
- Stage, commit

Commit 5: feat(core): implement metadata resolution with precedence
- File: crates/core/src/metadata.rs
- Add use: use crate::models::{MetadataSource, Platform};
- Add function: pub fn resolve_metadata_for_upload(filename: &str, file_metadata: &FileMetadata, user_title: Option<String>, user_starts_at: Option<DateTime<Utc>>) -> ResolvedMetadata
- Implement precedence logic:
  - Title: user_title > filename_meta.title > filename stem
  - Time: user_starts_at > filename_meta.datetime > file_metadata.creation_time > None
  - Source: user (if any user field set) > filename (if parsed) > probe
  - Platform: always Upload
- Return ResolvedMetadata
- Stage, commit

Commit 6: build(core): add regex dependency
- File: crates/core/Cargo.toml
- Add to [dependencies]: regex = "1"
- Stage, commit

Verification after all commits:
- cargo fmt --all
- cargo clippy --all --all-targets -- -D warnings
- cargo build --all

Report back: All 6 commit SHAs and verification results.

DO NOT push or create PR - orchestrator will handle that.
```

---

### Wave 3: Two Agents in Parallel (Pipeline Integration)

**Agent 1 - Working directory**: `/var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-runners-integration`

**Agent 1 Prompt**:
```
You are working in an isolated git worktree for Week 2 Wave 3: Pipeline Integration (Agent 1 - Runners).

Working directory: /var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-runners-integration
Branch: feat/pipeline-integration

Your task: Integrate metadata extraction and resolution into the API import pipeline.

Changes required (3 fine-grained commits):

Commit 1: feat(core): add metadata extraction to import pipeline
- File: crates/core/src/runners.rs
- Function: run_import_memory_inner() around line 341
- After the "working_audio" conversion step (line ~370), add:
  cfg.registry.update_progress(&cfg.job_id, ProgressEvent::new("metadata", "Extracting file metadata"));
  let file_metadata = tokio::task::spawn_blocking({ let bytes = cfg.audio_bytes.clone(); move || crate::metadata::probe_file_metadata_from_bytes(&bytes) }).await??;
- Stage, commit

Commit 2: feat(core): add metadata resolution to import pipeline
- File: crates/core/src/runners.rs
- After file_metadata extraction, add:
  let resolved = crate::metadata::resolve_metadata_for_upload(&cfg.audio_filename, &file_metadata, cfg.title.clone(), None);
- Replace the old meeting creation (lines ~379-385 that use Meeting::new()) with:
  let meeting = Meeting::from_resolved_metadata(resolved, file_metadata);
- Remove the old meeting_title variable creation
- Stage, commit

Commit 3: refactor(core): update progress event description
- File: crates/core/src/runners.rs
- Change progress event after metadata resolution from "processing"/"Creating meeting record" to "processing"/"Creating meeting record with resolved metadata"
- Stage, commit

Verification after all commits:
- cargo fmt --all
- cargo clippy --all --all-targets -- -D warnings
- cargo build --all

Report back: All 3 commit SHAs and verification results.

DO NOT push - wait for Agent 2 to finish, then orchestrator will create PR.
```

**Agent 2 - Working directory**: `/var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-cli-integration`

**Agent 2 Prompt**:
```
You are working in an isolated git worktree for Week 2 Wave 3: Pipeline Integration (Agent 2 - CLI).

Working directory: /var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-cli-integration
Branch: feat/pipeline-integration

Your task: Integrate metadata extraction and resolution into the CLI import command.

Changes required (3 fine-grained commits):

Commit 4: feat(cli): add metadata extraction to CLI import
- File: crates/cli/src/commands/import.rs
- Function: execute() around line 59
- After the file_path.exists() check, add:
  println!("Extracting file metadata...");
  let file_metadata = meeting_agent_core::metadata::probe_file_metadata(&file_path)?;
- Add:
  let filename = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string();
- Stage, commit

Commit 5: feat(cli): use from_resolved_metadata in CLI
- File: crates/cli/src/commands/import.rs
- After file_metadata extraction, add:
  let resolved = meeting_agent_core::metadata::resolve_metadata_for_upload(&filename, &file_metadata, self.title.clone(), None);
- Replace the old meeting creation (Meeting::new()) with:
  let meeting = Meeting::from_resolved_metadata(resolved, file_metadata);
- Stage, commit

Commit 6: feat(cli): print resolved metadata to console
- File: crates/cli/src/commands/import.rs
- After meeting creation, update println statements to show:
  println!("Created meeting: {}", meeting.id);
  println!("  Title: {}", meeting.title);
  if let Some(starts_at) = meeting.starts_at { println!("  Starts at: {}", starts_at); }
  println!("  Metadata source: {:?}", meeting.metadata_source);
  println!("  Platform: {:?}", meeting.platform);
- Stage, commit

Verification after all commits:
- cargo fmt --all
- cargo clippy --all --all-targets -- -D warnings
- cargo build --all

Report back: All 3 commit SHAs and verification results.

DO NOT push - wait for Agent 1 to finish, then orchestrator will create PR.
```

---

### Wave 4: Two Agents in Parallel (Tests & API)

**Agent 1 - Working directory**: `/var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-tests`

**Agent 1 Prompt**:
```
You are working in an isolated git worktree for Week 2 Wave 4: Tests & API (Agent 1 - Tests).

Working directory: /var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-tests
Branch: feat/tests-api

Your task: Add comprehensive tests for metadata extraction and resolution.

Changes required (4 fine-grained commits):

Commit 1: test(core): add filename parsing tests (pattern 1)
- Create file: crates/core/tests/metadata_test.rs
- Add imports: use meeting_agent_core::metadata::{parse_filename, probe_file_metadata_from_bytes, resolve_metadata_for_upload, FileMetadata}; use chrono::{DateTime, Utc, TimeZone};
- Add 2 test functions:
  - test_parse_filename_date_only(): Test "2026-07-01_lab-meeting.mp4"
  - test_parse_filename_date_and_time(): Test "2026-07-01-14.30_weekly-sync.mp4"
- Stage, commit

Commit 2: test(core): add filename parsing tests (pattern 2)
- File: crates/core/tests/metadata_test.rs
- Add 2 test functions:
  - test_parse_filename_compact_date(): Test "20260701_meeting.wav"
  - test_parse_filename_compact_date_time(): Test "20260701-1430_standup.mp3"
- Stage, commit

Commit 3: test(core): add filename parsing test (no pattern)
- File: crates/core/tests/metadata_test.rs
- Add test function: test_parse_filename_no_pattern(): Test "recording-001.wav"
- Assert datetime is None, title is full stem
- Stage, commit

Commit 4: test(core): add metadata resolution precedence tests
- File: crates/core/tests/metadata_test.rs
- Add 3 test functions:
  - test_resolve_metadata_user_precedence(): User overrides filename and probe
  - test_resolve_metadata_filename_precedence(): Filename overrides probe
  - test_resolve_metadata_probe_fallback(): Probe used when no filename pattern
- Stage, commit

Verification after all commits:
- cargo fmt --all
- cargo clippy --all --all-targets -- -D warnings
- cargo test --all (MUST PASS)

Report back: All 4 commit SHAs and test results.

DO NOT push - wait for Agent 2 to finish, then orchestrator will create PR.
```

**Agent 2 - Working directory**: `/var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-api-types`

**Agent 2 Prompt**:
```
You are working in an isolated git worktree for Week 2 Wave 4: Tests & API (Agent 2 - API Types).

Working directory: /var/folders/02/s71mb9mx0n136n9hsx3fz7th0000gn/T/opencode/worktree-api-types
Branch: feat/tests-api

Your task: Extend API response types to expose new metadata fields.

Changes required (3 fine-grained commits):

Commit 5: feat(server): add FileMetadataResponse struct
- File: crates/server/src/types.rs
- Add new struct after MeetingResponse:
  #[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
  pub struct FileMetadataResponse {
      pub duration: Option<f64>,
      pub format: Option<String>,
      pub bitrate: Option<u64>,
      pub creation_time: Option<DateTime<Utc>>,
      pub size: Option<u64>,
  }
- Stage, commit

Commit 6: feat(server): extend MeetingResponse with metadata fields
- File: crates/server/src/types.rs
- Add fields to MeetingResponse struct (after date field):
  - pub starts_at: Option<DateTime<Utc>>
  - pub metadata_source: String
  - pub platform: String
  - pub file_metadata: Option<FileMetadataResponse>
- Stage, commit

Commit 7: feat(server): update From<Meeting> implementation
- File: crates/server/src/types.rs
- Update From<Meeting> for MeetingResponse impl to map new fields:
  - starts_at: meeting.starts_at
  - metadata_source: format!("{:?}", meeting.metadata_source).to_lowercase()
  - platform: format!("{:?}", meeting.platform).to_lowercase()
  - file_metadata: meeting.file_metadata.map(|fm| FileMetadataResponse { duration: fm.duration, format: fm.format, bitrate: fm.bitrate, creation_time: fm.creation_time, size: fm.size })
- Stage, commit

Verification after all commits:
- cargo fmt --all
- cargo clippy --all --all-targets -- -D warnings
- cargo build --all

Report back: All 3 commit SHAs and verification results.

DO NOT push - wait for Agent 1 to finish, then orchestrator will create PR.
```

---

## Pre-Commit Verification Checklist

Before each commit:

1. **Format**: `cargo fmt --all`
2. **Lint**: `cargo clippy --all --all-targets -- -D warnings`
3. **Build**: `cargo build --all`
4. **Test** (after commit 6): `cargo test --all`

---

## Integration Testing

After all commits:

### Test Case 1: Filename with date pattern
```bash
# Upload file: 2026-07-01_lab-meeting.mp4
curl -X POST http://localhost:8080/import \
  -F "file=@2026-07-01_lab-meeting.mp4"

# Verify meeting.json:
# - starts_at: "2026-07-01T00:00:00Z"
# - metadata_source: "filename"
# - title: "lab-meeting"
# - platform: "upload"
# - file_metadata.format: "mov,mp4,m4a,3gp,3g2,mj2"
```

### Test Case 2: Filename with date and time
```bash
# Upload file: 2026-07-01-14.30_standup.wav
# Verify:
# - starts_at: "2026-07-01T14:30:00Z"
# - metadata_source: "filename"
```

### Test Case 3: Filename without pattern (ffprobe fallback)
```bash
# Upload file: recording-001.mp3
# Verify:
# - starts_at: <from ffprobe creation_time>
# - metadata_source: "probe"
# - title: "recording-001"
```

### Test Case 4: User-provided title (precedence)
```bash
# Upload with title query param: ?title=User%20Override
# Verify:
# - title: "User Override"
# - metadata_source: "user"
```

### Test Case 5: Video file with metadata
```bash
# Upload: 20260701-1430_demo.mkv
# Verify:
# - Video frames discarded (only audio in normalized.wav)
# - starts_at: "2026-07-01T14:30:00Z"
# - file_metadata.format: "matroska,webm"
# - file_metadata.duration: <actual duration>
```

---

## Week 2 Deliverables Checklist

After implementation, verify against Week 2 requirements:

| Requirement | Verification | Status |
|------------|--------------|--------|
| ffprobe file info stored per meeting | `meeting.json` contains `file_metadata` with duration, format, bitrate, creation_time, size | ⏳ |
| Filename date/time parser | `parse_filename()` handles `YYYY-MM-DD_title`, `YYYY-MM-DD-HH.MM_title`, `YYYYMMDD_title`, `YYYYMMDD-HHMM_title` | ⏳ |
| Metadata source precedence | `resolve_metadata_for_upload()` implements user > filename > ffprobe | ⏳ |
| `meeting.json` extended | `starts_at`, `metadata_source`, `platform` fields present | ⏳ |
| HTTP upload and CLI import use same runner | Both paths call `Meeting::from_resolved_metadata()` and `resolve_metadata_for_upload()` | ⏳ |
| Tests for metadata | `metadata_test.rs` covers filename parsing, resolution precedence, probe fallback | ⏳ |

---

## PRD Compliance Verification

| PRD Requirement | Verification | Status |
|----------------|--------------|--------|
| FR-3: Accept audio/video, extract to 16 kHz WAV | ✅ Already done (Week 2 early tasks) | ✅ |
| FR-4: Extract file info, parse filename date/time | `probe_file_metadata_from_bytes()`, `parse_filename()` implemented | ⏳ |
| FR-10: Metadata resolution with source tracking | `resolve_metadata_for_upload()` with precedence logic | ⏳ |
| FR-15: CLI and API share same pipeline | Both use `Meeting::from_resolved_metadata()` | ⏳ |
| G-2: One canonical pipeline | Metadata resolution stage shared between CLI/API | ⏳ |

---

## Notes

### Backward Compatibility

- `Meeting::new()` unchanged — existing code continues to work
- New fields are `Option` or have defaults — no breaking changes
- Old `meeting.json` files without new fields will deserialize (serde defaults)

### Future Extensions (Week 6)

When live bot capture is added:

1. Add `calendar` and `bot` variants to `MetadataSource` enum
2. Extend `resolve_metadata()` to accept calendar/bot inputs
3. Use same precedence: user > calendar/bot > filename > probe

### Open Questions

1. **User edit API**: Should we add `PATCH /meetings/{id}` to allow updating metadata after creation? (FR-14 mentions "rename session / edit metadata")
   - Defer to Week 7 when review gate is implemented?

2. **Timezone handling**: All timestamps currently in UTC. Should we store timezone from file metadata?
   - PRD doesn't specify, default to UTC is reasonable

3. **Ambiguous title detection**: FR-10 mentions "title ambiguous ⇒ flag for LLM topic inference"
   - What defines "ambiguous"? Empty? Generic like "recording"?
   - Defer to Week 5 (SOP minutes) when LLM inference is added?

---

## Files Changed Summary

| File | Change Type | Estimated Lines |
|------|-------------|-----------------|
| `crates/core/src/models.rs` | Modify | +80 |
| `crates/core/src/metadata.rs` | Create | +250 |
| `crates/core/src/lib.rs` | Modify | +1 |
| `crates/core/Cargo.toml` | Modify | +1 |
| `crates/core/src/runners.rs` | Modify | +20 |
| `crates/cli/src/commands/import.rs` | Modify | +15 |
| `crates/server/src/types.rs` | Modify | +30 |
| `crates/core/tests/metadata_test.rs` | Create | +120 |

**Total estimated**: ~517 lines across 8 files

---

## References

- PRD.md §6 Stage C: Metadata resolution
- PRD.md §9: Class diagram (Meeting, AudioArtifact, metadata_source)
- PRD.md FR-4: File info extraction and filename parsing
- PRD.md FR-10: Metadata resolution precedence
- Week 2 todo: `docs/daily-logs/08_MeetingAgent.md`

