# Handoff — read this first

You are continuing work on **J.A.R.V.I.S.**, a commercial desktop product: a
universal operating environment for software development with AI agents. It is
not a prototype and not an MVP. The quality bar is Cursor / Linear / ORCA.

This file tells you where things stand, how the work is done here, and what
will bite you. **Read `docs/DECISIONS.md` before changing anything in the core** —
several decisions there were paid for with real debugging.

---

## 1. The one idea everything rests on

> **A session is a single append-only event log.**

PTY bytes and the provider's structured transcript events go into the **same**
ordered log, written by **one** writer thread. Terminal View and Conversation
View are two *projections* of that log — not two components kept in sync.

Everything downstream reads the same stream: usage, evidence, activity, the
timeline. If you are ever tempted to add a second store for session data, that
is the thing this architecture exists to prevent.

`apps/desktop/src-tauri/src/session/` is the heart. Start there.

---

## 2. Non-negotiables that shaped the code

These come from the product spec and are not stylistic preferences:

- **Completed means verified (§30).** A mission cannot be marked complete while
  a required acceptance criterion lacks evidence — the *core* refuses the
  transition, the UI does not merely discourage it. Completion is also
  **revoked** if evidence later stops holding.
- **Nothing is stubbed to look finished (§81).** Navigation is derived from a
  list of surfaces that actually exist. A destination that is not built is
  absent, not a "coming soon" screen.
- **Confidence travels with every number (§28).** Usage figures carry
  Official/Observed/Estimated/Unknown, stamped by the adapter that produced
  them. An estimate must never render as something a provider reported.
- **Providers are not equal (§26).** Capabilities are data. Claude Code accepts
  `--session-id` (deterministic correlation); Codex 0.147.0 does not (file-watch
  correlation). A test fails if the two ever describe themselves identically.
- **Local-first (§3).** Projects, sessions, Git, credentials stay on the machine.
- **i18n from the first string (§65).** en + pt-BR, pt-BR typed against the
  English catalogue so a missing translation is a build error.
- **Quiet Intelligence (§6).** Near-monochrome. **Amber is the colour of agent
  work** — brand accent and "working" state are the same hue on purpose, so
  colour always reports state and never decorates.

---

## 3. How work is done here (this matters more than any feature)

The loop is: **implement → build → run the real app → screenshot it → look at it
→ fix what is actually wrong → repeat.**

Four of the most important bugs in this codebase were invisible to tests and
only appeared by running the product and looking at a screenshot. Do not skip
the looking.

- Tests go with the code that needs them. Prefer tests against **real**
  infrastructure — a real PTY, a real Git repo, real provider transcripts —
  over mocks (§80).
- When you patch a file with a script, **verify the patch applied**. A
  silently-failed `replace()` cost real time here.
- Commit messages explain *why*, especially for anything discovered by running
  the thing.

---

## 4. Current state

Repo: `alanaraujo-bit/jarvis-control-plane` (private) · branch `master` ·
12 commits · **205 tests** (197 Rust, 8 i18n) · all green.

Installed and working on this machine at `%LOCALAPPDATA%\J.A.R.V.I.S`.

### Working and verified in the installed app

| Area | Notes |
|---|---|
| Shell, design tokens (dark + light authored separately), i18n | both themes verified by screenshot |
| Command palette | subsequence ranking, cross-language keywords |
| Environment scan | real process probes: Git, Node, pnpm, Claude Code, Codex, gh |
| Projects | folder picker, Git detection, recents, archive |
| Session event log | crash-safe, byte-exact, bounded replay |
| Terminal | real ConPTY, tabs, restore, per-theme palettes |
| Providers | Claude Code + Codex adapters, capability model |
| Conversation View | structured projection, official token usage |
| Missions | criteria, evidence, verification, autonomy inheritance |
| Mission Control | needs-attention first, empty sections vanish |
| Mission → Agent thread | starting an agent from a mission tags the session |
| Activity | recorded at moments worth knowing, filterable |
| Analytics | tokens by provider/model/project/day, **human leverage (§53)** |
| Installer + updater | NSIS, per-user, minisign-verified, uninstall preserves data |
| **Guardrails (§35)** | policy per operation and project; real pre-execution enforcement for Claude Code and for our own verification commands |
| **Unattended runs (§32)** | an agent driven turn by turn until the mission is verified, blocked, or out of budget |

**The full loop has been executed end to end**: mission created → completion
refused → agent launched from the mission → Claude Code created a real file →
verification found the evidence → mission completed. Then the file was deleted,
re-verification ran, and completion was **revoked**.

### Not built — deliberately absent, not stubbed

Project Brain (§36–39), Notes (§40), Files/Editor/Diff (§41–43), Preview (§46),
Global Search (§51), onboarding (§13), mobile PWA (§55), cloud (§59),
voice (§54).

---

## 5. Things that will bite you

Every one of these is real and already cost time.

1. **There is another app on this machine also called `jarvis.exe`**
   (`%LOCALAPPDATA%\JARVIS`). Never touch it. Our binary is `jarvis-desktop.exe`
   (`mainBinaryName`). `tools/JarvisWindow.ps1` targets by **executable path**
   and refuses to send input unless our window has focus — earlier versions
   matched by title and by process name, and both sent synthetic keystrokes into
   an unrelated application.

