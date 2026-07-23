/**
 * Teams join flow — structure follows Vexa
 * `core/meetings/modules/join/src/msteams/join.ts` + admission.ts + modals.ts
 */
import type { Page } from "playwright";
import type { JoinContext, PlatformAdapter } from "../types";
import {
  cameraOffSelectors,
  cameraOnSelectors,
  computerAudioSelectors,
  continueBrowserSelectors,
  continueWithoutMediaSelectors,
  dontUseAudioSelectors,
  joinButtonFallbackSelectors,
  joinNowSelectors,
  leaveButtonSelectors,
  lobbyIndicators,
  nameInputSelectors,
  permissionGateText,
  rejectionIndicators,
} from "./selectors";

async function isVisible(page: Page, selector: string, timeout = 800): Promise<boolean> {
  try {
    return await page.locator(selector).first().isVisible({ timeout });
  } catch {
    return false;
  }
}

async function clickFirst(
  page: Page,
  selectors: readonly string[],
  opts?: { timeout?: number },
): Promise<boolean> {
  const timeout = opts?.timeout ?? 5000;
  for (const sel of selectors) {
    try {
      const loc = page.locator(sel).first();
      if (await loc.isVisible({ timeout: 1200 }).catch(() => false)) {
        await loc.click({ timeout });
        return true;
      }
    } catch {
      /* try next */
    }
  }
  return false;
}

