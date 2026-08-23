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

Ten of the most important bugs in this codebase were invisible to tests and
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
**244 tests** (236 Rust, 8 i18n) · all green.

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
| **Files (§41)** | lazy tree, Git-ignored entries dimmed rather than hidden, every path confined to the project |
| **Editor (§42)** | Monaco behind `packages/editor`, loaded on demand, themed from the tokens, line endings preserved on save |
| **Diff / Review (§43)** | what changed since `HEAD`, and **which agent changed it** — read-only |

**The full loop has been executed end to end**, twice over:

*By hand.* Mission created → completion refused → agent launched from the
mission → Claude Code created a real file → verification found the evidence →
mission completed. Then the file was deleted, re-verification ran, and
completion was **revoked**.

*Unattended (§32).* Mission set to Unattended → "run until done" → a real Claude
Code agent was started, driven turn by turn, created the file, verification
passed, and the mission **completed on its own** with `autopilot.completed` in
the activity log. Nobody typed anything in the middle.

*Guardrails (§35), against real agents.* A force push was refused before it
executed. With nobody able to answer, `rm -rf` was refused, the agent reported
it was blocked, explicitly declined to find another way, and the directory was
still there afterwards.

*Files, Editor and Review, in the installed app.* Browsed a real repository with
`node_modules` dimmed as ignored and `.git` absent; opened a file in Monaco,
edited it, saved with Ctrl+S, and checked on disk that the bytes changed and the
LF line endings were not rewritten. Then a **real Claude Code agent** was run in
a scratch repository, created a file, and Review put that file at the top of the
list attributed to Claude Code — the attribution path only works because the
session's `cwd` is folded into the recorded path, which is the kind of thing
that matches nothing and looks like an empty state when you get it wrong.

### Not built — deliberately absent, not stubbed

Project Brain (§36–39), Notes (§40), **Git write operations and worktrees
(§44/§45)**, Preview (§46), Global Search (§51), onboarding (§13),
mobile PWA (§55), cloud (§59), voice (§54).

Review deliberately reads and does not write: stage, discard and restore are
destructive Git operations, and D11 says those go through the guardrail rather
than behind a plain button. See ROADMAP M6.

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

12. **Monaco's `bracketPairColorization` editor option does nothing.** It
    exists, it type-checks, and in the standalone ESM build nothing reads it —
    it is wired through VS Code's configuration service, which is not here. The
    switch that works is on the **model**, and it drops a word:
    `model.updateOptions({ bracketColorizationOptions: { enabled: false } })`.
    The theme also paints all six depths as plain punctuation, so the worst
    case is never gold brackets competing with the amber that means agent work.

13. **A global shortcut has to be captured, not awaited.** Monaco treats Ctrl+K
    as a chord prefix and calls `stopPropagation`, so a bubble-phase listener on
    `window` never fires and the command palette silently stopped opening
    whenever the editor had focus. `App.tsx` now handles it in the **capture**
    phase. Consequence, stated plainly: Ctrl+K no longer reaches a shell as
    readline's kill-to-end-of-line.

14. **`visibility: hidden` on a container does not hide a child that sets
    `visibility: visible`.** The project areas hide each other that way, and
    `.workspace__pane[data-visible]` re-asserted it — leaving a live terminal
    painted and swallowing every click while Files was on screen. The tree
    rendered perfectly and simply did not respond, and only with a session
    actually running. `.workspace__area-body:not([data-visible])` now puts its
    own children back.

15. **A one-sided pathspec defeats Git's rename detection.**
    `git diff -M HEAD -- new.txt` reports `new file` with every line added,
    because `-M` has nothing to pair against. Both names must be on the command
    line. A moved file would otherwise be reported to a reviewer as one the
    agent rewrote from scratch.

16. **`git status` collapses a wholly untracked directory into one entry.**
    The default `--untracked-files=normal` reports `assets/` rather than the
    files inside it — a record with no filename, no line count and nothing to
    diff, which rendered in Review as a row with a blank name. Review asks for
    `--untracked-files=all`.

17. **`#[serde(rename_all = "camelCase")]` on an *enum* renames the variants,
    not the fields of its struct variants.** `WriteOutcome` went out as
    `{"status":"written","modified_ms":…}`, the webview read `modifiedMs` as
    `undefined`, and every save after the first quietly stopped checking
    whether the file had changed underneath it. Nothing errored and every test
    passed — it was found by saving a file in the real app and watching another
    writer's line disappear. `rename_all_fields = "camelCase"` is the missing
    half, and `a_write_outcome_serialises_with_the_field_names_the_webview_reads`
    now pins the wire shape. This is D13 one layer down: check the **bytes**,
    not the Rust type.

18. **The save conflict check is good, not absolute.** It compares the file's
    modified time, so two writes inside the same filesystem timestamp tick are
    indistinguishable. Measured on this machine: an external write is visible
    immediately and lands ~30 ms apart, so the realistic window is tiny — but
    it is not zero, and the product should never be described as making the
    race impossible.

19. **A driven session is not "attended", even with the terminal open.**
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

# The rail is more reliable than the command palette: the app may be running
# in pt-BR, where English search terms match nothing. Rail icons top to bottom
# are Mission Control, Projects, Missions, Activity, Analytics — and Settings
# at the foot (27,850).
& "$root\tools\send-keys.ps1"      -Steps "click:27,134|sleep:1600" -ExeRoot $inst
& "$root\tools\capture-window.ps1" -Out "$root\shots\x.png" -ExeRoot $inst
```

`send-keys.ps1` steps are `|`-separated: literal SendKeys text, `sleep:<ms>`, or
`click:<x>,<y>` (window-relative). It **refuses to run** if our window is not
focused — that guard is deliberate, do not weaken it.

**Use a scratch folder for agent tests — never run test agents in Alan's real
projects.** Your session's own scratchpad directory is the right place: create
a `demo-project` there and `git init` it, because several surfaces need a real
repository.

Note that the scratchpad path is **per session**. Earlier sessions did exactly
this, so the app's project list may already show a `demo-project` pointing at a
previous session's temp folder — a path that no longer exists. Add yours as a
new project rather than assuming the existing entry is live.

Driving the folder picker: click **Abrir pasta**, click the `Pasta:` field, then
`^a` and `{DEL}` **before** typing the path. The field pre-fills with whatever is
selected in the listing, and typing over it silently produces
`testsC:\Users\...` and an "invalid folder name" box.

---

## 7. Suggested next steps, in priority order

1. **Git + worktrees (§44/§45)** — the other half of **M6**, and the reason
   Review currently only reads.

   Every write here is a destructive operation run on the user's behalf —
   staging, discarding a change, restoring a file, and later pushing. D11 draws
   the line clearly: where J.A.R.V.I.S. owns the process, the guardrail is
   *unconditional*, so these must route through it rather than sit behind a
   plain button. `guardrail::classify` already knows the sensitive Git
   operations; the missing part is the surface asking it before acting, plus the
   confirmation for the ones a person should see first.

   Worktrees (§45) are the reason D5 chose the `git` executable over libgit2 in
   the first place, so that part should be straightforward.

   The pieces M6 leaves ready: `git::locate` gives the repo root and the
   project's prefix inside it, `git::status` parses porcelain `-z` (including
   both rename orderings, which differ between `status` and `diff --numstat`),
   and `git::diff` turns a patch into numbered hunks.
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
