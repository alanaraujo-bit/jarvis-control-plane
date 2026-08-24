import { create } from "zustand";
import { invoke, isTauri } from "./platform";

/**
 * Notifications (§49) — the surface's half.
 *
 * The core decides *whether* an agent stopping is worth telling somebody
 * about; everything here is about saying it. The split matters: the words are
 * translated and the catalogues are TypeScript, so composing prose in Rust
 * would mean either shipping English to a Portuguese user or duplicating the
 * catalogue on both sides of the bridge.
 *
 * This store holds the list and the badge. Composing the sentence lives in
 * `describe`, and putting it on the screen lives in the components — so a
 * toast, a row in the centre and a Windows toast all say the same thing
 * without any of them owning the wording.
 */

/** Mirrors `notify::Kind`. */
export type NotificationKind = "needsApproval" | "finished" | "stopped";

/** Mirrors `notify::Reason`. One key per variant: `notify.title.<reason>`. */
export type NotificationReason =
  | "providerPrompt"
  | "guardrailPending"
  | "guardrailAsked"
  | "guardrailBlocked"
  | "turnEnded"
  | "missionCompleted"
  | "runCompleted"
  | "sessionEnded"
  | "sessionFailed"
  | "missionBlocked"
  | "runStopped";

/** Mirrors `session::event::Confidence` (§28). */
export type Confidence = "official" | "observed" | "estimated" | "unknown";

export interface Notification {
  id: number;
  tsMs: number;
  kind: NotificationKind;
  reason: NotificationReason;
  confidence: Confidence;
  projectId: string | null;
  projectName: string | null;
  sessionId: string | null;
  missionId: string | null;
  missionTitle: string | null;
  provider: string | null;
  /** The agent's own words, verbatim and untranslated. */
  preview: string | null;
  detailCode: string | null;
  seenAt: number | null;
  actedAt: number | null;
}

interface Centre {
  notifications: Notification[];
  outstanding: number;
  enabled: boolean;
}

interface NotificationsState {
  items: Notification[];
  outstanding: number;
  enabled: boolean;
  loaded: boolean;
  load: () => Promise<void>;
  /** Take one straight off the event channel, newest first. */
  receive: (notification: Notification) => void;
  markSeen: (ids: number[]) => Promise<void>;
  markAllSeen: () => Promise<void>;
  markActed: (id: number) => Promise<void>;
  clear: () => Promise<void>;
  setEnabled: (enabled: boolean) => void;
  /**
   * Tell the core what is on screen (§49).
   *
   * The core cannot see the window, so this is the one input to the
   * suppression rule it cannot work out for itself.
   */
  reportAttention: (focused: boolean, sessionIds: string[]) => void;
}

/** How many rows the panel keeps in memory. Matches the core's own limit. */
const LIMIT = 100;

