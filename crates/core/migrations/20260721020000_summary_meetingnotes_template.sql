-- Allow MeetingNotes summary template and keep format unique key.
-- SQLite cannot alter CHECK constraints in place; recreate table.

CREATE TABLE summaries_new (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL,
    template TEXT NOT NULL CHECK(template IN ('keypoints', 'actionitems', 'decisions', 'full', 'meetingnotes')),
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

-- format column may be missing on very old DBs; COALESCE for safety when present.
INSERT INTO summaries_new (
    id, meeting_id, template, format, language, status, content,
    key_points, action_items, decisions, provider, model, created_at, updated_at
)
SELECT
    id,
    meeting_id,
    template,
    COALESCE(format, 'markdown'),
    language,
    status,
    content,
    key_points,
    action_items,
    decisions,
    provider,
    model,
    created_at,
    updated_at
FROM summaries;

DROP TABLE summaries;
ALTER TABLE summaries_new RENAME TO summaries;

CREATE INDEX IF NOT EXISTS idx_summaries_meeting ON summaries(meeting_id);
CREATE INDEX IF NOT EXISTS idx_summaries_status ON summaries(status);
