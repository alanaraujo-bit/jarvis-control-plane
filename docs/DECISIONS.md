# Decision Log

## D1 — Tauri v2 over Electron
**Date:** 2026-08-22
**Why:** §11 makes performance part of design and §12 demands a real Windows
installer. Tauri ships a native-size binary, a first-party NSIS installer, a
signed updater, and a Rust core capable of real PTY/git/fs work. Electron would
cost ~10x binary size and put the systems layer in Node.
**Alternatives:** Electron (heavier), pure-native WinUI (loses web UI velocity
and the mobile PWA code sharing).
**Verified:** Rust + MSVC links on this machine; Tauri CLI 2.11.4; WebView2 present.

## D2 — A session is an append-only event log
**Date:** 2026-08-22
**Why:** §23 requires Terminal and Conversation to be one session. Modeling them
as two components with separate state makes that impossible without constant
sync. One ordered log with two projections makes it structural — and §39
(timeline), §48 (activity), §30 (evidence), §55–57 (mobile deltas) fall out of
the same log for free.
**Consequence:** raw PTY bytes go to on-disk frame logs, not SQLite rows.

## D3 — Read provider transcripts instead of scraping ANSI
**Date:** 2026-08-22
**Why:** Verified that Claude Code and Codex both write structured JSONL while
running interactively. Scraping terminal output would be fragile and would lose
token usage entirely. Reading transcripts gives official usage data (§28) and a
faithful Conversation View (§24).
**Verified against:** real transcripts on this machine (see ARCHITECTURE.md).

## D4 — Monaco for the editor, behind a boundary
**Date:** 2026-08-22
**Why:** §42 wants multiple cursors, breadcrumbs, go-to-definition, diagnostics —
mostly free in Monaco. Kept behind `packages/editor` so an LSP client can land
later without touching surfaces.

## D5 — Git through the `git` executable, not libgit2
**Date:** 2026-08-22
**Why:** Worktrees (§45) are only fully supported by the CLI, and shelling out
means behaviour matches the user's own Git exactly — their config, hooks,
credential helpers and `safe.directory` rules. A library would silently diverge.
Git is already a required tool (§14), so this adds no dependency.
**Guards:** every invocation sets `GIT_TERMINAL_PROMPT=0` and `GIT_PAGER=cat` so
a subprocess can never hang waiting for input there is no UI to provide.

## D6 — The PTY host answers ConPTY's startup cursor query
**Date:** 2026-08-22
**Why:** Discovered empirically, not from documentation. On Windows, ConPTY
emits a Device Status Report (`ESC [ 6 n`) as a startup handshake and **stalls
the entire session until the terminal replies**. With no reply, a session
produces those four bytes and then nothing, forever.

When a terminal view is attached, xterm.js answers and everything works — which
is exactly why this is dangerous: it would have looked fine in every manual test
and failed precisely in **Unattended mode** (§32), where an agent runs with no
view attached. That is the mode the product most needs to be reliable.

**Decision:** the Rust side tracks whether a view is attached to each session and
answers the query itself when none is. Terminal semantics belong to the core, not
to whether someone happens to be looking.

**Verified:** an isolated reproduction confirmed both halves — no reply stalls the
stream; replying `ESC [ 1 ; 1 R` releases it immediately.

## D7 — Sessions are spawned with the parent agent's environment scrubbed
**Date:** 2026-08-22
**Why:** Found by running the product, not by reasoning about it. When
J.A.R.V.I.S. is itself launched from inside an agent session, that agent's
environment markers (`CLAUDE_CODE_CHILD_SESSION`, `CLAUDECODE`, `CLAUDE_PID`, …)
are in our process environment and are inherited by everything we spawn.

A nested Claude Code sees the child-session marker, concludes it is a
sub-session, and **turns transcript saving off**. That would silently remove the
structured stream Conversation View (§24) and usage reporting (§28) depend on —
the terminal would look perfectly fine while the entire structured half of the
product quietly stopped working.

**Decision:** `pty::spawn` removes these markers for every session kind. A
session launched by J.A.R.V.I.S. is a new top-level session, and a plain shell
should not believe it is running inside an agent either.

**Tested:** a real child process is spawned and asked to echo the marker back.

