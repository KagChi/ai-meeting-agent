/**
 * In-page MediaRecorder capture — same approach Vexa uses for Teams/GMeet
 * (`MediaRecorderCapture` in @vexa/recording audio-pipeline).
 *
 * Taps remote meeting audio via Web Audio API (all <audio>/<video> elements +
 * track events), encodes Opus in WebM, streams chunks to Node via
 * exposeFunction. No PulseAudio / host ffmpeg required for Teams.
 */
import type { Page } from "playwright";
import { mkdirSync, writeFileSync, existsSync, statSync } from "node:fs";
import { dirname } from "node:path";

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
            console.warn("[media-recorder] chunk decode error", e);
          }
        },
      );
      this.exposed = true;
    }

    await this.page.evaluate(async () => {
      const w = window as unknown as {
        __meetingBotRecorder?: MediaRecorder;
        __meetingBotCtx?: AudioContext;
        __meetingBotDest?: MediaStreamAudioDestinationNode;
        __meetingBotSaveChunk?: (p: {
          base64: string;
          isFinal?: boolean;
        }) => void | Promise<void>;
      };

      // Tear down previous run if any
      try {
        w.__meetingBotRecorder?.stop();
      } catch {
        /* ignore */
      }

      const ctx = new AudioContext();
      const dest = ctx.createMediaStreamDestination();
      w.__meetingBotCtx = ctx;
      w.__meetingBotDest = dest;

      const connected = new WeakSet<Element>();

      function connectEl(el: HTMLMediaElement) {
        if (connected.has(el)) return;
        try {
          // Capture remote participants (typically not muted locally for playback)
          const src = ctx.createMediaElementSource(el);
          src.connect(dest);
          // Keep hearing in the tab (optional for headless)
          src.connect(ctx.destination);
          connected.add(el);
        } catch {
          // Already connected or cross-origin — try captureStream
          try {
            const cs = (
              el as HTMLMediaElement & { captureStream?: () => MediaStream }
            ).captureStream?.();
            if (cs) {
              cs.getAudioTracks().forEach((track) => {
                dest.stream.addTrack(track);
              });
              connected.add(el);
            }
          } catch {
            /* ignore */
          }
        }
      }

      function scan() {
        document.querySelectorAll("audio, video").forEach((node) => {
          connectEl(node as HTMLMediaElement);
        });
      }

      scan();
      const mo = new MutationObserver(() => scan());
      mo.observe(document.documentElement, { childList: true, subtree: true });

      // Also pull tracks from any getUserMedia / RTCPeerConnection if exposed
      // (best-effort; Teams may keep PC private)

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
        for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
        const base64 = btoa(binary);
        await w.__meetingBotSaveChunk?.({ base64, isFinal: false });
      };

      rec.onstop = async () => {
        // final flush already via last ondataavailable; signal empty final
        await w.__meetingBotSaveChunk?.({ base64: "", isFinal: true });
        try {
          mo.disconnect();
        } catch {
          /* ignore */
        }
      };

      if (ctx.state === "suspended") {
        await ctx.resume().catch(() => {});
      }

      // 1s timeslices → frequent chunks (Vexa uses ~15s; shorter is fine for lab)
      rec.start(1000);
    });

    this.started = true;
    console.log(`[media-recorder] started (in-page) → ${this.outPath}`);
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
          __meetingBotCtx?: AudioContext;
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
          await w.__meetingBotCtx?.close();
        } catch {
          /* ignore */
        }
      });
    } catch (e) {
      console.warn("[media-recorder] stop evaluate failed", e);
    }

    // Allow last chunks to arrive
    await new Promise((r) => setTimeout(r, 500));

    if (this.chunks.length === 0) {
      return {
        path: this.outPath,
        bytes: 0,
        ok: false,
        error:
          "no audio chunks captured (Teams may not have attached remote audio elements yet)",
      };
    }

    const webm = Buffer.concat(this.chunks);
    const webmPath = this.outPath.replace(/\.wav$/i, ".webm");
    writeFileSync(webmPath, webm);

    // Transcode to 16 kHz mono WAV for meeting-agent import
    const wavOk = await transcodeToWav(webmPath, this.outPath);
    if (!wavOk) {
      // Fall back: import webm if agent accepts it (it does via ffmpeg convert)
      writeFileSync(this.outPath.replace(/\.wav$/i, ".webm"), webm);
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
    console.log(`[media-recorder] stopped wav=${this.outPath} bytes=${bytes}`);
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
    console.warn("[media-recorder] ffmpeg convert failed:", err.slice(-300));
    return false;
  }
  return existsSync(output);
}
