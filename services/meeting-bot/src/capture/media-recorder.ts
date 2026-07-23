/**
 * Vexa mixed-lane recording for Teams:
 *   remote WebRTC streams (via installRemoteAudioHook) → mix → MediaRecorder
 *   → webm chunks → 16 kHz mono WAV for meeting-agent import.
 *
 * Aligns with Vexa capture-bridge mixed path + record-chunker (single file, not
 * chunked upload).
 */
import type { Page } from "playwright";
import { mkdirSync, writeFileSync, existsSync, statSync } from "node:fs";
import { dirname } from "node:path";
import { log } from "../logger";

export class MediaRecorderCapture {
  private page: Page;
  private outPath: string;
  private chunks: Buffer[] = [];
  private exposed = false;
  private started = false;

  constructor(page: Page, outPath: string) {
    this.page = page;
    this.outPath = outPath;
  }

  get path(): string {
    return this.outPath;
  }

  async start(): Promise<void> {
    if (this.started) return;
    mkdirSync(dirname(this.outPath), { recursive: true });
    this.chunks = [];

    if (!this.exposed) {
      await this.page.exposeFunction(
        "__meetingBotSaveChunk",
        (payload: { base64: string; isFinal?: boolean }) => {
          try {
            const buf = Buffer.from(payload.base64 || "", "base64");
            if (buf.length > 0) this.chunks.push(buf);
          } catch (e) {
            log.warn({ err: e }, "media-recorder chunk decode error");
          }
        },
      );
      this.exposed = true;
    }

    const stats = await this.page.evaluate(async () => {
      const w = window as unknown as {
        __meetingBotRecorder?: MediaRecorder;
        __meetingBotMixCtx?: AudioContext;
        __meetingBotMixDest?: MediaStreamAudioDestinationNode;
        __meetingBotMixSeen?: Set<string>;
        __meetingBotMixRescan?: number;
        __meetingBotRemoteStreams?: MediaStream[];
        __meetingBotSaveChunk?: (p: {
          base64: string;
          isFinal?: boolean;
        }) => void | Promise<void>;
      };

      try {
        w.__meetingBotRecorder?.stop();
      } catch {
        /* ignore */
      }
      try {
        if (w.__meetingBotMixRescan) window.clearInterval(w.__meetingBotMixRescan);
      } catch {
        /* ignore */
      }

      // Native-rate mix context (MediaRecorder handles encode); Vexa STT uses 16k separately
      const ctx = new AudioContext();
      const dest = ctx.createMediaStreamDestination();
      w.__meetingBotMixCtx = ctx;
      w.__meetingBotMixDest = dest;
      w.__meetingBotMixSeen = new Set();

      function connectStreams() {
        const streams = w.__meetingBotRemoteStreams || [];
        let n = 0;
        for (const s of streams) {
          if (!s || w.__meetingBotMixSeen!.has(s.id)) continue;
          try {
            if (s.getAudioTracks().length === 0) continue;
            ctx.createMediaStreamSource(s).connect(dest);
            w.__meetingBotMixSeen!.add(s.id);
            n += 1;
          } catch {
            /* stream not ready */
          }
        }
        // Also connect injected <audio> elements' srcObject streams
        document
          .querySelectorAll("audio[data-meeting-bot-injected='true']")
          .forEach((el) => {
            const media = el as HTMLAudioElement;
            const s = media.srcObject;
            if (!(s instanceof MediaStream) || w.__meetingBotMixSeen!.has(s.id)) return;
            try {
              if (s.getAudioTracks().length === 0) return;
              ctx.createMediaStreamSource(s).connect(dest);
              w.__meetingBotMixSeen!.add(s.id);
              n += 1;
            } catch {
              /* ignore */
            }
          });
        return { connected: n, total: w.__meetingBotMixSeen!.size };
      }

      const first = connectStreams();
      w.__meetingBotMixRescan = window.setInterval(() => {
        connectStreams();
      }, 2000);

      const mimeCandidates = [
        "audio/webm;codecs=opus",
        "audio/webm",
        "audio/ogg;codecs=opus",
      ];
      let mime = "";
      for (const m of mimeCandidates) {
        if (typeof MediaRecorder !== "undefined" && MediaRecorder.isTypeSupported(m)) {
          mime = m;
          break;
        }
      }

      if (ctx.state === "suspended") {
        await ctx.resume().catch(() => {});
      }

      const rec = new MediaRecorder(
        dest.stream,
        mime ? { mimeType: mime, audioBitsPerSecond: 128_000 } : undefined,
      );
      w.__meetingBotRecorder = rec;

      rec.ondataavailable = async (ev) => {
        if (!ev.data || ev.data.size === 0) return;
        const buf = await ev.data.arrayBuffer();
        const bytes = new Uint8Array(buf);
        let binary = "";
        const step = 0x8000;
        for (let i = 0; i < bytes.length; i += step) {
          binary += String.fromCharCode(...bytes.subarray(i, i + step));
        }
        await w.__meetingBotSaveChunk?.({ base64: btoa(binary), isFinal: false });
      };

      rec.onstop = async () => {
        await w.__meetingBotSaveChunk?.({ base64: "", isFinal: true });
        try {
          if (w.__meetingBotMixRescan) window.clearInterval(w.__meetingBotMixRescan);
        } catch {
          /* ignore */
        }
      };

      // 1s timeslices (import is single-file; shorter chunks are fine)
      rec.start(1000);

      return {
        connected: first.total,
        remoteQueued: (w.__meetingBotRemoteStreams || []).length,
        ctxState: ctx.state,
        mime: mime || rec.mimeType || "default",
      };
    });

    this.started = true;
    log.info(
      {
        path: this.outPath,
        connected: stats?.connected ?? 0,
        remoteQueued: stats?.remoteQueued ?? 0,
        ctx: stats?.ctxState,
        mime: stats?.mime,
      },
      "media-recorder started (vexa mix)",
    );
  }

