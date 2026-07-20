//! Meeting storage operations
//!
//! SQLite-based storage for meetings, transcripts, summaries, and audio files.

use crate::db;
use crate::fs;
use crate::models::{
    FileMetadata, MatchedSegment, Meeting, MeetingSearchResult, MeetingStatus, MetadataSource,
    Summary, SummaryFormat, SummaryStatus, SummaryTemplate, TranscriptionInfo, TranscriptVersion,
};
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
                        tokens, temperature, avg_logprob, compression_ratio, no_speech_prob
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
            "INSERT INTO transcript_versions (meeting_id, version, provider, model, language, segment_count)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(meeting_id)
        .bind(version)
        .bind(provider)
        .bind(model)
        .bind(response.language.as_deref())
        .bind(response.segments.as_ref().map(|s| s.len()).unwrap_or(0) as i64)
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

        Ok(TranscriptionResponse {
            text,
            language: None,
            duration,
            segments: Some(segments),
            refined_text: None,
        })
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
            "SELECT segment_id, start, end, text, speaker, tokens, temperature,
                    avg_logprob, compression_ratio, no_speech_prob
             FROM transcript_segments
             WHERE meeting_id = ? {}
             ORDER BY segment_id ASC
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
                "SELECT segment_id, start, end, text, speaker
                 FROM transcript_search
                 WHERE meeting_id = ? AND version = ? AND transcript_search MATCH ?
                 ORDER BY rank
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
            "SELECT segment_id, start, end, text, speaker
             FROM transcript_search
             WHERE meeting_id = ? AND version = ? AND transcript_search MATCH ?
             ORDER BY rank
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
        }
    }

    fn template_from_db(value: &str) -> Result<SummaryTemplate> {
        match value {
            "keypoints" => Ok(SummaryTemplate::KeyPoints),
            "actionitems" => Ok(SummaryTemplate::ActionItems),
            "decisions" => Ok(SummaryTemplate::Decisions),
            "full" => Ok(SummaryTemplate::Full),
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
        .context("Failed to save summary")?;
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
