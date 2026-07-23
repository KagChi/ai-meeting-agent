import { Elysia, t } from "elysia";
import { z } from "zod";
import { config, CreateBotBodySchema, ensureDataDirs } from "./config";
import {
  countActiveJobs,
  getJob,
  insertJob,
  listJobs,
} from "./db/client";
import { log } from "./logger";
import { isSupported, listPlatforms } from "./platforms/registry";
import { abortJob, runBotJob } from "./runner";
import type { BotJob, BotJobStatus, Platform } from "./types";

ensureDataDirs();

function unauthorized(): Response {
  return new Response(JSON.stringify({ error: "Unauthorized" }), {
    status: 401,
    headers: { "content-type": "application/json" },
  });
}

function checkAuth(request: Request): boolean {
  if (!config.apiKey) return true;
  const key =
    request.headers.get("x-api-key") ||
    request.headers.get("X-API-Key") ||
    request.headers.get("authorization")?.replace(/^Bearer\s+/i, "");
  return key === config.apiKey;
}

const app = new Elysia()
  .onBeforeHandle(({ request }) => {
    const path = new URL(request.url).pathname;
    if (path === "/health") return;
    if (!checkAuth(request)) return unauthorized();
  })
  .get("/health", () => ({
    ok: true,
    service: "meeting-bot",
    platforms: listPlatforms(),
  }))
  .get("/platforms", () => ({ platforms: listPlatforms() }))
  .get(
    "/bots",
    ({ query }) => {
      const limit = Number(query.limit ?? 50);
      const status = query.status as BotJobStatus | undefined;
      return { bots: listJobs(limit, status) };
    },
    {
      query: t.Object({
        limit: t.Optional(t.String()),
        status: t.Optional(t.String()),
      }),
    },
  )
  .get("/bots/:id", ({ params, set }) => {
    const job = getJob(params.id);
    if (!job) {
      set.status = 404;
      return { error: "not found" };
    }
    return job;
  })
  .post(
    "/bots",
    async ({ body, set }) => {
      const parsed = CreateBotBodySchema.safeParse(body);
      if (!parsed.success) {
        set.status = 400;
        return {
          error: "validation failed",
          details: z.treeifyError(parsed.error),
        };
      }
      const req = parsed.data;

      if (!isSupported(req.platform)) {
        set.status = 400;
        return {
          error: `platform not implemented: ${req.platform}; available: ${listPlatforms()
            .filter((p) => p.status === "available")
            .map((p) => p.id)
            .join(", ")}`,
        };
      }

      if (countActiveJobs() >= config.maxConcurrent) {
        set.status = 409;
        return {
          error: `max concurrent bots (${config.maxConcurrent}) reached`,
        };
      }

      const now = new Date().toISOString();
      const job: BotJob = {
        id: crypto.randomUUID(),
        platform: req.platform as Platform,
        status: "queued",
        meeting_url: req.meeting_url?.trim() || null,
        native_meeting_id: req.native_meeting_id?.trim() || null,
        bot_name: req.bot_name?.trim() || config.defaultBotName,
        title: req.title?.trim() || null,
        recording_path: null,
        meeting_agent_job_id: null,
        error: null,
        created_at: now,
        updated_at: now,
      };
      insertJob(job);
      void runBotJob(job);
      set.status = 202;
      return {
        job_id: job.id,
        id: job.id,
        platform: job.platform,
        status: job.status,
      };
    },
    {
      body: t.Object({
        platform: t.String(),
        meeting_url: t.Optional(t.String()),
        native_meeting_id: t.Optional(t.String()),
        bot_name: t.Optional(t.String()),
        title: t.Optional(t.String()),
      }),
    },
  )
  .delete("/bots/:id", ({ params, set }) => {
    const job = getJob(params.id);
    if (!job) {
      set.status = 404;
      return { error: "not found" };
    }
    abortJob(params.id);
    return getJob(params.id) ?? job;
  })
  .listen({
    port: config.port,
    hostname: config.host,
  });

log.info(
  {
    host: config.host,
    port: config.port,
    sqlite: config.sqlitePath,
    meetingAgentUrl: config.meetingAgentUrl,
    importAuth: Boolean(config.meetingAgentApiKey),
    maxBots: config.maxConcurrent,
  },
  "meeting-bot listening",
);
if (
  config.meetingAgentUrl.includes("127.0.0.1") ||
  config.meetingAgentUrl.includes("localhost")
) {
  log.warn(
    "MEETING_AGENT_URL points at localhost — inside Docker this is the bot container itself. Use the API service hostname, e.g. http://meeting-agent-api:8080",
  );
}

export type App = typeof app;
