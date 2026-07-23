import { join } from "node:path";
import { config } from "./config";
import { MediaRecorderCapture } from "./capture/media-recorder";
import { getJob, updateJob } from "./db/client";
import { importToMeetingAgent } from "./handoff/import";
import { launchBrowser } from "./browser";
import { getAdapter } from "./platforms/registry";
import type { BotJob, Platform } from "./types";

const abortControllers = new Map<string, AbortController>();

export function abortJob(jobId: string): void {
  abortControllers.get(jobId)?.abort();
}

/**
 * Job lifecycle mirrors Vexa bot:
 *   join (Playwright) → admit → MediaRecorder in-page capture → leave → upload
 * Teams uses browser MediaRecorder (Vexa MediaRecorderCapture), not host Pulse.
 */
export async function runBotJob(job: BotJob): Promise<void> {
  const ac = new AbortController();
  abortControllers.set(job.id, ac);
  const platform = job.platform as Platform;
  let session: Awaited<ReturnType<typeof launchBrowser>> | null = null;
  let capture: MediaRecorderCapture | null = null;

  try {
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

    // Vexa-style: start in-page MediaRecorder once admitted
    const outPath = join(config.recordingsDir, job.id, "recording.wav");
    capture = new MediaRecorderCapture(session.page, outPath);
    await capture.start();
    updateJob(job.id, { status: "recording", recording_path: outPath });

    // Stay until leave, abort, or call ends
    while (!ac.signal.aborted) {
      const stillIn = await adapter.isInCall(session.page);
      if (!stillIn) break;
      await session.page.waitForTimeout(3000);
    }

    const wasCancelled = ac.signal.aborted;

    if (!wasCancelled) {
      await adapter.leave(session.page).catch(() => {});
    }

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

    updateJob(job.id, { status: "uploading", recording_path: stopped.path });

    try {
      const imported = await importToMeetingAgent(stopped.path, job.title);
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
    console.error(`[job ${job.id}] failed:`, msg);
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
