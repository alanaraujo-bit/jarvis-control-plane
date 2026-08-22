import { useEffect, useMemo, useRef, useState } from "react";
import { CornerDownLeft, type LucideIcon } from "lucide-react";
import { useT } from "../app/i18n";
import "./CommandPalette.css";

export interface Command {
  id: string;
  title: string;
  /** Grouping label, e.g. "Go to", "Appearance". */
  group: string;
  icon: LucideIcon;
  /** Extra words that should match this command without being displayed. */
  keywords?: string;
  hint?: string;
  run: () => void;
}

/**
 * Score a command against the query using subsequence matching.
 *
 * Contiguous runs and matches at word starts score higher, so typing "mc"
 * finds "Mission Control" above "Rescan Environment". Returns null for no match
 * so filtering and ranking share one pass.
 */
function score(command: Command, query: string): number | null {
  if (!query) return 0;
  const haystack = `${command.title} ${command.group} ${command.keywords ?? ""}`.toLowerCase();
  const needle = query.toLowerCase();

  let hay = 0;
  let total = 0;
  let streak = 0;

  for (const char of needle) {
    if (char === " ") continue;
    const found = haystack.indexOf(char, hay);
    if (found === -1) return null;

    const atWordStart = found === 0 || haystack[found - 1] === " ";
    streak = found === hay ? streak + 1 : 0;
    total += 1 + streak * 2 + (atWordStart ? 3 : 0);
    hay = found + 1;
  }
  // Prefer shorter titles when scores tie — the more specific command wins.
  return total - command.title.length * 0.01;
}

interface CommandPaletteProps {
  open: boolean;
  commands: Command[];
  onClose: () => void;
}

export function CommandPalette({ open, commands, onClose }: CommandPaletteProps) {
  const t = useT();
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const results = useMemo(() => {
    return commands
      .map((command) => ({ command, rank: score(command, query.trim()) }))
      .filter((entry): entry is { command: Command; rank: number } => entry.rank !== null)
      .sort((a, b) => b.rank - a.rank)
      .map((entry) => entry.command);
  }, [commands, query]);

  // Reset on each open so the palette never resumes a stale query.
  useEffect(() => {
    if (open) {
      setQuery("");
      setIndex(0);
      // Focus after paint, otherwise the element is not yet in the document.
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  useEffect(() => setIndex(0), [query]);

  // Keep the active row in view during keyboard traversal.
  useEffect(() => {
    listRef.current
      ?.querySelector<HTMLElement>('[data-active="true"]')
      ?.scrollIntoView({ block: "nearest" });
  }, [index]);

  if (!open) return null;

  const runAt = (position: number) => {
    const command = results[position];
    if (!command) return;
    onClose();
    command.run();
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
      runAt(index);
    }
  };

  let lastGroup = "";

  return (
    <div className="palette__scrim" onMouseDown={onClose} role="presentation">
      <div
        className="palette"
        role="dialog"
        aria-modal="true"
        aria-label={t("window.search")}
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={onKeyDown}
      >
        <input
          ref={inputRef}
          className="palette__input"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder={t("window.search")}
          spellCheck={false}
          autoComplete="off"
          aria-activedescendant={results[index] ? `palette-${results[index].id}` : undefined}
        />

        <div className="palette__results" ref={listRef} role="listbox">
          {results.length === 0 ? (
            <p className="palette__empty">—</p>
          ) : (
            results.map((command, position) => {
              const showGroup = command.group !== lastGroup;
              lastGroup = command.group;
              const Icon = command.icon;
              return (
                <div key={command.id}>
                  {showGroup && <div className="palette__group">{command.group}</div>}
                  <button
                    type="button"
                    id={`palette-${command.id}`}
                    role="option"
                    aria-selected={position === index}
                    data-active={position === index}
                    className="palette__item"
                    onMouseMove={() => setIndex(position)}
                    onClick={() => runAt(position)}
                  >
                    <Icon size={14} strokeWidth={1.75} aria-hidden="true" />
                    <span className="palette__label">{command.title}</span>
                    {command.hint && <span className="palette__hint">{command.hint}</span>}
                    {position === index && (
                      <CornerDownLeft
                        size={12}
                        strokeWidth={2}
                        className="palette__enter"
                        aria-hidden="true"
                      />
                    )}
                  </button>
                </div>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
}
