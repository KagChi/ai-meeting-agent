import type { Page } from "playwright";
import type { Platform } from "../types";

export interface JoinContext {
  meetingUrl: string;
  botName: string;
  page: Page;
  signal: AbortSignal;
  joinTimeoutMs: number;
}

export interface PlatformAdapter {
  readonly id: Platform;
  /** Resolve a full join URL from url and/or native id. */
  resolveUrl(input: {
    meetingUrl?: string;
    nativeMeetingId?: string;
  }): string;
  /** Navigate and join until in-call (or throw). */
  join(ctx: JoinContext): Promise<void>;
  isInCall(page: Page): Promise<boolean>;
  leave(page: Page): Promise<void>;
}
