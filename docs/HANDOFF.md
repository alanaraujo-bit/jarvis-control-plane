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
**333 tests** (324 Rust, 9 i18n) · all green.

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
| **Diff / Review (§43)** | what changed since `HEAD`, and **which agent changed it** |
| **Git write operations (§44)** | stage, unstage, discard, restore, commit — discard routed through the guardrail (D11/D19/D20) |
| **Worktrees (§45)** | a worktree is a project (D18), so opening one opens it everywhere |
| **Project Brain (§36–§38)** | what is known about a project, **briefed to every agent that starts here** (D21/D23) |
| **Project history (§39)** | the project's own story, and what the record shows — derived, never stored (D22) |
| **Notes (§40)** | working memory, never sent to an agent; promotable into knowledge |

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


*Git, in the installed app (§44).* Staged a file and watched the row say so;
discarded one carrying **both** staged and working-tree changes and checked on
disk that it came back as the committed version rather than the staged one;
restored a deleted file, where the same command is a recovery and the surface
says so; committed three staged files; then chose **Never allow** and watched
the next discard be refused with the working tree untouched.

*Worktrees, in the installed app (§45).* Created one for `agent/login-form` and
saw the slash become a dash rather than a nested directory; opened it and read
its own diff in Review, against its own branch; put uncommitted work in it and
removed it — refused by Git first, then held by the guardrail, then done, with
its project row archived. Both themes, both languages.


*The memory layer, in the installed app (§36–§40).* Wrote four things into a
scratch project's Brain, started Claude Code from the app, and asked what it
knew **without reading any files**. It answered with all four, grouped under
the brief's own headings, in a folder it had never seen before — and said
plainly it had no other context without reading the repo. The brief was 484
bytes in our own log directory beside the guardrail snapshot, matching the size
the panel reported, and `git status` in the project was empty: nothing of ours
in the user's repository. Promoted a note into knowledge and watched it leave
the notes list and the count fall. Both themes, both languages.

### Not built — deliberately absent, not stubbed

Preview (§46), Global Search (§51), onboarding (§13), mobile PWA (§55),
cloud (§59), voice (§54).

Also absent by choice: **push and pull**. Review commits but does not talk to a
remote — see section 7.

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

20. **`git restore <path>` restores from the *index*, not from `HEAD`.** A file
    with staged content comes back as the staged version, so a "discard" built
    on it throws away half a change and reports success. It also fails outright
    on a **staged deletion**. The spelling that reaches the commit is
    `restore --source=HEAD --staged --worktree`, and an untracked file is not a
    restore at all — there is nothing committed to return to, so it is
    `git clean -f`. In a repository with **no commits** there is no `HEAD`
    either: unstaging is `git rm --cached`, because `restore --staged` dies with
    `could not resolve HEAD`. See D20; all three are tested against real repos.

21. **Porcelain reports an untracked file as `??`, which is neither staged nor
    unstaged.** A faithful reading of the two status columns, and the wrong
    answer for a Review row: it removed the stage button from every new file, so
    nothing an agent created could be staged or committed. Nothing errored, no
    test noticed, and the row looked completely normal — found by hovering one
    in the real app and counting the buttons. `review::staging_state` is the
    correction and `a_new_file_can_be_staged` pins it.

22. **`git worktree list --porcelain -z` separates records with an empty
    field.** Records are NUL-terminated *lines*, so the blank line between
    records arrives as two NULs in a row. And `detached`, `bare`, `locked` and
    `prunable` are **bare words with no value** — a parser assuming `key value`
    throughout misreads them. `-z` is not optional here: the common path on this
    machine contains a space.

23. **A tauri command that is not in `invoke_handler!` fails at runtime, not at
    build time.** Three worktree commands were written, exported and compiled
    cleanly while the surface reported `Command worktree_report not found`. The
    registration had been added by a script whose earlier step failed, so it
    never ran — which is rule 3 in section 3 of this file, met again. Verify the
    patch applied.

24. **A project area that is hidden is still mounted, so `useEffect` never
    fires again.** Files, Review, Worktrees and the Brain are mounted once and
    hidden with CSS, deliberately, so returning to one keeps its open file and
    scroll position. An effect keyed on the project id therefore runs on mount
    and never again — and Review carried a comment saying it re-read on every
    visit directly above an effect that did not. Nothing errors; the surface
    shows what was true when it was first opened. `useVisitRefresh` watches
    the transition into view. See D24, and note the shape: the **comment was
    right and the code was wrong**, so reading the code would have confirmed
    the intention rather than the behaviour.