2. **ConPTY stalls until the terminal answers its startup cursor query.** With a
   view attached xterm.js answers, so it looks fine — and fails precisely in
   Unattended mode. The core answers it when no view is attached. See D6.

3. **ConPTY will not pump while the parent holds a slave handle.** It is dropped
   explicitly in `pty::spawn`. Do not "tidy" that away.

4. **Spawned agents must not inherit our agent-session env markers.**
   `CLAUDE_CODE_CHILD_SESSION` and friends make a nested Claude Code turn
   transcript saving *off*, which silently kills Conversation View and usage.
   `pty::spawn` scrubs them. See D7.

5. **xterm's `fontFamily` must be a literal stack** — a CSS custom property does
   not resolve there and text renders with uneven spacing. See D8.

6. **Popovers must be portalled.** The tab strip scrolls, and an absolutely
   positioned menu inside it is clipped to invisibility.

7. **On Windows, `CreateProcess` does not apply PATHEXT.** `Command::new("pnpm")`
   cannot find `pnpm.cmd`; batch wrappers need the interpreter.

8. **`.keys/jarvis-updater.key` is gitignored and must be backed up.** Losing it
   means no installed copy can ever be updated again.

9. **Never edit a migration that has already run anywhere** — including on your
   own machine mid-session. Appending `ALTER TABLE` to an applied migration
   leaves the columns missing on every database that already recorded that
   version, while a fresh one looks perfect. It cost a blank surface here.
   `a_shipped_migration_is_never_edited` fingerprints each migration's SQL and
   fails if the text changes; when it fires, add a new migration instead.

10. **Guardrails are one layer, and the code says so.** A command that does not
    match has not been proven safe — it failed to match a pattern. The guard
    also **fails open**: no snapshot, bad JSON or an unknown version means it
    stays silent and the tool call proceeds, because a guard that fails closed
    turns any bug in it into an agent that cannot work at all.

11. **Typing into an agent CLI is not one write.** Sending a whole instruction
    in a single write loses characters — the line editor re-renders on every
    keystroke and a burst outruns it (observed: "so tere is no"). And the
    submit key must be a **separate write after a pause**, or the editor
    swallows it and the instruction sits in the prompt unsent. That one is
    vicious: the terminal looks completely correct while the agent has been
    told nothing. See `autopilot::driver::send`.

12. **A driven session is not "attended", even with the terminal open.**
    Watching is not answering. `Snapshot::can_ask_a_person` requires a view
    *and* no autopilot in the seat; conflating them would park an unattended
    agent on a permission prompt nobody can answer.

---

## 6. Commands

```bash
pnpm install
pnpm dev                    # desktop app against Vite
pnpm -r typecheck

# Rust tests, including real PTY and real Git repositories
cd apps/desktop/src-tauri && cargo test
cargo test -- --ignored     # also parses every Claude Code transcript on this machine

# i18n tests (Node runs the TypeScript directly)
cd packages/i18n && node --test src/index.test.ts
```

Release build (needs the signing key in the environment):

```bash
cd apps/desktop
export TAURI_SIGNING_PRIVATE_KEY="$(cat ../../.keys/jarvis-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
pnpm tauri build
```

Install silently and drive the real app for visual review:

```powershell
$root = "c:\Users\Alan Araujo\Projetos\j.a.r.v.i.s"
$inst = "$env:LOCALAPPDATA\J.A.R.V.I.S"
Start-Process "$root\apps\desktop\src-tauri\target\release\bundle\nsis\J.A.R.V.I.S_0.1.0_x64-setup.exe" -ArgumentList "/S" -Wait
Start-Process "$inst\jarvis-desktop.exe"

& "$root\tools\send-keys.ps1"      -Steps "^k|sleep:600|miss|sleep:500|{ENTER}" -ExeRoot $inst
& "$root\tools\capture-window.ps1" -Out "$root\shots\x.png" -ExeRoot $inst
```

`send-keys.ps1` steps are `|`-separated: literal SendKeys text, `sleep:<ms>`, or
`click:<x>,<y>` (window-relative). It **refuses to run** if our window is not
focused — that guard is deliberate, do not weaken it.

A scratch project for testing lives at
`…\Temp\claude\…\scratchpad\demo-project` (a real git repo). Use a scratch
folder for agent tests — **never run test agents in Alan's real projects.**

---

## 7. Suggested next steps, in priority order

1. **Files, Editor, Diff/Review (§41–43)** — the largest remaining surface.
   Monaco is the intended editor, behind a `packages/editor` boundary.
2. **Project Brain (§36–39) and Notes (§40)** — the memory layer.
3. **Onboarding (§13)** — first-run experience; the environment scan already
   provides its data.
4. **Finish localising evidence summaries (§65)** — the mechanism now exists
   (`evidence.code` + `code_args`, rendered through the catalogue) and the
   guardrail refusal uses it. The command/file summaries still need converting.

---

## 8. Blockers needing Alan (see `docs/BLOCKERS.md`)

- **B1** Authenticode code-signing certificate — a purchase. Installer is
  unsigned, so Windows warns. No code changes needed when it arrives.
- **B2** Somewhere public to host `latest.json` — the repo is private, so update
  checks fail honestly rather than pretending to be up to date.
- **B3** Vercel connector needs an interactive OAuth sign-in. Blocks cloud and
  mobile only; nothing local depends on it.
