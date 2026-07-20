-- Add version support to transcript_segments for full transcript history

-- Drop existing FTS triggers and table
DROP TRIGGER IF EXISTS segments_ai;
DROP TRIGGER IF EXISTS segments_ad;
DROP TRIGGER IF EXISTS segments_au;
DROP TABLE IF EXISTS transcript_search;

-- Create temporary table with new schema
CREATE TABLE transcript_segments_new (
    meeting_id TEXT NOT NULL,
    version INTEGER NOT NULL,
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

    PRIMARY KEY (meeting_id, version, segment_id),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
);

-- Copy existing data (all segments are version 1)
INSERT INTO transcript_segments_new (
    meeting_id, version, segment_id, start, end, text, speaker,
    tokens, temperature, avg_logprob, compression_ratio, no_speech_prob
)
SELECT 
    meeting_id, 1 as version, segment_id, start, end, text, speaker,
    tokens, temperature, avg_logprob, compression_ratio, no_speech_prob
FROM transcript_segments;

-- Drop old table and rename new one
DROP TABLE transcript_segments;
ALTER TABLE transcript_segments_new RENAME TO transcript_segments;

-- Recreate indexes
CREATE INDEX IF NOT EXISTS idx_segments_meeting_version ON transcript_segments(meeting_id, version DESC);
CREATE INDEX IF NOT EXISTS idx_segments_speaker ON transcript_segments(meeting_id, version, speaker);

-- Recreate FTS5 table with version support
CREATE VIRTUAL TABLE IF NOT EXISTS transcript_search USING fts5(
    meeting_id UNINDEXED,
    version UNINDEXED,
    segment_id UNINDEXED,
    start UNINDEXED,
    end UNINDEXED,
    speaker UNINDEXED,
    text,
    content='transcript_segments',
    content_rowid='rowid',
    tokenize = 'unicode61 remove_diacritics 2'
);

-- Recreate triggers with version support
CREATE TRIGGER IF NOT EXISTS segments_ai AFTER INSERT ON transcript_segments BEGIN
    INSERT INTO transcript_search(rowid, meeting_id, version, segment_id, start, end, speaker, text)
    VALUES (new.rowid, new.meeting_id, new.version, new.segment_id, new.start, new.end, new.speaker, new.text);
END;

CREATE TRIGGER IF NOT EXISTS segments_ad AFTER DELETE ON transcript_segments BEGIN
    DELETE FROM transcript_search WHERE rowid = old.rowid;
END;

CREATE TRIGGER IF NOT EXISTS segments_au AFTER UPDATE ON transcript_segments BEGIN
    DELETE FROM transcript_search WHERE rowid = old.rowid;
    INSERT INTO transcript_search(rowid, meeting_id, version, segment_id, start, end, speaker, text)
    VALUES (new.rowid, new.meeting_id, new.version, new.segment_id, new.start, new.end, new.speaker, new.text);
END;
