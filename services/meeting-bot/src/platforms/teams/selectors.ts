/**
 * Teams web UI selectors — isolated so UI breakage is one-file maintenance.
 * Prefer role/text; CSS is fallback.
 */
export const teamsSelectors = {
  // Guest / pre-join
  nameInput: [
    'input[data-tid="prejoin-display-name-input"]',
    'input[placeholder*="name" i]',
    'input[aria-label*="name" i]',
    "#username",
  ],
  joinButton: [
    'button[data-tid="prejoin-join-button"]',
    'button:has-text("Join now")',
    'button:has-text("Join")',
    'button:has-text("Join meeting")',
  ],
  continueOnBrowser: [
    'a:has-text("Continue on this browser")',
    'button:has-text("Continue on this browser")',
    'a:has-text("Join on the web instead")',
  ],
  // In-call heuristics
  leaveButton: [
    'button[data-tid="hangup-button"]',
    'button[aria-label*="Leave" i]',
    'button:has-text("Leave")',
  ],
  // Lobby / waiting
  lobbyText: [
    "text=/someone will let you in/i",
    "text=/waiting/i",
    "text=/lobby/i",
  ],
  // Mic/cam toggles (best-effort off)
  micToggle: ['button[aria-label*="microphone" i]', 'button[data-tid*="mic" i]'],
  camToggle: ['button[aria-label*="camera" i]', 'button[data-tid*="camera" i]'],
} as const;
