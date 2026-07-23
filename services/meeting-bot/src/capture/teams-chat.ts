/**
 * Teams chat scraper — port of Vexa createTeamsChat.
 * Requires chat panel open for messages to be in the DOM.
 */
import type { Page } from "playwright";
import { log } from "../logger";

export interface ChatMessage {
  sender: string;
  text: string;
  /** ISO wall clock when bot first saw the message */
  ts?: string;
}

export class TeamsChatCapture {
  private page: Page;
  private messages: ChatMessage[] = [];
  private exposed = false;
  private started = false;

  constructor(page: Page) {
    this.page = page;
  }

  getMessages(): ChatMessage[] {
    return [...this.messages];
  }

  async start(): Promise<void> {
    if (this.started) return;

    if (!this.exposed) {
      await this.page.exposeFunction(
        "__meetingBotChatMessage",
        (msg: { sender?: string; text?: string }) => {
          const sender = (msg.sender || "Unknown").trim();
          const text = (msg.text || "").trim();
          if (!text) return;
          this.messages.push({
            sender,
            text,
            ts: new Date().toISOString(),
          });
          log.info(
            { sender, preview: text.slice(0, 80) },
            "teams chat message",
          );
        },
      );
      this.exposed = true;
    }

    await this.page.evaluate(() => {
      const w = window as unknown as {
        __meetingBotChatDestroy?: () => void;
        __meetingBotChatMessage?: (m: { sender: string; text: string }) => void;
      };
      try {
        w.__meetingBotChatDestroy?.();
      } catch {
        /* ignore */
      }

      const CONTAINER_SELECTORS = [
        '[data-tid="chat-pane-list"]',
        '[data-tid="message-pane-list-runway"]',
        '[data-tid="chatPaneMessageList"]',
        '[role="log"]',
        '[aria-label*="Chat messages"]',
        '[class*="chat-pane-list"]',
        '[class*="messageList"]',
      ];
      const MESSAGE_SELECTORS = [
        '[data-tid="chat-pane-message"]',
        'div[data-tid^="chat-pane-message"]',
        "div[data-mid]",
        '[data-tid="message"]',
        '[class*="chat-message"]',
        '[role="listitem"]',
      ];
      const SENDER_SELECTORS = [
        '[data-tid="message-author-name"]',
        '[data-tid*="author"]',
        '[class*="author-name"]',
        '[class*="authorName"]',
        '[class*="sender"]',
        '[class*="display-name"]',
      ];
      const TEXT_SELECTORS = [
        '[data-tid="messageBodyContent"]',
        '[id^="content-"]',
        '[class*="messageBody"]',
        '[class*="message-body"]',
        '[class*="messageText"]',
        'div[dir="auto"]',
      ];

      const seenNodes = new WeakSet<Element>();
      const seenHashes = new Set<string>();
      let container: Element | null = null;

      const firstText = (root: Element, selectors: string[]): string => {
        for (const s of selectors) {
          const el = root.querySelector(s);
          const t = el?.textContent?.trim();
          if (t) return t;
        }
        return "";
      };

      const senderFromAria = (node: Element): string => {
        let cur: Element | null = node;
        for (let i = 0; i < 4 && cur; i++, cur = cur.parentElement) {
          const al = cur.getAttribute?.("aria-label") || "";
          const m = al.match(/^(.+?)\s*,\s*\d{1,2}:\d{2}/);
          if (m?.[1]?.trim()) return m[1].trim();
        }
        return "";
      };

      const extract = (
        node: Element,
      ): { sender: string; text: string } | null => {
        let text = firstText(node, TEXT_SELECTORS);
        let sender = firstText(node, SENDER_SELECTORS);
        if (!sender) {
          let cur: Element | null = node.parentElement;
          for (let i = 0; i < 4 && cur && !sender; i++, cur = cur.parentElement) {
            sender = firstText(cur, SENDER_SELECTORS);
          }
        }
        if (!sender) sender = senderFromAria(node);
        if (!text) {
          const frags = Array.from(node.querySelectorAll("*"))
            .map((e) =>
              e.childElementCount === 0 ? (e.textContent || "").trim() : "",
            )
            .filter((t) => t.length > 0);
          if (!frags.length) return null;
          const longest = frags.reduce((a, b) => (b.length > a.length ? b : a), "");
          text = longest;
          if (!sender) {
            const shortName = frags.find(
              (f) =>
                f !== longest && f.length <= 40 && !/^\d{1,2}:\d{2}/.test(f),
            );
            if (shortName) sender = shortName;
          }
        }
        sender =
          sender.replace(/\s*\d{1,2}:\d{2}\s*(AM|PM)?\s*$/i, "").trim() ||
          "Unknown";
        if (!text) return null;
        return { sender, text };
      };

      const emit = (node: Element) => {
        if (seenNodes.has(node)) return;
        seenNodes.add(node);
        const msg = extract(node);
        if (!msg) return;
        const hash = `${msg.sender}\0${msg.text}`;
        if (seenHashes.has(hash)) return;
        seenHashes.add(hash);
        w.__meetingBotChatMessage?.(msg);
      };

      const scanMessages = (root: ParentNode) => {
        for (const sel of MESSAGE_SELECTORS) {
          const nodes = root.querySelectorAll(sel);
          if (nodes.length) {
            nodes.forEach((n) => emit(n));
            return;
          }
        }
      };

      const findContainer = (): Element | null => {
        for (const sel of CONTAINER_SELECTORS) {
          const el = document.querySelector(sel);
          if (el) return el;
        }
        return null;
      };

      const observer = new MutationObserver(() => {
        if (container) scanMessages(container);
      });

      const attach = () => {
        const found = findContainer();
        if (found && found !== container) {
          container = found;
          observer.disconnect();
          observer.observe(container, { childList: true, subtree: true });
          scanMessages(container);
        } else if (found && container) {
          scanMessages(container);
        }
      };
      attach();
      const poll = window.setInterval(attach, 2000);

      w.__meetingBotChatDestroy = () => {
        window.clearInterval(poll);
        observer.disconnect();
      };
    });

    this.started = true;
    log.info("teams chat capture started");
  }

  async stop(): Promise<ChatMessage[]> {
    if (!this.started) return this.getMessages();
    try {
      await this.page.evaluate(() => {
        const w = window as unknown as { __meetingBotChatDestroy?: () => void };
        try {
          w.__meetingBotChatDestroy?.();
        } catch {
          /* ignore */
        }
      });
    } catch {
      /* page gone */
    }
    this.started = false;
    log.info({ count: this.messages.length }, "teams chat capture stopped");
    return this.getMessages();
  }
}
