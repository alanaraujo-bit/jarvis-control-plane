import { useEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronRight, TriangleAlert, X } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "../../app/platform";
import { useT } from "../../app/i18n";
import "./ConversationView.css";

export type Role = "user" | "assistant" | "system";

export interface TokenUsage {
  input: number | null;
  output: number | null;
  cacheRead: number | null;
  cacheWrite: number | null;
  costUsd: number | null;
  model: string | null;
  /** Provenance travels with the number; older logs may omit it. */
  confidence?: "official" | "observed" | "estimated" | "unknown";
}

export type ConversationItem =
  | { kind: "message"; role: Role; text: string; tsMs: number; usage: TokenUsage | null }
  | { kind: "thinking"; text: string; tsMs: number }
  | { kind: "toolCall"; id: string; name: string; summary: string; tsMs: number }
  | { kind: "toolResult"; id: string; ok: boolean; summary: string; tsMs: number }
  | { kind: "error"; message: string; tsMs: number };

/** Refresh cadence while a session is live. */
const POLL_MS = 700;

function formatTime(tsMs: number, locale: string): string {
  return new Date(tsMs).toLocaleTimeString(locale, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatTokens(value: number): string {
  if (value < 1000) return String(value);
  return `${(value / 1000).toFixed(value < 10_000 ? 1 : 0)}k`;
}

/**
 * Conversation View (§24).
 *
 * The structured projection of the same session the terminal shows — same log,
 * same order, different rendering. Deliberately not a chat clone: development
 * is made of prompts, tool calls, results and cost, so each is rendered as
 * itself rather than flattened into speech bubbles.
 */
export function ConversationView({ sessionId, live }: { sessionId: string; live: boolean }) {
  const t = useT();
  const [items, setItems] = useState<ConversationItem[]>([]);
  const [loaded, setLoaded] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const pinnedToBottom = useRef(true);

  useEffect(() => {
    if (!isTauri()) {
      setLoaded(true);
      return;
    }
    let cancelled = false;

    const read = async () => {
      try {
        const next = await invoke<ConversationItem[]>("session_conversation", { sessionId });
        if (!cancelled) {
          setItems(next);
          setLoaded(true);
        }
      } catch {
        if (!cancelled) setLoaded(true);
      }
    };

    void read();
    // Polling rather than a push channel: the conversation grows in bursts of a
    // few items, and a poll keeps the view identical whether the session is
    // live, reattached, or being read long after it ended.
    const timer = live ? window.setInterval(() => void read(), POLL_MS) : undefined;
    return () => {
      cancelled = true;
      if (timer) window.clearInterval(timer);
    };
  }, [sessionId, live]);

  // Follow new output only when the reader is already at the bottom, so
  // scrolling back to read something is never yanked away.
  useEffect(() => {
    const el = scrollRef.current;
    if (el && pinnedToBottom.current) el.scrollTop = el.scrollHeight;
  }, [items]);

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    pinnedToBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  };

  // Attach each tool result to the call it belongs to, so they render as one
  // unit instead of drifting apart in the timeline.
  const rows = useMemo(() => {
    const resultFor = new Map<string, Extract<ConversationItem, { kind: "toolResult" }>>();
    for (const item of items) {
      if (item.kind === "toolResult") resultFor.set(item.id, item);
    }
    return items
      .filter((item) => item.kind !== "toolResult")
      .map((item) => ({
        item,
        result: item.kind === "toolCall" ? resultFor.get(item.id) : undefined,
      }));
  }, [items]);

  if (loaded && items.length === 0) {
    return (
      <div className="conv conv--empty">
        <p className="conv__empty-title">{t("conversation.empty.title")}</p>
        <p className="conv__empty-body">{t("conversation.empty.body")}</p>
      </div>
    );
  }

  return (
    <div className="conv" ref={scrollRef} onScroll={onScroll}>
      <div className="conv__inner">
        {rows.map(({ item, result }, index) => (
          <Row key={index} item={item} result={result} />
        ))}
      </div>
    </div>
  );
}

function Row({
  item,
  result,
}: {
  item: ConversationItem;
  result?: Extract<ConversationItem, { kind: "toolResult" }>;
}) {
  const t = useT();
  const [thinkingOpen, setThinkingOpen] = useState(false);

  switch (item.kind) {
    case "message": {
      if (!item.text.trim()) return null;
      return (
        <article className="conv__turn" data-role={item.role}>
          <header className="conv__meta">
            <span className="conv__who">
              {item.role === "user" ? t("conversation.you") : item.usage?.model ?? t("conversation.agent")}
            </span>
            <time className="conv__time">{formatTime(item.tsMs, navigator.language)}</time>
          </header>
          <div className="conv__text selectable">{item.text}</div>
          {item.usage && <Usage usage={item.usage} />}
        </article>
      );
    }

    case "thinking":
      return (
        <div className="conv__thinking">
          <button
            type="button"
            className="conv__thinking-toggle"
            onClick={() => setThinkingOpen((open) => !open)}
            aria-expanded={thinkingOpen}
          >
            <ChevronRight
              size={12}
              strokeWidth={2}
              className="conv__chevron"
              data-open={thinkingOpen || undefined}
              aria-hidden="true"
            />
            {t("conversation.thinking")}
          </button>
          {thinkingOpen && <div className="conv__thinking-body selectable">{item.text}</div>}
        </div>
      );

    case "toolCall":
      return (
        <div className="conv__tool">
          <div className="conv__tool-inner">
            <span className="conv__tool-name">{item.name}</span>
            <code className="conv__tool-summary selectable">{item.summary}</code>
            {result && (
              <span className="conv__tool-result" data-ok={result.ok || undefined}>
                {result.ok ? (
                  <Check size={11} strokeWidth={2.4} aria-hidden="true" />
                ) : (
                  <X size={11} strokeWidth={2.4} aria-hidden="true" />
                )}
              </span>
            )}
          </div>
        </div>
      );

    case "error":
      return (
        <div className="conv__error">
          <TriangleAlert size={13} strokeWidth={2} aria-hidden="true" />
          <span className="selectable">{item.message}</span>
        </div>
      );

    default:
      return null;
  }
}

/**
 * Token usage for a turn.
 *
 * Shown quietly, and only from figures the provider reported — this is
 * `Official` data in the confidence model, never an estimate dressed up as one
 * (§28).
 */
function Usage({ usage }: { usage: TokenUsage }) {
  const t = useT();
  const parts: string[] = [];
  if (usage.input != null) parts.push(`${formatTokens(usage.input)} ${t("conversation.in")}`);
  if (usage.output != null) parts.push(`${formatTokens(usage.output)} ${t("conversation.out")}`);
  if (usage.cacheRead) parts.push(`${formatTokens(usage.cacheRead)} ${t("conversation.cached")}`);
  if (parts.length === 0) return null;

  return (
    <div className="conv__usage" title={t("conversation.usageOfficial")}>
      {parts.join(" · ")}
    </div>
  );
}