25. **Claude Code asks "is this a project you trust?" the first time it opens
    any folder, and waits.** Everything typed before it is answered goes into
    that dialog rather than to the agent. Under Unattended (§32) nobody can
    answer, so the run would sit there until its budget ran out. **A worktree
    is a brand-new folder**, so §45 made this reachable the moment it shipped.
    `autopilot_start` refuses with `autopilot.folderNotTrusted`; trust is read
    from `~/.claude.json` and never written — that is the user's decision to
    make in Claude Code's own interface.

26. **A capability verified in `-p` mode has not been verified.** Every session
    this product starts is an interactive PTY.
    `--append-system-prompt-file` plainly works under `claude -p`, and assuming
    that settled it is the same shape as the Monaco option that exists,
    type-checks and does nothing (item 12). It was settled properly by writing
    knowledge into a project's Brain and asking a real interactive session what
    it knew — see D23.

27. **Do not scrape the TUI to find out what an agent said.** An early version
    of the briefing test searched the PTY stream for a word and reported the
    question "never reached the agent" while the agent was answering: Claude
    Code redraws its input line character by character with cursor moves
    interleaved, so typed text very often never appears as a contiguous run of
    bytes anywhere in the stream. D3 rejected scraping for the product and it is
    no more sound in a test — read the transcript.

28. **A sentence with a number in it needs the number, not just the digits.**
    The Brain's derived facts passed positional arguments and never reached the
    plural machinery, so every sentence was written for the plural and used for
    everything: pt-BR rendered **"1 sessões"**. A `Fact` now carries the count
    it has to agree with, separately from its arguments, because guessing which
    argument decides the plural is not the interface's job. Found on screen.

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

**M6 is finished** (Files, editor, Review, Git, worktrees) and **M7 is nearly
finished** — the memory layer is built and verified against a real agent.

1. **Global Search (§51)** — the last of M7, and the one question the product
   cannot yet answer: *"where did I see that?"* Everything it would search is
   already in one place (D2): `session_events` exists for exactly this, and the
   Brain, missions and activity are all queryable rows.
2. **Let an agent write to the Brain.** The whole path is in place —
   `add_knowledge` takes `Source::Agent`, `session_id` and `mission_id`, the
   surface renders "an agent recorded this" in amber, and no code produces such
   a row. What is missing is the *moment*: most plausibly at the end of a
   verified mission, asking the agent what it learned that should outlive the
   session. Worth thinking about carefully — an agent that writes freely into
   a project's memory will fill it with restatements of its own last task.
3. **Onboarding (§13)** — the environment scan already provides its data.
4. **Finish localising evidence summaries (§65)** — the mechanism exists
   (`evidence.code` + `code_args`); the command/file summaries still need it.
5. **The rest of M2** — split panes (§20), scrollback search, image paste (§22).
   The terminal is the hero surface and these are what it still lacks.

### Things left on the table, deliberately

- **Push and pull.** Review commits but does not talk to a remote.
  `git.force-push` has been in the classifier since §35 and `guardrail::surface`
  is ready for it, so this is a surface rather than a design. Worth doing with
  the credential story in mind: `GIT_TERMINAL_PROMPT=0` (D5) means a push
  needing authentication fails rather than hangs — the right failure, not yet an
  explained one.
- **Nothing offers to start an agent *in* a worktree.** That is the reason
  worktrees exist (§45), and the pieces are all there since a worktree is a
  project (D18). What is missing is the one gesture that does both from a
  mission — and note item 25 before building it: a fresh worktree is an
  untrusted folder.
- **Briefing Codex.** It reports `OpeningMessage` and is given nothing. The
  delivery would be the autopilot's own typing path (D16), and it costs a turn
  and a wall of text in the terminal, so it is worth deciding whether that is
  better than starting unbriefed rather than assuming it is.

## 8. Blockers needing Alan (see `docs/BLOCKERS.md`)

- **B1** Authenticode code-signing certificate — a purchase. Installer is
  unsigned, so Windows warns. No code changes needed when it arrives.
- **B2** Somewhere public to host `latest.json` — the repo is private, so update
  checks fail honestly rather than pretending to be up to date.
- **B3** Vercel connector needs an interactive OAuth sign-in. Blocks cloud and
  mobile only; nothing local depends on it.
