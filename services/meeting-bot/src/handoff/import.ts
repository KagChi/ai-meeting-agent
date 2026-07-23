import { config } from "../config";
import { log } from "../logger";
import type { Platform } from "../types";
import type { ChatMessage } from "../capture/teams-chat";

export interface ImportResult {
  job_id: string;
  status: string;
}

export interface ImportOptions {
  filePath: string;
  title?: string | null;
  /** Bot platform id: teams | zoom | google_meet — stored as Teams/Zoom/Meet */
  platform?: Platform | string | null;
  /** In-meeting chat for LLM context */
  chat?: ChatMessage[] | null;
}

/**
 * Upload local recording (+ optional chat) to meeting-agent POST /import.
 */
export async function importToMeetingAgent(
  options: ImportOptions,
): Promise<ImportResult> {
  const { filePath, title, platform, chat } = options;
  const file = Bun.file(filePath);
  if (!(await file.exists())) {
    throw new Error(`recording file missing: ${filePath}`);
  }
  const size = file.size;
  if (size < 256) {
    throw new Error(`recording file too small (${size} bytes): ${filePath}`);
  }

  const form = new FormData();
  const name = filePath.split("/").pop() || "recording.webm";
  form.append("file", file, name);
  if (title?.trim()) {
    form.append("title", title.trim());
  }
  if (platform?.toString().trim()) {
    form.append("platform", platform.toString().trim());
  }
  if (chat && chat.length > 0) {
    form.append("chat", JSON.stringify(chat));
  }

  const headers: Record<string, string> = {};
  if (config.meetingAgentApiKey) {
    headers["X-API-Key"] = config.meetingAgentApiKey;
  }

  const url = `${config.meetingAgentUrl}/import`;
  log.info(
    {
      url,
      file: name,
      bytes: size,
      platform: platform ?? null,
      chatMessages: chat?.length ?? 0,
      auth: Boolean(config.meetingAgentApiKey),
    },
    "import POST",
  );

  let res: Response;
  try {
    res = await fetch(url, {
      method: "POST",
      headers,
      body: form,
    });
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    throw new Error(
      `import network error to ${url}: ${msg}. ` +
        `Check: (1) MEETING_AGENT_URL is the Docker service name (e.g. http://meeting-agent-api:8080), ` +
        `not 127.0.0.1; (2) both containers share a network; (3) compose uses "environment:" not "environtment:".`,
    );
  }

  const text = await res.text();
  if (!res.ok) {
    throw new Error(`import failed HTTP ${res.status}: ${text.slice(0, 500)}`);
  }

  let data: { job_id?: string; status?: string };
  try {
    data = JSON.parse(text) as { job_id?: string; status?: string };
  } catch {
    throw new Error(`import response not JSON: ${text.slice(0, 200)}`);
  }
  if (!data.job_id) {
    throw new Error(`import response missing job_id: ${text.slice(0, 200)}`);
  }
  log.info({ jobId: data.job_id, status: data.status ?? "pending" }, "import ok");
  return { job_id: data.job_id, status: data.status ?? "pending" };
}
