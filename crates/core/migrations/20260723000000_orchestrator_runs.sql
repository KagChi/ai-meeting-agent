-- Idempotent orchestrator runs: one import per external meeting/recording key.
CREATE TABLE IF NOT EXISTS orchestrator_runs (
    id TEXT PRIMARY KEY NOT NULL,
    source TEXT NOT NULL DEFAULT 'vexa',
    platform TEXT,
    native_meeting_id TEXT,
    recording_key TEXT,
    external_key TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'received',
    job_id TEXT,
    meeting_id TEXT,
    title TEXT,
    error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_orchestrator_runs_status ON orchestrator_runs(status);
CREATE INDEX IF NOT EXISTS idx_orchestrator_runs_meeting ON orchestrator_runs(meeting_id);
