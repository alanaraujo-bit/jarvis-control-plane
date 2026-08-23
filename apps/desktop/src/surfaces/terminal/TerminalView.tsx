import { useCallback, useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebglAddon } from "@xterm/addon-webgl";
import "@xterm/xterm/css/xterm.css";

import type { MessageKey } from "@jarvis/i18n";
import { useT } from "../../app/i18n";
import { useTheme } from "../../app/theme";
import {
  attachSession,
  replaySession,
  resizeSession,
  saveAttachment,
  writeSession,
  type Attachment,
} from "../../app/sessions";
import { PastedImage } from "./PastedImage";
import { TerminalFind, type FindState } from "./TerminalFind";
import { terminalSearchDecorations, terminalTheme } from "./theme";
import "./TerminalView.css";

interface TerminalViewProps {
  sessionId: string;
  /** Focus the terminal once it is ready. */
  autoFocus?: boolean;
}

/**
 * How wide the overview ruler is while the find bar is open.
 *
 * Zero the rest of the time. A permanent gutter beside every terminal, empty
 * in every session nobody is searching, would be paying for the feature all
 * the time to have it when it is used — and the ruler is what makes a 20,000
 * line scrollback searchable rather than merely search*ed*: it shows where the
 * matches are, not just how many.
 */
const RULER_WIDTH = 10;

const NO_SEARCH: FindState = { query: "", caseSensitive: false, index: 0, total: 0 };

/**
 * Quote a path if the shell would otherwise split it on a space.
 *
 * The session log directory lives under `%APPDATA%`, which on a machine whose
 * user has a space in their name — as this one does — produces exactly the
 * path that breaks unquoted. Double quotes work in PowerShell, cmd and POSIX
 * shells alike; a path containing a double quote is not something any of the
 * clipboard formats here can produce, since the name is ours.
 */
function quoteIfNeeded(path: string): string {
  return path.includes(" ") ? `"${path}"` : path;
}

/**
 * Turn a paste failure into a sentence.
 *
 * The core answers with a stable code (`attachment.tooLarge`, …) which is
 * localised here — the same shape evidence summaries use (§65). Anything else
 * is an OS error string in whatever language Windows is in, and dropping that
 * on a person would be worse than saying plainly that the paste failed, so it
 * falls back to a general sentence and keeps the detail for the log.
 */
function pasteMessage(t: (key: MessageKey) => string, code: string): string {
  const known = ["empty", "tooLarge", "unsupported", "outsideSession"];
  const tail = code.split(".").pop() ?? "";
  return known.includes(tail)
    ? t(`terminal.paste.${tail}` as MessageKey)
    : t("terminal.paste.failed");
}

/**
 * A live terminal bound to one session.
 *
 * The terminal is the raw projection of the session log (§23): every byte the
 * process produced, replayed on attach and streamed thereafter. Switching to
 * Conversation View shows the same session, not a different one.
 */
