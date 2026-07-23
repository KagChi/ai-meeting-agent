/**
 * Host-level ffmpeg capture (Pulse/ALSA) — Vexa uses this mainly for Zoom Web.
 * Teams uses MediaRecorderCapture (media-recorder.ts) instead.
 */
import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, statSync } from "node:fs";
import { dirname } from "node:path";

export class AudioRecorder {
  private proc: ChildProcess | null = null;
  private readonly outPath: string;
  private stderrBuf = "";
  private startedAt = 0;
  private spawnError: string | null = null;

  constructor(outPath: string) {
    this.outPath = outPath;
  }

  get path(): string {
    return this.outPath;
  }

  start(): void {
    mkdirSync(dirname(this.outPath), { recursive: true });
    this.stderrBuf = "";
    this.spawnError = null;
    this.startedAt = Date.now();

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
      this.stderrBuf += line;
      if (this.stderrBuf.length > 8000) this.stderrBuf = this.stderrBuf.slice(-4000);
      if (line.toLowerCase().includes("error")) {
        console.warn("[recorder]", line.trim().slice(0, 240));
      }
    });

    this.proc.on("error", (err) => {
      this.spawnError = err.message;
      console.error("[recorder] ffmpeg spawn error:", err.message);
    });
  }

  async stop(): Promise<{ path: string; bytes: number; ok: boolean; error?: string }> {
    const proc = this.proc;
    this.proc = null;
    if (proc && !proc.killed) {
      await new Promise<void>((resolve) => {
        const t = setTimeout(() => {
          proc.kill("SIGKILL");
          resolve();
        }, 8000);
        proc.once("exit", () => {
          clearTimeout(t);
          resolve();
        });
        proc.kill("SIGINT");
      });
    }

    if (this.spawnError) {
      return {
        path: this.outPath,
        bytes: 0,
        ok: false,
        error: `ffmpeg spawn failed: ${this.spawnError}`,
      };
    }
    if (!existsSync(this.outPath)) {
      return {
        path: this.outPath,
        bytes: 0,
        ok: false,
        error: `recording file was not created. ffmpeg: ${this.stderrBuf.trim().slice(-400) || "no output"}`,
      };
    }
    const bytes = statSync(this.outPath).size;
    if (bytes < 1024) {
      return {
        path: this.outPath,
        bytes,
        ok: false,
        error: `recording too short (${bytes} bytes)`,
      };
    }
    console.log(
      `[recorder] stopped path=${this.outPath} bytes=${bytes} elapsed_ms=${Date.now() - this.startedAt}`,
    );
    return { path: this.outPath, bytes, ok: true };
  }
}
