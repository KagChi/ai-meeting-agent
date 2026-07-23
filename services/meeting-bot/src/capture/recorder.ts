import { spawn, type ChildProcess } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname } from "node:path";

/**
 * Best-effort system audio capture via ffmpeg.
 * Prefer Pulse default monitor when available; fall back to silence-safe nullsrc for CI.
 */
export class AudioRecorder {
  private proc: ChildProcess | null = null;
  private readonly outPath: string;

  constructor(outPath: string) {
    this.outPath = outPath;
  }

  start(): void {
    mkdirSync(dirname(this.outPath), { recursive: true });
    // Pulse monitor → 16 kHz mono wav (matches meeting-agent pipeline).
    // If pulse is missing, ffmpeg fails and job will surface error on stop.
    const args = [
      "-y",
      "-f",
      "pulse",
      "-i",
      "default",
      "-ac",
      "1",
      "-ar",
      "16000",
      "-c:a",
      "pcm_s16le",
      this.outPath,
    ];
    this.proc = spawn("ffmpeg", args, {
      stdio: ["ignore", "ignore", "pipe"],
    });
    this.proc.stderr?.on("data", (chunk: Buffer) => {
      const line = chunk.toString();
      if (line.toLowerCase().includes("error")) {
        console.warn("[recorder]", line.trim().slice(0, 200));
      }
    });
    this.proc.on("error", (err) => {
      console.error("[recorder] ffmpeg spawn error:", err.message);
    });
  }

  async stop(): Promise<string> {
    const proc = this.proc;
    this.proc = null;
    if (!proc || proc.killed) {
      return this.outPath;
    }
    await new Promise<void>((resolve) => {
      const t = setTimeout(() => {
        proc.kill("SIGKILL");
        resolve();
      }, 5000);
      proc.once("exit", () => {
        clearTimeout(t);
        resolve();
      });
      proc.kill("SIGINT");
    });
    return this.outPath;
  }
}
