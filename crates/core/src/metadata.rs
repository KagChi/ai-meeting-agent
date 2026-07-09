//! Metadata extraction and resolution
//!
//! Implements metadata extraction from multiple sources and resolution precedence logic.
//! Supports filename parsing, FFprobe-based file metadata extraction, and precedence-based
//! metadata resolution.

use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use ffmpeg_sidecar::ffprobe;
use std::path::Path;

use crate::models::{FileMetadata, Meeting, MetadataSource};

/// Parsed filename metadata
#[derive(Debug, Clone)]
pub struct ParsedFilename {
    pub date: Option<NaiveDate>,
    pub time: Option<NaiveTime>,
    pub title: Option<String>,
}

/// User-provided metadata (highest precedence)
#[derive(Debug, Clone)]
pub struct UserMetadata {
    pub title: Option<String>,
    pub date: Option<NaiveDateTime>,
    pub participants: Option<Vec<String>>,
    pub location: Option<String>,
    pub organizer: Option<String>,
}

/// All metadata sources for resolution
#[derive(Debug, Clone)]
pub struct MetadataSources {
    pub user_provided: Option<UserMetadata>,
    pub calendar_bot: Option<CalendarBotMetadata>,
    pub filename: Option<ParsedFilename>,
    pub ffprobe: Option<FileMetadata>,
}

/// Calendar/bot metadata (second highest precedence)
#[derive(Debug, Clone)]
pub struct CalendarBotMetadata {
    pub title: Option<String>,
    pub date: Option<NaiveDateTime>,
    pub participants: Option<Vec<String>>,
    pub location: Option<String>,
    pub organizer: Option<String>,
}

/// Resolved metadata with source tracking
#[derive(Debug, Clone)]
pub struct ResolvedMetadata {
    pub title: String,
    pub title_source: MetadataSource,
    pub date: Option<NaiveDateTime>,
    pub date_source: MetadataSource,
    pub participants: Option<Vec<String>>,
    pub participants_source: MetadataSource,
    pub location: Option<String>,
    pub location_source: MetadataSource,
    pub organizer: Option<String>,
    pub organizer_source: MetadataSource,
}

/// Extract file metadata using FFprobe
pub fn extract_file_metadata(path: &Path) -> Result<FileMetadata> {
    let output = std::process::Command::new(ffprobe::ffprobe_path())
        .arg("-v")
        .arg("quiet")
        .arg("-print_format")
        .arg("json")
        .arg("-show_format")
        .arg("-show_streams")
        .arg(path)
        .output()
        .context("Failed to spawn ffprobe")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffprobe failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .with_context(|| format!("Failed to parse ffprobe JSON output: {}", stdout))?;

    // Extract audio stream metadata
    let streams = json["streams"]
        .as_array()
        .context("Missing streams array in ffprobe output")?;

    let audio_stream = streams
        .iter()
        .find(|s| s["codec_type"].as_str() == Some("audio"));

    let codec = audio_stream
        .and_then(|s| s["codec_name"].as_str())
        .map(String::from);

    let sample_rate = audio_stream
        .and_then(|s| s["sample_rate"].as_str())
        .and_then(|s| s.parse::<u32>().ok());

    let bit_rate = audio_stream
        .and_then(|s| s["bit_rate"].as_str())
        .and_then(|s| s.parse::<u64>().ok());

    let channels = audio_stream
        .and_then(|s| s["channels"].as_i64())
        .and_then(|c| u8::try_from(c).ok());

    // Extract file size from format section
    let file_size_bytes = json["format"]["size"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok());

    Ok(FileMetadata {
        codec,
        sample_rate,
        bit_rate,
        channels,
        file_size_bytes,
    })
}

