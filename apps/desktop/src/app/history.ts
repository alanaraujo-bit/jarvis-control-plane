import { invoke, isTauri } from "./platform";

/**
 * Session History bridge (§88).
 *
 * Mirrors `history::Entry` and friends on the Rust side. Nothing is computed
 * here that the core already knows — a row arrives complete, including how many
 * bytes its log takes on disk, because that is a fact about the filesystem and
 * the webview cannot see it.
 */

/** Which of the three named a session. Never inferred from the title (D36). */
export type TitleSource = "user" | "provider" | "derived";

export interface HistoryEntry {
  id: string;
  projectId: string;
  projectName: string;
  provider: string;
  title: string | null;
  titleSource: TitleSource | null;
  state: string;
  createdAt: number;
  endedAt: number | null;
  missionId: string | null;
  missionTitle: string | null;
  /** How many times a person said something. */
  turns: number;
  /** Every structured item recorded in the session. */
  events: number;
  /** `null` means no provider reported any — which is not zero. */
  tokens: number | null;
  bytes: number;
  live: boolean;
  /** Set only on a search hit: the line that matched, in context. */
  snippet: string | null;
}

export interface HistoryQuery {
  text?: string;
  projectId?: string;
  provider?: string;
  sinceMs?: number;
  beforeTs?: number;
  beforeId?: string;
}

export interface HistoryPage {
  entries: HistoryEntry[];
  hasMore: boolean;
  /** True when this page answers a text query. */
  searched: boolean;
}

export interface HistoryStorage {
  sessions: number;
  bytes: number;
}

export interface Deleted {
  bytesFreed: number;
  logRemoved: boolean;
}

const EMPTY_PAGE: HistoryPage = { entries: [], hasMore: false, searched: false };

export async function historyPage(query: HistoryQuery): Promise<HistoryPage> {
  if (!isTauri()) return EMPTY_PAGE;
  return invoke<HistoryPage>("history_page", { query });
}

export async function historyEntry(sessionId: string): Promise<HistoryEntry | null> {
  if (!isTauri()) return null;
  return invoke<HistoryEntry | null>("history_entry", { sessionId });
}

/**
 * Rename a session. Returns the name as stored — trimmed and clipped by the
 * core, so the row shows what is actually on disk rather than what was typed.
 */
export async function historyRename(sessionId: string, title: string): Promise<string> {
  return invoke<string>("history_rename", { sessionId, title });
}

export async function historyDelete(sessionId: string): Promise<Deleted> {
  return invoke<Deleted>("history_delete", { sessionId });
}

export async function historyStorage(): Promise<HistoryStorage> {
  if (!isTauri()) return { sessions: 0, bytes: 0 };
  return invoke<HistoryStorage>("history_storage");
}

export async function historyProviders(): Promise<string[]> {
  if (!isTauri()) return [];
  return invoke<string[]>("history_providers");
}