  /** How many remote streams the page hook has seen (diagnostic). */
  async remoteStreamCount(): Promise<number> {
    try {
      return await this.page.evaluate(() => {
        const w = window as unknown as { __meetingBotRemoteStreams?: MediaStream[] };
        return (w.__meetingBotRemoteStreams || []).length;
      });
    } catch {
      return 0;
    }
  }

  async stop(): Promise<{ path: string; bytes: number; ok: boolean; error?: string }> {
    if (!this.started) {
      return {
        path: this.outPath,
        bytes: 0,
        ok: false,
        error: "media recorder never started",
      };
    }

    try {
      await this.page.evaluate(async () => {
        const w = window as unknown as {
          __meetingBotRecorder?: MediaRecorder;
          __meetingBotMixCtx?: AudioContext;
          __meetingBotMixRescan?: number;
        };
        const rec = w.__meetingBotRecorder;
        if (rec && rec.state !== "inactive") {
          await new Promise<void>((resolve) => {
            rec.addEventListener("stop", () => resolve(), { once: true });
            try {
              rec.stop();
            } catch {
              resolve();
            }
            setTimeout(resolve, 5000);
          });
        }
        try {
          if (w.__meetingBotMixRescan) window.clearInterval(w.__meetingBotMixRescan);
        } catch {
          /* ignore */
        }
        try {
          await w.__meetingBotMixCtx?.close();
        } catch {
          /* ignore */
        }
      });
    } catch (e) {
      log.warn({ err: e }, "media-recorder stop evaluate failed");
    }

    await new Promise((r) => setTimeout(r, 500));

    if (this.chunks.length === 0) {
      return {
        path: this.outPath,
        bytes: 0,
        ok: false,
        error:
          "no audio chunks (no remote WebRTC streams — check computer audio + remote participants)",
      };
    }

    const webm = Buffer.concat(this.chunks);
    const webmPath = this.outPath.replace(/\.wav$/i, ".webm");
    writeFileSync(webmPath, webm);
    log.info({ bytes: webm.length, path: webmPath }, "media-recorder webm written");

    const wavOk = await transcodeToWav(webmPath, this.outPath);
    if (!wavOk) {
      return {
        path: webmPath,
        bytes: webm.length,
        ok: webm.length > 1024,
        error:
          webm.length > 1024
            ? undefined
            : "webm too small; ffmpeg wav convert failed",
      };
    }

    const bytes = existsSync(this.outPath) ? statSync(this.outPath).size : 0;
    const rms = await roughPcmRms(this.outPath);
    log.info(
      { path: this.outPath, bytes, roughRms: rms },
      "media-recorder stopped",
    );

    if (bytes > 1024 && rms !== null && rms < 0.0005) {
      log.warn(
        { roughRms: rms },
        "recording near-silent — remote audio may not have been mixed",
      );
    }

    return {
      path: this.outPath,
      bytes,
      ok: bytes > 1024,
      error: bytes > 1024 ? undefined : "wav too short after convert",
    };
  }
}

async function transcodeToWav(input: string, output: string): Promise<boolean> {
  const proc = Bun.spawn(
    [
      "ffmpeg",
      "-y",
      "-i",
      input,
      "-ac",
      "1",
      "-ar",
      "16000",
      "-c:a",
      "pcm_s16le",
      output,
    ],
    { stdout: "ignore", stderr: "pipe" },
  );
  const code = await proc.exited;
  if (code !== 0) {
    const err = await new Response(proc.stderr).text().catch(() => "");
    log.warn({ stderr: err.slice(-300) }, "media-recorder ffmpeg convert failed");
    return false;
  }
  return existsSync(output);
}

async function roughPcmRms(wavPath: string): Promise<number | null> {
  try {
    const f = Bun.file(wavPath);
    if (!(await f.exists())) return null;
    const buf = Buffer.from(await f.arrayBuffer());
    if (buf.length < 44) return null;
    let sum = 0;
    let n = 0;
    const end = Math.min(buf.length, 44 + 16000 * 2 * 2);
    for (let i = 44; i + 1 < end; i += 2) {
      const s = buf.readInt16LE(i) / 32768;
      sum += s * s;
      n += 1;
    }
    if (n === 0) return null;
    return Math.sqrt(sum / n);
  } catch {
    return null;
  }
}
