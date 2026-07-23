import { Database } from "bun:sqlite";
import { config, ensureDataDirs } from "../config";
import type { BotJob, BotJobStatus, Platform } from "../types";

let db: Database | null = null;

export function getDb(): Database {
  if (!db) {
    ensureDataDirs();
    db = new Database(config.sqlitePath, { create: true });
    db.exec("PRAGMA journal_mode = WAL;");
    db.exec("PRAGMA foreign_keys = ON;");
    migrate(db);
    failStaleJobs(db);
  }
  return db;
}

function migrate(database: Database): void {
  database.exec(`
    CREATE TABLE IF NOT EXISTS bot_jobs (
      id TEXT PRIMARY KEY NOT NULL,
      platform TEXT NOT NULL,
      status TEXT NOT NULL,
      meeting_url TEXT,
      native_meeting_id TEXT,
      bot_name TEXT,
      title TEXT,
      recording_path TEXT,
      meeting_agent_job_id TEXT,
      error TEXT,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_bot_jobs_status ON bot_jobs(status);
    CREATE INDEX IF NOT EXISTS idx_bot_jobs_platform ON bot_jobs(platform);
  `);
}

/** Mark in-flight jobs failed after process restart (v1: no resume). */
function failStaleJobs(database: Database): void {
  const now = new Date().toISOString();
  database
    .prepare(
      `UPDATE bot_jobs SET status = 'failed', error = ?, updated_at = ?
       WHERE status IN ('queued','joining','in_call','recording','uploading')`,
    )
    .run("process restarted", now);
}

function rowToJob(row: Record<string, unknown>): BotJob {
  return {
    id: String(row.id),
    platform: row.platform as Platform,
    status: row.status as BotJobStatus,
    meeting_url: (row.meeting_url as string) ?? null,
    native_meeting_id: (row.native_meeting_id as string) ?? null,
    bot_name: (row.bot_name as string) ?? null,
    title: (row.title as string) ?? null,
    recording_path: (row.recording_path as string) ?? null,
    meeting_agent_job_id: (row.meeting_agent_job_id as string) ?? null,
    error: (row.error as string) ?? null,
    created_at: String(row.created_at),
    updated_at: String(row.updated_at),
  };
}

export function insertJob(job: BotJob): void {
  getDb()
    .prepare(
      `INSERT INTO bot_jobs (
        id, platform, status, meeting_url, native_meeting_id, bot_name, title,
        recording_path, meeting_agent_job_id, error, created_at, updated_at
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    )
    .run(
      job.id,
      job.platform,
      job.status,
      job.meeting_url,
      job.native_meeting_id,
      job.bot_name,
      job.title,
      job.recording_path,
      job.meeting_agent_job_id,
      job.error,
      job.created_at,
      job.updated_at,
    );
}

export function getJob(id: string): BotJob | null {
  const row = getDb().prepare("SELECT * FROM bot_jobs WHERE id = ?").get(id) as
    | Record<string, unknown>
    | undefined;
  return row ? rowToJob(row) : null;
}

export function listJobs(limit = 50, status?: BotJobStatus): BotJob[] {
  if (status) {
    const rows = getDb()
      .prepare(
        "SELECT * FROM bot_jobs WHERE status = ? ORDER BY created_at DESC LIMIT ?",
      )
      .all(status, limit) as Record<string, unknown>[];
    return rows.map(rowToJob);
  }
  const rows = getDb()
    .prepare("SELECT * FROM bot_jobs ORDER BY created_at DESC LIMIT ?")
    .all(limit) as Record<string, unknown>[];
  return rows.map(rowToJob);
}

export function updateJob(
  id: string,
  patch: Partial<
    Pick<
      BotJob,
      | "status"
      | "recording_path"
      | "meeting_agent_job_id"
      | "error"
      | "meeting_url"
      | "title"
    >
  >,
): BotJob | null {
  const current = getJob(id);
  if (!current) return null;
  const next: BotJob = {
    ...current,
    ...patch,
    updated_at: new Date().toISOString(),
  };
  getDb()
    .prepare(
      `UPDATE bot_jobs SET
        status = ?, recording_path = ?, meeting_agent_job_id = ?, error = ?,
        meeting_url = ?, title = ?, updated_at = ?
       WHERE id = ?`,
    )
    .run(
      next.status,
      next.recording_path,
      next.meeting_agent_job_id,
      next.error,
      next.meeting_url,
      next.title,
      next.updated_at,
      id,
    );
  return next;
}

export function countActiveJobs(): number {
  const row = getDb()
    .prepare(
      `SELECT COUNT(*) AS c FROM bot_jobs
       WHERE status IN ('queued','joining','in_call','recording','uploading')`,
    )
    .get() as { c: number };
  return row.c;
}
