import "dotenv/config";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";

const emptyToUndef = (v: unknown) =>
  v === undefined || v === null || v === "" ? undefined : v;

const boolish = z.preprocess((v) => {
  if (v === undefined || v === null || v === "") return undefined;
  const s = String(v).toLowerCase();
  if (["1", "true", "yes", "on"].includes(s)) return true;
  if (["0", "false", "no", "off"].includes(s)) return false;
  return v;
}, z.boolean().optional());

const stripQuotes = (v: unknown) => {
  if (typeof v !== "string") return v;
  return v.trim().replace(/^["']|["']$/g, "");
};

const EnvSchema = z.object({
  BOT_PORT: z.preprocess(
    emptyToUndef,
    z.coerce.number().int().min(1).max(65535).default(8091),
  ),
  BOT_HOST: z.preprocess(emptyToUndef, z.string().default("0.0.0.0")),
  BOT_API_KEY: z.preprocess(emptyToUndef, z.string().optional()),
  MEETING_BOT_INTERNAL_KEY: z.preprocess(emptyToUndef, z.string().optional()),
  BOT_NAME: z.preprocess(emptyToUndef, z.string().min(1).default("BMW-Lab-Bot")),
  DATA_DIR: z.preprocess(emptyToUndef, z.string().optional()),
  SQLITE_PATH: z.preprocess(emptyToUndef, z.string().optional()),
  KEEP_RECORDING: boolish,
  MEETING_AGENT_URL: z.preprocess((v) => {
    const s = emptyToUndef(stripQuotes(v));
    return typeof s === "string" ? s.replace(/\/$/, "") : s;
  }, z.string().url().default("http://127.0.0.1:8080")),
  MEETING_AGENT_API_KEY: z.preprocess(emptyToUndef, z.string().optional()),
  JOIN_TIMEOUT_MS: z.preprocess(
    emptyToUndef,
    z.coerce.number().int().positive().default(900_000),
  ),
  ALONE_TIMEOUT_MS: z.preprocess(
    emptyToUndef,
    z.coerce.number().int().positive().default(300_000),
  ),
  HEADLESS: boolish,
  MAX_CONCURRENT_BOTS: z.preprocess(
    emptyToUndef,
    z.coerce.number().int().positive().default(1),
  ),
});

const parsed = EnvSchema.safeParse(process.env);
if (!parsed.success) {
  // logger may not be ready for pretty transport on boot — stderr is fine
  process.stderr.write(`[config] invalid environment:\n${z.prettifyError(parsed.error)}\n`);
  process.exit(1);
}

const env = parsed.data;
const dataDir = env.DATA_DIR || join(process.cwd(), "data");

export const config = {
  port: env.BOT_PORT,
  host: env.BOT_HOST,
  apiKey: env.BOT_API_KEY || env.MEETING_BOT_INTERNAL_KEY || "",
  defaultBotName: env.BOT_NAME,
  dataDir,
  recordingsDir: join(dataDir, "recordings"),
  sqlitePath: env.SQLITE_PATH || join(dataDir, "meeting-bot.db"),
  keepRecording: env.KEEP_RECORDING ?? true,
  meetingAgentUrl: env.MEETING_AGENT_URL,
  meetingAgentApiKey: env.MEETING_AGENT_API_KEY || "",
  joinTimeoutMs: env.JOIN_TIMEOUT_MS,
  aloneTimeoutMs: env.ALONE_TIMEOUT_MS,
  headless: env.HEADLESS ?? false,
  maxConcurrent: env.MAX_CONCURRENT_BOTS,
};

export function ensureDataDirs(): void {
  mkdirSync(config.recordingsDir, { recursive: true });
  mkdirSync(join(config.dataDir), { recursive: true });
}

/** POST /bots body — Teams links are not always strict URL-parseable. */
export const CreateBotBodySchema = z
  .object({
    platform: z.enum(["teams", "zoom", "google_meet"]),
    meeting_url: z.string().min(1).optional(),
    native_meeting_id: z.string().min(1).optional(),
    bot_name: z.string().min(1).optional(),
    title: z.string().optional(),
  })
  .refine((b) => Boolean(b.meeting_url?.trim() || b.native_meeting_id?.trim()), {
    message: "meeting_url or native_meeting_id required",
  });

export type CreateBotBody = z.infer<typeof CreateBotBodySchema>;
