export type Platform = "teams" | "zoom" | "google_meet";

export type BotJobStatus =
  | "queued"
  | "joining"
  | "in_call"
  | "recording"
  | "uploading"
  | "completed"
  | "failed";

export interface BotJob {
  id: string;
  platform: Platform;
  status: BotJobStatus;
  meeting_url: string | null;
  native_meeting_id: string | null;
  bot_name: string | null;
  title: string | null;
  recording_path: string | null;
  meeting_agent_job_id: string | null;
  error: string | null;
  created_at: string;
  updated_at: string;
}

export interface CreateBotRequest {
  platform: Platform;
  meeting_url?: string;
  native_meeting_id?: string;
  bot_name?: string;
  title?: string;
}

export interface PlatformInfo {
  id: Platform;
  status: "available" | "not_implemented";
}
