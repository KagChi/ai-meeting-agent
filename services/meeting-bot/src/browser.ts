import { chromium, type Browser, type BrowserContext, type Page } from "playwright";
import { config } from "./config";

export interface BrowserSession {
  browser: Browser;
  context: BrowserContext;
  page: Page;
  close: () => Promise<void>;
}

export async function launchBrowser(): Promise<BrowserSession> {
  // Fake media UI + stream reduces Teams "allow mic/camera" and AV confirm modals
  // (same idea as Vexa headless bots).
  const browser = await chromium.launch({
    headless: config.headless,
    args: [
      "--no-sandbox",
      "--disable-dev-shm-usage",
      "--use-fake-ui-for-media-stream",
      "--use-fake-device-for-media-stream",
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
  // Grant media for Teams hosts up front
  await context.grantPermissions(["microphone", "camera"], {
    origin: "https://teams.microsoft.com",
  });
  await context.grantPermissions(["microphone", "camera"], {
    origin: "https://teams.live.com",
  });
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
