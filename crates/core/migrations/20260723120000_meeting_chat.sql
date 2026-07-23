-- In-meeting chat (Teams/Zoom/…) for LLM summary context
CREATE TABLE IF NOT EXISTS meeting_chat_messages (
    meeting_id TEXT NOT NULL,
    message_id INTEGER NOT NULL,
    sent_at TEXT,
    author TEXT,
    body TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'teams',
    PRIMARY KEY (meeting_id, message_id),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_chat_meeting
    ON meeting_chat_messages(meeting_id, message_id);
