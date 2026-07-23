/**
 * Teams web UI selectors — aligned with Vexa `core/meetings/modules/join/src/msteams/selectors.ts`
 * (join / admission / leave). Keep free of runtime logic.
 */

/** "Continue on this browser" (desktop app interstitial). */
export const continueBrowserSelectors = [
  'button:has-text("Continue on this browser")',
  'a:has-text("Continue on this browser")',
  'button:has-text("Join on the web instead")',
  'a:has-text("Join on the web instead")',
  'button:has-text("Continue")',
] as const;

/**
 * "Continue without audio or video" — blocks prejoin when media is denied.
 * Must dismiss or Join now never enables (Vexa #467 / intermittent Chromium).
 */
export const continueWithoutMediaSelectors = [
  'button:has-text("Continue without audio or video")',
  'button[aria-label="Continue without audio or video"]',
  'button[aria-label*="Continue without audio"]',
  '[role="dialog"] button:has-text("Continue without audio or video")',
  '[role="alertdialog"] button:has-text("Continue without audio or video")',
] as const;

/** Prefer "Join now" first — generic "Join" is ambiguous. */
export const joinNowSelectors = [
  'button:has-text("Join now")',
  'button[aria-label*="Join now"]',
] as const;

export const joinButtonFallbackSelectors = [
  'button:has-text("Join now")',
  'button:has-text("Join")',
  'button[data-tid="prejoin-join-button"]',
] as const;

export const nameInputSelectors = [
  'input[data-tid="prejoin-display-name-input"]',
  'input[placeholder*="name" i]',
  'input[placeholder*="Name"]',
  'input[type="text"]',
] as const;

export const cameraOffSelectors = [
  'button[aria-label*="Turn off camera"]',
  'button[aria-label*="Turn camera off"]',
  'button[aria-label*="Turn off video"]',
  'button[aria-label*="Turn video off"]',
  'button[aria-label="Turn off camera"]',
  'button[aria-label="Turn off video"]',
] as const;

export const cameraOnSelectors = [
  'button[aria-label*="Turn on camera"]',
  'button[aria-label*="Turn camera on"]',
  'button[aria-label*="Turn on video"]',
  'button[aria-label*="Turn video on"]',
] as const;

export const computerAudioSelectors = [
  'radio[aria-label*="Computer audio"]',
  '[role="radio"][aria-label*="Computer audio"]',
  'radio:has-text("Computer audio")',
] as const;

export const dontUseAudioSelectors = [
  'radio[aria-label*="Don\'t use audio"]',
  '[role="radio"][aria-label*="Don\'t use audio"]',
  'radio:has-text("Don\'t use audio")',
] as const;

/** In-meeting leave/hangup — primary admission success signal (Vexa). */
export const leaveButtonSelectors = [
  'button[id="hangup-button"]',
  'button[data-tid="hangup-main-btn"]',
  'button[aria-label="Leave"]',
  'button[aria-label*="Leave"]',
  '[role="toolbar"] button[aria-label*="Leave"]',
] as const;

/** Lobby / still waiting (do not treat as in-call). */
export const lobbyIndicators = [
  'text=Someone will let you in shortly',
  'text=You\'re in the lobby',
  'text=Waiting for someone to let you in',
  'text=Waiting to be admitted',
  'text=Your request to join has been sent',
  'text=Please wait until someone admits you',
] as const;

export const rejectionIndicators = [
  'text=Sorry, but you were denied',
  'text=You were denied entry',
  'text=Access denied',
  'text=Admission denied',
] as const;

export const permissionGateText =
  /Select Allow to let Microsoft Teams use your mic and camera/i;
