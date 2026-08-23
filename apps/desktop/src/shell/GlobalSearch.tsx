import { useEffect, useMemo, useRef, useState } from "react";
import {
  Activity,
  BookOpen,
  History,
  MessageSquare,
  Search,
  StickyNote,
  Target,
  type LucideIcon,
} from "lucide-react";
import type { MessageKey } from "@jarvis/i18n";
import { useT } from "../app/i18n";
import { globalSearch, type SearchKind, type SearchResult } from "../app/search";
import "./GlobalSearch.css";

/** How long to wait after the last keystroke before asking the core. */
const DEBOUNCE_MS = 220;

const GROUP_ICON: Record<SearchKind, LucideIcon> = {
  conversation: MessageSquare,
  knowledge: BookOpen,
  note: StickyNote,
  mission: Target,
  activity: Activity,
};

/** The order these answer "where did I see that" best in, not alphabetical. */
const GROUP_ORDER: SearchKind[] = ["conversation", "knowledge", "mission", "note", "activity"];

function formatWhen(tsMs: number, locale: string): string {
  const date = new Date(tsMs);
  const sameDay = date.toDateString() === new Date().toDateString();
  return sameDay
    ? date.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" })
    : date.toLocaleString(locale, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

/** A translated label, or the raw code when no translation exists (§65) —
 * the same fallback `Activity` already relies on for a kind it does not know. */
function labelOr(t: ReturnType<typeof useT>, key: string, fallback: string): string {
  const label = t(key as MessageKey);
  return label === key ? fallback : label;
}

/** A short, human line under the heading: who said it, or what state it is in. */
function subtitle(t: ReturnType<typeof useT>, result: SearchResult): string | null {
  switch (result.kind) {
    case "mission":
      return result.subKind ? labelOr(t, `state.${result.subKind}`, result.subKind) : null;
    case "activity":
      return result.subKind ? labelOr(t, `activity.kind.${result.subKind}`, result.subKind) : null;
    case "knowledge":
      return result.subKind ? labelOr(t, `brain.kind.${result.subKind}`, result.subKind) : null;
    case "conversation":
      if (result.subKind === "message") {
        return result.label === "user" ? t("conversation.you") : t("conversation.agent");
      }
      if (result.subKind === "thinking") return t("conversation.thinking");
      // A tool's own name is a proper noun, not a translation key.
      return result.label ?? null;
    case "note":
      return result.label === "pinned" ? "📌" : null;
    default:
      return null;
  }
}

interface GlobalSearchProps {
  open: boolean;
  onClose: () => void;
  onSelect: (result: SearchResult) => void;
}

export function GlobalSearch({ open, onClose, onSelect }: GlobalSearchProps) {
  const t = useT();
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [index, setIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (open) {
      setQuery("");
      setResults([]);
      setIndex(0);
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  useEffect(() => {
    const trimmed = query.trim();
    if (trimmed.replace(/\s+/g, "").length < 2) {
      setResults([]);
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void globalSearch(trimmed).then((next) => {
        if (!cancelled) setResults(next);
      });
    }, DEBOUNCE_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [query]);

  useEffect(() => setIndex(0), [results]);

  useEffect(() => {
    listRef.current
      ?.querySelector<HTMLElement>('[data-active="true"]')
      ?.scrollIntoView({ block: "nearest" });
  }, [index]);

  const grouped = useMemo(() => {
    const byKind = new Map<SearchKind, SearchResult[]>();
    for (const result of results) {
      const bucket = byKind.get(result.kind) ?? [];
      bucket.push(result);
      byKind.set(result.kind, bucket);
    }
    return GROUP_ORDER.filter((kind) => byKind.has(kind)).map((kind) => ({
      kind,
      items: byKind.get(kind)!,
    }));
  }, [results]);

  if (!open) return null;

  const selectAt = (position: number) => {
    const result = results[position];
    if (!result) return;
    onClose();
    onSelect(result);
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      setIndex((i) => (results.length ? (i + 1) % results.length : 0));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setIndex((i) => (results.length ? (i - 1 + results.length) % results.length : 0));
    } else if (event.key === "Enter") {
      event.preventDefault();
      selectAt(index);
    }
  };

  const trimmed = query.trim();
  const tooShort = trimmed.replace(/\s+/g, "").length < 2 && trimmed.length > 0;

  // Flat position of each row, so keyboard traversal ignores group boundaries.
  let position = -1;

  return (
    <div className="gsearch__scrim" onMouseDown={onClose} role="presentation">
      <div
        className="gsearch"
        role="dialog"
        aria-modal="true"
        aria-label={t("search.title")}
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={onKeyDown}
      >
        <div className="gsearch__field">
          <Search size={14} strokeWidth={2} aria-hidden="true" />
          <input
            ref={inputRef}
            className="gsearch__input"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t("search.placeholder")}
            spellCheck={false}
            autoComplete="off"
          />
          <span className="gsearch__scope">{t("search.everyProject")}</span>
        </div>

        <div className="gsearch__results" ref={listRef} role="listbox">
          {trimmed.length === 0 ? (
            <p className="gsearch__empty">{t("search.empty.prompt")}</p>
          ) : tooShort ? (
            <p className="gsearch__empty">{t("search.empty.tooShort")}</p>
          ) : results.length === 0 ? (
            <p className="gsearch__empty">{t("search.empty.noResults")}</p>
          ) : (
            grouped.map(({ kind, items }) => {
              const GroupIcon = GROUP_ICON[kind];
              return (
                <div key={kind}>
                  <div className="gsearch__group">
                    <GroupIcon size={11} strokeWidth={2} aria-hidden="true" />
                    {t(`search.group.${kind}` as MessageKey)}
                  </div>
                  {items.map((result) => {
                    position += 1;
                    const at = position;
                    const sub = subtitle(t, result);
                    return (
                      <button
                        key={`${result.kind}-${result.entityId}-${result.tsMs}-${at}`}
                        type="button"
                        role="option"
                        aria-selected={at === index}
                        data-active={at === index || undefined}
                        className="gsearch__item"
                        onMouseMove={() => setIndex(at)}
                        onClick={() => selectAt(at)}
                      >
                        <div className="gsearch__item-header">
                          {result.heading && <span className="gsearch__heading">{result.heading}</span>}
                          {sub && <span className="gsearch__sub">{sub}</span>}
                          <span className="gsearch__spacer" />
                          {result.projectName && (
                            <span className="gsearch__project">{result.projectName}</span>
                          )}
                          <span className="gsearch__time">
                            {formatWhen(result.tsMs, navigator.language)}
                          </span>
                        </div>
                        {result.snippet && <div className="gsearch__snippet">{result.snippet}</div>}
                      </button>
                    );
                  })}
                </div>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
}

/** A small badge for a tab opened from a search result rather than started
 * here — the tab strip's one honest way of saying "this is read-only". */
export function HistoricalTabBadge({ title }: { title: string }) {
  return (
    <span className="gsearch__historical-badge" title={title}>
      <History size={10} strokeWidth={2.2} aria-hidden="true" />
    </span>
  );
}
