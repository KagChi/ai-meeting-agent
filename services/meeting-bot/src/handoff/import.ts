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

  const headers: Record<string, string> = {};
  if (config.meetingAgentApiKey) {
    headers["X-API-Key"] = config.meetingAgentApiKey;
  }

  const url = `${config.meetingAgentUrl}/import`;
  console.log(
    `[import] POST ${url} file=${name} bytes=${size} auth=${config.meetingAgentApiKey ? "yes" : "no"}`,
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
  console.log(`[import] ok job_id=${data.job_id} status=${data.status ?? "pending"}`);
  return { job_id: data.job_id, status: data.status ?? "pending" };
}
