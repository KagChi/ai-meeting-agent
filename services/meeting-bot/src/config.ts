import { mkdirSync } from "node:fs";
import { join } from "node:path";

function envBool(name: string, fallback: boolean): boolean {
  const v = process.env[name];
  if (v === undefined || v === "") return fallback;
  return ["1", "true", "yes", "on"].includes(v.toLowerCase());
}

function envInt(name: string, fallback: number): number {
  const v = process.env[name];
  if (!v) return fallback;
  const n = Number.parseInt(v, 10);
  return Number.isFinite(n) ? n : fallback;
}

const dataDir = process.env.DATA_DIR?.trim() || join(process.cwd(), "data");
const recordingsDir = join(dataDir, "recordings");
const sqlitePath =
  process.env.SQLITE_PATH?.trim() || join(dataDir, "meeting-bot.db");

export const config = {
  port: envInt("BOT_PORT", 8091),
  host: process.env.BOT_HOST?.trim() || "0.0.0.0",
  /** Shared secret for agent → bot (optional). Header X-API-Key. */
  apiKey: process.env.BOT_API_KEY?.trim() || process.env.MEETING_BOT_INTERNAL_KEY?.trim() || "",
  defaultBotName: process.env.BOT_NAME?.trim() || "BMW-Lab-Bot",
  dataDir,
  recordingsDir,
  sqlitePath,
  keepRecording: envBool("KEEP_RECORDING", true),
  meetingAgentUrl: (process.env.MEETING_AGENT_URL?.trim() || "http://127.0.0.1:8080").replace(
    /\/$/,
    "",
  ),
  meetingAgentApiKey: process.env.MEETING_AGENT_API_KEY?.trim() || "",
  joinTimeoutMs: envInt("JOIN_TIMEOUT_MS", 900_000),
  aloneTimeoutMs: envInt("ALONE_TIMEOUT_MS", 300_000),
  headless: envBool("HEADLESS", false),
  /** Max concurrent bot jobs (v1: 1 is safest). */
  maxConcurrent: envInt("MAX_CONCURRENT_BOTS", 1),
};

export function ensureDataDirs(): void {
  mkdirSync(config.recordingsDir, { recursive: true });
  mkdirSync(join(config.dataDir), { recursive: true });
}
