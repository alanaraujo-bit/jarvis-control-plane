import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebglAddon } from "@xterm/addon-webgl";
import "@xterm/xterm/css/xterm.css";

import { useTheme } from "../../app/theme";
import { attachSession, replaySession, resizeSession, writeSession } from "../../app/sessions";
import { terminalTheme } from "./theme";
import "./TerminalView.css";

interface TerminalViewProps {
  sessionId: string;
  /** Focus the terminal once it is ready. */
  autoFocus?: boolean;
}

/**
 * A live terminal bound to one session.
 *
 * The terminal is the raw projection of the session log (§23): every byte the
 * process produced, replayed on attach and streamed thereafter. Switching to
 * Conversation View shows the same session, not a different one.
 */
export function TerminalView({ sessionId, autoFocus = true }: TerminalViewProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const resolved = useTheme((state) => state.resolved);
  const termRef = useRef<Terminal | null>(null);

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
    term.loadAddon(fit);
    term.loadAddon(search);
    term.open(host);

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
      detach?.();
      term.dispose();
      termRef.current = null;
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

  // Apply theme changes without recreating the terminal.
  useEffect(() => {
    const term = termRef.current;
    if (term) term.options.theme = terminalTheme(resolved);
  }, [resolved]);

  return <div className="terminal" ref={hostRef} />;
}
