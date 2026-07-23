import { chromium, type Browser, type BrowserContext, type Page } from "playwright";
import { config } from "./config";

export interface BrowserSession {
  browser: Browser;
  context: BrowserContext;
  page: Page;
  close: () => Promise<void>;
}

export async function launchBrowser(): Promise<BrowserSession> {
  const browser = await chromium.launch({
    headless: config.headless,
    args: [
      "--no-sandbox",
      "--disable-dev-shm-usage",
      "--use-fake-ui-for-media-stream",
      "--autoplay-policy=no-user-gesture-required",
    ],
  });
  const context = await browser.newContext({
    permissions: ["microphone", "camera"],
    viewport: { width: 1280, height: 720 },
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
