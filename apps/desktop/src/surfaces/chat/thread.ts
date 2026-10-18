/**
 * The conversation as a chat reads it (M29): what the person said, what the
 * agent answered, and the work in between folded into one line.
 *
 * Built from the session's own log (`session_conversation`), the same items
 * Conversation View reads, so a chat is the same whether it is live, reopened
 * or found in History.
 */

/** The log's conversation items, as `session_conversation` returns them. */
export type ChatItem =
  | {
      kind: "message";
      role: "user" | "assistant" | "system";
      text: string;
      tsMs: number;
      usage: { model: string | null } | null;
    }
  | { kind: "thinking"; text: string; tsMs: number }
  | { kind: "toolCall"; id: string; name: string; summary: string; tsMs: number }
  | { kind: "toolResult"; id: string; ok: boolean; summary: string; tsMs: number }
  | { kind: "error"; message: string; tsMs: number }
  | { kind: "turnEnded"; reason: string; tsMs: number };

export interface Step {
  name: string;
  summary: string;
  /** `null` until the result comes back. */
  ok: boolean | null;
  thinking: boolean;
}

export type Entry =
  | { type: "user"; text: string; tsMs: number }
  | { type: "assistant"; text: string; tsMs: number; model: string | null }
  | { type: "activity"; steps: Step[]; tsMs: number }
  | { type: "error"; message: string; tsMs: number };

export function buildThread(items: ChatItem[]): Entry[] {
  const entries: Entry[] = [];
  let activity: Extract<Entry, { type: "activity" }> | null = null;
  const calls = new Map<string, Step>();

  const work = (tsMs: number) => {
    if (!activity) {
      activity = { type: "activity", steps: [], tsMs };
      entries.push(activity);
    }
    return activity;
  };

  for (const item of items) {
    switch (item.kind) {
      case "message": {
        if (item.role === "system" || !item.text.trim()) break;
        activity = null;
        entries.push(
          item.role === "user"
            ? { type: "user", text: item.text, tsMs: item.tsMs }
            : { type: "assistant", text: item.text, tsMs: item.tsMs, model: item.usage?.model ?? null },
        );
        break;
      }
      case "thinking": {
        const steps = work(item.tsMs).steps;
        // Several thoughts in a row are one "thought about it".
        if (!steps.at(-1)?.thinking) steps.push({ name: "thinking", summary: "", ok: null, thinking: true });
        break;
      }
      case "toolCall": {
        const step: Step = { name: item.name, summary: item.summary, ok: null, thinking: false };
        work(item.tsMs).steps.push(step);
        calls.set(item.id, step);
        break;
      }
      case "toolResult": {
        const step = calls.get(item.id);
        if (step) step.ok = item.ok;
        break;
      }
      case "error":
        activity = null;
        entries.push({ type: "error", message: item.message, tsMs: item.tsMs });
        break;
      case "turnEnded":
        activity = null;
        break;
    }
  }
  return entries;
}

/** The reply being written right now, from the protocol's stream. */
export interface Draft {
  text: string;
  startedAt: number;
  /** When the stream said this block was complete; `null` while writing. */
  doneAt: number | null;
}

/** A message the person sent that the log has not shown yet. */
export interface Sent {
  text: string;
  at: number;
}

/** A message as it leaves, stamped so the log's copy can be matched to it. */
export function sentNow(text: string): Sent {
  return { text, at: Date.now() };
}

/** How long a finished draft waits for the log before it is let go anyway. */
const DRAFT_GRACE_MS = 10_000;
/** Provider clocks and ours are the same machine; this absorbs rounding. */
const SKEW_MS = 2_000;

/**
 * Drop what the log has caught up with: the streamed draft once the durable
 * reply with the same text is there, each optimistic send once its message is.
 * Matching on text keeps the screen from flickering between the two copies.
 */
export function reconcile(
  entries: Entry[],
  draft: Draft | null,
  sent: Sent[],
  now: number,
): { draft: Draft | null; sent: Sent[] } {
  let kept: Draft | null = draft;
  if (draft) {
    const landed = entries.some(
      (entry) =>
        entry.type === "assistant" &&
        entry.tsMs >= draft.startedAt - SKEW_MS &&
        entry.text.trim() === draft.text.trim(),
    );
    const stale = draft.doneAt !== null && now - draft.doneAt > DRAFT_GRACE_MS;
    if (landed || stale) kept = null;
  }

  const used = new Set<Entry>();
  const remaining = sent.filter((message) => {
    const match = entries.find(
      (entry) =>
        entry.type === "user" &&
        !used.has(entry) &&
        entry.tsMs >= message.at - SKEW_MS &&
        entry.text.trim() === message.text.trim(),
    );
    if (match) used.add(match);
    return !match;
  });

  return { draft: kept, sent: remaining };
}
