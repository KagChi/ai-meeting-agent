import type { Page } from "playwright";
import type { JoinContext, PlatformAdapter } from "../types";
import { teamsSelectors } from "./selectors";

async function clickFirst(page: Page, selectors: readonly string[]): Promise<boolean> {
  for (const sel of selectors) {
    try {
      const loc = page.locator(sel).first();
      if (await loc.isVisible({ timeout: 1500 }).catch(() => false)) {
        await loc.click({ timeout: 5000 });
        return true;
      }
    } catch {
      /* try next */
    }
  }
  return false;
}

async function fillFirst(
  page: Page,
  selectors: readonly string[],
  value: string,
): Promise<boolean> {
  for (const sel of selectors) {
    try {
      const loc = page.locator(sel).first();
      if (await loc.isVisible({ timeout: 1500 }).catch(() => false)) {
        await loc.fill(value, { timeout: 5000 });
        return true;
      }
    } catch {
      /* try next */
    }
  }
  return false;
}

export const teamsAdapter: PlatformAdapter = {
  id: "teams",

  resolveUrl(input) {
    if (input.meetingUrl?.trim()) {
      return input.meetingUrl.trim();
    }
    const id = input.nativeMeetingId?.trim();
    if (!id) {
      throw new Error("teams requires meeting_url or native_meeting_id");
    }
    // Native id alone is not always a full URL; prefer meetup-join style if raw.
    if (id.startsWith("http://") || id.startsWith("https://")) {
      return id;
    }
    throw new Error(
      "teams native_meeting_id alone is not enough; pass full meeting_url (Teams share link)",
    );
  },

  async join(ctx: JoinContext) {
    const { page, meetingUrl, botName, signal, joinTimeoutMs } = ctx;
    const deadline = Date.now() + joinTimeoutMs;

    await page.goto(meetingUrl, { waitUntil: "domcontentloaded", timeout: 120_000 });

    // Dismiss "open app" → continue in browser
    await clickFirst(page, teamsSelectors.continueOnBrowser);
    await page.waitForTimeout(1000);

    await fillFirst(page, teamsSelectors.nameInput, botName);
    await page.waitForTimeout(500);

    // Best-effort mute cam/mic before join
    await clickFirst(page, teamsSelectors.camToggle);
    await clickFirst(page, teamsSelectors.micToggle);

    const joined = await clickFirst(page, teamsSelectors.joinButton);
    if (!joined) {
      // Some layouts auto-join after name
      console.warn("[teams] join button not found; waiting for in-call UI");
    }

    while (Date.now() < deadline) {
      if (signal.aborted) {
        throw new Error("join aborted");
      }
      if (await this.isInCall(page)) {
        return;
      }
      // Stay in lobby — host must admit
      await page.waitForTimeout(2000);
    }
    throw new Error("join timeout: still not in call (admit bot from lobby?)");
  },

  async isInCall(page: Page): Promise<boolean> {
    for (const sel of teamsSelectors.leaveButton) {
      try {
        if (await page.locator(sel).first().isVisible({ timeout: 500 }).catch(() => false)) {
          return true;
        }
      } catch {
        /* continue */
      }
    }
    // URL heuristic
    const url = page.url();
    if (/\/l\/meetup-join\//i.test(url) === false && /teams\.microsoft\.com.*meeting/i.test(url)) {
      // weak signal
    }
    return false;
  },

  async leave(page: Page): Promise<void> {
    const clicked = await clickFirst(page, teamsSelectors.leaveButton);
    if (!clicked) {
      console.warn("[teams] leave button not found");
    }
    await page.waitForTimeout(1000);
  },
};
