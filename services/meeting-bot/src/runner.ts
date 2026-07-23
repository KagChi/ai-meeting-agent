import { join } from "node:path";
import { writeFileSync } from "node:fs";
import { config } from "./config";
import { MediaRecorderCapture } from "./capture/media-recorder";
import { TeamsChatCapture } from "./capture/teams-chat";
import { getJob, updateJob } from "./db/client";
import { importToMeetingAgent } from "./handoff/import";
import { launchBrowser } from "./browser";
import { child } from "./logger";
import { getAdapter } from "./platforms/registry";
import type { BotJob, Platform } from "./types";

const abortControllers = new Map<string, AbortController>();

export function abortJob(jobId: string): void {
  abortControllers.get(jobId)?.abort();
}

/**
 * Job lifecycle (Vexa mixed-lane style for Teams):
 *   join → admit → open chat → MediaRecorder mix of remote WebRTC streams
 *   → leave → upload WAV + chat JSON to meeting-agent
 */
export async function runBotJob(job: BotJob): Promise<void> {
  const ac = new AbortController();
  abortControllers.set(job.id, ac);
  const platform = job.platform as Platform;
  const log = child({ jobId: job.id, platform });
  let session: Awaited<ReturnType<typeof launchBrowser>> | null = null;
  let capture: MediaRecorderCapture | null = null;
  let chat: TeamsChatCapture | null = null;

  try {
    log.info({ title: job.title, meetingUrl: job.meeting_url }, "job start");
    const adapter = getAdapter(platform);
    const meetingUrl = adapter.resolveUrl({
      meetingUrl: job.meeting_url ?? undefined,
      nativeMeetingId: job.native_meeting_id ?? undefined,
    });
    updateJob(job.id, { status: "joining", meeting_url: meetingUrl });

    session = await launchBrowser();
    await adapter.join({
      meetingUrl,
      botName: job.bot_name || config.defaultBotName,
      page: session.page,
      signal: ac.signal,
      joinTimeoutMs: config.joinTimeoutMs,
    });

    if (ac.signal.aborted) {
      updateJob(job.id, {
        status: "failed",
        error: "cancelled by user (before recording)",
      });
      return;
    }

    updateJob(job.id, { status: "in_call" });

    // Chat scrape (Teams only — panel opened in adapter after admit)
    if (platform === "teams") {
      chat = new TeamsChatCapture(session.page);
      await chat.start().catch((e) => {
        log.warn({ err: e }, "teams chat start failed");
        chat = null;
      });
    }

    // Vexa-style: in-page mix of remote streams + MediaRecorder
    const outPath = join(config.recordingsDir, job.id, "recording.wav");
    capture = new MediaRecorderCapture(session.page, outPath);
    await capture.start();
    updateJob(job.id, { status: "recording", recording_path: outPath });

    // Brief wait for first remote stream (diagnostic only)
    for (let i = 0; i < 10 && !ac.signal.aborted; i++) {
      const n = await capture.remoteStreamCount();
      if (n > 0) {
        log.info({ remoteStreams: n }, "remote audio streams present");
        break;
      }
      await session.page.waitForTimeout(1000);
    }

    while (!ac.signal.aborted) {
      const stillIn = await adapter.isInCall(session.page);
      if (!stillIn) break;
      await session.page.waitForTimeout(3000);
    }

    const wasCancelled = ac.signal.aborted;

    if (!wasCancelled) {
      await adapter.leave(session.page).catch(() => {});
    }

    const chatMessages = chat ? await chat.stop() : [];
    chat = null;

    const stopped = await capture.stop();
    capture = null;

    if (!stopped.ok) {
      updateJob(job.id, {
        status: "failed",
        error: wasCancelled
          ? `cancelled by user; ${stopped.error || "no recording"}`
          : stopped.error || "recording failed",
        recording_path: stopped.path,
      });
      return;
    }

    // Persist chat sidecar next to recording (also sent on import)
    if (chatMessages.length > 0) {
      const chatPath = join(config.recordingsDir, job.id, "chat.json");
      writeFileSync(chatPath, JSON.stringify(chatMessages, null, 2));
      log.info({ path: chatPath, count: chatMessages.length }, "chat saved");
    }

    updateJob(job.id, { status: "uploading", recording_path: stopped.path });

    try {
      const imported = await importToMeetingAgent({
        filePath: stopped.path,
        title: job.title,
        platform,
        chat: chatMessages.length > 0 ? chatMessages : undefined,
      });
      updateJob(job.id, {
        status: "completed",
        meeting_agent_job_id: imported.job_id,
        recording_path: stopped.path,
        error: wasCancelled ? "stopped by user (recording uploaded)" : null,
      });
    } catch (impErr) {
      const msg = impErr instanceof Error ? impErr.message : String(impErr);
      updateJob(job.id, {
        status: "failed",
        error: `upload failed: ${msg}`,
        recording_path: stopped.path,
      });
    }
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    log.error({ err: msg }, "job failed");
    if (chat) {
      await chat.stop().catch(() => {});
    }
    if (capture) {
      await capture.stop().catch(() => {});
    }
    const current = getJob(job.id);
    if (!current || (current.status !== "completed" && current.status !== "failed")) {
      updateJob(job.id, {
        status: "failed",
        error: ac.signal.aborted ? "cancelled by user" : msg,
      });
    }
  } finally {
    abortControllers.delete(job.id);
    if (session) {
      await session.close();
    }
  }
}