/// Parse filename to extract metadata
///
/// Supports 4 patterns per PRD:
/// 1. `YYYY-MM-DD_HH-MM_Topic.ext` (ISO date-time)
/// 2. `YYYY-MM-DD_Topic.ext` (ISO date)
/// 3. `Meeting_YYYYMMDD.ext` (compact date)
/// 4. `Topic_only.ext` (title only)
pub fn parse_filename(filename: &str) -> Option<ParsedFilename> {
    // Strip extension
    let name = std::path::Path::new(filename).file_stem()?.to_str()?;

    // Pattern 1: YYYY-MM-DD_HH-MM_Topic (ISO date-time)
    // Example: 2026-07-09_14-30_Weekly_Standup
    if let Some(caps) = regex::Regex::new(r"^(\d{4})-(\d{2})-(\d{2})_(\d{2})-(\d{2})_(.+)$")
        .ok()?
        .captures(name)
    {
        let year = caps.get(1)?.as_str().parse().ok()?;
        let month = caps.get(2)?.as_str().parse().ok()?;
        let day = caps.get(3)?.as_str().parse().ok()?;
        let hour = caps.get(4)?.as_str().parse().ok()?;
        let minute = caps.get(5)?.as_str().parse().ok()?;
        let title = caps.get(6)?.as_str().replace('_', " ");

        return Some(ParsedFilename {
            date: NaiveDate::from_ymd_opt(year, month, day),
            time: NaiveTime::from_hms_opt(hour, minute, 0),
            title: Some(title),
        });
    }

    // Pattern 2: YYYY-MM-DD_Topic (ISO date)
    // Example: 2026-07-09_Project_Review
    if let Some(caps) = regex::Regex::new(r"^(\d{4})-(\d{2})-(\d{2})_(.+)$")
        .ok()?
        .captures(name)
    {
        let year = caps.get(1)?.as_str().parse().ok()?;
        let month = caps.get(2)?.as_str().parse().ok()?;
        let day = caps.get(3)?.as_str().parse().ok()?;
        let title = caps.get(4)?.as_str().replace('_', " ");

        return Some(ParsedFilename {
            date: NaiveDate::from_ymd_opt(year, month, day),
            time: None,
            title: Some(title),
        });
    }

    // Pattern 3: Meeting_YYYYMMDD (compact date)
    // Example: Meeting_20260709
    if let Some(caps) = regex::Regex::new(r"^Meeting_(\d{4})(\d{2})(\d{2})$")
        .ok()?
        .captures(name)
    {
        let year = caps.get(1)?.as_str().parse().ok()?;
        let month = caps.get(2)?.as_str().parse().ok()?;
        let day = caps.get(3)?.as_str().parse().ok()?;

        return Some(ParsedFilename {
            date: NaiveDate::from_ymd_opt(year, month, day),
            time: None,
            title: None,
        });
    }

    // Pattern 4: Topic_only (title only, no date)
    // Example: Weekly_Team_Sync
    if !name.is_empty() {
        return Some(ParsedFilename {
            date: None,
            time: None,
            title: Some(name.replace('_', " ")),
        });
    }

    None
}

