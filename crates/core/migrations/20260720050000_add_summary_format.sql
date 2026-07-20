-- Add format column to summaries table for output format support (markdown, raw_text).
-- SQLite doesn't support modifying constraints, so we recreate the table.

-- Create new table with format column and updated UNIQUE constraint
CREATE TABLE summaries_new (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL,
    template TEXT NOT NULL CHECK(template IN ('keypoints', 'actionitems', 'decisions', 'full')),
    format TEXT NOT NULL DEFAULT 'markdown' CHECK(format IN ('markdown', 'rawtext')),
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
    UNIQUE(meeting_id, template, format)
);

-- Copy existing data (all existing summaries are markdown format)
INSERT INTO summaries_new (
    id, meeting_id, template, format, language, status, content,
    key_points, action_items, decisions, provider, model, created_at, updated_at
)
SELECT 
    id, meeting_id, template, 'markdown', language, status, content,
    key_points, action_items, decisions, provider, model, created_at, updated_at
FROM summaries;

-- Drop old table
DROP TABLE summaries;

-- Rename new table
ALTER TABLE summaries_new RENAME TO summaries;

-- Recreate indexes
CREATE INDEX idx_summaries_meeting ON summaries(meeting_id);
CREATE INDEX idx_summaries_status ON summaries(status);