## D8 — xterm's font family must be a literal stack
**Date:** 2026-08-22
**Why:** `fontFamily: "var(--font-mono), …"` looks reasonable and is silently
wrong. xterm measures glyph width by rendering into a canvas from that string,
where `var(...)` does not resolve; it falls back to a proportional font and
every terminal line renders with visibly uneven letter spacing. Caught by
looking at a screenshot of the real terminal, not by any test.

## D10 — Guardrails enforce through the provider's own pre-tool hook
**Date:** 2026-08-23
**Why:** §35 needs a sensitive operation to be *stopped*, not merely noticed. An
agent CLI running inside a PTY cannot be intercepted from outside — by the time
bytes reach the terminal the command has run. The only real enforcement point is
the callback the provider itself makes before running a tool.

Verified empirically **before** any of it was designed, against Claude Code
2.1.240: a `PreToolUse` hook receives `tool_name` and `tool_input.command` on
stdin, and `permissionDecision: "deny"` genuinely stops the command and hands
the reason to the model. Also verified: a hook that exits non-zero is treated as
having no opinion, which is what makes failing open safe.

**Consequence:** the same executable runs as the hook (`--jarvis-guardrail-hook`),
checked in `main` before Tauri starts. It reads a policy snapshot the app wrote
for that session and never opens the database — a program that runs once per
tool call must not contend with the application for it.

**It cannot write the session log either.** A session has one writer (D2), so
the guard appends its decisions to its own JSONL and the session runtime
projects them into the log as `Approval` frames — the same shape as following a
provider transcript.

## D11 — Guardrails fail open, deliberately
**Date:** 2026-08-23
**Why:** This process runs before **every** tool call in every agent session. A
guard that failed closed would turn any bug in it — a missing file, malformed
JSON, a snapshot from another version — into an agent that cannot work at all.

The cost is real and is not hidden: a rule can silently fail to apply. So the
product never claims this layer is absolute. `ProviderCapabilities::guardrails`
reports what each provider actually enforces, the classifier's own module doc
says a command that does not match has not been proven safe, and the UI reports
what *matched* rather than declaring what a command is.

Where enforcement **is** unconditional is a verification command J.A.R.V.I.S.
runs itself (§30): we own that process, so a refusal there means it truly does
not execute.

## D12 — "Ask" becomes a refusal when nobody is attached
**Date:** 2026-08-23
**Why:** Under Guided or Autonomous someone is looking at the terminal, so the
provider's own prompt reaches a person. Under Unattended (§32) nobody is, and
that same prompt would wait forever — which is precisely the indefinite resource
consumption §34 exists to forbid.

So when a rule says *ask* and no view is attached, the guard **refuses** and the
mission goes to Waiting with a reason. Stopping and explaining beats hanging
quietly. The snapshot's `attended` flag is rewritten on every attach and detach,
because whether a human is present is a fact about *now*, not about when the
session started.

**Verified against real Claude Code:** with no view attached, `rm -rf junk` was
refused, the agent reported it was blocked and explicitly did not try another
deletion method, and the directory was still there afterwards.

## D13 — An operation has exactly one spelling
**Date:** 2026-08-23
**Why:** `Operation` is serialised through `as_str`, not a serde `rename_all`
rule. With `rename_all = "kebab-case"` serde emitted `git-force-push` while
storage, the policy snapshot and the i18n keys all used `git.force-push`.

Nothing failed. The core was correct, every test passed, and the Settings screen
rendered eight raw message keys — found by looking at a screenshot, which is the
fourth time in this codebase that has been the only thing that would have caught
a bug. `an_operation_serialises_as_the_id_used_everywhere_else` now pins it.

## D9 — The main binary is named `jarvis-desktop`, not `jarvis`
**Date:** 2026-08-22
**Why:** Found by running the real installer on a real machine, not by reading
config. Tauri derives the executable name from the crate, giving `jarvis.exe` —
and this machine already had an unrelated application installed with that exact
binary name.

NSIS detects a running instance **by executable name**, so our installer
displayed "J.A.R.V.I.S is open! Click OK to close it" while pointing at
somebody else's program. Accepting would have terminated an application that has
nothing to do with us.

It failed safe when declined, but the collision is the bug. A distinctive binary
name removes the ambiguity at the source.

**Also affected:** `tools/JarvisWindow.ps1`, which locates the dev window for
screenshots and UI automation. It already filtered by executable *path* for this
same reason — two earlier attempts, matching by window title and then by process
name, each mis-targeted a real unrelated window on this machine.