export function TerminalView({ sessionId, autoFocus = true }: TerminalViewProps) {
  const t = useT();
  const hostRef = useRef<HTMLDivElement>(null);
  const resolved = useTheme((state) => state.resolved);
  const termRef = useRef<Terminal | null>(null);
  const searchRef = useRef<SearchAddon | null>(null);
  /** Refit and tell the PTY, without reaching into the build effect's locals. */
  const refit = useRef<(() => void) | null>(null);

  const [find, setFind] = useState<FindState | null>(null);
  const [attachment, setAttachment] = useState<Attachment | null>(null);
  const [pasteError, setPasteError] = useState<string | null>(null);
  // The effect that builds the terminal must not be rebuilt when the find bar
  // opens — that would kill the process. A ref lets the key handler inside it
  // read the current state without depending on it.
  const findOpen = useRef(false);
  findOpen.current = find !== null;
  const decorations = useRef(terminalSearchDecorations(resolved));
  decorations.current = terminalSearchDecorations(resolved);

  // Recreate only when the session changes. Theme changes are applied in place
  // below, because tearing down the terminal would lose the scrollback.
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const term = new Terminal({
      // A literal stack, never a CSS custom property: xterm measures glyph
      // width by rendering into a canvas from this string, where `var(...)`
      // does not resolve. It then falls back to a proportional font and every
      // line renders with visibly uneven spacing.
      fontFamily: '"JetBrains Mono Variable", "JetBrains Mono", Consolas, "Courier New", monospace',
      fontSize: 13,
      lineHeight: 1.32,
      letterSpacing: 0,
      cursorBlink: true,
      cursorStyle: "bar",
      cursorWidth: 2,
      // Deep enough for a long agent run without unbounded memory growth.
      scrollback: 20000,
      allowProposedApi: true,
      macOptionIsMeta: true,
      drawBoldTextInBrightColors: false,
      minimumContrastRatio: 1,
      theme: terminalTheme(resolved),
    });
    termRef.current = term;

    const fit = new FitAddon();
    const search = new SearchAddon();
    searchRef.current = search;
    term.loadAddon(fit);
    term.loadAddon(search);
    term.open(host);

    // Escape closes the find bar even when the terminal itself holds the
    // keyboard, so a search can be dismissed without clicking back into it.
    // Ctrl+F is **not** handled here — see the capture-phase effect below for
    // why a widget-level handler was not enough.
    term.attachCustomKeyEventHandler((event) => {
      if (event.type !== "keydown") return true;
      if (event.key === "Escape" && findOpen.current) {
        event.preventDefault();
        setFind(null);
        return false;
      }
      return true;
    });

    const results = search.onDidChangeResults(({ resultIndex, resultCount }) => {
      setFind((current) =>
        current === null
          ? current
          : // xterm reports a 0-based index, and -1 when it stopped counting.
            { ...current, index: resultIndex < 0 ? 0 : resultIndex + 1, total: resultCount },
      );
    });

    // WebGL keeps a busy session at frame rate. It is unavailable in some
    // WebView2 configurations, so failure must degrade rather than break.
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      term.loadAddon(webgl);
    } catch {
      // Canvas/DOM renderer remains in place.
    }

    let disposed = false;
    let detach: (() => void) | null = null;

    const sync = () => {
      fit.fit();
      void resizeSession(sessionId, term.cols, term.rows).catch(() => {
        // The session may have ended; the view stays usable for reading.
      });
    };
    refit.current = sync;

    const start = async () => {
      fit.fit();

      // Restore history before streaming, so output cannot interleave ahead of
      // the scrollback it belongs after.
      const history = await replaySession(sessionId);
      if (disposed) return;
      if (history.length > 0) term.write(history);

      detach = await attachSession(sessionId, (bytes) => {
        term.write(bytes);
      });
      if (disposed) {
        detach();
        return;
      }

      sync();
      if (autoFocus) term.focus();
    };

    void start();

    const onData = term.onData((data) => {
      void writeSession(sessionId, data).catch(() => {
        // Dropped input on a dead session is expected.
      });
    });

    // ResizeObserver rather than a window listener: panes resize without the
    // window changing size at all.
    const observer = new ResizeObserver(() => {
      if (host.clientWidth > 0 && host.clientHeight > 0) sync();
    });
    observer.observe(host);

    return () => {
      disposed = true;
      observer.disconnect();
      onData.dispose();
      results.dispose();
      detach?.();
      term.dispose();
      termRef.current = null;
      searchRef.current = null;
      refit.current = null;
    };
    // `resolved` is deliberately excluded — see the theme effect below.
    //
    // `autoFocus` is excluded too, and that one matters: it changes whenever
    // the user switches tab or leaves the Sessions area, and rebuilding the
    // terminal for it would kill the process and throw away the scrollback.
    // Focus is a separate effect for exactly that reason.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  /**
   * Take the keyboard back when this terminal becomes the active one.
   *
   * Focusing only at start-up is not enough once a project has more than one
   * place to be: coming back from Files left the terminal on screen, alive and
   * scrolled where it was, and silently ignoring everything typed at it. Found
   * by switching away and back in the real app and typing.
   */
  useEffect(() => {
    if (autoFocus) termRef.current?.focus();
  }, [autoFocus]);

  /**
   * Ctrl+F opens the find bar — resolved on `window`, in the **capture**
   * phase, before anything else can answer for it.
   *
   * The first version handled it inside xterm's own
   * `attachCustomKeyEventHandler`, which works only while the terminal
   * literally holds DOM focus. Come back from the command palette, or click
   * the tab strip, and the key never reached xterm at all — at which point
   * **WebView2 answered it with its own built-in find-in-page**: a browser
   * widget floating over the app, styled like nothing else in the product,
   * searching the DOM rather than the scrollback. Found by pressing Ctrl+F in
   * the real app after using the palette, and seeing a Chromium find bar
   * appear where ours should have been.
   *
   * This is item 13 in `docs/HANDOFF.md` a second time, with a different
   * villain: a shortcut the product owns has to be taken before any widget —
   * or any embedded browser — gets an opinion about it. `preventDefault` here
   * is what stops WebView2, and nothing further down can undo it.
   *
   * Guarded on `autoFocus`, which is true exactly for the terminal the user is
   * actually looking at, so several mounted terminals cannot all answer at
   * once — and so Ctrl+F still reaches Monaco's own find when the editor is
   * the visible surface instead.
   *
   * **This takes the key away from the shell**: readline binds ^F to
   * forward-char and it no longer arrives. Stated rather than hidden — a
   * keystroke that moves the cursor one column, with an arrow key beside it,
   * is worth less than searching twenty thousand lines of agent output.
   * Ctrl+C, Ctrl+D and everything else still go straight through.
   */
  useEffect(() => {
    if (!autoFocus) return;
    const onKeyDown = (event: KeyboardEvent) => {
      // `toLowerCase`, because `event.key` is "F" whenever Shift or CapsLock
      // is down. A plain `=== "f"` silently stops working for anyone with
      // CapsLock on — `App.tsx` already spells its own shortcuts this way and
      // this one did not. Shift is excluded outright: Ctrl+Shift+F is Global
      // Search (§51) and the terminal must never answer for it.
      if (
        event.ctrlKey &&
        !event.altKey &&
        !event.metaKey &&
        !event.shiftKey &&
        event.key.toLowerCase() === "f"
      ) {
        event.preventDefault();
        event.stopPropagation();
        setFind((current) => current ?? { ...NO_SEARCH });
      }
    };
    window.addEventListener("keydown", onKeyDown, { capture: true });
    return () => window.removeEventListener("keydown", onKeyDown, { capture: true });
  }, [autoFocus]);

  // Apply theme changes without recreating the terminal.
  useEffect(() => {
    const term = termRef.current;
    if (term) term.options.theme = terminalTheme(resolved);
  }, [resolved]);

  /**
   * Show the overview ruler only while searching.
   *
   * It is what turns a count into a map — where in twenty thousand lines the
   * matches actually are — and an empty gutter beside every terminal the rest
   * of the time would be paying for that permanently.
   */
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.overviewRulerWidth = find === null ? 0 : RULER_WIDTH;
    // The ruler takes its width out of the terminal's own row area, so the
    // column count is now wrong until something refits — and a terminal whose
    // cols disagree with the PTY's wraps every line in the wrong place.
    refit.current?.();
  }, [find === null]); // eslint-disable-line react-hooks/exhaustive-deps

  /**
   * Run the search whenever the term or the options change.
   *
   * `findPrevious`, not `findNext`, and that is not arbitrary: `findNext` with
   * `incremental` walks *forward* from the viewport, so typing into a bar
   * opened at the bottom of a long scrollback finds nothing and reports zero
   * while the answer is above. A terminal's history reads backwards — the line
   * you are looking for is almost always one you have already scrolled past —
   * so the first hit should be the most recent one, not the first one after
   * wrapping around.
   */
  /**
   * Pasting an image types its path (§22).
   *
   * There is no way to put a picture *through* a PTY — a terminal carries
   * bytes, and an agent CLI reads a terminal. What actually reaches the agent
   * is a path to a file on disk, typed at the prompt exactly as a person would
   * type it. Claude Code reads an image when a path points at one; that is the
   * whole mechanism and it is worth being plain about rather than implying the
   * terminal grew a new capability.
   *
   * Registered on the host element in the **capture** phase so it runs before
   * xterm's own paste handling, which would otherwise write the clipboard's
   * text rendering of an image — usually nothing at all — into the shell.
   * Text pastes are untouched and still go to xterm.
   */
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const onPaste = (event: ClipboardEvent) => {
      const file = Array.from(event.clipboardData?.items ?? [])
        .find((item) => item.kind === "file" && item.type.startsWith("image/"))
        ?.getAsFile();
      if (!file) return; // A text paste. xterm's own handling is correct.

      event.preventDefault();
      event.stopPropagation();

      void file
        .arrayBuffer()
        .then((buffer) => saveAttachment(sessionId, new Uint8Array(buffer)))
        .then((saved) => {
          setAttachment(saved);
          // A trailing space, so the next thing typed does not run into the
          // path — and no newline: this is the person's prompt to submit, the
          // same rule voice dictation follows (§54).
          return writeSession(sessionId, `${quoteIfNeeded(saved.path)} `);
        })
        .catch((cause) => setPasteError(String(cause)));
    };

    host.addEventListener("paste", onPaste, { capture: true });
    return () => host.removeEventListener("paste", onPaste, { capture: true });
  }, [sessionId]);

  const paintedFor = useRef(resolved);
  useEffect(() => {
    const search = searchRef.current;
    if (!search) return;
    if (find === null || find.query.length === 0) {
      search.clearDecorations();
      return;
    }
    // The addon only rebuilds its highlights when the *term* or a matching
    // option changes — it does not look at the colours. So a theme switch with
    // a search open would leave every match painted for the theme that is no
    // longer on screen. Dropping them makes the next call repaint.
    if (paintedFor.current !== resolved) {
      paintedFor.current = resolved;
      search.clearDecorations();
    }
    search.findPrevious(find.query, {
      caseSensitive: find.caseSensitive,
      decorations: decorations.current,
    });
    // Only the inputs to a search, never the results it reports back — the
    // count arriving on `onDidChangeResults` must not start another search.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [find?.query, find?.caseSensitive, resolved]);

  // Read through a ref rather than from inside a `setState` updater: React may
  // run an updater more than once, and an updater that also runs a search
  // would step two matches for one press of Enter.
  const findRef = useRef<FindState | null>(null);
  findRef.current = find;

  const step = useCallback((direction: "next" | "previous") => {
    const search = searchRef.current;
    const current = findRef.current;
    if (!search || !current || !current.query) return;
    const options = { caseSensitive: current.caseSensitive, decorations: decorations.current };
    if (direction === "next") search.findNext(current.query, options);
    else search.findPrevious(current.query, options);
  }, []);

  const close = useCallback(() => {
    searchRef.current?.clearDecorations();
    setFind(null);
    // The keyboard goes back where it came from. Leaving focus on a bar that
    // is no longer there is how a terminal ends up silently ignoring typing —
    // the same failure the autoFocus effect above exists to prevent.
    termRef.current?.focus();
  }, []);

  // The find bar is a **sibling** of the xterm host, not a child of it.
  // xterm owns every node inside the element it was opened on and inserts its
  // own as it renders; putting a React-managed element in there means two
  // owners for one child list, which is how a mounted component ends up
  // detached from the DOM it thinks it is in.
  return (
    <div className="terminal-pane">
      <div className="terminal" ref={hostRef} />

      {attachment && (
        <PastedImage
          sessionId={sessionId}
          attachment={attachment}
          // Dismisses the chip only. The path is already typed at the prompt
          // and the file is already on disk — removing them would mean editing
          // the shell's input line from underneath the person, which is theirs
          // to edit. This says "stop showing me the picture", nothing more.
          onRemove={() => setAttachment(null)}
        />
      )}

      {pasteError && (
        <p className="terminal__paste-error" role="alert">
          {pasteMessage(t, pasteError)}
        </p>
      )}

      {find !== null && (
        <TerminalFind
          state={find}
          onChange={(next) => setFind((current) => (current ? { ...current, ...next } : current))}
          onNext={() => step("next")}
          onPrevious={() => step("previous")}
          onClose={close}
        />
      )}
    </div>
  );
}
