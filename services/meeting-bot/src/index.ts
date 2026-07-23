import { Elysia, t } from "elysia";
import { config, ensureDataDirs } from "./config";
import {
  countActiveJobs,
  getJob,
  insertJob,
  listJobs,
  updateJob,
} from "./db/client";
import { isSupported, listPlatforms } from "./platforms/registry";
import { abortJob, runBotJob } from "./runner";
import type { BotJob, BotJobStatus, CreateBotRequest, Platform } from "./types";

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
      const req = body as CreateBotRequest;
      if (!req.platform || !isSupported(req.platform)) {
        set.status = 400;
        return {
          error: `invalid platform; supported: ${listPlatforms()
            .filter((p) => p.status === "available")
            .map((p) => p.id)
            .join(", ")}`,
        };
      }
      if (!req.meeting_url?.trim() && !req.native_meeting_id?.trim()) {
        set.status = 400;
        return { error: "meeting_url or native_meeting_id required" };
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
      // Fire and forget
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
    const active = ["queued", "joining", "in_call", "recording", "uploading"];
    if (active.includes(job.status)) {
      updateJob(params.id, {
        status: "failed",
        error: "cancelled by user",
      });
    }
    return getJob(params.id);
  })
  .listen({
    port: config.port,
    hostname: config.host,
  });

console.log(
  `meeting-bot listening on http://${config.host}:${config.port} (sqlite=${config.sqlitePath})`,
);

export type App = typeof app;
