//! Meeting storage operations
//!
//! Handles file system operations for meetings, transcripts, and audio files.

use crate::fs;
use crate::models::{Meeting, MeetingStatus, Summary, SummaryTemplate, TranscriptionInfo};
use crate::transcription::TranscriptionResponse;
use anyhow::{Context, Result};
use chrono::Utc;
use std::path::PathBuf;

/// Meeting storage manager
pub struct MeetingStorage;

impl MeetingStorage {
    /// Create a new meeting with metadata
    pub fn create_meeting(&self, meeting: &Meeting) -> Result<()> {
        let meeting_path = fs::meeting_dir(&meeting.id)?;
        std::fs::create_dir_all(&meeting_path).context("Failed to create meeting directory")?;

        let meeting_json = serde_json::to_string_pretty(&meeting)
            .context("Failed to serialize meeting metadata")?;
        std::fs::write(meeting_path.join("meeting.json"), meeting_json)
            .context("Failed to write meeting metadata")?;

        Ok(())
    }

    /// Get meeting by ID
    pub fn get_meeting(&self, meeting_id: &str) -> Result<Meeting> {
        let meeting_path = fs::meeting_dir(meeting_id)?;
        let meeting_file = meeting_path.join("meeting.json");

        if !meeting_file.exists() {
            anyhow::bail!("Meeting not found: {}", meeting_id);
        }

        let meeting_json =
            std::fs::read_to_string(&meeting_file).context("Failed to read meeting metadata")?;
        let meeting: Meeting =
            serde_json::from_str(&meeting_json).context("Failed to parse meeting metadata")?;

        Ok(meeting)
    }

    /// Update meeting metadata
    pub fn update_meeting(&self, meeting: &Meeting) -> Result<()> {
        let meeting_path = fs::meeting_dir(&meeting.id)?;
        let meeting_file = meeting_path.join("meeting.json");

        if !meeting_file.exists() {
            anyhow::bail!("Meeting not found: {}", meeting.id);
        }

        let meeting_json = serde_json::to_string_pretty(&meeting)
            .context("Failed to serialize meeting metadata")?;
        std::fs::write(meeting_file, meeting_json).context("Failed to write meeting metadata")?;

        Ok(())
    }

    /// Delete meeting and all associated files
    pub fn delete_meeting(&self, meeting_id: &str) -> Result<()> {
        let meeting_path = fs::meeting_dir(meeting_id)?;

        if !meeting_path.exists() {
            anyhow::bail!("Meeting not found: {}", meeting_id);
        }

        std::fs::remove_dir_all(&meeting_path).context("Failed to delete meeting directory")?;

        Ok(())
    }

    /// List all meetings
    pub fn list_meetings(&self) -> Result<Vec<Meeting>> {
        let meetings_path = fs::meetings_dir()?;

        if !meetings_path.exists() {
            return Ok(vec![]);
        }

        let mut meetings = Vec::new();

        for entry in
            std::fs::read_dir(&meetings_path).context("Failed to read meetings directory")?
        {
            let entry = entry.context("Failed to read directory entry")?;
            let path = entry.path();

            if path.is_dir() {
                let meeting_file = path.join("meeting.json");
                if meeting_file.exists() {
                    if let Ok(meeting_json) = std::fs::read_to_string(&meeting_file) {
                        if let Ok(meeting) = serde_json::from_str::<Meeting>(&meeting_json) {
                            meetings.push(meeting);
                        }
                    }
                }
            }
        }

        // Sort by date (most recent first)
        meetings.sort_by_key(|m| std::cmp::Reverse(m.date));

        Ok(meetings)
    }

    /// Save transcript data
    pub fn save_transcript(
        &self,
        meeting_id: &str,
        response: &TranscriptionResponse,
    ) -> Result<()> {
        let meeting_path = fs::meeting_dir(meeting_id)?;

        if !meeting_path.exists() {
            anyhow::bail!("Meeting not found: {}", meeting_id);
        }

        let transcript_json =
            serde_json::to_string_pretty(&response).context("Failed to serialize transcript")?;
        std::fs::write(meeting_path.join("transcript.json"), transcript_json)
            .context("Failed to write transcript")?;

        Ok(())
    }

    /// Get transcript for a meeting
    pub fn get_transcript(&self, meeting_id: &str) -> Result<TranscriptionResponse> {
        let meeting_path = fs::meeting_dir(meeting_id)?;
        let transcript_file = meeting_path.join("transcript.json");

        if !transcript_file.exists() {
            anyhow::bail!("Transcript not found for meeting: {}", meeting_id);
        }

        let transcript_json =
            std::fs::read_to_string(&transcript_file).context("Failed to read transcript")?;
        let transcript: TranscriptionResponse =
            serde_json::from_str(&transcript_json).context("Failed to parse transcript")?;

        Ok(transcript)
    }

    /// Save audio file to meeting directory
    pub fn save_audio(&self, meeting_id: &str, audio_path: &PathBuf) -> Result<PathBuf> {
        let meeting_path = fs::meeting_dir(meeting_id)?;

        if !meeting_path.exists() {
            anyhow::bail!("Meeting not found: {}", meeting_id);
        }

        let file_name = audio_path.file_name().context("Invalid audio file path")?;
        let dest_path = meeting_path.join("audio").join(file_name);

        // Create audio subdirectory
        std::fs::create_dir_all(dest_path.parent().unwrap())
            .context("Failed to create audio directory")?;

        // Copy audio file
        std::fs::copy(audio_path, &dest_path).context("Failed to copy audio file")?;

        Ok(dest_path)
    }

