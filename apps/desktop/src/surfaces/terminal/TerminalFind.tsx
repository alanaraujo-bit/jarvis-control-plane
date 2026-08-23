import { useEffect, useRef } from "react";
import { ArrowDown, ArrowUp, CaseSensitive, Search, X } from "lucide-react";

import { useT } from "../../app/i18n";
import "./TerminalFind.css";

export interface FindState {
  /** What is being looked for. */
  query: string;
  /** Whether case has to match. */
  caseSensitive: boolean;
  /** 1-based position of the active match, or 0 when there is none. */
  index: number;
  /** How many matches there are, or -1 when there are more than xterm counts. */
  total: number;
}

interface TerminalFindProps {
  state: FindState;
  onChange: (next: Partial<FindState>) => void;
  onNext: () => void;
  onPrevious: () => void;
  onClose: () => void;
}

/**
 * Find within one terminal's scrollback (§20).
 *
 * A panel over the terminal rather than a dialog over the app: the whole point
 * is to read the output while narrowing it down, and a modal that covers what
 * you are searching would be answering the wrong question. It sits top-right,
 * where it overlaps the least of a shell's own left-aligned output.
 *
 * The bar never steals the process's input except while it is open and
 * focused — the terminal keeps the keyboard the rest of the time.
 */
export function TerminalFind({
  state,
  onChange,
  onNext,
  onPrevious,
  onClose,
}: TerminalFindProps) {
  const t = useT();
  const inputRef = useRef<HTMLInputElement>(null);

  // Focus and select on open, so typing a second search replaces the first
  // rather than appending to it — the behaviour every find bar has.
  useEffect(() => {
    const input = inputRef.current;
    if (!input) return;
    input.focus();
    input.select();
  }, []);

  const searching = state.query.length > 0;
  const nothingFound = searching && state.total === 0;
  // xterm reports -1 when a search exceeds its highlight limit. Saying "1000+"
  // is honest; showing a wrong total is not.
  const uncounted = state.total < 0;

  const status = !searching
    ? ""
    : nothingFound
      ? t("terminal.find.noResults")
      : uncounted
        ? t("terminal.find.manyResults")
        : `${state.index}/${state.total}`;

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      if (event.shiftKey) onPrevious();
      else onNext();
      return;
    }
    // Pressing the same shortcut again while the bar is open reselects the
    // term instead of doing nothing — the second Ctrl+F is almost always
    // "search for something else", not "open what is already open".
    if (event.ctrlKey && event.key === "f") {
      event.preventDefault();
      inputRef.current?.select();
    }
  };

  return (
    <div className="tfind" role="search" onKeyDown={onKeyDown}>
      <Search className="tfind__icon" size={13} aria-hidden />
      <input
        ref={inputRef}
        className="tfind__input"
        type="text"
        value={state.query}
        spellCheck={false}
        autoComplete="off"
        placeholder={t("terminal.find.placeholder")}
        aria-label={t("terminal.find.placeholder")}
        onChange={(event) => onChange({ query: event.target.value })}
      />

      <span
        className="tfind__status"
        data-empty={nothingFound || undefined}
        // The count changes as you type, and a screen reader should hear the
        // result rather than only the letters that produced it.
        aria-live="polite"
      >
        {status}
      </span>

      <button
        type="button"
        className="tfind__toggle"
        data-on={state.caseSensitive || undefined}
        aria-pressed={state.caseSensitive}
        title={t("terminal.find.matchCase")}
        aria-label={t("terminal.find.matchCase")}
        onClick={() => {
          onChange({ caseSensitive: !state.caseSensitive });
          // Narrowing a search and continuing to type is one thought, not two.
          // Leaving focus on the button means the next keystroke goes nowhere
          // and the person has to click back into the field to carry on.
          inputRef.current?.focus();
        }}
      >
        <CaseSensitive size={14} aria-hidden />
      </button>

      <span className="tfind__divider" aria-hidden />

      <button
        type="button"
        className="tfind__button"
        disabled={!searching || nothingFound}
        title={t("terminal.find.previous")}
        aria-label={t("terminal.find.previous")}
        onClick={onPrevious}
      >
        <ArrowUp size={13} aria-hidden />
      </button>
      <button
        type="button"
        className="tfind__button"
        disabled={!searching || nothingFound}
        title={t("terminal.find.next")}
        aria-label={t("terminal.find.next")}
        onClick={onNext}
      >
        <ArrowDown size={13} aria-hidden />
      </button>
      <button
        type="button"
        className="tfind__button"
        title={t("terminal.find.close")}
        aria-label={t("terminal.find.close")}
        onClick={onClose}
      >
        <X size={13} aria-hidden />
      </button>
    </div>
  );
}
