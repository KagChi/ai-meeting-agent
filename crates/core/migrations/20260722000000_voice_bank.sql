-- Voice bank: persons, voiceprints (centroid embeddings), enrollment samples.
-- Sample audio lives on disk; DB stores paths + identity index.
--
-- Note: transcript_segments.person_id / identify_confidence are added in
-- `db::ensure_voice_bank_segment_columns` (idempotent) so re-runs and
-- concurrent migrators do not fail on duplicate column.

CREATE TABLE IF NOT EXISTS persons (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    aliases TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_persons_name ON persons(name);

CREATE TABLE IF NOT EXISTS voiceprints (
    id TEXT PRIMARY KEY,
    person_id TEXT NOT NULL UNIQUE,
    model TEXT NOT NULL,
    dim INTEGER NOT NULL,
    centroid BLOB NOT NULL,
    enrolled_from TEXT NOT NULL CHECK(enrolled_from IN ('sample', 'meeting_turn')),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (person_id) REFERENCES persons(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_voiceprints_person ON voiceprints(person_id);

CREATE TABLE IF NOT EXISTS voiceprint_samples (
    id TEXT PRIMARY KEY,
    person_id TEXT NOT NULL,
    voiceprint_id TEXT,
    audio_path TEXT NOT NULL,
    duration_s REAL NOT NULL,
    source TEXT NOT NULL CHECK(source IN ('upload', 'meeting_turn')),
    meeting_id TEXT,
    segment_ids TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (person_id) REFERENCES persons(id) ON DELETE CASCADE,
    FOREIGN KEY (voiceprint_id) REFERENCES voiceprints(id) ON DELETE SET NULL,
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_samples_person ON voiceprint_samples(person_id);

CREATE TRIGGER IF NOT EXISTS persons_update_timestamp
AFTER UPDATE ON persons
BEGIN
    UPDATE persons SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS voiceprints_update_timestamp
AFTER UPDATE ON voiceprints
BEGIN
    UPDATE voiceprints SET updated_at = CURRENT_TIMESTAMP WHERE id = NEW.id;
END;