export const useNotifications = create<NotificationsState>((set) => ({
  items: [],
  outstanding: 0,
  enabled: true,
  loaded: false,

  load: async () => {
    if (!isTauri()) {
      set({ loaded: true });
      return;
    }
    try {
      const centre = await invoke<Centre>("notifications_centre");
      set({
        items: centre.notifications,
        outstanding: centre.outstanding,
        enabled: centre.enabled,
        loaded: true,
      });
    } catch {
      // A notification centre that cannot load must not take the shell with
      // it. An empty bell is a worse product; a broken window is not a product.
      set({ loaded: true });
    }
  },

  receive: (notification) => {
    set((state) => {
      // The core deduplicates, but a reconnect can replay one. Keyed on id
      // rather than trusting arrival order.
      if (state.items.some((item) => item.id === notification.id)) return state;
      return {
        items: [notification, ...state.items].slice(0, LIMIT),
        outstanding: state.outstanding + (notification.seenAt === null ? 1 : 0),
      };
    });
  },

  markSeen: async (ids) => {
    if (ids.length === 0) return;
    const now = Date.now();
    // Optimistic: the badge must fall the moment the panel opens, not a round
    // trip later. The core's answer replaces the count either way.
    set((state) => ({
      items: state.items.map((item) =>
        ids.includes(item.id) && item.seenAt === null ? { ...item, seenAt: now } : item,
      ),
    }));
    if (!isTauri()) return;
    const outstanding = await invoke<number>("notifications_mark_seen", { ids });
    set({ outstanding });
  },

  markAllSeen: async () => {
    const now = Date.now();
    set((state) => ({
      items: state.items.map((item) => (item.seenAt === null ? { ...item, seenAt: now } : item)),
      outstanding: 0,
    }));
    if (!isTauri()) return;
    const outstanding = await invoke<number>("notifications_mark_all_seen");
    set({ outstanding });
  },

  markActed: async (id) => {
    if (!isTauri()) return;
    const outstanding = await invoke<number>("notifications_mark_acted", { id });
    const now = Date.now();
    set((state) => ({
      items: state.items.map((item) =>
        item.id === id ? { ...item, actedAt: now, seenAt: item.seenAt ?? now } : item,
      ),
      outstanding,
    }));
  },

  clear: async () => {
    set({ items: [], outstanding: 0 });
    if (isTauri()) await invoke("notifications_clear");
  },

  setEnabled: (enabled) => set({ enabled }),

  reportAttention: (focused, sessionIds) => {
    if (!isTauri()) return;
    void invoke("notifications_attention", { focused, sessionIds }).catch(() => {
      // Losing one report means the next notification is decided against
      // slightly stale attention. Not worth a surfaced error.
    });
  },
}));

/**
 * What the person is looking at, as two independent facts.
 *
 * Kept outside the store on purpose. These change on window focus and on every
 * tab switch, and neither is anything a component should re-render over — they
 * exist solely to be posted to the core. Putting them in the store would make
 * the whole shell re-render when a window loses focus.
 *
 * Both are reported together because the core's rule needs both: a visible
 * terminal in a window behind something else is not being watched.
 */
let windowFocused = true;
let visibleSessions: string[] = [];

/** Told whenever what is on screen changes, so a toast can stand down. */
const watchers = new Set<(ids: string[]) => void>();

function syncAttention() {
  useNotifications.getState().reportAttention(windowFocused, visibleSessions);
  for (const watcher of watchers) watcher(visibleSessions);
}

/**
 * Be told which sessions are on screen.
 *
 * Exists so a toast about a session can disappear the moment somebody goes to
 * that session. It has done its job at that point, and leaving it up means the
 * person has to dismiss a card telling them about the thing they are looking
 * at — which is the sort of small indignity that makes people switch a feature
 * off. Found by clicking through to a session in the real app and watching its
 * own toast sit there afterwards.
 */
export function onVisibleSessions(watcher: (ids: string[]) => void): () => void {
  watchers.add(watcher);
  return () => {
    watchers.delete(watcher);
  };
}

/** Report whether the window has focus. */
export function setWindowFocused(focused: boolean) {
  if (windowFocused === focused) return;
  windowFocused = focused;
  syncAttention();
}

/** Report which sessions are on screen. Pass `[]` when none are. */
export function setVisibleSessions(ids: string[]) {
  const same =
    ids.length === visibleSessions.length && ids.every((id, i) => id === visibleSessions[i]);
  if (same) return;
  visibleSessions = ids;
  syncAttention();
}

/** The status-dot vocabulary a kind belongs to, so it reads like the rest (§7). */
export function dotFor(kind: NotificationKind): "waiting" | "completed" | "failed" {
  switch (kind) {
    case "needsApproval":
      return "waiting";
    case "finished":
      return "completed";
    case "stopped":
      return "failed";
  }
}

/** The provider's own name, for a sentence that names who stopped. */
export function agentName(provider: string | null): string {
  switch (provider) {
    case "claude-code":
      return "Claude Code";
    case "codex":
      return "Codex";
    case "shell":
      return "Terminal";
    default:
      return "Agent";
  }
}
