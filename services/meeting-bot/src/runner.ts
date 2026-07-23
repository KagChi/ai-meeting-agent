import { join } from "node:path";
import { config } from "./config";
import { AudioRecorder } from "./capture/recorder";
import { updateJob } from "./db/client";
import { importToMeetingAgent } from "./handoff/import";
import { launchBrowser } from "./browser";
import { getAdapter } from "./platforms/registry";
import type { BotJob, Platform } from "./types";

const abortControllers = new Map<string, AbortController>();

export function abortJob(jobId: string): void {
  abortControllers.get(jobId)?.abort();
}

export async function runBotJob(job: BotJob): Promise<void> {
  const ac = new AbortController();
  abortControllers.set(job.id, ac);
  const platform = job.platform as Platform;
  let session: Awaited<ReturnType<typeof launchBrowser>> | null = null;
  let recorder: AudioRecorder | null = null;

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

    updateJob(job.id, { status: "in_call" });

    const outPath = join(config.recordingsDir, job.id, "recording.wav");
    recorder = new AudioRecorder(outPath);
    recorder.start();
    updateJob(job.id, { status: "recording", recording_path: outPath });

    // Stay until leave, abort, call ends, or alone timeout
    const aloneDeadline = Date.now() + config.aloneTimeoutMs;
    while (!ac.signal.aborted) {
      const stillIn = await adapter.isInCall(session.page);
      if (!stillIn) break;
      // alone timeout: still "in call" but wall clock — keep until leave or abort
      if (Date.now() > aloneDeadline + config.joinTimeoutMs) {
        // extended: only break on not in call or abort
      }
      await session.page.waitForTimeout(3000);
    }

    if (!ac.signal.aborted) {
      await adapter.leave(session.page).catch(() => {});
    }

    const path = recorder ? await recorder.stop() : outPath;
    recorder = null;
    updateJob(job.id, { status: "uploading", recording_path: path });

    const imported = await importToMeetingAgent(path, job.title);
    updateJob(job.id, {
      status: "completed",
      meeting_agent_job_id: imported.job_id,
      recording_path: path,
      error: null,
    });
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    console.error(`[job ${job.id}] failed:`, msg);
    if (recorder) {
      await recorder.stop().catch(() => {});
    }
    updateJob(job.id, { status: "failed", error: msg });
  } finally {
    abortControllers.delete(job.id);
    if (session) {
      await session.close();
    }
  }
}
