//! Meeting storage operations
//!
//! SQLite-based storage for meetings, transcripts, summaries, and audio files.

use crate::db;
use crate::fs;
use crate::models::{
    FileMetadata, MatchedSegment, Meeting, MeetingSearchResult, MeetingStatus, MetadataSource,
    Person, Summary, SummaryFormat, SummaryStatus, SummaryTemplate, TranscriptionInfo,
    TranscriptVersion, Voiceprint, VoiceprintEnrolledFrom, VoiceprintSample,
    VoiceprintSampleSource,
};
use crate::orchestrator::{OrchestratorRun, OrchestratorRunStatus};
use crate::transcription::{TranscriptSegment, TranscriptionResponse};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::{Pool, Row, Sqlite};
use std::path::Path;
use std::path::PathBuf;

/// Escape a user search string for SQLite FTS5 `MATCH`.
///
/// Treats input as plain text (not FTS operators). Each whitespace-separated
/// token is double-quoted so characters like `+`, `-`, `:`, `*` do not cause
/// FTS5 syntax errors. Returns `None` if there are no usable tokens.
pub fn escape_fts5_query(query: &str) -> Option<String> {
    let mut parts = Vec::new();
    for token in query.split_whitespace() {
        if token.is_empty() {
            continue;
        }
        // FTS5 phrase: double any embedded quotes
        let escaped = token.replace('"', "\"\"");
        parts.push(format!("\"{escaped}\""));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// Meeting storage manager with SQLite backend
#[derive(Clone)]
pub struct MeetingStorage {
    base: PathBuf,
    db: Pool<Sqlite>,
}

impl MeetingStorage {
    /// Create a new storage instance using the default data directory
    pub async fn new() -> Result<Self> {
        let base = fs::data_dir().context("Failed to determine home directory")?;
        Self::with_base(base).await
    }

    /// Create a storage instance with a custom base directory (for testing)
    pub async fn with_base(base: PathBuf) -> Result<Self> {
        // Ensure base directory exists
        std::fs::create_dir_all(&base).context("Failed to create base directory")?;

        // Initialize SQLite database
        let db_path = base.join("meetings.db");
        let pool = db::init_database(&db_path).await?;

        Ok(Self { base, db: pool })
    }

    /// Create a storage instance with in-memory database (for testing)
    pub async fn in_memory(base: PathBuf) -> Result<Self> {
        // Ensure base directory exists
        std::fs::create_dir_all(&base).context("Failed to create base directory")?;

        // Initialize in-memory SQLite database
        let pool = db::init_memory_database().await?;

        Ok(Self { base, db: pool })
    }

    /// Get the meetings directory path
    fn meetings_dir(&self) -> PathBuf {
        self.base.join("meetings")
    }

    /// Get a specific meeting's directory path
    fn meeting_dir(&self, meeting_id: &str) -> PathBuf {
        self.meetings_dir().join(meeting_id)
    }

    fn audio_file_name(audio_file: Option<&str>) -> Option<String> {
        audio_file.and_then(|value| {
            Path::new(value)
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.to_string())
        })
    }

    /// Create a new meeting with metadata
    pub async fn create_meeting(&self, meeting: &Meeting) -> Result<()> {
        // Convert enums to lowercase strings for DB
        let status = format!("{:?}", meeting.status).to_lowercase();
        let metadata_source = meeting
            .metadata_source
            .as_ref()
            .map(|s| format!("{:?}", s).to_lowercase());
        let audio_file = Self::audio_file_name(meeting.audio_file.as_deref());

        sqlx::query(
            "INSERT INTO meetings (
                id, title, date, duration_seconds, status,
                transcription_provider, transcription_model, transcription_completed_at,
                participants, location, organizer, metadata_source, recording_date, platform,
                file_metadata, audio_file,
                created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&meeting.id)
        .bind(&meeting.title)
        .bind(meeting.date)
        .bind(meeting.duration_seconds.map(|d| d as i64))
        .bind(&status)
        .bind(meeting.transcription.as_ref().map(|t| t.provider.as_str()))
        .bind(meeting.transcription.as_ref().map(|t| t.model.as_str()))
        .bind(meeting.transcription.as_ref().map(|t| t.completed_at))
        .bind(
            meeting
                .participants
                .as_ref()
                .map(|p| serde_json::to_string(p).unwrap()),
        )
        .bind(meeting.location.as_deref())
        .bind(meeting.organizer.as_deref())
        .bind(metadata_source.as_deref())
        .bind(meeting.recording_date)
        .bind(meeting.platform.as_deref())
        .bind(
            meeting
                .file_metadata
                .as_ref()
                .map(|m| serde_json::to_string(m).unwrap()),
        )
        .bind(audio_file.as_deref())
        .bind(meeting.created_at)
        .bind(meeting.updated_at)
        .execute(&self.db)
        .await
        .context("Failed to insert meeting into database")?;

        // Create meeting directory for audio files
        let meeting_path = self.meeting_dir(&meeting.id);
        std::fs::create_dir_all(meeting_path.join("audio"))
            .context("Failed to create meeting directory")?;

        Ok(())
    }

    /// Get meeting by ID
    pub async fn get_meeting(&self, meeting_id: &str) -> Result<Meeting> {
        let row = sqlx::query(
            "SELECT 
                id, title, date, duration_seconds, status,
                transcription_provider, transcription_model, transcription_completed_at,
                participants, location, organizer, metadata_source, recording_date, platform,
                file_metadata, audio_file,
                created_at, updated_at
            FROM meetings WHERE id = ?",
        )
        .bind(meeting_id)
        .fetch_optional(&self.db)
        .await
        .context("Failed to query meeting")?
        .ok_or_else(|| anyhow::anyhow!("Meeting not found: {}", meeting_id))?;

        let meeting = self.row_to_meeting(row)?;
        Ok(meeting)
    }

    /// Convert a sqlx Row to a Meeting struct
    fn row_to_meeting(&self, row: sqlx::sqlite::SqliteRow) -> Result<Meeting> {
        let status_str: String = row.try_get(4)?;
        let status = match status_str.as_str() {
            "importing" => MeetingStatus::Importing,
            "ready" => MeetingStatus::Ready,
            "failed" => MeetingStatus::Failed,
            _ => MeetingStatus::Failed,
        };

        let transcription = if let (Some(provider), Some(model), Some(completed_at)) = (
            row.try_get::<Option<String>, _>(5)?,
            row.try_get::<Option<String>, _>(6)?,
            row.try_get::<Option<DateTime<Utc>>, _>(7)?,
        ) {
            Some(TranscriptionInfo {
                provider,
                model,
                completed_at,
            })
        } else {
            None
        };

        let participants: Option<Vec<String>> = row
            .try_get::<Option<String>, _>(8)?
            .and_then(|s: String| serde_json::from_str(&s).ok());

        let metadata_source: Option<MetadataSource> = row
            .try_get::<Option<String>, _>(11)?
            .and_then(|s: String| match s.as_str() {
                "userprovided" => Some(MetadataSource::UserProvided),
                "calendarbot" => Some(MetadataSource::CalendarBot),
                "filename" => Some(MetadataSource::Filename),
                "ffprobe" => Some(MetadataSource::FFprobe),
                "default" => Some(MetadataSource::Default),
                _ => None,
            });

        let file_metadata: Option<FileMetadata> = row
            .try_get::<Option<String>, _>(14)?
            .and_then(|s: String| serde_json::from_str(&s).ok());

        let id: String = row.try_get(0)?;
        let audio_file = row.try_get::<Option<String>, _>(15)?.map(|name| {
            self.meeting_dir(&id)
                .join("audio")
                .join(name)
                .to_string_lossy()
                .to_string()
        });

        Ok(Meeting {
            id,
            title: row.try_get(1)?,
            date: row.try_get(2)?,
            duration_seconds: row.try_get::<Option<i64>, _>(3)?.map(|d| d as u64),
            status,
            transcription,
            created_at: row.try_get(16)?,
            updated_at: row.try_get(17)?,
            participants,
            location: row.try_get(9)?,
            organizer: row.try_get(10)?,
            metadata_source,
            recording_date: row.try_get(12)?,
            platform: row.try_get(13)?,
            file_metadata,
            audio_file,
        })
    }

    /// Resolve a meeting ID or short prefix (≥8 chars) to a full UUID
    pub async fn resolve_meeting_id(&self, id_or_prefix: &str) -> Result<Option<String>> {
        if id_or_prefix.len() < 8 {
            anyhow::bail!("ID too short, need at least 8 characters");
        }

        // Fast path: exact match
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM meetings WHERE id = ?)")
            .bind(id_or_prefix)
            .fetch_one(&self.db)
            .await
            .context("Failed to check meeting existence")?;

        if exists {
            return Ok(Some(id_or_prefix.to_string()));
        }

        // Prefix search
        let pattern = format!("{}%", id_or_prefix);
        let matches: Vec<String> =
            sqlx::query_scalar("SELECT id FROM meetings WHERE id LIKE ? LIMIT 4")
                .bind(&pattern)
                .fetch_all(&self.db)
                .await
                .context("Failed to execute prefix query")?;

        match matches.len() {
            0 => Ok(None),
            1 => Ok(Some(matches.into_iter().next().unwrap())),
            _ => {
                let preview: Vec<&str> = matches.iter().take(3).map(|s| s.as_str()).collect();
                anyhow::bail!(
                    "Ambiguous ID '{}', matches: {}",
                    id_or_prefix,
                    preview.join(", ")
                );
            }
        }
    }

    /// Update meeting metadata
    pub async fn update_meeting(&self, meeting: &Meeting) -> Result<()> {
        let status = format!("{:?}", meeting.status).to_lowercase();
        let metadata_source = meeting
            .metadata_source
            .as_ref()
            .map(|s| format!("{:?}", s).to_lowercase());
        let audio_file = Self::audio_file_name(meeting.audio_file.as_deref());

        let result = sqlx::query(
            "UPDATE meetings SET
                title = ?, date = ?, duration_seconds = ?, status = ?,
                transcription_provider = ?, transcription_model = ?, transcription_completed_at = ?,
                participants = ?, location = ?, organizer = ?, metadata_source = ?,
                recording_date = ?, platform = ?, file_metadata = ?, audio_file = ?
            WHERE id = ?",
        )
        .bind(&meeting.title)
        .bind(meeting.date)
        .bind(meeting.duration_seconds.map(|d| d as i64))
        .bind(&status)
        .bind(meeting.transcription.as_ref().map(|t| t.provider.as_str()))
        .bind(meeting.transcription.as_ref().map(|t| t.model.as_str()))
        .bind(meeting.transcription.as_ref().map(|t| t.completed_at))
        .bind(
            meeting
                .participants
                .as_ref()
                .map(|p| serde_json::to_string(p).unwrap()),
        )
        .bind(meeting.location.as_deref())
        .bind(meeting.organizer.as_deref())
        .bind(metadata_source.as_deref())
        .bind(meeting.recording_date)
        .bind(meeting.platform.as_deref())
        .bind(
            meeting
                .file_metadata
                .as_ref()
                .map(|m| serde_json::to_string(m).unwrap()),
        )
        .bind(audio_file.as_deref())
        .bind(&meeting.id)
        .execute(&self.db)
        .await
        .context("Failed to update meeting")?;

        if result.rows_affected() == 0 {
            anyhow::bail!("Meeting not found: {}", meeting.id);
        }

        Ok(())
    }

    /// Delete meeting and all associated files
    pub async fn delete_meeting(&self, meeting_id: &str) -> Result<()> {
        // SQLite CASCADE will delete transcript_segments, summaries, etc.
        let result = sqlx::query("DELETE FROM meetings WHERE id = ?")
            .bind(meeting_id)
            .execute(&self.db)
            .await
            .context("Failed to delete meeting from database")?;

        if result.rows_affected() == 0 {
            anyhow::bail!("Meeting not found: {}", meeting_id);
        }

        // Delete meeting directory (audio files)
        let meeting_path = self.meeting_dir(meeting_id);
        if meeting_path.exists() {
            std::fs::remove_dir_all(&meeting_path).context("Failed to delete meeting directory")?;
        }

        Ok(())
    }

    /// List all meetings (sorted by date descending)
    pub async fn list_meetings(&self) -> Result<Vec<Meeting>> {
        let rows = sqlx::query(
            "SELECT 
                id, title, date, duration_seconds, status,
                transcription_provider, transcription_model, transcription_completed_at,
                participants, location, organizer, metadata_source, recording_date, platform,
                file_metadata, audio_file,
                created_at, updated_at
            FROM meetings ORDER BY date DESC",
        )
        .fetch_all(&self.db)
        .await
        .context("Failed to query meetings")?;

        let meetings: Result<Vec<Meeting>> = rows
            .into_iter()
            .map(|row| self.row_to_meeting(row))
            .collect();

        meetings
    }

    /// List meetings with pagination (sorted by date descending).
    pub async fn list_meetings_paginated(&self, limit: u32, offset: u32) -> Result<Vec<Meeting>> {
        let rows = sqlx::query(
            "SELECT 
                id, title, date, duration_seconds, status,
                transcription_provider, transcription_model, transcription_completed_at,
                participants, location, organizer, metadata_source, recording_date, platform,
                file_metadata, audio_file,
                created_at, updated_at
            FROM meetings ORDER BY date DESC LIMIT ? OFFSET ?",
        )
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.db)
        .await
        .context("Failed to query paginated meetings")?;

        rows.into_iter()
            .map(|row| self.row_to_meeting(row))
            .collect()
    }

    /// Count all meetings.
    pub async fn count_meetings(&self) -> Result<u64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings")
            .fetch_one(&self.db)
            .await
            .context("Failed to count meetings")?;
        Ok(count as u64)
    }

    /// Bulk-rename speaker labels on the latest transcript version.
    ///
    /// `mapping` keys are existing labels (e.g. `"SPEAKER_00"`); values are the
    /// new display names. FTS index is kept in sync via the `segments_au` trigger.
    /// Returns total number of segment rows updated.
    pub async fn rename_speakers(
        &self,
        meeting_id: &str,
        mapping: &std::collections::HashMap<String, String>,
    ) -> Result<u64> {
        // Ensure meeting exists
        self.get_meeting(meeting_id).await?;

        if mapping.is_empty() {
            return Ok(0);
        }

        let version: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(version), 0) FROM transcript_segments WHERE meeting_id = ?",
        )
        .bind(meeting_id)
        .fetch_one(&self.db)
        .await
        .context("Failed to resolve latest transcript version")?;

        if version == 0 {
            anyhow::bail!("Transcript not found for meeting: {}", meeting_id);
        }

        let mut total: u64 = 0;
        for (old, new) in mapping {
            if old == new {
                continue;
            }
            
            // Check if this speaker is identified from voice bank
            let identified: Option<(String, String)> = sqlx::query_as(
                "SELECT DISTINCT ts.speaker, p.name 
                 FROM transcript_segments ts
                 JOIN persons p ON ts.person_id = p.id
                 WHERE ts.meeting_id = ? AND ts.version = ? AND ts.speaker = ?
                 LIMIT 1"
            )
            .bind(meeting_id)
            .bind(version)
            .bind(old)
            .fetch_optional(&self.db)
            .await
            .context("Failed to check speaker identification")?;
            
            if let Some((_, person_name)) = identified {
                // Allow rename only for Guest entries
                if !person_name.starts_with("Guest-") {
                    anyhow::bail!(
                        "Cannot rename speaker '{}' - identified as '{}' from voice bank. \
                         Use 'Clear Voice Identification' to reset.",
                        old, person_name
                    );
                }
            }
            
            let result = sqlx::query(
                "UPDATE transcript_segments
                 SET speaker = ?
                 WHERE meeting_id = ? AND version = ? AND speaker = ?",
            )
            .bind(new)
            .bind(meeting_id)
            .bind(version)
            .bind(old)
            .execute(&self.db)
            .await
            .context("Failed to rename speaker labels")?;
            total += result.rows_affected();
        }

        Ok(total)
    }

    /// Save transcript data and create transcript version metadata.
    pub async fn save_transcript(
        &self,
        meeting_id: &str,
        response: &TranscriptionResponse,
        provider: &str,
        model: &str,
        duration_seconds: u64,
    ) -> Result<()> {
        // Ensure meeting exists and fail with existing message shape.
        self.get_meeting(meeting_id).await?;

        // Compute next version number
        let version: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM transcript_versions WHERE meeting_id = ?",
        )
        .bind(meeting_id)
        .fetch_one(&self.db)
        .await
        .context("Failed to compute transcript version")?;

        // Insert new segments with version number (old versions are preserved)
        // Use array index as segment_id to guarantee uniqueness
        if let Some(segments) = &response.segments {
            for (idx, segment) in segments.iter().enumerate() {
                let result = sqlx::query(
                    "INSERT INTO transcript_segments (
                        meeting_id, version, segment_id, start, end, text, speaker,
                        tokens, temperature, avg_logprob, compression_ratio, no_speech_prob,
                        refined_text, person_id, identify_confidence
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(meeting_id)
                .bind(version)
                .bind(idx as i64)  // Use array index for guaranteed unique segment_id
                .bind(segment.start)
                .bind(segment.end)
                .bind(&segment.text)
                .bind(segment.speaker.as_deref())
                .bind(
                    segment
                        .tokens
                        .as_ref()
                        .map(|t| serde_json::to_string(t).unwrap()),
                )
                .bind(segment.temperature.map(|v| v as f64))
                .bind(segment.avg_logprob.map(|v| v as f64))
                .bind(segment.compression_ratio.map(|v| v as f64))
                .bind(segment.no_speech_prob.map(|v| v as f64))
                .bind(segment.refined_text.as_deref())
                .bind(segment.person_id.as_deref())
                .bind(segment.identify_confidence.map(|v| v as f64))
                .execute(&self.db)
                .await;

                if let Err(e) = result {
                    log::error!(
                        "Failed to insert transcript segment: meeting_id={}, version={}, segment_idx={}, start={:.2}, end={:.2}, error={}",
                        meeting_id, version, idx, segment.start, segment.end, e
                    );
                    return Err(anyhow::anyhow!(
                        "Failed to insert transcript segment (meeting={}, ver={}, idx={}): {}",
                        meeting_id, version, idx, e
                    ));
                }
            }
        }

        // Record version metadata
        sqlx::query(
            "INSERT INTO transcript_versions (meeting_id, version, provider, model, language, segment_count, refined_text)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(meeting_id)
        .bind(version)
        .bind(provider)
        .bind(model)
        .bind(response.language.as_deref())
        .bind(response.segments.as_ref().map(|s| s.len()).unwrap_or(0) as i64)
        .bind(response.refined_text.as_deref())
        .execute(&self.db)
        .await
        .context("Failed to insert transcript version")?;

        Ok(())
    }

    /// Get transcript for a meeting (latest version by default, or specific version).
    pub async fn get_transcript(&self, meeting_id: &str, version: Option<u32>) -> Result<TranscriptionResponse> {
        let segments = self
            .get_transcript_paginated(meeting_id, version, u32::MAX, 0)
            .await?;
        if segments.is_empty() {
            anyhow::bail!("Transcript not found for meeting: {}", meeting_id);
        }

        let text = segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let duration = segments.last().map(|s| s.end);
        let refined_text = self.get_refined_text(meeting_id, version).await?;

        Ok(TranscriptionResponse {
            text,
            language: None,
            duration,
            segments: Some(segments),
            refined_text,
        })
    }

    async fn get_refined_text(
        &self,
        meeting_id: &str,
        version: Option<u32>,
    ) -> Result<Option<String>> {
        let refined = if let Some(v) = version {
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT refined_text FROM transcript_versions WHERE meeting_id = ? AND version = ?",
            )
            .bind(meeting_id)
            .bind(v as i64)
            .fetch_optional(&self.db)
            .await
            .context("Failed to load refined_text")?
            .flatten()
        } else {
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT refined_text FROM transcript_versions
                 WHERE meeting_id = ?
                 ORDER BY version DESC
                 LIMIT 1",
            )
            .bind(meeting_id)
            .fetch_optional(&self.db)
            .await
            .context("Failed to load refined_text")?
            .flatten()
        };

        Ok(refined.filter(|s| !s.is_empty()))
    }

    /// Get transcript segments with pagination and optional version.
    /// If version is None, returns the latest version.
    pub async fn get_transcript_paginated(
        &self,
        meeting_id: &str,
        version: Option<u32>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<TranscriptSegment>> {
        let version_clause = if let Some(v) = version {
            format!("AND version = {}", v)
        } else {
            format!(
                "AND version = (SELECT COALESCE(MAX(version), 1) FROM transcript_segments WHERE meeting_id = ?)"
            )
        };

        let query = format!(
            "SELECT ts.segment_id, ts.start, ts.end, ts.text, ts.speaker, ts.tokens, ts.temperature,
                    ts.avg_logprob, ts.compression_ratio, ts.no_speech_prob, ts.refined_text,
                    ts.person_id, ts.identify_confidence, p.name AS display_name
             FROM transcript_segments ts
             LEFT JOIN persons p ON ts.person_id = p.id
             WHERE ts.meeting_id = ? {}
             ORDER BY ts.segment_id ASC
             LIMIT ? OFFSET ?",
            version_clause
        );

        let mut q = sqlx::query(&query).bind(meeting_id);
        
        // Bind meeting_id again for subquery if version is None
        if version.is_none() {
            q = q.bind(meeting_id);
        }
        
        let rows = q
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.db)
            .await
            .context("Failed to query transcript segments")?;

        rows.into_iter()
            .map(|row| {
                let start: f64 = row.try_get(1)?;
                let tokens = row
                    .try_get::<Option<String>, _>(5)?
                    .and_then(|s| serde_json::from_str(&s).ok());
                let refined_text = row
                    .try_get::<Option<String>, _>(10)?
                    .filter(|s| !s.is_empty());
                let display_name: Option<String> = row.try_get(13)?;
                Ok(TranscriptSegment {
                    id: row.try_get::<i64, _>(0)? as u32,
                    start,
                    end: row.try_get(2)?,
                    text: row.try_get(3)?,
                    timestamp: Some(crate::transcription::format_timestamp_readable(start)),
                    speaker: row.try_get(4)?,
                    tokens,
                    temperature: row.try_get::<Option<f64>, _>(6)?.map(|v| v as f32),
                    avg_logprob: row.try_get::<Option<f64>, _>(7)?.map(|v| v as f32),
                    compression_ratio: row.try_get::<Option<f64>, _>(8)?.map(|v| v as f32),
                    no_speech_prob: row.try_get::<Option<f64>, _>(9)?.map(|v| v as f32),
                    refined_text,
                    person_id: row.try_get(11)?,
                    identify_confidence: row.try_get::<Option<f64>, _>(12)?.map(|v| v as f32),
                    display_name,
                })
            })
            .collect()
    }

    /// Max matched segments returned per meeting in global search.
    pub const SEARCH_SEGMENTS_PER_MEETING: usize = 10;

    /// Global transcript search across all ready meetings (latest version only).
    ///
    /// Returns meetings ordered by FTS5 relevance (lower score = better match),
    /// each with up to [`Self::SEARCH_SEGMENTS_PER_MEETING`] top segments.
    /// `limit`/`offset` paginate meetings, not segments.
    pub async fn search_all_transcripts(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<MeetingSearchResult>, u64)> {
        let Some(fts_query) = escape_fts5_query(query) else {
            return Ok((Vec::new(), 0));
        };

        // Count distinct ready meetings with matches
        let total: i64 = sqlx::query_scalar(
            "WITH latest_versions AS (
                SELECT meeting_id, MAX(version) AS version
                FROM transcript_versions
                GROUP BY meeting_id
             )
             SELECT COUNT(DISTINCT ts.meeting_id)
             FROM transcript_search ts
             JOIN latest_versions lv
               ON ts.meeting_id = lv.meeting_id AND ts.version = lv.version
             JOIN meetings m ON m.id = ts.meeting_id
             WHERE m.status = 'ready' AND transcript_search MATCH ?",
        )
        .bind(&fts_query)
        .fetch_one(&self.db)
        .await
        .context("Failed to count global transcript search results")?;

        if total == 0 {
            return Ok((Vec::new(), 0));
        }

        // Rank meetings by sum of segment ranks (FTS5: lower rank = better)
        let meeting_rows = sqlx::query(
            "WITH latest_versions AS (
                SELECT meeting_id, MAX(version) AS version
                FROM transcript_versions
                GROUP BY meeting_id
             ),
             matching AS (
                SELECT ts.meeting_id, ts.rank AS rank
                FROM transcript_search ts
                JOIN latest_versions lv
                  ON ts.meeting_id = lv.meeting_id AND ts.version = lv.version
                JOIN meetings m ON m.id = ts.meeting_id
                WHERE m.status = 'ready' AND transcript_search MATCH ?
             )
             SELECT meeting_id, SUM(rank) AS relevance_score, COUNT(*) AS match_count
             FROM matching
             GROUP BY meeting_id
             ORDER BY relevance_score ASC
             LIMIT ? OFFSET ?",
        )
        .bind(&fts_query)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.db)
        .await
        .context("Failed to rank meetings for global transcript search")?;

        if meeting_rows.is_empty() {
            return Ok((Vec::new(), total as u64));
        }

        let mut results = Vec::with_capacity(meeting_rows.len());
        for row in meeting_rows {
            let meeting_id: String = row.try_get(0)?;
            let relevance_score: f64 = row.try_get(1)?;
            let match_count: i64 = row.try_get(2)?;

            let meeting = self.get_meeting(&meeting_id).await?;

            // Latest version for this meeting
            let latest_version: Option<i64> = sqlx::query_scalar(
                "SELECT MAX(version) FROM transcript_segments WHERE meeting_id = ?",
            )
            .bind(&meeting_id)
            .fetch_one(&self.db)
            .await
            .context("Failed to get latest transcript version")?;
            let latest_version = latest_version.unwrap_or(1);

            let segment_rows = sqlx::query(
                "SELECT ts.segment_id, ts.start, ts.end, ts.text, ts.speaker, ts.person_id
                 FROM transcript_search
                 INNER JOIN transcript_segments AS ts ON ts.rowid = transcript_search.rowid
                 WHERE transcript_search.meeting_id = ? AND transcript_search.version = ?
                   AND transcript_search MATCH ?
                 ORDER BY transcript_search.rank
                 LIMIT ?",
            )
            .bind(&meeting_id)
            .bind(latest_version)
            .bind(&fts_query)
            .bind(Self::SEARCH_SEGMENTS_PER_MEETING as i64)
            .fetch_all(&self.db)
            .await
            .context("Failed to fetch matched segments for meeting")?;

            let matched_segments = segment_rows
                .into_iter()
                .map(|srow| {
                    let start: f64 = srow.try_get(1)?;
                    Ok(MatchedSegment {
                        segment_id: srow.try_get::<i64, _>(0)? as u32,
                        start,
                        end: srow.try_get(2)?,
                        text: srow.try_get(3)?,
                        timestamp: crate::transcription::format_timestamp_readable(start),
                        speaker: srow.try_get(4)?,
                        person_id: srow.try_get(5)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;

            results.push(MeetingSearchResult {
                id: meeting.id,
                title: meeting.title,
                date: meeting.date,
                duration_seconds: meeting.duration_seconds,
                status: meeting.status,
                participants: meeting.participants,
                matched_segments,
                match_count: match_count as usize,
                relevance_score,
            });
        }

        Ok((results, total as u64))
    }

    /// Search transcript segments using SQLite FTS5 (searches latest version only).
    pub async fn search_transcripts(
        &self,
        meeting_id: &str,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<TranscriptSegment>> {
        let Some(fts_query) = escape_fts5_query(query) else {
            return Ok(Vec::new());
        };

        // Get latest version for this meeting
        let latest_version: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(version) FROM transcript_segments WHERE meeting_id = ?"
        )
        .bind(meeting_id)
        .fetch_optional(&self.db)
        .await
        .context("Failed to get latest version")?
        .flatten();

        let version = match latest_version {
            Some(v) => v,
            None => return Ok(Vec::new()), // No transcript exists
        };

        let rows = sqlx::query(
            "SELECT ts.segment_id, ts.start, ts.end, ts.text, ts.speaker,
                    ts.person_id, ts.identify_confidence
             FROM transcript_search
             INNER JOIN transcript_segments AS ts ON ts.rowid = transcript_search.rowid
             WHERE transcript_search.meeting_id = ? AND transcript_search.version = ?
               AND transcript_search MATCH ?
             ORDER BY transcript_search.rank
             LIMIT ? OFFSET ?",
        )
        .bind(meeting_id)
        .bind(version)
        .bind(&fts_query)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.db)
        .await
        .context("Failed to search transcripts")?;

        rows.into_iter()
            .map(|row| {
                let start: f64 = row.try_get(1)?;
                Ok(TranscriptSegment {
                    id: row.try_get::<i64, _>(0)? as u32,
                    start,
                    end: row.try_get(2)?,
                    text: row.try_get(3)?,
                    timestamp: Some(crate::transcription::format_timestamp_readable(start)),
                    speaker: row.try_get(4)?,
                    tokens: None,
                    temperature: None,
                    avg_logprob: None,
                    compression_ratio: None,
                    no_speech_prob: None,
                    refined_text: None,
                    display_name: None,
                    person_id: row.try_get(5)?,
                    identify_confidence: row.try_get::<Option<f64>, _>(6)?.map(|v| v as f32),
                })
            })
            .collect()
    }

    /// List all transcript versions for a meeting.
    pub async fn list_transcript_versions(
        &self,
        meeting_id: &str,
    ) -> Result<Vec<TranscriptVersion>> {
        let rows = sqlx::query(
            "SELECT id, meeting_id, version, provider, model, language, segment_count, created_at
             FROM transcript_versions
             WHERE meeting_id = ?
             ORDER BY version DESC",
        )
        .bind(meeting_id)
        .fetch_all(&self.db)
        .await
        .context("Failed to fetch transcript versions")?;

        rows.into_iter()
            .map(|row| {
                Ok(TranscriptVersion {
                    id: row.try_get(0)?,
                    meeting_id: row.try_get(1)?,
                    version: row.try_get::<i64, _>(2)? as u32,
                    provider: row.try_get(3)?,
                    model: row.try_get(4)?,
                    language: row.try_get(5)?,
                    segment_count: row.try_get::<i64, _>(6)? as u32,
                    created_at: row.try_get(7)?,
                })
            })
            .collect()
    }

    /// Save audio file to meeting directory.
    pub async fn save_audio(&self, meeting_id: &str, audio_path: &PathBuf) -> Result<PathBuf> {
        self.get_meeting(meeting_id).await?;
        let file_name = audio_path.file_name().context("Invalid audio file path")?;
        let dest_path = self.meeting_dir(meeting_id).join("audio").join(file_name);

        std::fs::create_dir_all(dest_path.parent().unwrap())
            .context("Failed to create audio directory")?;
        std::fs::copy(audio_path, &dest_path).context("Failed to copy audio file")?;

        let file_name = file_name.to_string_lossy().to_string();
        sqlx::query("UPDATE meetings SET audio_file = ? WHERE id = ?")
            .bind(&file_name)
            .bind(meeting_id)
            .execute(&self.db)
            .await
            .context("Failed to update audio file")?;

        Ok(dest_path)
    }

    /// Save audio bytes to meeting directory.
    pub async fn save_audio_from_bytes(
        &self,
        meeting_id: &str,
        audio_bytes: &[u8],
        file_name: &str,
    ) -> Result<PathBuf> {
        self.get_meeting(meeting_id).await?;
        let dest_path = self.meeting_dir(meeting_id).join("audio").join(file_name);

        std::fs::create_dir_all(dest_path.parent().unwrap())
            .context("Failed to create audio directory")?;
        std::fs::write(&dest_path, audio_bytes).context("Failed to write audio file")?;

        sqlx::query("UPDATE meetings SET audio_file = ? WHERE id = ?")
            .bind(file_name)
            .bind(meeting_id)
            .execute(&self.db)
            .await
            .context("Failed to update audio file")?;

        Ok(dest_path)
    }

    /// Return saved recording path.
    pub async fn get_recording_path(&self, meeting_id: &str) -> Result<PathBuf> {
        let meeting = self.get_meeting(meeting_id).await?;
        let audio_file = meeting
            .audio_file
            .ok_or_else(|| anyhow::anyhow!("Recording not found for meeting: {}", meeting_id))?;
        let path = self.meeting_dir(meeting_id).join("audio").join(audio_file);
        if !path.exists() {
            anyhow::bail!("Recording not found for meeting: {}", meeting_id);
        }
        Ok(path)
    }

    /// Detect MIME type for saved recording.
    pub fn recording_mime_type(path: &Path) -> &'static str {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "mp3" => "audio/mpeg",
            "wav" => "audio/wav",
            "m4a" => "audio/mp4",
            "flac" => "audio/flac",
            "ogg" | "opus" => "audio/ogg",
            "webm" => "audio/webm",
            _ => "application/octet-stream",
        }
    }

    /// Mark meeting as completed with transcription info.
    pub async fn mark_transcription_complete(
        &self,
        meeting_id: &str,
        provider: &str,
        model: &str,
        duration_seconds: Option<u64>,
    ) -> Result<()> {
        let mut meeting = self.get_meeting(meeting_id).await?;
        meeting.status = MeetingStatus::Ready;
        meeting.duration_seconds = duration_seconds;
        meeting.transcription = Some(TranscriptionInfo {
            provider: provider.to_string(),
            model: model.to_string(),
            completed_at: Utc::now(),
        });
        meeting.updated_at = Utc::now();
        self.update_meeting(&meeting).await
    }

    /// Mark meeting as failed.
    pub async fn mark_transcription_failed(&self, meeting_id: &str) -> Result<()> {
        let mut meeting = self.get_meeting(meeting_id).await?;
        meeting.status = MeetingStatus::Failed;
        meeting.updated_at = Utc::now();
        self.update_meeting(&meeting).await
    }

    fn template_to_db(template: &SummaryTemplate) -> &'static str {
        match template {
            SummaryTemplate::KeyPoints => "keypoints",
            SummaryTemplate::ActionItems => "actionitems",
            SummaryTemplate::Decisions => "decisions",
            SummaryTemplate::Full => "full",
            SummaryTemplate::MeetingNotes => "meetingnotes",
        }
    }

    fn template_from_db(value: &str) -> Result<SummaryTemplate> {
        match value {
            "keypoints" => Ok(SummaryTemplate::KeyPoints),
            "actionitems" => Ok(SummaryTemplate::ActionItems),
            "decisions" => Ok(SummaryTemplate::Decisions),
            "full" => Ok(SummaryTemplate::Full),
            "meetingnotes" => Ok(SummaryTemplate::MeetingNotes),
            _ => anyhow::bail!("Invalid summary template in database: {}", value),
        }
    }

    fn format_to_db(format: &SummaryFormat) -> &'static str {
        match format {
            SummaryFormat::Markdown => "markdown",
            SummaryFormat::RawText => "rawtext",
        }
    }

    fn format_from_db(value: &str) -> Result<SummaryFormat> {
        match value {
            "markdown" => Ok(SummaryFormat::Markdown),
            "rawtext" => Ok(SummaryFormat::RawText),
            _ => anyhow::bail!("Invalid summary format in database: {}", value),
        }
    }

    fn status_to_db(status: &SummaryStatus) -> &'static str {
        match status {
            SummaryStatus::Pending => "pending",
            SummaryStatus::Processing => "processing",
            SummaryStatus::Completed => "completed",
            SummaryStatus::Failed => "failed",
        }
    }

    fn status_from_db(value: &str) -> Result<SummaryStatus> {
        match value {
            "pending" => Ok(SummaryStatus::Pending),
            "processing" => Ok(SummaryStatus::Processing),
            "completed" => Ok(SummaryStatus::Completed),
            "failed" => Ok(SummaryStatus::Failed),
            _ => anyhow::bail!("Invalid summary status in database: {}", value),
        }
    }

    fn row_to_summary(row: sqlx::sqlite::SqliteRow) -> Result<Summary> {
        let template: String = row.try_get(2)?;
        let format: String = row.try_get(3)?;
        let status: String = row.try_get(5)?;
        let key_points = row
            .try_get::<Option<String>, _>(7)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let action_items = row
            .try_get::<Option<String>, _>(8)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let decisions = row
            .try_get::<Option<String>, _>(9)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        Ok(Summary {
            id: row.try_get(0)?,
            meeting_id: row.try_get(1)?,
            template: Self::template_from_db(&template)?,
            format: Self::format_from_db(&format)?,
            language: row.try_get(4)?,
            status: Self::status_from_db(&status)?,
            content: row.try_get(6)?,
            key_points,
            action_items,
            decisions,
            provider: row.try_get(10)?,
            model: row.try_get(11)?,
            created_at: row.try_get(12)?,
            updated_at: row.try_get(13)?,
        })
    }

    /// Save a summary for a meeting.
    pub async fn save_summary(&self, meeting_id: &str, summary: &Summary) -> Result<()> {
        self.get_meeting(meeting_id).await?;
        sqlx::query(
            "INSERT INTO summaries (
                id, meeting_id, template, format, language, status, content,
                key_points, action_items, decisions, provider, model, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(meeting_id, template, format) DO UPDATE SET
                language = excluded.language,
                status = excluded.status,
                content = excluded.content,
                key_points = excluded.key_points,
                action_items = excluded.action_items,
                decisions = excluded.decisions,
                provider = excluded.provider,
                model = excluded.model,
                updated_at = excluded.updated_at",
        )
        .bind(&summary.id)
        .bind(meeting_id)
        .bind(Self::template_to_db(&summary.template))
        .bind(Self::format_to_db(&summary.format))
        .bind(summary.language.as_deref())
        .bind(Self::status_to_db(&summary.status))
        .bind(&summary.content)
        .bind(serde_json::to_string(&summary.key_points).unwrap())
        .bind(serde_json::to_string(&summary.action_items).unwrap())
        .bind(serde_json::to_string(&summary.decisions).unwrap())
        .bind(&summary.provider)
        .bind(&summary.model)
        .bind(summary.created_at)
        .bind(summary.updated_at)
        .execute(&self.db)
        .await
        .with_context(|| {
            format!(
                "Failed to save summary (meeting_id={}, template={:?}, format={:?})",
                meeting_id, summary.template, summary.format
            )
        })?;
        Ok(())
    }

    /// Get a specific summary by template and format for a meeting.
    pub async fn get_summary(
        &self,
        meeting_id: &str,
        template: SummaryTemplate,
        format: SummaryFormat,
    ) -> Result<Summary> {
        let row = sqlx::query(
            "SELECT id, meeting_id, template, format, language, status, content,
                    key_points, action_items, decisions, provider, model, created_at, updated_at
             FROM summaries WHERE meeting_id = ? AND template = ? AND format = ?",
        )
        .bind(meeting_id)
        .bind(Self::template_to_db(&template))
        .bind(Self::format_to_db(&format))
        .fetch_optional(&self.db)
        .await
        .context("Failed to query summary")?
        .ok_or_else(|| anyhow::anyhow!("Summary not found for meeting: {}", meeting_id))?;

        Self::row_to_summary(row)
    }

    /// List all summaries for a meeting.
    pub async fn list_summaries(&self, meeting_id: &str) -> Result<Vec<Summary>> {
        let rows = sqlx::query(
            "SELECT id, meeting_id, template, format, language, status, content,
                    key_points, action_items, decisions, provider, model, created_at, updated_at
             FROM summaries WHERE meeting_id = ? ORDER BY created_at ASC",
        )
        .bind(meeting_id)
        .fetch_all(&self.db)
        .await
        .context("Failed to query summaries")?;

        rows.into_iter().map(Self::row_to_summary).collect()
    }

    /// Delete a specific summary by template and format for a meeting.
    pub async fn delete_summary(
        &self,
        meeting_id: &str,
        template: SummaryTemplate,
        format: SummaryFormat,
    ) -> Result<()> {
        let result = sqlx::query("DELETE FROM summaries WHERE meeting_id = ? AND template = ? AND format = ?")
            .bind(meeting_id)
            .bind(Self::template_to_db(&template))
            .bind(Self::format_to_db(&format))
            .execute(&self.db)
            .await
            .context("Failed to delete summary")?;
        if result.rows_affected() == 0 {
            anyhow::bail!("Summary not found for meeting: {}", meeting_id);
        }
        Ok(())
    }

    // ── Voice bank ──────────────────────────────────────────────────────

    fn voiceprints_dir(&self) -> PathBuf {
        self.base.join("voiceprints")
    }

    fn person_samples_dir(&self, person_id: &str) -> PathBuf {
        self.voiceprints_dir().join(person_id).join("samples")
    }

    /// Relative path stored in DB: `voiceprints/{person_id}/samples/{sample_id}.wav`
    fn sample_relative_path(person_id: &str, sample_id: &str) -> String {
        format!("voiceprints/{person_id}/samples/{sample_id}.wav")
    }

    fn absolute_from_relative(&self, relative: &str) -> PathBuf {
        self.base.join(relative)
    }

    fn aliases_to_db(aliases: &[String]) -> Option<String> {
        if aliases.is_empty() {
            None
        } else {
            Some(serde_json::to_string(aliases).unwrap_or_else(|_| "[]".to_string()))
        }
    }

    fn aliases_from_db(raw: Option<String>) -> Vec<String> {
        raw.and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn enrolled_from_to_db(v: &VoiceprintEnrolledFrom) -> &'static str {
        match v {
            VoiceprintEnrolledFrom::Sample => "sample",
            VoiceprintEnrolledFrom::MeetingTurn => "meeting_turn",
        }
    }

    fn enrolled_from_from_db(s: &str) -> Result<VoiceprintEnrolledFrom> {
        match s {
            "sample" => Ok(VoiceprintEnrolledFrom::Sample),
            "meeting_turn" => Ok(VoiceprintEnrolledFrom::MeetingTurn),
            other => anyhow::bail!("unknown enrolled_from: {other}"),
        }
    }

    fn sample_source_to_db(v: &VoiceprintSampleSource) -> &'static str {
        match v {
            VoiceprintSampleSource::Upload => "upload",
            VoiceprintSampleSource::MeetingTurn => "meeting_turn",
        }
    }

    fn sample_source_from_db(s: &str) -> Result<VoiceprintSampleSource> {
        match s {
            "upload" => Ok(VoiceprintSampleSource::Upload),
            "meeting_turn" => Ok(VoiceprintSampleSource::MeetingTurn),
            other => anyhow::bail!("unknown sample source: {other}"),
        }
    }

    fn centroid_to_bytes(centroid: &[f32]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(centroid.len() * 4);
        for v in centroid {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes
    }

    fn centroid_from_bytes(bytes: &[u8], dim: u32) -> Result<Vec<f32>> {
        let expected = dim as usize * 4;
        if bytes.len() != expected {
            anyhow::bail!(
                "centroid blob size {} does not match dim {} (expected {} bytes)",
                bytes.len(),
                dim,
                expected
            );
        }
        let mut out = Vec::with_capacity(dim as usize);
        for chunk in bytes.chunks_exact(4) {
            out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        Ok(out)
    }

    fn row_to_person(row: sqlx::sqlite::SqliteRow) -> Result<Person> {
        Ok(Person {
            id: row.try_get(0)?,
            name: row.try_get(1)?,
            aliases: Self::aliases_from_db(row.try_get(2)?),
            created_at: row.try_get(3)?,
            updated_at: row.try_get(4)?,
        })
    }

    fn row_to_voiceprint(row: sqlx::sqlite::SqliteRow) -> Result<Voiceprint> {
        let dim: i64 = row.try_get(3)?;
        let centroid_blob: Vec<u8> = row.try_get(4)?;
        let enrolled: String = row.try_get(5)?;
        Ok(Voiceprint {
            id: row.try_get(0)?,
            person_id: row.try_get(1)?,
            model: row.try_get(2)?,
            dim: dim as u32,
            centroid: Self::centroid_from_bytes(&centroid_blob, dim as u32)?,
            enrolled_from: Self::enrolled_from_from_db(&enrolled)?,
            created_at: row.try_get(6)?,
            updated_at: row.try_get(7)?,
        })
    }

    fn row_to_sample(row: sqlx::sqlite::SqliteRow) -> Result<VoiceprintSample> {
        let source: String = row.try_get(5)?;
        let segment_ids = row
            .try_get::<Option<String>, _>(7)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Ok(VoiceprintSample {
            id: row.try_get(0)?,
            person_id: row.try_get(1)?,
            voiceprint_id: row.try_get(2)?,
            audio_path: row.try_get(3)?,
            duration_s: row.try_get(4)?,
            source: Self::sample_source_from_db(&source)?,
            meeting_id: row.try_get(6)?,
            segment_ids,
            created_at: row.try_get(8)?,
        })
    }

    /// Create a person in the voice bank.
    pub async fn create_person(&self, person: &Person) -> Result<()> {
        sqlx::query(
            "INSERT INTO persons (id, name, aliases, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&person.id)
        .bind(&person.name)
        .bind(Self::aliases_to_db(&person.aliases))
        .bind(person.created_at)
        .bind(person.updated_at)
        .execute(&self.db)
        .await
        .context("Failed to create person")?;

        let samples_dir = self.person_samples_dir(&person.id);
        std::fs::create_dir_all(&samples_dir)
            .with_context(|| format!("Failed to create samples dir {}", samples_dir.display()))?;
        Ok(())
    }

    /// Get a person by id.
    pub async fn get_person(&self, person_id: &str) -> Result<Person> {
        let row = sqlx::query(
            "SELECT id, name, aliases, created_at, updated_at FROM persons WHERE id = ?",
        )
        .bind(person_id)
        .fetch_optional(&self.db)
        .await
        .context("Failed to query person")?
        .ok_or_else(|| anyhow::anyhow!("Person not found: {person_id}"))?;
        Self::row_to_person(row)
    }

    /// List all persons ordered by name.
    pub async fn list_persons(&self) -> Result<Vec<Person>> {
        let rows = sqlx::query(
            "SELECT id, name, aliases, created_at, updated_at FROM persons ORDER BY name ASC",
        )
        .fetch_all(&self.db)
        .await
        .context("Failed to list persons")?;
        rows.into_iter().map(Self::row_to_person).collect()
    }

    /// Update person name/aliases.
    pub async fn update_person(
        &self,
        person_id: &str,
        name: &str,
        aliases: &[String],
    ) -> Result<()> {
        let result = sqlx::query("UPDATE persons SET name = ?, aliases = ? WHERE id = ?")
            .bind(name)
            .bind(Self::aliases_to_db(aliases))
            .bind(person_id)
            .execute(&self.db)
            .await
            .context("Failed to update person")?;
        if result.rows_affected() == 0 {
            anyhow::bail!("Person not found: {person_id}");
        }
        Ok(())
    }

    /// Delete person, cascade voiceprint/samples rows, remove disk dir.
    pub async fn delete_person(&self, person_id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM persons WHERE id = ?")
            .bind(person_id)
            .execute(&self.db)
            .await
            .context("Failed to delete person")?;
        if result.rows_affected() == 0 {
            anyhow::bail!("Person not found: {person_id}");
        }
        let dir = self.voiceprints_dir().join(person_id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .with_context(|| format!("Failed to remove voiceprint dir {}", dir.display()))?;
        }
        Ok(())
    }

    /// Upsert voiceprint centroid for a person (1:1 in v1).
    pub async fn upsert_voiceprint(&self, vp: &Voiceprint) -> Result<()> {
        self.get_person(&vp.person_id).await?;
        if vp.centroid.len() != vp.dim as usize {
            anyhow::bail!(
                "centroid len {} does not match dim {}",
                vp.centroid.len(),
                vp.dim
            );
        }
        let blob = Self::centroid_to_bytes(&vp.centroid);
        sqlx::query(
            "INSERT INTO voiceprints (id, person_id, model, dim, centroid, enrolled_from, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(person_id) DO UPDATE SET
               id = excluded.id,
               model = excluded.model,
               dim = excluded.dim,
               centroid = excluded.centroid,
               enrolled_from = excluded.enrolled_from,
               updated_at = excluded.updated_at",
        )
        .bind(&vp.id)
        .bind(&vp.person_id)
        .bind(&vp.model)
        .bind(vp.dim as i64)
        .bind(&blob)
        .bind(Self::enrolled_from_to_db(&vp.enrolled_from))
        .bind(vp.created_at)
        .bind(vp.updated_at)
        .execute(&self.db)
        .await
        .context("Failed to upsert voiceprint")?;
        Ok(())
    }

    /// Get voiceprint for a person, if any.
    pub async fn get_voiceprint(&self, person_id: &str) -> Result<Option<Voiceprint>> {
        let row = sqlx::query(
            "SELECT id, person_id, model, dim, centroid, enrolled_from, created_at, updated_at
             FROM voiceprints WHERE person_id = ?",
        )
        .bind(person_id)
        .fetch_optional(&self.db)
        .await
        .context("Failed to query voiceprint")?;
        match row {
            Some(r) => Ok(Some(Self::row_to_voiceprint(r)?)),
            None => Ok(None),
        }
    }

    /// Load all voiceprints for cosine matching.
    pub async fn list_voiceprints(&self) -> Result<Vec<Voiceprint>> {
        let rows = sqlx::query(
            "SELECT id, person_id, model, dim, centroid, enrolled_from, created_at, updated_at
             FROM voiceprints",
        )
        .fetch_all(&self.db)
        .await
        .context("Failed to list voiceprints")?;
        rows.into_iter().map(Self::row_to_voiceprint).collect()
    }

    /// Write sample audio to disk and insert metadata row.
    ///
    /// `audio_bytes` is the WAV (or other) payload. Returns the sample record.
    pub async fn add_voiceprint_sample(
        &self,
        person_id: &str,
        audio_bytes: &[u8],
        duration_s: f64,
        source: VoiceprintSampleSource,
        meeting_id: Option<&str>,
        segment_ids: &[u32],
    ) -> Result<VoiceprintSample> {
        self.get_person(person_id).await?;

        let sample_id = uuid::Uuid::new_v4().to_string();
        let relative = Self::sample_relative_path(person_id, &sample_id);
        let abs = self.absolute_from_relative(&relative);

        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        std::fs::write(&abs, audio_bytes)
            .with_context(|| format!("Failed to write sample {}", abs.display()))?;

        let segment_ids_json = if segment_ids.is_empty() {
            None
        } else {
            Some(serde_json::to_string(segment_ids)?)
        };
        let created_at = chrono::Utc::now();

        sqlx::query(
            "INSERT INTO voiceprint_samples (
                id, person_id, voiceprint_id, audio_path, duration_s, source,
                meeting_id, segment_ids, created_at
             ) VALUES (?, ?, NULL, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&sample_id)
        .bind(person_id)
        .bind(&relative)
        .bind(duration_s)
        .bind(Self::sample_source_to_db(&source))
        .bind(meeting_id)
        .bind(segment_ids_json.as_deref())
        .bind(created_at)
        .execute(&self.db)
        .await
        .context("Failed to insert voiceprint sample")?;

        Ok(VoiceprintSample {
            id: sample_id,
            person_id: person_id.to_string(),
            voiceprint_id: None,
            audio_path: relative,
            duration_s,
            source,
            meeting_id: meeting_id.map(|s| s.to_string()),
            segment_ids: segment_ids.to_vec(),
            created_at,
        })
    }

    /// List enrollment samples for a person.
    pub async fn list_voiceprint_samples(&self, person_id: &str) -> Result<Vec<VoiceprintSample>> {
        let rows = sqlx::query(
            "SELECT id, person_id, voiceprint_id, audio_path, duration_s, source,
                    meeting_id, segment_ids, created_at
             FROM voiceprint_samples WHERE person_id = ? ORDER BY created_at ASC",
        )
        .bind(person_id)
        .fetch_all(&self.db)
        .await
        .context("Failed to list voiceprint samples")?;
        rows.into_iter().map(Self::row_to_sample).collect()
    }

    /// Absolute path to a sample's audio file on disk.
    pub fn voiceprint_sample_abs_path(&self, sample: &VoiceprintSample) -> PathBuf {
        self.absolute_from_relative(&sample.audio_path)
    }

    /// Delete one sample (row + file). Does not rebuild centroid.
    pub async fn delete_voiceprint_sample(&self, sample_id: &str) -> Result<()> {
        let row = sqlx::query(
            "SELECT id, person_id, voiceprint_id, audio_path, duration_s, source,
                    meeting_id, segment_ids, created_at
             FROM voiceprint_samples WHERE id = ?",
        )
        .bind(sample_id)
        .fetch_optional(&self.db)
        .await
        .context("Failed to query sample")?
        .ok_or_else(|| anyhow::anyhow!("Voiceprint sample not found: {sample_id}"))?;
        let sample = Self::row_to_sample(row)?;

        sqlx::query("DELETE FROM voiceprint_samples WHERE id = ?")
            .bind(sample_id)
            .execute(&self.db)
            .await
            .context("Failed to delete voiceprint sample")?;

        let abs = self.absolute_from_relative(&sample.audio_path);
        if abs.exists() {
            let _ = std::fs::remove_file(&abs);
        }
        Ok(())
    }

    /// Link segments on the latest transcript version to a person_id (and optional confidence).
    ///
    /// `speaker_label` is the current diarization/display label to match
    /// (e.g. `SPEAKER_00` or `Alice`). Sets `person_id` and `identify_confidence`
    /// on matching rows; does not change `speaker` text.
    pub async fn set_segments_person(
        &self,
        meeting_id: &str,
        speaker_label: &str,
        person_id: Option<&str>,
        confidence: Option<f32>,
    ) -> Result<u64> {
        self.get_meeting(meeting_id).await?;
        if let Some(pid) = person_id {
            self.get_person(pid).await?;
        }

        let version: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(version), 0) FROM transcript_segments WHERE meeting_id = ?",
        )
        .bind(meeting_id)
        .fetch_one(&self.db)
        .await
        .context("Failed to resolve latest transcript version")?;

        if version == 0 {
            anyhow::bail!("Transcript not found for meeting: {meeting_id}");
        }

        let result = sqlx::query(
            "UPDATE transcript_segments
             SET person_id = ?, identify_confidence = ?
             WHERE meeting_id = ? AND version = ? AND speaker = ?",
        )
        .bind(person_id)
        .bind(confidence.map(|c| c as f64))
        .bind(meeting_id)
        .bind(version)
        .bind(speaker_label)
        .execute(&self.db)
        .await
        .context("Failed to set segment person_id")?;

        Ok(result.rows_affected())
    }

    /// Apply identify results on the latest transcript version.
    ///
    /// For each entry `(old_speaker_label → (display_name, person_id, confidence))`:
    /// updates `speaker`, `person_id`, and `identify_confidence` on matching rows.
    pub async fn apply_speaker_identities(
        &self,
        meeting_id: &str,
        assignments: &std::collections::HashMap<
            String,
            (String, Option<String>, Option<f32>),
        >,
    ) -> Result<u64> {
        self.get_meeting(meeting_id).await?;
        if assignments.is_empty() {
            return Ok(0);
        }

        let version: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(version), 0) FROM transcript_segments WHERE meeting_id = ?",
        )
        .bind(meeting_id)
        .fetch_one(&self.db)
        .await
        .context("Failed to resolve latest transcript version")?;

        if version == 0 {
            anyhow::bail!("Transcript not found for meeting: {meeting_id}");
        }

        let mut total: u64 = 0;
        for (old_label, (_display, person_id, confidence)) in assignments {
            if let Some(pid) = person_id.as_deref() {
                self.get_person(pid).await?;
            }
            let result = sqlx::query(
                "UPDATE transcript_segments
                 SET person_id = ?, identify_confidence = ?
                 WHERE meeting_id = ? AND version = ? AND speaker = ?",
            )
            .bind(person_id.as_deref())
            .bind(confidence.map(|c| c as f64))
            .bind(meeting_id)
            .bind(version)
            .bind(old_label)
            .execute(&self.db)
            .await
            .context("Failed to apply speaker identity")?;
            total += result.rows_affected();
        }
        Ok(total)
    }

    /// Clear person_id and identify_confidence from all segments in a meeting.
    /// Reverts to showing manual/diarization labels only.
    pub async fn clear_speaker_identification(&self, meeting_id: &str) -> Result<u64> {
        // Ensure meeting exists
        self.get_meeting(meeting_id).await?;

        let version: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(version), 0) FROM transcript_segments WHERE meeting_id = ?",
        )
        .bind(meeting_id)
        .fetch_one(&self.db)
        .await
        .context("Failed to resolve latest transcript version")?;

        if version == 0 {
            anyhow::bail!("Transcript not found for meeting: {}", meeting_id);
        }

        let result = sqlx::query(
            "UPDATE transcript_segments
             SET person_id = NULL, identify_confidence = NULL
             WHERE meeting_id = ? AND version = ?",
        )
        .bind(meeting_id)
        .bind(version)
        .execute(&self.db)
        .await
        .context("Failed to clear speaker identification")?;

        Ok(result.rows_affected())
    }

    // --- Orchestrator runs (idempotent live-bot import) ---

    pub async fn insert_orchestrator_run(&self, run: &OrchestratorRun) -> Result<()> {
        sqlx::query(
            "INSERT INTO orchestrator_runs (
                id, source, platform, native_meeting_id, recording_key, external_key,
                status, job_id, meeting_id, title, error, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&run.id)
        .bind(&run.source)
        .bind(run.platform.as_deref())
        .bind(run.native_meeting_id.as_deref())
        .bind(run.recording_key.as_deref())
        .bind(&run.external_key)
        .bind(run.status.as_str())
        .bind(run.job_id.as_deref())
        .bind(run.meeting_id.as_deref())
        .bind(run.title.as_deref())
        .bind(run.error.as_deref())
        .bind(run.created_at.to_rfc3339())
        .bind(run.updated_at.to_rfc3339())
        .execute(&self.db)
        .await
        .context("Failed to insert orchestrator_run")?;
        Ok(())
    }

    pub async fn get_orchestrator_run(&self, id: &str) -> Result<Option<OrchestratorRun>> {
        let row = sqlx::query(
            "SELECT id, source, platform, native_meeting_id, recording_key, external_key,
                    status, job_id, meeting_id, title, error, created_at, updated_at
             FROM orchestrator_runs WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.db)
        .await
        .context("Failed to get orchestrator_run")?;
        match row {
            Some(r) => Ok(Some(self.row_to_orchestrator_run(r)?)),
            None => Ok(None),
        }
    }

    pub async fn get_orchestrator_run_by_key(
        &self,
        external_key: &str,
    ) -> Result<Option<OrchestratorRun>> {
        let row = sqlx::query(
            "SELECT id, source, platform, native_meeting_id, recording_key, external_key,
                    status, job_id, meeting_id, title, error, created_at, updated_at
             FROM orchestrator_runs WHERE external_key = ?",
        )
        .bind(external_key)
        .fetch_optional(&self.db)
        .await
        .context("Failed to get orchestrator_run by key")?;
        match row {
            Some(r) => Ok(Some(self.row_to_orchestrator_run(r)?)),
            None => Ok(None),
        }
    }

    pub async fn update_orchestrator_run(&self, run: &OrchestratorRun) -> Result<()> {
        sqlx::query(
            "UPDATE orchestrator_runs SET
                status = ?, job_id = ?, meeting_id = ?, title = ?, error = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(run.status.as_str())
        .bind(run.job_id.as_deref())
        .bind(run.meeting_id.as_deref())
        .bind(run.title.as_deref())
        .bind(run.error.as_deref())
        .bind(run.updated_at.to_rfc3339())
        .bind(&run.id)
        .execute(&self.db)
        .await
        .context("Failed to update orchestrator_run")?;
        Ok(())
    }

    pub async fn set_orchestrator_run_status(
        &self,
        id: &str,
        status: OrchestratorRunStatus,
        job_id: Option<&str>,
        meeting_id: Option<&str>,
        error: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE orchestrator_runs SET
                status = ?,
                job_id = COALESCE(?, job_id),
                meeting_id = COALESCE(?, meeting_id),
                error = ?,
                updated_at = ?
             WHERE id = ?",
        )
        .bind(status.as_str())
        .bind(job_id)
        .bind(meeting_id)
        .bind(error)
        .bind(now)
        .bind(id)
        .execute(&self.db)
        .await
        .context("Failed to set orchestrator_run status")?;
        Ok(())
    }

    fn row_to_orchestrator_run(&self, row: sqlx::sqlite::SqliteRow) -> Result<OrchestratorRun> {
        let created_at: String = row.try_get(11)?;
        let updated_at: String = row.try_get(12)?;
        let status: String = row.try_get(6)?;
        Ok(OrchestratorRun {
            id: row.try_get(0)?,
            source: row.try_get(1)?,
            platform: row.try_get(2)?,
            native_meeting_id: row.try_get(3)?,
            recording_key: row.try_get(4)?,
            external_key: row.try_get(5)?,
            status: OrchestratorRunStatus::parse(&status),
            job_id: row.try_get(7)?,
            meeting_id: row.try_get(8)?,
            title: row.try_get(9)?,
            error: row.try_get(10)?,
            created_at: DateTime::parse_from_rfc3339(&created_at)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            updated_at: DateTime::parse_from_rfc3339(&updated_at)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        })
    }
}

