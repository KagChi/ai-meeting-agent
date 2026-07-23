import { chromium, type Browser, type BrowserContext, type Page } from "playwright";
import { config } from "./config";

export interface BrowserSession {
  browser: Browser;
  context: BrowserContext;
  page: Page;
  close: () => Promise<void>;
}

/**
 * Install silent audio + black video for getUserMedia so Teams never shows
 * Chromium's green fake-device countdown pattern if the camera is on.
 */
async function installBlankMediaShim(context: BrowserContext): Promise<void> {
  await context.addInitScript(() => {
    const g = globalThis as typeof globalThis & {
      __meetingBotMediaPatched?: boolean;
    };
    if (g.__meetingBotMediaPatched) return;
    g.__meetingBotMediaPatched = true;

    const md = navigator.mediaDevices;
    if (!md?.getUserMedia) return;

    const original = md.getUserMedia.bind(md);

    function silentAudioTrack(): MediaStreamTrack {
      const ctx = new AudioContext();
      const oscillator = ctx.createOscillator();
      const gain = ctx.createGain();
      gain.gain.value = 0;
      const dest = ctx.createMediaStreamDestination();
      oscillator.connect(gain);
      gain.connect(dest);
      oscillator.start();
      const track = dest.stream.getAudioTracks()[0];
      // Keep context alive via track; stop osc when track ends
      track.addEventListener("ended", () => {
        try {
          oscillator.stop();
          void ctx.close();
        } catch {
          /* ignore */
        }
      });
      return track;
    }

    function blackVideoTrack(): MediaStreamTrack {
      const canvas = document.createElement("canvas");
      canvas.width = 640;
      canvas.height = 480;
      const c2d = canvas.getContext("2d");
      if (c2d) {
        c2d.fillStyle = "#000000";
        c2d.fillRect(0, 0, canvas.width, canvas.height);
      }
      // Drive frames so the track stays live
      const stream = canvas.captureStream(5);
      return stream.getVideoTracks()[0];
    }

    md.getUserMedia = async (constraints?: MediaStreamConstraints) => {
      try {
        // Prefer real devices when available (Pulse/mic in Docker)
        return await original(constraints ?? { audio: true, video: false });
      } catch {
        // Fallback: blank tracks so permission/join still works without green test pattern
        const tracks: MediaStreamTrack[] = [];
        const wantAudio =
          constraints === undefined ||
          constraints.audio === true ||
          (typeof constraints.audio === "object" && constraints.audio !== null);
        const wantVideo =
          constraints?.video === true ||
          (typeof constraints?.video === "object" && constraints.video !== null);

        if (wantAudio) tracks.push(silentAudioTrack());
        if (wantVideo) tracks.push(blackVideoTrack());
        if (tracks.length === 0) tracks.push(silentAudioTrack());
        return new MediaStream(tracks);
      }
    };
  });
}

export async function launchBrowser(): Promise<BrowserSession> {
  // Do NOT use --use-fake-device-for-media-stream: that is the green countdown
  // video Teams shows as the bot camera. Keep --use-fake-ui-for-media-stream
  // only to auto-accept the browser permission prompt.
  const browser = await chromium.launch({
    headless: config.headless,
    args: [
      "--no-sandbox",
      "--disable-dev-shm-usage",
      "--use-fake-ui-for-media-stream",
      "--autoplay-policy=no-user-gesture-required",
      "--disable-blink-features=AutomationControlled",
    ],
  });
  const context = await browser.newContext({
    permissions: ["microphone", "camera"],
    viewport: { width: 1280, height: 720 },
    locale: "en-US",
    userAgent:
      "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36",
  });
  await context.grantPermissions(["microphone", "camera"], {
    origin: "https://teams.microsoft.com",
  });
  await context.grantPermissions(["microphone", "camera"], {
    origin: "https://teams.live.com",
  });
  await installBlankMediaShim(context);

  const page = await context.newPage();
  return {
    browser,
    context,
    page,
    close: async () => {
      await context.close().catch(() => {});
      await browser.close().catch(() => {});
    },
  };
}
