import { chromium, type Browser, type BrowserContext, type Page } from "playwright";
import { config } from "./config";

export interface BrowserSession {
  browser: Browser;
  context: BrowserContext;
  page: Page;
  close: () => Promise<void>;
}

/**
 * Install page-start hooks (Vexa mixed-lane style):
 * 1) blank getUserMedia fallback (no green Chromium fake-camera)
 * 2) installRemoteAudioHook — mirror each remote WebRTC audio track into a
 *    hidden playing <audio> so MediaRecorder can mix real meeting audio
 */
async function installPageHooks(context: BrowserContext): Promise<void> {
  await context.addInitScript(() => {
    const g = globalThis as typeof globalThis & {
      __meetingBotMediaPatched?: boolean;
      __meetingBotRemoteAudioHookInstalled?: boolean;
      /** Vexa: remote streams mirrored for mixed capture */
      __meetingBotRemoteStreams?: MediaStream[];
      __meetingBotInjectedAudioElements?: HTMLAudioElement[];
      __meetingBotRemoteAudioTracks?: MediaStreamTrack[];
    };
    if (g.__meetingBotMediaPatched) return;
    g.__meetingBotMediaPatched = true;
    g.__meetingBotRemoteStreams = g.__meetingBotRemoteStreams || [];
    g.__meetingBotInjectedAudioElements = g.__meetingBotInjectedAudioElements || [];
    g.__meetingBotRemoteAudioTracks = g.__meetingBotRemoteAudioTracks || [];

    // ── Vexa installRemoteAudioHook ─────────────────────────────────────
    if (
      !g.__meetingBotRemoteAudioHookInstalled &&
      typeof RTCPeerConnection === "function"
    ) {
      g.__meetingBotRemoteAudioHookInstalled = true;
      const streamIds = new Set<string>();
      const trackIds = new Set<string>();

      const handleTrack = (event: RTCTrackEvent) => {
        try {
          if (!event.track || event.track.kind !== "audio") return;
          if (trackIds.has(event.track.id)) return;
          trackIds.add(event.track.id);
          g.__meetingBotRemoteAudioTracks!.push(event.track);

          const stream =
            (event.streams && event.streams[0]) || new MediaStream([event.track]);
          if (!streamIds.has(stream.id)) {
            streamIds.add(stream.id);
            g.__meetingBotRemoteStreams!.push(stream);
          }

          // Critical: mirror into a real playing <audio> (Teams hides DOM audio)
          const audioEl = document.createElement("audio");
          audioEl.autoplay = true;
          audioEl.muted = false;
          audioEl.volume = 1.0;
          audioEl.dataset.meetingBotInjected = "true";
          audioEl.style.cssText =
            "position:absolute;left:-9999px;width:1px;height:1px;opacity:0;pointer-events:none";
          audioEl.srcObject = stream;
          void audioEl.play?.().catch(() => {});

          if (document.body) document.body.appendChild(audioEl);
          else
            document.addEventListener(
              "DOMContentLoaded",
              () => document.body?.appendChild(audioEl),
              { once: true },
            );

          g.__meetingBotInjectedAudioElements!.push(audioEl);
        } catch {
          /* ignore */
        }
      };

      const OriginalPC = RTCPeerConnection;
      function wrapPeerConnection(
        this: unknown,
        ...args: ConstructorParameters<typeof RTCPeerConnection>
      ) {
        const pc = new OriginalPC(...args);
        pc.addEventListener("track", handleTrack);

        try {
          const desc = Object.getOwnPropertyDescriptor(
            OriginalPC.prototype,
            "ontrack",
          );
          if (desc?.set) {
            Object.defineProperty(pc, "ontrack", {
              set(handler: ((this: RTCPeerConnection, ev: RTCTrackEvent) => void) | null) {
                if (typeof handler !== "function") {
                  return desc.set!.call(pc, handler);
                }
                const wrapped = function (
                  this: RTCPeerConnection,
                  event: RTCTrackEvent,
                ) {
                  handleTrack(event);
                  return handler.call(this, event);
                };
                return desc.set!.call(pc, wrapped);
              },
              get: desc.get,
              configurable: true,
              enumerable: true,
            });
          }
        } catch {
          /* ignore */
        }
        return pc;
      }
      wrapPeerConnection.prototype = OriginalPC.prototype;
      Object.setPrototypeOf(wrapPeerConnection, OriginalPC);
      (globalThis as unknown as { RTCPeerConnection: typeof RTCPeerConnection }).RTCPeerConnection =
        wrapPeerConnection as unknown as typeof RTCPeerConnection;
      try {
        (
          globalThis as unknown as { webkitRTCPeerConnection: typeof RTCPeerConnection }
        ).webkitRTCPeerConnection =
          wrapPeerConnection as unknown as typeof RTCPeerConnection;
      } catch {
        /* ignore */
      }
    }

    // ── Blank getUserMedia fallback ─────────────────────────────────────
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
      return canvas.captureStream(5).getVideoTracks()[0];
    }

    md.getUserMedia = async (constraints?: MediaStreamConstraints) => {
      try {
        return await original(constraints ?? { audio: true, video: false });
      } catch {
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
  // Do NOT use --use-fake-device-for-media-stream: green countdown camera.
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
  await installPageHooks(context);

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
