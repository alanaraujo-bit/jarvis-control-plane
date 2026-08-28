import type { Terminal } from "@xterm/xterm";

/**
 * The terminals currently on screen, by session id.
 *
 * ## Why a registry rather than a Tauri command
 *
 * The Notebook (M19) hands a prompt to the agent you are watching. The obvious
 * implementation is a Rust command writing bytes to the PTY — and it would be
 * wrong, because of what was measured before any of this was built.
 *
 * `session::typing::type_text`, the path the autopilot and dictation use,
 * **flattens every newline to a space**: a bare `\n` reaching an agent CLI's
 * line editor submits the fragment before it. Prompts are multi-paragraph, so
 * that path would deliver Alan's library as run-on lines and look like it had
 * worked.
 *
 * What actually works is bracketed paste — `ESC[200~ … ESC[201~` — and it was
 * confirmed against real Claude Code 2.1.245 in the running app: three lines
 * arrived intact and unsent. But whether a given terminal accepts it depends on
 * whether *that program* set DECSET 2004, which a plain shell may not and an
 * agent CLI does. Rust would have to guess.
 *
 * `xterm` does not have to guess. `Terminal.paste()` reads
 * `decPrivateModes.bracketedPasteMode` for that terminal, wraps or does not
 * wrap accordingly, and raises `onData` — which `TerminalView` already writes
 * to the PTY. It is the identical path a real Ctrl+V takes, which makes it the
 * most-exercised code in the product for this exact job.
 *
 * So the missing piece was never a command. It was a way to reach the right
 * `Terminal` object from outside the component that owns it.
 *
 * ## What this deliberately is not
 *
 * Not a store, and nothing subscribes to it. A React store would re-render
 * every consumer whenever a terminal mounts, to publish a fact — "there is a
 * live terminal for this session" — that `useTerminals` already tracks as
 * tabs. This holds the *object*, which cannot live in state anyway, and is read
 * only at the moment somebody presses Send.
 */
const live = new Map<string, Terminal>();

/** Called by `TerminalView` on mount. */
export function registerTerminal(sessionId: string, term: Terminal) {
  live.set(sessionId, term);
}

/** Called by `TerminalView` on unmount. Idempotent. */
export function unregisterTerminal(sessionId: string) {
  live.delete(sessionId);
}

/** Whether this session has a terminal on screen right now. */
export function hasLiveTerminal(sessionId: string): boolean {
  return live.has(sessionId);
}

export interface LiveTokenProgress {
  elapsedSeconds: number;
  outputTokens: number;
}

/** Parse the provider's own in-progress status line. Exported for focused tests. */
export function parseLiveTokenProgress(line: string): LiveTokenProgress | null {
  // Claude Code 2.1.248 renders, for example:
  //   Puttering… (24s ↓ /750 tokens)
  // Longer turns may gain a minute component and large counts use k/m suffixes.
  // The symbols and activity verb are deliberately ignored: both have changed
  // between releases, while seconds + the literal provider label `tokens` are
  // the stable, meaningful part.
  const match = line.match(
    /(?:(\d+)m\s*)?(\d+(?:[.,]\d+)?)s[^()\r\n]{0,80}?(\d+(?:[.,]\d+)?\s*[kKmM]?)\s+tokens\b/i,
  );
  if (!match) return null;

  const minutes = Number(match[1] ?? 0);
  const seconds = Number(match[2].replace(",", "."));
  const tokenText = match[3].replace(/\s/g, "");
  const suffix = tokenText.at(-1)?.toLowerCase();
  const multiplier = suffix === "k" ? 1_000 : suffix === "m" ? 1_000_000 : 1;
  const numeric = multiplier === 1 ? tokenText : tokenText.slice(0, -1);
  // With no suffix, a comma followed by three digits is a thousands separator;
  // otherwise it is a decimal separator (the provider normally emits a dot,
  // but this keeps a localised build readable too).
  const normalised =
    multiplier === 1 && /^\d{1,3}(,\d{3})+$/.test(numeric)
      ? numeric.replace(/,/g, "")
      : numeric.replace(",", ".");
  const outputTokens = Number(normalised) * multiplier;
  const elapsedSeconds = minutes * 60 + seconds;

  if (!Number.isFinite(outputTokens) || !Number.isFinite(elapsedSeconds) || elapsedSeconds <= 0) {
    return null;
  }
  return { elapsedSeconds, outputTokens };
}

/**
 * Read live token progress from xterm's canonical screen buffer.
 *
 * This is more accurate than parsing raw PTY bytes: xterm has already applied
 * cursor movement, line erasure and partial UTF-8 sequences, so the string here
 * is exactly the status the provider is currently showing the person. Search
 * from the bottom because the active status is always the newest candidate.
 */
export function readLiveTokenProgress(sessionId: string): LiveTokenProgress | null {
  const term = live.get(sessionId);
  if (!term) return null;

  const buffer = term.buffer.active;
  const from = Math.max(0, buffer.length - Math.max(term.rows + 8, 40));
  for (let index = buffer.length - 1; index >= from; index -= 1) {
    const line = buffer.getLine(index)?.translateToString(true);
    if (!line) continue;
    const progress = parseLiveTokenProgress(line);
    if (progress) return progress;
  }
  return null;
}

/** What happened, so the caller can say something true about it. */
export type PasteOutcome = "sent" | "noTerminal" | "wouldSubmit";

/**
 * Put text into a session's prompt, exactly as a paste would.
 *
 * **Never submits.** `paste` writes the text and stops; the return key is a
 * separate act, and it belongs to the person. That is §54's rule for dictation
 * and it holds here for the same reason — text that did not come from a
 * keyboard should be looked at before it is sent.
 *
 * ## Why multi-line text is refused unless the program asked for it
 *
 * `paste` normalises `\n` to `\r` **before** deciding whether to bracket, and
 * it brackets only when that terminal set DECSET 2004. An agent CLI does; a
 * plain PowerShell does not. So the same call that hands Claude Code a
 * twenty-line prompt intact would hand a shell twenty carriage returns — which
 * is twenty commands, run.
 *
 * That is not a cosmetic failure. A prompt library is full of sentences like
 * `git reset --hard` and `rm -rf build` written as *examples*, and this
 * product's own guardrails are explicit that they govern agents rather than
 * what a person types (§35). Nothing downstream would catch it.
 *
 * It is also not a question about which *kind* of session this is. The gate is
 * "has the program on the other end enabled bracketed paste **right now**",
 * which `term.modes` answers — and it answers it correctly for the case a
 * kind-based check would miss: an agent CLI that has started but has not yet
 * drawn its prompt. The experiment that established all this waited nine
 * seconds for exactly that reason.
 *
 * Single-line text is always safe: there is no newline to become a submit.
 */
export function pasteIntoSession(sessionId: string, text: string): PasteOutcome {
  const term = live.get(sessionId);
  if (!term) return "noTerminal";

  if (/[\r\n]/.test(text) && !term.modes.bracketedPasteMode) return "wouldSubmit";

  term.paste(text);
  term.focus();
  return "sent";
}