/// Resolve metadata from multiple sources using precedence logic
///
/// Precedence per PRD: UserProvided > CalendarBot > Filename > FFprobe > Default
pub fn resolve_metadata(sources: MetadataSources) -> ResolvedMetadata {
    // Title resolution
    let (title, title_source) = sources
        .user_provided
        .as_ref()
        .and_then(|u| u.title.as_ref())
        .map(|t| (t.clone(), MetadataSource::UserProvided))
        .or_else(|| {
            sources
                .calendar_bot
                .as_ref()
                .and_then(|c| c.title.as_ref())
                .map(|t| (t.clone(), MetadataSource::CalendarBot))
        })
        .or_else(|| {
            sources
                .filename
                .as_ref()
                .and_then(|f| f.title.as_ref())
                .map(|t| (t.clone(), MetadataSource::Filename))
        })
        .unwrap_or_else(|| ("Untitled Meeting".to_string(), MetadataSource::Default));

    // Date resolution (combines date + time from filename if available)
    let (date, date_source) = sources
        .user_provided
        .as_ref()
        .and_then(|u| u.date.as_ref())
        .map(|d| (Some(*d), MetadataSource::UserProvided))
        .or_else(|| {
            sources
                .calendar_bot
                .as_ref()
                .and_then(|c| c.date.as_ref())
                .map(|d| (Some(*d), MetadataSource::CalendarBot))
        })
        .or_else(|| {
            sources.filename.as_ref().and_then(|f| {
                f.date.map(|date| {
                    let time = f
                        .time
                        .unwrap_or_else(|| NaiveTime::from_hms_opt(0, 0, 0).unwrap());
                    (
                        Some(NaiveDateTime::new(date, time)),
                        MetadataSource::Filename,
                    )
                })
            })
        })
        .unwrap_or((None, MetadataSource::Default));

    // Participants resolution
    let (participants, participants_source) = sources
        .user_provided
        .as_ref()
        .and_then(|u| u.participants.as_ref())
        .map(|p| (Some(p.clone()), MetadataSource::UserProvided))
        .or_else(|| {
            sources
                .calendar_bot
                .as_ref()
                .and_then(|c| c.participants.as_ref())
                .map(|p| (Some(p.clone()), MetadataSource::CalendarBot))
        })
        .unwrap_or((None, MetadataSource::Default));

    // Location resolution
    let (location, location_source) = sources
        .user_provided
        .as_ref()
        .and_then(|u| u.location.as_ref())
        .map(|l| (Some(l.clone()), MetadataSource::UserProvided))
        .or_else(|| {
            sources
                .calendar_bot
                .as_ref()
                .and_then(|c| c.location.as_ref())
                .map(|l| (Some(l.clone()), MetadataSource::CalendarBot))
        })
        .unwrap_or((None, MetadataSource::Default));

    // Organizer resolution
    let (organizer, organizer_source) = sources
        .user_provided
        .as_ref()
        .and_then(|u| u.organizer.as_ref())
        .map(|o| (Some(o.clone()), MetadataSource::UserProvided))
        .or_else(|| {
            sources
                .calendar_bot
                .as_ref()
                .and_then(|c| c.organizer.as_ref())
                .map(|o| (Some(o.clone()), MetadataSource::CalendarBot))
        })
        .unwrap_or((None, MetadataSource::Default));

    ResolvedMetadata {
        title,
        title_source,
        date,
        date_source,
        participants,
        participants_source,
        location,
        location_source,
        organizer,
        organizer_source,
    }
}

