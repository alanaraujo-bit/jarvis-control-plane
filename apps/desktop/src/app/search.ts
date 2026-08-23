import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./platform";

/**
 * Global Search (§51), mirroring `search::SearchResult` in Rust.
 *
 * `kind` and `subKind` are codes the UI localises (§65); `heading` and
 * `snippet` are the matched content itself and are shown as-is.
 */
export type SearchKind = "knowledge" | "note" | "mission" | "activity" | "conversation";

export interface SearchResult {
  kind: SearchKind;
  projectId: string | null;
  projectName: string | null;
  entityId: string;
  sessionId: string | null;
  sessionProvider: string | null;
  missionId: string | null;
  tsMs: number;
  subKind: string | null;
  label: string | null;
  heading: string;
  snippet: string;
}

export async function globalSearch(query: string): Promise<SearchResult[]> {
  if (!isTauri()) return [];
  return invoke<SearchResult[]>("global_search", { query });
}