async function fillFirst(page: Page, selectors: readonly string[], value: string): Promise<boolean> {
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

/** Vexa: dismiss "Continue without audio or video" / AV confirm modal. */
async function dismissAvConfirmModal(page: Page): Promise<boolean> {
  const clicked = await clickFirst(page, continueWithoutMediaSelectors);
  if (clicked) {
    console.log('[teams] dismissed "Continue without audio or video" modal');
    await page.waitForTimeout(500);
  }
  return clicked;
}

async function warmUpMedia(page: Page): Promise<void> {
  try {
    const result = await page.evaluate(async () => {
      try {
        if (!navigator.mediaDevices?.getUserMedia) return "getUserMedia unavailable";
        const stream = await navigator.mediaDevices.getUserMedia({
          audio: true,
          video: true,
        });
        stream.getTracks().forEach((t) => t.stop());
        return "media warm-up ok";
      } catch (err: unknown) {
        const msg = err instanceof Error ? err.message : String(err);
        return `media warm-up failed: ${msg}`;
      }
    });
    console.log(`[teams] ${result}`);
  } catch (e) {
    console.warn("[teams] media warm-up evaluate failed", e);
  }
}

/**
 * Wait until pre-join controls are ready (Join now / name / camera / audio).
 * Ported from Vexa waitForTeamsPreJoinReadiness.
 */
async function waitForPreJoinReadiness(page: Page, timeoutMs: number): Promise<boolean> {
  const start = Date.now();
  let mediaWarmup = false;
  let continueClicks = 0;
  let withoutMediaClicks = 0;

  while (Date.now() - start < timeoutMs) {
    // Modal can block prejoin entirely
    if (
      (await isVisible(page, continueWithoutMediaSelectors[0])) &&
      withoutMediaClicks < 3
    ) {
      withoutMediaClicks += 1;
      await dismissAvConfirmModal(page);
      continue;
    }

    const joinNow =
      (await isVisible(page, 'button:has-text("Join now")')) ||
      (await isVisible(page, 'button[aria-label*="Join now"]'));
    const cancel = await isVisible(page, 'button:has-text("Cancel")');
    const nameOk = await isVisible(page, nameInputSelectors[0]);
    const camOk =
      (await isVisible(page, cameraOffSelectors[0])) ||
      (await isVisible(page, cameraOnSelectors[0]));
    const audioOk = await isVisible(page, computerAudioSelectors[0]);

    if (joinNow || (cancel && (nameOk || camOk || audioOk))) {
      console.log("[teams] pre-join controls ready");
      return true;
    }

    const cont = await isVisible(page, continueBrowserSelectors[0]);
    if (cont && continueClicks < 2) {
      continueClicks += 1;
      await clickFirst(page, continueBrowserSelectors);
      await page.waitForTimeout(500);
      continue;
    }

    const permGate = await page
      .locator(`text=${permissionGateText.source}`)
      .first()
      .isVisible()
      .catch(() => false);
    // Prefer regex text locator
    const permGate2 = await page.getByText(permissionGateText).first().isVisible().catch(() => false);
    if ((permGate || permGate2) && !mediaWarmup) {
      mediaWarmup = true;
      console.log("[teams] permission gate — media warm-up");
      await warmUpMedia(page);
    }

    await page.waitForTimeout(300);
  }

  console.warn(`[teams] pre-join readiness timeout after ${timeoutMs}ms url=${page.url()}`);
  return false;
}

async function clickJoinNow(page: Page): Promise<boolean> {
  // Prefer exact "Join now" (Vexa step 6)
  const joinNow = page.locator('button:has-text("Join now")').first();
  if (await joinNow.isVisible().catch(() => false)) {
    await joinNow.click();
    console.log('[teams] clicked "Join now"');
    return true;
  }
  if (await clickFirst(page, joinNowSelectors)) {
    console.log('[teams] clicked Join now (selector list)');
    return true;
  }
  if (await clickFirst(page, joinButtonFallbackSelectors, { timeout: 10_000 })) {
    console.log("[teams] clicked join (fallback)");
    return true;
  }
  return false;
}

/** After Join now: dismiss AV modal and re-click Join now (Vexa step 6c). */
async function handlePostJoinAvModal(page: Page): Promise<void> {
  for (let attempt = 0; attempt < 6; attempt++) {
    const dismissed = await dismissAvConfirmModal(page);
    if (dismissed) {
      const again = page.locator('button:has-text("Join now")').first();
      if (await again.isVisible().catch(() => false)) {
        await again.click().catch(() => {});
        console.log('[teams] re-clicked "Join now" after AV modal');
      }
    }

    const inLobby = await isInLobby(page);
    const admitted = await isAdmitted(page);
    if (inLobby || admitted) {
      console.log(inLobby ? "[teams] reached lobby" : "[teams] admitted after Join now");
      return;
    }
    if (!dismissed) {
      // no modal and not in lobby yet — keep polling a bit
    }
    await page.waitForTimeout(1000);
  }
}

async function isInLobby(page: Page): Promise<boolean> {
  for (const sel of lobbyIndicators) {
    if (await isVisible(page, sel, 400)) return true;
  }
  // Pre-join still showing Join now (Vexa treats this as waiting-room-ish)
  const joinNow = await isVisible(page, 'button:has-text("Join now")', 400);
  const leave = await isAdmitted(page);
  return joinNow && !leave;
}

async function isAdmitted(page: Page): Promise<boolean> {
  for (const sel of leaveButtonSelectors) {
    if (await isVisible(page, sel, 400)) {
      // Ensure not still on pre-join
      const joinNow = await isVisible(page, 'button:has-text("Join now")', 300);
      const lobby = await isVisible(page, lobbyIndicators[0], 300);
      if (!joinNow && !lobby) return true;
      // Leave visible and no lobby text — admitted
      if (!lobby) return true;
    }
  }
  return false;
}

async function isRejected(page: Page): Promise<boolean> {
  for (const sel of rejectionIndicators) {
    if (await isVisible(page, sel, 400)) return true;
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
    if (id.startsWith("http://") || id.startsWith("https://")) {
      return id;
    }
    throw new Error(
      "teams native_meeting_id alone is not enough; pass full meeting_url (Teams share link)",
    );
  },

  async join(ctx: JoinContext) {
    const { page, meetingUrl, botName, signal, joinTimeoutMs } = ctx;

    // Step 1 — navigate
    console.log(`[teams] navigate ${meetingUrl}`);
    await page.goto(meetingUrl, { waitUntil: "domcontentloaded", timeout: 60_000 });
    await page.waitForTimeout(500);

    // Step 2 — continue in browser
    if (await clickFirst(page, continueBrowserSelectors, { timeout: 10_000 })) {
      console.log("[teams] clicked Continue on this browser");
      await page.waitForTimeout(500);
    } else {
      console.log("[teams] Continue button not found, continuing…");
    }

    // Step 2.5 — pre-join readiness (incl. without-media modal)
    await waitForPreJoinReadiness(page, 45_000);

    // Step 3 — camera OFF (avoid any preview; no green test pattern if cam was on)
    if (await clickFirst(page, cameraOffSelectors)) {
      console.log("[teams] camera turned off");
    } else {
      // Already off shows "Turn on camera" — leave it
      console.log("[teams] camera off control not found (may already be off)");
    }

    // Step 4 — display name
    if (await fillFirst(page, nameInputSelectors, botName)) {
      console.log(`[teams] display name set to "${botName}"`);
    } else {
      console.log("[teams] name input not found");
    }

    // Step 5 — computer audio (capture needs this)
    try {
      const dontUse = page.locator(dontUseAudioSelectors.join(", ")).first();
      if (
        (await dontUse.isVisible().catch(() => false)) &&
        (await dontUse.getAttribute("aria-checked")) === "true"
      ) {
        await clickFirst(page, computerAudioSelectors);
      } else {
        await clickFirst(page, computerAudioSelectors);
      }
    } catch {
      /* optional */
    }

    // Step 6 — Join now
    const joined = await clickJoinNow(page);
    if (!joined) {
      console.warn("[teams] Join now not found — will wait for lobby/admission UI");
    }
    await page.waitForTimeout(1000);

    // Step 6c — AV confirmation modal after Join now (Vexa #467)
    await handlePostJoinAvModal(page);

    // Mute mic again in-call; re-assert camera off if toolbar is available
    try {
      await page.keyboard.press("Control+Shift+M");
    } catch {
      /* ignore */
    }
    await clickFirst(page, cameraOffSelectors);

    // Step 7 — wait for admission (lobby or Leave button)
    const deadline = Date.now() + joinTimeoutMs;
    let sawLobby = false;

    while (Date.now() < deadline) {
      if (signal.aborted) throw new Error("join aborted");

      if (await isTeamsAvConfirmModalVisible(page)) {
        await dismissAvConfirmModal(page);
        await clickJoinNow(page).catch(() => {});
      }

      if (await isRejected(page)) {
        throw new Error("Teams admission rejected by host");
      }

      if (await isAdmitted(page)) {
        console.log("[teams] admitted (Leave / hangup control visible)");
        return;
      }

      if (await isInLobby(page)) {
        if (!sawLobby) {
          sawLobby = true;
          console.log("[teams] in lobby — waiting for host to admit…");
        }
      }

      await page.waitForTimeout(2000);
    }

    throw new Error(
      "join timeout: still not in call (admit the bot from the Teams lobby?)",
    );
  },

  async isInCall(page: Page): Promise<boolean> {
    if (await isRejected(page)) return false;
    // Must not treat pre-join "Join now" as in-call
    if (await isInLobby(page)) return false;
    return isAdmitted(page);
  },

  async leave(page: Page): Promise<void> {
    const clicked = await clickFirst(page, leaveButtonSelectors);
    if (!clicked) {
      // Cancel in lobby
      await clickFirst(page, ['button:has-text("Cancel")', 'button[aria-label="Cancel"]']);
    }
    await page.waitForTimeout(1000);
  },
};

async function isTeamsAvConfirmModalVisible(page: Page): Promise<boolean> {
  return isVisible(page, continueWithoutMediaSelectors[0], 500);
}