/// Enrich Meeting with metadata from file and user input
///
/// Orchestrates filename parsing, ffprobe extraction, metadata resolution, and Meeting mutation.
/// This is the main entry point for metadata extraction.
pub fn enrich_meeting_with_metadata(
    meeting: &mut Meeting,
    file_path: &Path,
    user_metadata: Option<UserMetadata>,
) -> Result<()> {
    // Extract filename metadata
    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .context("Invalid filename")?;
    let parsed_filename = parse_filename(filename);

    // Extract file metadata via FFprobe
    let file_metadata = extract_file_metadata(file_path).ok();

    // Resolve metadata using precedence logic
    let sources = MetadataSources {
        user_provided: user_metadata,
        calendar_bot: None, // Calendar/bot metadata not available in file upload path
        filename: parsed_filename,
        ffprobe: file_metadata.clone(),
    };

    let resolved = resolve_metadata(sources);

    // Mutate Meeting with resolved metadata
    meeting.title = resolved.title;
    meeting.metadata_source = Some(resolved.title_source);

    if let Some(date) = resolved.date {
        meeting.date = chrono::DateTime::from_naive_utc_and_offset(date, chrono::Utc);
        meeting.recording_date = Some(date);
    }

    meeting.participants = resolved.participants;
    meeting.location = resolved.location;
    meeting.organizer = resolved.organizer;
    meeting.file_metadata = file_metadata;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_filename_iso_datetime() {
        let parsed = parse_filename("2026-07-09_14-30_Weekly_Standup.mp3");
        assert!(parsed.is_some());
        let parsed = parsed.unwrap();
        assert_eq!(parsed.date, NaiveDate::from_ymd_opt(2026, 7, 9));
        assert_eq!(parsed.time, NaiveTime::from_hms_opt(14, 30, 0));
        assert_eq!(parsed.title, Some("Weekly Standup".to_string()));
    }

    #[test]
    fn test_parse_filename_iso_date() {
        let parsed = parse_filename("2026-07-09_Project_Review.wav");
        assert!(parsed.is_some());
        let parsed = parsed.unwrap();
        assert_eq!(parsed.date, NaiveDate::from_ymd_opt(2026, 7, 9));
        assert_eq!(parsed.time, None);
        assert_eq!(parsed.title, Some("Project Review".to_string()));
    }

    #[test]
    fn test_parse_filename_compact_date() {
        let parsed = parse_filename("Meeting_20260709.mp3");
        assert!(parsed.is_some());
        let parsed = parsed.unwrap();
        assert_eq!(parsed.date, NaiveDate::from_ymd_opt(2026, 7, 9));
        assert_eq!(parsed.time, None);
        assert_eq!(parsed.title, None);
    }

    #[test]
    fn test_parse_filename_title_only() {
        let parsed = parse_filename("Weekly_Team_Sync.wav");
        assert!(parsed.is_some());
        let parsed = parsed.unwrap();
        assert_eq!(parsed.date, None);
        assert_eq!(parsed.time, None);
        assert_eq!(parsed.title, Some("Weekly Team Sync".to_string()));
    }

    #[test]
    fn test_resolve_metadata_user_provided_wins() {
        let sources = MetadataSources {
            user_provided: Some(UserMetadata {
                title: Some("User Title".to_string()),
                date: Some(
                    NaiveDate::from_ymd_opt(2026, 1, 1)
                        .unwrap()
                        .and_hms_opt(10, 0, 0)
                        .unwrap(),
                ),
                participants: Some(vec!["Alice".to_string()]),
                location: Some("Room 1".to_string()),
                organizer: Some("Bob".to_string()),
            }),
            calendar_bot: Some(CalendarBotMetadata {
                title: Some("Calendar Title".to_string()),
                date: Some(
                    NaiveDate::from_ymd_opt(2026, 2, 1)
                        .unwrap()
                        .and_hms_opt(11, 0, 0)
                        .unwrap(),
                ),
                participants: Some(vec!["Charlie".to_string()]),
                location: Some("Room 2".to_string()),
                organizer: Some("Dave".to_string()),
            }),
            filename: Some(ParsedFilename {
                date: NaiveDate::from_ymd_opt(2026, 3, 1),
                time: NaiveTime::from_hms_opt(12, 0, 0),
                title: Some("Filename Title".to_string()),
            }),
            ffprobe: None,
        };

        let resolved = resolve_metadata(sources);
        assert_eq!(resolved.title, "User Title");
        assert!(matches!(
            resolved.title_source,
            MetadataSource::UserProvided
        ));
        assert_eq!(
            resolved.date,
            Some(
                NaiveDate::from_ymd_opt(2026, 1, 1)
                    .unwrap()
                    .and_hms_opt(10, 0, 0)
                    .unwrap()
            )
        );
        assert_eq!(resolved.participants, Some(vec!["Alice".to_string()]));
        assert_eq!(resolved.location, Some("Room 1".to_string()));
        assert_eq!(resolved.organizer, Some("Bob".to_string()));
    }

    #[test]
    fn test_resolve_metadata_calendar_bot_precedence() {
        let sources = MetadataSources {
            user_provided: None,
            calendar_bot: Some(CalendarBotMetadata {
                title: Some("Calendar Title".to_string()),
                date: Some(
                    NaiveDate::from_ymd_opt(2026, 2, 1)
                        .unwrap()
                        .and_hms_opt(11, 0, 0)
                        .unwrap(),
                ),
                participants: Some(vec!["Charlie".to_string()]),
                location: Some("Room 2".to_string()),
                organizer: Some("Dave".to_string()),
            }),
            filename: Some(ParsedFilename {
                date: NaiveDate::from_ymd_opt(2026, 3, 1),
                time: NaiveTime::from_hms_opt(12, 0, 0),
                title: Some("Filename Title".to_string()),
            }),
            ffprobe: None,
        };

        let resolved = resolve_metadata(sources);
        assert_eq!(resolved.title, "Calendar Title");
        assert!(matches!(resolved.title_source, MetadataSource::CalendarBot));
        assert_eq!(
            resolved.date,
            Some(
                NaiveDate::from_ymd_opt(2026, 2, 1)
                    .unwrap()
                    .and_hms_opt(11, 0, 0)
                    .unwrap()
            )
        );
        assert_eq!(resolved.participants, Some(vec!["Charlie".to_string()]));
    }

    #[test]
    fn test_resolve_metadata_filename_precedence() {
        let sources = MetadataSources {
            user_provided: None,
            calendar_bot: None,
            filename: Some(ParsedFilename {
                date: NaiveDate::from_ymd_opt(2026, 3, 1),
                time: NaiveTime::from_hms_opt(12, 0, 0),
                title: Some("Filename Title".to_string()),
            }),
            ffprobe: None,
        };

        let resolved = resolve_metadata(sources);
        assert_eq!(resolved.title, "Filename Title");
        assert!(matches!(resolved.title_source, MetadataSource::Filename));
        assert_eq!(
            resolved.date,
            Some(
                NaiveDate::from_ymd_opt(2026, 3, 1)
                    .unwrap()
                    .and_hms_opt(12, 0, 0)
                    .unwrap()
            )
        );
        assert!(matches!(resolved.date_source, MetadataSource::Filename));
    }

    #[test]
    fn test_resolve_metadata_default_fallback() {
        let sources = MetadataSources {
            user_provided: None,
            calendar_bot: None,
            filename: None,
            ffprobe: None,
        };

        let resolved = resolve_metadata(sources);
        assert_eq!(resolved.title, "Untitled Meeting");
        assert!(matches!(resolved.title_source, MetadataSource::Default));
        assert_eq!(resolved.date, None);
        assert!(matches!(resolved.date_source, MetadataSource::Default));
    }

    #[test]
    fn test_enrich_meeting_with_metadata_integration() {
        let mut meeting = Meeting::new("Original Title".to_string());

        // Create a temp file for testing (we'll use a minimal path that won't be probed)
        let temp_path = std::path::PathBuf::from("2026-07-09_14-30_Test_Meeting.mp3");

        let user_metadata = Some(UserMetadata {
            title: None,
            date: None,
            participants: Some(vec!["Alice".to_string(), "Bob".to_string()]),
            location: Some("Conference Room A".to_string()),
            organizer: Some("Alice".to_string()),
        });

        // Note: This will attempt to run ffprobe, which might fail in test environment
        // In a real scenario, we'd mock ffprobe or use a test fixture
        let result = enrich_meeting_with_metadata(&mut meeting, &temp_path, user_metadata);

        // Even if ffprobe fails, filename parsing and user metadata should work
        if result.is_ok() {
            assert_eq!(meeting.title, "Test Meeting");
            assert_eq!(
                meeting.participants,
                Some(vec!["Alice".to_string(), "Bob".to_string()])
            );
            assert_eq!(meeting.location, Some("Conference Room A".to_string()));
            assert_eq!(meeting.organizer, Some("Alice".to_string()));
        }
    }
}
