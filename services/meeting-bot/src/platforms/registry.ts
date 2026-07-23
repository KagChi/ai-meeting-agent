import type { Platform, PlatformInfo } from "../types";
import type { PlatformAdapter } from "./types";
import { teamsAdapter } from "./teams/adapter";

const adapters = new Map<Platform, PlatformAdapter>([["teams", teamsAdapter]]);

const allPlatforms: Platform[] = ["teams", "zoom", "google_meet"];

export function getAdapter(platform: Platform): PlatformAdapter {
  const a = adapters.get(platform);
  if (!a) {
    throw new Error(
      `platform not implemented: ${platform}. Supported: ${listPlatforms()
        .filter((p) => p.status === "available")
        .map((p) => p.id)
        .join(", ")}`,
    );
  }
  return a;
}

export function listPlatforms(): PlatformInfo[] {
  return allPlatforms.map((id) => ({
    id,
    status: adapters.has(id) ? "available" : "not_implemented",
  }));
}

export function isSupported(platform: string): platform is Platform {
  return allPlatforms.includes(platform as Platform);
}
