import { create } from "zustand";
import {
  historyDelete,
  historyPage,
  historyProviders,
  historyRename,
  historyStorage,
  type HistoryEntry,
  type HistoryQuery,
  type HistoryStorage,
} from "../../app/history";

/**
 * The history surface's own state (§88).
 *
 * Paging lives here rather than in the component because it survives navigating
 * away and back — somebody who scrolled four pages into last month and stepped
 * into a session does not want to do it again on their way back.
 *
 * ## The two races this has to be right about
 *
 * A search box fires a request per keystroke, and the network is not ordered.
 * `token` is bumped on every new query and every reply checks it before landing,
 * so a slow reply for "log" can never overwrite a fast one for "login". The
 * same counter also covers a "load more" that arrives after the filters changed
 * underneath it, which would otherwise append last query's rows to this one's.
 */
export type Range = "all" | "today" | "week" | "month";

interface Filters {
  text: string;
  projectId: string | null;
  provider: string | null;
  range: Range;
}

interface HistoryState {
  entries: HistoryEntry[];
  hasMore: boolean;
  searched: boolean;
  loading: boolean;
  /** True only while appending, so the list does not blank out under a page 2. */
  loadingMore: boolean;
  error: string | null;
  filters: Filters;
  providers: string[];
  storage: HistoryStorage | null;
  /** The row being renamed, if any. Held here so it survives a re-read. */
  renaming: string | null;
  /** The row whose delete is awaiting confirmation. */
  confirmingDelete: string | null;

  load: () => Promise<void>;
  loadMore: () => Promise<void>;
  setFilters: (patch: Partial<Filters>) => void;
  beginRename: (sessionId: string | null) => void;
  rename: (sessionId: string, title: string) => Promise<void>;
  askDelete: (sessionId: string | null) => void;
  remove: (sessionId: string) => Promise<void>;
}

const DAY = 86_400_000;

/**
 * The instant a range starts, or `undefined` for all of it.
 *
 * "Today" is midnight local, not twenty-four hours ago: somebody at 09:00
 * asking what happened today means since they woke up, not since yesterday
 * morning. Week and month are rolling, because "this month" on the 1st would
 * be an almost empty list.
 */
function since(range: Range): number | undefined {
  const now = new Date();
  switch (range) {
    case "today": {
      const midnight = new Date(now.getFullYear(), now.getMonth(), now.getDate());
      return midnight.getTime();
    }
    case "week":
      return now.getTime() - 7 * DAY;
    case "month":
      return now.getTime() - 30 * DAY;
    default:
      return undefined;
  }
}

function queryFrom(filters: Filters): HistoryQuery {
  const query: HistoryQuery = {};
  const text = filters.text.trim();
  if (text) query.text = text;
  if (filters.projectId) query.projectId = filters.projectId;
  if (filters.provider) query.provider = filters.provider;
  const from = since(filters.range);
  if (from !== undefined) query.sinceMs = from;
  return query;
}

/** Bumped on every query. A reply carrying a stale token is dropped. */
let token = 0;

export const useHistory = create<HistoryState>((set, get) => ({
  entries: [],
  hasMore: false,
  searched: false,
  loading: false,
  loadingMore: false,
  error: null,
  filters: { text: "", projectId: null, provider: null, range: "all" },
  providers: [],
  storage: null,
  renaming: null,
  confirmingDelete: null,

  load: async () => {
    const mine = ++token;
    set({ loading: true, error: null });
    try {
      const page = await historyPage(queryFrom(get().filters));
      if (mine !== token) return;
      set({
        entries: page.entries,
        hasMore: page.hasMore,
        searched: page.searched,
        loading: false,
      });
    } catch (error) {
      if (mine !== token) return;
      set({ loading: false, error: String(error) });
    }

    // The facets and the disk figure change far more slowly than the list, and
    // neither is worth blocking the rows on — so they are read alongside rather
    // than awaited before anything renders.
    void historyProviders()
      .then((providers) => set({ providers }))
      .catch(() => {});
    void historyStorage()
      .then((storage) => set({ storage }))
      .catch(() => {});
  },

  loadMore: async () => {
    const { entries, hasMore, loading, loadingMore, filters } = get();
    if (!hasMore || loading || loadingMore) return;
    const last = entries[entries.length - 1];
    if (!last) return;

    const mine = token;
    set({ loadingMore: true });
    try {
      const page = await historyPage({
        ...queryFrom(filters),
        beforeTs: last.createdAt,
        beforeId: last.id,
      });
      // The filters may have changed while this was in flight. Appending now
      // would mix two different questions' answers into one list.
      if (mine !== token) return;
      set((state) => ({
        entries: [...state.entries, ...page.entries],
        hasMore: page.hasMore,
        loadingMore: false,
      }));
    } catch (error) {
      if (mine !== token) return;
      set({ loadingMore: false, error: String(error) });
    }
  },

  setFilters: (patch) => {
    set((state) => ({ filters: { ...state.filters, ...patch } }));
    void get().load();
  },

  beginRename: (sessionId) => set({ renaming: sessionId, confirmingDelete: null }),

  rename: async (sessionId, title) => {
    try {
      const stored = await historyRename(sessionId, title);
      // Patched in place rather than re-read: a re-read would re-run the whole
      // paged query and throw away everything already scrolled to.
      set((state) => ({
        renaming: null,
        entries: state.entries.map((entry) =>
          entry.id === sessionId
            ? { ...entry, title: stored, titleSource: "user" as const }
            : entry,
        ),
      }));
    } catch (error) {
      set({ renaming: null, error: String(error) });
    }
  },

  askDelete: (sessionId) => set({ confirmingDelete: sessionId, renaming: null }),

  remove: async (sessionId) => {
    try {
      const outcome = await historyDelete(sessionId);
      set((state) => ({
        confirmingDelete: null,
        entries: state.entries.filter((entry) => entry.id !== sessionId),
        // Kept honest rather than re-read: the core just said exactly how many
        // bytes went, so the figure on screen moves by that and not by an
        // estimate. A full re-read would also be a second disk walk.
        storage: state.storage
          ? {
              sessions: state.storage.sessions - 1,
              bytes: Math.max(0, state.storage.bytes - outcome.bytesFreed),
            }
          : state.storage,
      }));
    } catch (error) {
      set({ confirmingDelete: null, error: String(error) });
    }
  },
}));
