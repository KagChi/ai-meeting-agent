import { config } from "../config";

export interface ImportResult {
  job_id: string;
  status: string;
}

/**
 * Upload local recording to meeting-agent POST /import.
 */
export async function importToMeetingAgent(
  filePath: string,
  title?: string | null,
): Promise<ImportResult> {
  const form = new FormData();
  const file = Bun.file(filePath);
  if (!(await file.exists())) {
    throw new Error(`recording file missing: ${filePath}`);
  }
  const name = filePath.split("/").pop() || "recording.wav";
  form.append("file", file, name);
  if (title?.trim()) {
    form.append("title", title.trim());
  }

  const headers: Record<string, string> = {};
  if (config.meetingAgentApiKey) {
    headers["X-API-Key"] = config.meetingAgentApiKey;
  }

  const res = await fetch(`${config.meetingAgentUrl}/import`, {
    method: "POST",
    headers,
    body: form,
  });

  const text = await res.text();
  if (!res.ok) {
    throw new Error(`import failed HTTP ${res.status}: ${text.slice(0, 500)}`);
  }

  const data = JSON.parse(text) as { job_id?: string; status?: string };
  if (!data.job_id) {
    throw new Error(`import response missing job_id: ${text.slice(0, 200)}`);
  }
  return { job_id: data.job_id, status: data.status ?? "pending" };
}