    /// Mark meeting as completed with transcription info
    pub fn mark_transcription_complete(
        &self,
        meeting_id: &str,
        provider: &str,
        model: &str,
        duration_seconds: Option<u64>,
    ) -> Result<()> {
        let mut meeting = self.get_meeting(meeting_id)?;

        meeting.status = MeetingStatus::Ready;
        meeting.duration_seconds = duration_seconds;
        meeting.transcription = Some(TranscriptionInfo {
            provider: provider.to_string(),
            model: model.to_string(),
            completed_at: Utc::now(),
        });
        meeting.updated_at = Utc::now();

        self.update_meeting(&meeting)?;

        Ok(())
    }

    /// Mark meeting as failed
    pub fn mark_transcription_failed(&self, meeting_id: &str) -> Result<()> {
        let mut meeting = self.get_meeting(meeting_id)?;

        meeting.status = MeetingStatus::Failed;
        meeting.updated_at = Utc::now();

        self.update_meeting(&meeting)?;

        Ok(())
    }

    fn summaries_dir(meeting_id: &str) -> Result<PathBuf> {
        let meeting_path = fs::meeting_dir(meeting_id)?;
        Ok(meeting_path.join("summaries"))
    }

    fn summary_file_name(template: SummaryTemplate) -> String {
        let name = match template {
            SummaryTemplate::KeyPoints => "key_points",
            SummaryTemplate::ActionItems => "action_items",
            SummaryTemplate::Decisions => "decisions",
            SummaryTemplate::Full => "full",
        };
        format!("{name}.json")
    }

    /// Save a summary for a meeting. Stored at `meetings/{id}/summaries/{template}.json`.
    pub fn save_summary(&self, meeting_id: &str, summary: &Summary) -> Result<()> {
        let dir = Self::summaries_dir(meeting_id)?;
        std::fs::create_dir_all(&dir).context("Failed to create summaries directory")?;

        let path = dir.join(Self::summary_file_name(summary.template.clone()));
        let json = serde_json::to_string_pretty(summary).context("Failed to serialize summary")?;
        std::fs::write(&path, json).context("Failed to write summary")?;

        Ok(())
    }

    /// Get a specific summary by template for a meeting.
    pub fn get_summary(&self, meeting_id: &str, template: SummaryTemplate) -> Result<Summary> {
        let path = Self::summaries_dir(meeting_id)?.join(Self::summary_file_name(template));
        if !path.exists() {
            anyhow::bail!("Summary not found for meeting: {}", meeting_id);
        }
        let json = std::fs::read_to_string(&path).context("Failed to read summary")?;
        let summary: Summary = serde_json::from_str(&json).context("Failed to parse summary")?;
        Ok(summary)
    }

    /// List all summaries for a meeting.
    pub fn list_summaries(&self, meeting_id: &str) -> Result<Vec<Summary>> {
        let dir = Self::summaries_dir(meeting_id)?;
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut summaries = Vec::new();
        for entry in std::fs::read_dir(&dir).context("Failed to read summaries directory")? {
            let entry = entry.context("Failed to read directory entry")?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json = std::fs::read_to_string(&path).context("Failed to read summary file")?;
            let summary: Summary =
                serde_json::from_str(&json).context("Failed to parse summary")?;
            summaries.push(summary);
        }

        summaries.sort_by_key(|a| a.created_at);
        Ok(summaries)
    }

    /// Delete a specific summary by template for a meeting.
    pub fn delete_summary(&self, meeting_id: &str, template: SummaryTemplate) -> Result<()> {
        let path = Self::summaries_dir(meeting_id)?.join(Self::summary_file_name(template));
        if !path.exists() {
            anyhow::bail!("Summary not found for meeting: {}", meeting_id);
        }
        std::fs::remove_file(&path).context("Failed to delete summary")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Meeting;

    #[test]
    fn test_create_and_get_meeting() {
        let storage = MeetingStorage;
        let meeting = Meeting::new("Test Meeting".to_string());

        // Ensure data directory exists
        fs::ensure_data_dir().unwrap();

        // Create meeting
        storage.create_meeting(&meeting).unwrap();

        // Get meeting
        let retrieved = storage.get_meeting(&meeting.id).unwrap();
        assert_eq!(retrieved.id, meeting.id);
        assert_eq!(retrieved.title, meeting.title);

        // Cleanup
        storage.delete_meeting(&meeting.id).unwrap();
    }

    #[test]
    fn test_list_meetings() {
        let storage = MeetingStorage;
        fs::ensure_data_dir().unwrap();

        let meeting1 = Meeting::new("Meeting 1".to_string());
        let meeting2 = Meeting::new("Meeting 2".to_string());

        storage.create_meeting(&meeting1).unwrap();
        storage.create_meeting(&meeting2).unwrap();

        let meetings = storage.list_meetings().unwrap();
        assert!(meetings.len() >= 2);

        // Cleanup
        storage.delete_meeting(&meeting1.id).unwrap();
        storage.delete_meeting(&meeting2.id).unwrap();
    }
}
