import { invoke } from "@tauri-apps/api/core";

/** Mirrors `AppVersion` in src-tauri/src/lib.rs. */
export interface AppVersion {
  app: string;
  manifest_schema: number;
}

export function appVersion(): Promise<AppVersion> {
  return invoke<AppVersion>("app_version");
}