impl std::fmt::Debug for MeetingStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeetingStorage")
            .field("base", &self.base)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MeetingStatus;
    use crate::transcription::{TranscriptSegment, TranscriptionResponse};
    use tempfile::TempDir;

    async fn setup() -> (TempDir, MeetingStorage) {
        let dir = TempDir::new().unwrap();
        let storage = MeetingStorage::in_memory(dir.path().to_path_buf())
            .await
            .unwrap();
        (dir, storage)
    }

    #[tokio::test]
    async fn orchestrator_run_insert_and_get_by_key() {
        use crate::orchestrator::{OrchestratorRun, OrchestratorRunStatus};
        let (_dir, storage) = setup().await;
        let run = OrchestratorRun {
            id: "run-1".into(),
            source: "vexa".into(),
            platform: Some("teams".into()),
            native_meeting_id: Some("abc".into()),
            recording_key: None,
            external_key: "vexa:teams:abc".into(),
            status: OrchestratorRunStatus::Received,
            job_id: None,
            meeting_id: None,
            title: Some("t".into()),
            error: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        storage.insert_orchestrator_run(&run).await.unwrap();
        let loaded = storage
            .get_orchestrator_run_by_key("vexa:teams:abc")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.id, "run-1");
        storage
            .set_orchestrator_run_status(
                "run-1",
                OrchestratorRunStatus::Completed,
                Some("job-1"),
                Some("meeting-1"),
                None,
            )
            .await
            .unwrap();
        let done = storage.get_orchestrator_run("run-1").await.unwrap().unwrap();
        assert_eq!(done.status, OrchestratorRunStatus::Completed);
        assert_eq!(done.job_id.as_deref(), Some("job-1"));
        assert_eq!(done.meeting_id.as_deref(), Some("meeting-1"));
    }

    fn segment(id: u32, start: f64, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            id,
            start,
            end: start + 5.0,
            text: text.to_string(),
            timestamp: None,
            tokens: None,
            temperature: None,
            avg_logprob: None,
            compression_ratio: None,
            no_speech_prob: None,
            speaker: None,
            display_name: None,
            person_id: None,
            identify_confidence: None,
            refined_text: None,
        }
    }

    async fn ready_meeting_with_transcript(
        storage: &MeetingStorage,
        title: &str,
        texts: &[&str],
    ) -> String {
        let mut meeting = Meeting::new(title.to_string());
        meeting.status = MeetingStatus::Ready;
        storage.create_meeting(&meeting).await.unwrap();

        let segments: Vec<TranscriptSegment> = texts
            .iter()
            .enumerate()
            .map(|(i, t)| segment(i as u32, (i as f64) * 10.0, t))
            .collect();

        let response = TranscriptionResponse {
            text: texts.join(" "),
            language: Some("en".to_string()),
            duration: Some(segments.last().map(|s| s.end).unwrap_or(0.0)),
            segments: Some(segments),
            refined_text: None,
        };

        storage
            .save_transcript(&meeting.id, &response, "test", "test-model", 60)
            .await
            .unwrap();

        meeting.id
    }

    #[tokio::test]
    async fn save_and_load_refined_text() {
        let (_dir, storage) = setup().await;
        let mut meeting = Meeting::new("Refined".to_string());
        meeting.status = MeetingStatus::Ready;
        storage.create_meeting(&meeting).await.unwrap();

        let mut seg = segment(0, 0.0, "raw uh text");
        seg.refined_text = Some("Raw text.".to_string());
        let response = TranscriptionResponse {
            text: "raw uh text".to_string(),
            language: Some("en".to_string()),
            duration: Some(5.0),
            segments: Some(vec![seg]),
            refined_text: Some("Raw text.".to_string()),
        };
        storage
            .save_transcript(&meeting.id, &response, "test", "test-model", 5)
            .await
            .unwrap();

        let loaded = storage.get_transcript(&meeting.id, None).await.unwrap();
        assert_eq!(loaded.text, "raw uh text");
        assert_eq!(loaded.refined_text.as_deref(), Some("Raw text."));
        let segs = loaded.segments.as_ref().unwrap();
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].refined_text.as_deref(), Some("Raw text."));
    }

    #[tokio::test]
    async fn voice_bank_person_sample_and_centroid() {
        let (dir, storage) = setup().await;

        let mut person = Person::new("Alice".to_string());
        person.aliases = vec!["Alicia".to_string()];
        storage.create_person(&person).await.unwrap();

        let loaded = storage.get_person(&person.id).await.unwrap();
        assert_eq!(loaded.name, "Alice");
        assert_eq!(loaded.aliases, vec!["Alicia".to_string()]);

        let listed = storage.list_persons().await.unwrap();
        assert_eq!(listed.len(), 1);

        let wav = b"RIFF....WAVEfmt "; // dummy bytes; path/meta only in Phase A
        let sample = storage
            .add_voiceprint_sample(
                &person.id,
                wav,
                32.0,
                VoiceprintSampleSource::Upload,
                None,
                &[],
            )
            .await
            .unwrap();
        assert!(sample.audio_path.starts_with("voiceprints/"));
        let abs = storage.voiceprint_sample_abs_path(&sample);
        assert!(abs.exists());
        assert_eq!(std::fs::read(&abs).unwrap(), wav);

        let samples = storage.list_voiceprint_samples(&person.id).await.unwrap();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].duration_s, 32.0);

        let centroid = vec![0.1f32, 0.2, 0.3, 0.4];
        let now = chrono::Utc::now();
        let vp = Voiceprint {
            id: uuid::Uuid::new_v4().to_string(),
            person_id: person.id.clone(),
            model: "test-embed".to_string(),
            dim: 4,
            centroid: centroid.clone(),
            enrolled_from: VoiceprintEnrolledFrom::Sample,
            created_at: now,
            updated_at: now,
        };
        storage.upsert_voiceprint(&vp).await.unwrap();
        let loaded_vp = storage.get_voiceprint(&person.id).await.unwrap().unwrap();
        assert_eq!(loaded_vp.dim, 4);
        assert_eq!(loaded_vp.centroid, centroid);
        assert_eq!(loaded_vp.model, "test-embed");

        let all = storage.list_voiceprints().await.unwrap();
        assert_eq!(all.len(), 1);

        // Cascade delete person removes sample file dir
        storage.delete_person(&person.id).await.unwrap();
        assert!(storage.get_person(&person.id).await.is_err());
        assert!(storage.get_voiceprint(&person.id).await.unwrap().is_none());
        assert!(storage.list_voiceprint_samples(&person.id).await.unwrap().is_empty());
        let person_dir = dir.path().join("voiceprints").join(&person.id);
        assert!(!person_dir.exists());
    }

    #[tokio::test]
    async fn set_segments_person_links_identity() {
        let (_dir, storage) = setup().await;
        let mut meeting = Meeting::new("ID link".to_string());
        meeting.status = MeetingStatus::Ready;
        storage.create_meeting(&meeting).await.unwrap();

        let mut s0 = segment(0, 0.0, "hello");
        s0.speaker = Some("SPEAKER_00".to_string());
        let mut s1 = segment(1, 5.0, "world");
        s1.speaker = Some("SPEAKER_01".to_string());
        let response = TranscriptionResponse {
            text: "hello world".to_string(),
            language: Some("en".to_string()),
            duration: Some(10.0),
            segments: Some(vec![s0, s1]),
            refined_text: None,
        };
        storage
            .save_transcript(&meeting.id, &response, "test", "test-model", 10)
            .await
            .unwrap();

        let person = Person::new("Alice".to_string());
        storage.create_person(&person).await.unwrap();

        let n = storage
            .set_segments_person(&meeting.id, "SPEAKER_00", Some(&person.id), Some(0.91))
            .await
            .unwrap();
        assert_eq!(n, 1);

        let loaded = storage.get_transcript(&meeting.id, None).await.unwrap();
        let segs = loaded.segments.as_ref().unwrap();
        assert_eq!(segs[0].person_id.as_deref(), Some(person.id.as_str()));
        assert!((segs[0].identify_confidence.unwrap() - 0.91).abs() < 1e-5);
        assert_eq!(segs[0].speaker.as_deref(), Some("SPEAKER_00"));
        assert!(segs[1].person_id.is_none());
    }

    #[tokio::test]
    async fn rename_speakers_updates_latest_version_only() {
        let (_dir, storage) = setup().await;
        let mut meeting = Meeting::new("Speakers".to_string());
        meeting.status = MeetingStatus::Ready;
        storage.create_meeting(&meeting).await.unwrap();

        let mut s0 = segment(0, 0.0, "hello from alice");
        s0.speaker = Some("SPEAKER_00".to_string());
        let mut s1 = segment(1, 5.0, "hello from bob");
        s1.speaker = Some("SPEAKER_01".to_string());
        let mut s2 = segment(2, 10.0, "alice again");
        s2.speaker = Some("SPEAKER_00".to_string());

        let response = TranscriptionResponse {
            text: "hello from alice hello from bob alice again".to_string(),
            language: Some("en".to_string()),
            duration: Some(15.0),
            segments: Some(vec![s0, s1, s2]),
            refined_text: None,
        };
        storage
            .save_transcript(&meeting.id, &response, "test", "test-model", 15)
            .await
            .unwrap();

        let mut mapping = std::collections::HashMap::new();
        mapping.insert("SPEAKER_00".to_string(), "Alice".to_string());
        mapping.insert("SPEAKER_01".to_string(), "Bob".to_string());

        let updated = storage
            .rename_speakers(&meeting.id, &mapping)
            .await
            .unwrap();
        assert_eq!(updated, 3);

        let loaded = storage.get_transcript(&meeting.id, None).await.unwrap();
        let segs = loaded.segments.as_ref().unwrap();
        assert_eq!(segs[0].speaker.as_deref(), Some("Alice"));
        assert_eq!(segs[1].speaker.as_deref(), Some("Bob"));
        assert_eq!(segs[2].speaker.as_deref(), Some("Alice"));
    }

    #[tokio::test]
    async fn search_all_returns_meetings_with_matches() {
        let (_dir, storage) = setup().await;
        let id1 = ready_meeting_with_transcript(
            &storage,
            "Planning",
            &["We discussed the product roadmap today"],
        )
        .await;
        let _id2 =
            ready_meeting_with_transcript(&storage, "Standup", &["Daily sync about bugs"]).await;
        let id3 = ready_meeting_with_transcript(
            &storage,
            "Review",
            &["Finalizing the roadmap for Q4"],
        )
        .await;

        let (results, total) = storage
            .search_all_transcripts("roadmap", 50, 0)
            .await
            .unwrap();

        assert_eq!(total, 2);
        assert_eq!(results.len(), 2);
        let ids: Vec<&str> = results.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&id1.as_str()));
        assert!(ids.contains(&id3.as_str()));
        for r in &results {
            assert!(!r.matched_segments.is_empty());
            assert!(r.match_count >= 1);
            assert_eq!(r.status, MeetingStatus::Ready);
        }
    }

    #[tokio::test]
    async fn search_all_excludes_non_ready_meetings() {
        let (_dir, storage) = setup().await;
        let mut meeting = Meeting::new("Importing".to_string());
        meeting.status = MeetingStatus::Importing;
        storage.create_meeting(&meeting).await.unwrap();

        let segments = vec![segment(0, 0.0, "roadmap discussion")];
        let response = TranscriptionResponse {
            text: "roadmap discussion".to_string(),
            language: None,
            duration: Some(5.0),
            segments: Some(segments),
            refined_text: None,
        };
        storage
            .save_transcript(&meeting.id, &response, "test", "test-model", 5)
            .await
            .unwrap();

        let (results, total) = storage
            .search_all_transcripts("roadmap", 50, 0)
            .await
            .unwrap();
        assert_eq!(total, 0);
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn search_all_pagination_by_meetings() {
        let (_dir, storage) = setup().await;
        for i in 0..3 {
            ready_meeting_with_transcript(
                &storage,
                &format!("M{i}"),
                &["unique_search_token appears here"],
            )
            .await;
        }

        let (page1, total) = storage
            .search_all_transcripts("unique_search_token", 2, 0)
            .await
            .unwrap();
        assert_eq!(total, 3);
        assert_eq!(page1.len(), 2);

        let (page2, _) = storage
            .search_all_transcripts("unique_search_token", 2, 2)
            .await
            .unwrap();
        assert_eq!(page2.len(), 1);
    }

    #[tokio::test]
    async fn search_all_empty_query_match_returns_empty() {
        let (_dir, storage) = setup().await;
        ready_meeting_with_transcript(&storage, "A", &["hello world"])
            .await;
        let (results, total) = storage
            .search_all_transcripts("zzzznonexistent", 50, 0)
            .await
            .unwrap();
        assert_eq!(total, 0);
        assert!(results.is_empty());
    }

    #[test]
    fn escape_fts5_quotes_special_chars() {
        assert_eq!(
            escape_fts5_query("C++").as_deref(),
            Some("\"C++\"")
        );
        assert_eq!(
            escape_fts5_query("hello-world").as_deref(),
            Some("\"hello-world\"")
        );
        assert_eq!(
            escape_fts5_query("what's up").as_deref(),
            Some("\"what's\" \"up\"")
        );
        assert_eq!(
            escape_fts5_query("foo:bar AND").as_deref(),
            Some("\"foo:bar\" \"AND\"")
        );
        assert_eq!(
            escape_fts5_query("say \"hi\"").as_deref(),
            Some("\"say\" \"\"\"hi\"\"\"")
        );
        assert_eq!(escape_fts5_query("   ").as_deref(), None);
        assert_eq!(escape_fts5_query("roadmap").as_deref(), Some("\"roadmap\""));
    }

    #[tokio::test]
    async fn search_all_accepts_plus_and_punctuation() {
        let (_dir, storage) = setup().await;
        let id = ready_meeting_with_transcript(
            &storage,
            "Lang",
            &["We use C++ and hello-world examples"],
        )
        .await;

        let (results, total) = storage
            .search_all_transcripts("C++", 50, 0)
            .await
            .expect("C++ must not fail FTS5");
        assert_eq!(total, 1);
        assert_eq!(results[0].id, id);

        let (results, total) = storage
            .search_all_transcripts("hello-world", 50, 0)
            .await
            .expect("hyphen must not fail FTS5");
        assert_eq!(total, 1);
        assert_eq!(results[0].id, id);

        let _ = storage
            .search_all_transcripts("what's", 50, 0)
            .await
            .expect("apostrophe must not fail FTS5");
    }
}
