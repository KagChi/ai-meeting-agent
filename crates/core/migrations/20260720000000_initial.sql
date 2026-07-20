-- Initial SQLite schema for ai-meeting-agent.

CREATE TABLE IF NOT EXISTS meetings (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    date TIMESTAMP NOT NULL,
    duration_seconds INTEGER,
    status TEXT NOT NULL CHECK(status IN ('importing', 'ready', 'failed')),

    transcription_provider TEXT,
    transcription_model TEXT,
    transcription_completed_at TIMESTAMP,

    participants TEXT,
    location TEXT,
    organizer TEXT,
    metadata_source TEXT CHECK(metadata_source IN ('userprovided', 'calendarbot', 'filename', 'ffprobe', 'default')),
    recording_date TIMESTAMP,
    platform TEXT,

    file_metadata TEXT,
    audio_file TEXT,

    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_meetings_date ON meetings(date DESC);
CREATE INDEX IF NOT EXISTS idx_meetings_status ON meetings(status);
CREATE INDEX IF NOT EXISTS idx_meetings_created_at ON meetings(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_meetings_updated_at ON meetings(updated_at DESC);

CREATE TABLE IF NOT EXISTS transcript_segments (
    meeting_id TEXT NOT NULL,
    segment_id INTEGER NOT NULL,
    start REAL NOT NULL,
    end REAL NOT NULL,
    text TEXT NOT NULL,
    speaker TEXT,

    tokens TEXT,
    temperature REAL,
    avg_logprob REAL,
    compression_ratio REAL,
    no_speech_prob REAL,

    PRIMARY KEY (meeting_id, segment_id),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_segments_meeting ON transcript_segments(meeting_id);
CREATE INDEX IF NOT EXISTS idx_segments_speaker ON transcript_segments(meeting_id, speaker);

CREATE VIRTUAL TABLE IF NOT EXISTS transcript_search USING fts5(
    meeting_id UNINDEXED,
    segment_id UNINDEXED,
    start UNINDEXED,
    end UNINDEXED,
    speaker UNINDEXED,
    text,
    content='transcript_segments',
    content_rowid='rowid',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS segments_ai AFTER INSERT ON transcript_segments BEGIN
    INSERT INTO transcript_search(rowid, meeting_id, segment_id, start, end, speaker, text)
    VALUES (new.rowid, new.meeting_id, new.segment_id, new.start, new.end, new.speaker, new.text);
END;

CREATE TRIGGER IF NOT EXISTS segments_ad AFTER DELETE ON transcript_segments BEGIN
    DELETE FROM transcript_search WHERE rowid = old.rowid;
END;

CREATE TRIGGER IF NOT EXISTS segments_au AFTER UPDATE ON transcript_segments BEGIN
    DELETE FROM transcript_search WHERE rowid = old.rowid;
    INSERT INTO transcript_search(rowid, meeting_id, segment_id, start, end, speaker, text)
    VALUES (new.rowid, new.meeting_id, new.segment_id, new.start, new.end, new.speaker, new.text);
END;

CREATE TABLE IF NOT EXISTS summaries (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL,
    template TEXT NOT NULL CHECK(template IN ('keypoints', 'actionitems', 'decisions', 'full')),
    language TEXT,
    status TEXT NOT NULL CHECK(status IN ('pending', 'processing', 'completed', 'failed')),

    content TEXT NOT NULL,
    key_points TEXT,
    action_items TEXT,
    decisions TEXT,

    provider TEXT NOT NULL,
    model TEXT NOT NULL,

    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,

    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE,
    UNIQUE(meeting_id, template)
);

CREATE INDEX IF NOT EXISTS idx_summaries_meeting ON summaries(meeting_id);
CREATE INDEX IF NOT EXISTS idx_summaries_status ON summaries(status);

CREATE TABLE IF NOT EXISTS transcript_versions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    meeting_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    language TEXT,
    segment_count INTEGER NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,

    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_transcript_versions_meeting ON transcript_versions(meeting_id, version DESC);

CREATE TRIGGER IF NOT EXISTS meetings_update_timestamp
AFTER UPDATE ON meetings
BEGIN
    UPDATE meetings SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS summaries_update_timestamp
AFTER UPDATE ON summaries
BEGIN
    UPDATE summaries SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
END;
