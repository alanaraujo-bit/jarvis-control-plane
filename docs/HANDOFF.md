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
**379 tests** (370 Rust — 365 run, 5 intentionally `#[ignore]`d because they
need a real `claude` CLI or other environment this machine cannot guarantee —
9 i18n) · all green.

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
| **Global Search (§51)** | knowledge, notes, missions, activity and conversation content, across **every** project, from one shortcut (D25/D26) |
| **An agent writes to its own Brain (§36–§38)** | once, right after an Unattended mission completes, asked one narrow question — never a manually completed mission (D27) |
| **Onboarding (§13)** | shown once per install, reuses the environment scan and `openFolder` as-is, gates the window reveal so there is no flash of the wrong screen |
| **Voice dictation (§54)** | fully local speech-to-text, primed with the project's own vocabulary, typed into the prompt — never auto-submitted (D29) |
| **Split panes (§20)** | up to four terminals at once, three layout presets; splitting changes each pane's CSS box only, so no terminal is re-parented and no scrollback is lost |
| **Scrollback search (§20)** | Ctrl+F over the terminal, match-case, live counter, overview ruler only while searching |
| **Image paste (§22)** | Ctrl+V writes the clipboard image into the session's own directory and types the path; a chip with a hover preview, never a bare filename |
| **Global Search backfill (§51, D30)** | sessions recorded *before* search existed are indexed once, in the background, idempotently — verified against this machine's own recorded sessions |
| **Real-time streaming transcription (§54, D31)** | live captions while recording, VS Code/Cursor-style; a warm `whisper-server.exe` polled every second or so, LocalAgreement-style commit/tail split, animated as each word settles — never touches what gets typed, which still comes from one complete unstreamed pass on stop |

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

*Global Search (§51), in the installed app.* `session_events` turned out to
have no writer since migration 1 — HANDOFF said otherwise; that was wrong.
Wired it into the same transcript tailer that already mirrors usage and file
changes, then ran a real Claude Code turn in a scratch project and searched
for its own reply: only the question came back. The reply carried real usage
alongside its own text, and `mirror`'s routing had one arm pick usage over
text whenever both were present — which is the ordinary shape of a reply, not
an edge case (D26). Fixed, rebuilt, same probe again: both sides of the
exchange found, labelled **Você** and **Agente**, snippet and timestamp
correct, from Mission Control with nothing open. Closed the session and
searched again — Global Search opened it back up as a read-only conversation
tab it had never started, with the model name and token counts still shown,
and closing that tab called nothing on the backend because nothing had ever
been attached.

*An agent writes to its own Brain (§36–§38, D27), in the installed app.* A
scratch project's README stated one fact a reader would not get from the code
alone — the dev server listens on 4173 because something unrelated on the
machine already holds port 3000. Created a mission with a single trivial
criterion, set it Unattended, and ran it against a real Claude Code agent:
the mission completed, and the terminal showed the reflection question arrive
as the agent's very next input rather than folded into the finished turn —
confirming a real bug (item 30 below) actually stays fixed outside its unit
test. The agent answered `GOTCHA: The dev server runs on port 4173, not the
conventional 3000, because port 3000 on this machine is already held by an
unrelated port scanner — anyone expecting localhost:3000 will find nothing
there.` The Brain's Gotcha tab showed exactly that sentence under **Um
agente registrou isto** in amber, the briefed-size counter moved, and
Activity recorded it in pt-BR right after the mission's own completion.
Nothing from `done.txt`'s own "Work confirmed complete" line leaked into the
recorded knowledge. A manually completed mission is never asked — see D27 for
why that gap is deliberate.

*Evidence summaries, fully localised (§65), in the installed app.* The
guardrail refusal was the one sentence J.A.R.V.I.S. authors that already
carried a `code`; the other ten — a command's pass, fail, timeout and
spawn-failure, a file existing, missing, containing text or not, or being
unreadable, and a manual criterion's two states — did not. Gave `Outcome` the
same `code`/`code_args` fields and found, before ever running it, that the
`INSERT` in `run_and_record` named only the original columns: every code
would have been computed and then silently dropped on the way into the
database, the same shape as item 17 below. Fixed both that `INSERT` and
`confirm_manual`'s own copy of the same gap. Ran a Command criterion built to
fail and read `` `exit 1` saiu com código 1, esperado 0 `` in pt-BR with both
exit codes substituted; confirmed a Manual criterion and read `Confirmado por
you`.

*Onboarding (§13), in a real build.* Deleted `jarvis.db` to simulate a fresh
install and launched: the welcome screen showed, the environment scan
rendered inside it exactly as it does in Settings, and clicking **Abrir
pasta** opened a scratch project and landed *inside its workspace* — not on
Mission Control. That first attempt is why item 33 below exists: the wiring
that got there the first time silently failed. Reopening the same project
later from the Projects list, and reopening the app a second time after
onboarding had already completed, both skipped straight past the welcome
screen with no flash of it. Also caught mid-verification and refused before
it went anywhere: the folder picker's own remembered last-used location was
one of Alan's real projects (Casco) — cancelled without selecting it, never
touched.

*Voice dictation (§54, D29), in the installed app.* Downloaded the real
~490MB model from Hugging Face through the app's own UI and watched the
progress bar move in real percentages, then verified — hash mismatch, not
assumed — that the file that landed on disk matches the pinned SHA-256.
Clicking the mic with no microphone on this machine produced an honest,
immediate "no microphone is available" rather than a hang or a silent
no-op. With a temporary, fully-reverted bypass feeding the pipeline one
second of silence in place of a live device, ran the whole thing for
real: capture → resample → WAV → the bundled `whisper-cli.exe` → a real
model inference → the transcript typed into the actual terminal prompt.
whisper.cpp hallucinated `[MÚSICA DE FUNDO]` on the silence — in
Portuguese, confirming the locale-to-language mapping actually reaches the
binary — and it sat unsubmitted after the shell prompt, exactly where a
person's own typing would land.

*Voice dictation, against a real headset (§54, D29), in the installed
app.* Alan connected a real microphone and dictated live — the feature
worked and a real sentence landed correctly, unsubmitted, in the terminal
prompt. Two problems came back from that real test, both fixed and
re-verified, not just patched and assumed:

1. **An irritating high-pitched sound played every time a recording
   stopped**, and nothing in this feature intentionally plays audio. Found
   by inspecting a real session log byte-for-byte: literal BEL (0x07) bytes
   sitting in the PTY output, right where typed text landed. The source is
   PSReadLine's default `BellStyle: Audible`, which writes a BEL for
   ordinary line-editor redraws — nothing to do with dictation specifically,
   it would fire from a person's own typing too. Fixed by setting
   `Set-PSReadLineOption -BellStyle None` when a PowerShell session starts
   (`session::commands::default_shell`) — verified by launching the exact
   `-NoLogo -NoExit -Command "..."` args the app uses and reading back
   `(Get-PSReadLineOption).BellStyle` as `None`, then confirming no bare BEL
   appears in a fresh session's log after typing through it.
2. **Dictated Portuguese text came out garbled specifically inside Claude
   Code's own terminal**, while the identical text typed into a plain shell
   arrived intact. Root cause: `session::typing::type_text`'s chunker split
   text by a fixed 48-byte offset with no regard for UTF-8 character
   boundaries, so an accented letter (ção, informação, ...) landing on a
   chunk boundary got sliced across two separate writes. A plain shell's
   line buffer often reassembles that; Claude Code's TUI decodes each PTY
   read independently and renders the split character as replacement
   garbage instead. Fixed by walking every chunk boundary back to the
   nearest real char boundary. Proved against the actual regression, not
   just the isolated logic: a real-PTY test
   (`a_multibyte_sentence_survives_claude_codes_own_tui`, `#[ignore]`d
   because it spawns the real `claude` CLI) drives an actual Claude Code
   session through its first-run trust prompt and types "testando
   informação e configuração" into it — the raw captured PTY bytes show the
   accented words rendered perfectly.

Also implemented in response to the same conversation: soft, two-note start
and finish chimes (`surfaces/voice/sound.ts`, pure Web Audio sine tones,
gain ramped rather than switched to avoid a click), synthesized rather than
shipped as audio files because this app's CSP has no `media-src`
allowance. The start chime is awaited before the microphone opens, so a
recording never captures its own cue. **Not yet re-verified by ear against
this latest rebuild** — Alan asked to move that verification, plus a much
bigger ask (real-time streaming transcription, see next steps), to a fresh
conversation. Read `docs/DECISIONS.md`'s voice dictation entry for the full
before/after on both fixes.

A real human voice through a real microphone was verified once (see
above); a **repeat** pass confirming the sound fixes and testing the
now-fixed Claude Code path with real speech has not happened yet — see next
steps.

*Real-time streaming transcription (§54, D31), in the installed app.* Built
in the same conversation that asked for it: a warm `whisper-server.exe`,
polled every second or so, folded through a LocalAgreement-style commit/tail
split (`voice::stream::AgreementState`) so the caption only ever grows and
never rewrites a word a person already read. Verified against real
infrastructure at every layer before it was considered done — the HTTP
contract (`multipart/form-data`, `response_format=json`, per-request
`prompt`/`language`) was reverse-engineered against the real binary with
`curl.exe` before any Rust was written; an `#[ignore]`d integration test
spawns the real `whisper-server.exe` against the real downloaded model and
posts real audio over HTTP; and — since this machine still has no
microphone — the full command → poll-thread → `Channel` → caption path was
driven through the **installed app** with a temporary, fully-reverted
synthetic-audio generator standing in for `cpal` (same shape as D29's own
silence bypass). It worked on screen: `whisper-server` hallucinated
`[Música]` on the synthetic tone, two consecutive polls agreeing on it
committed it in the settled colour, a third poll's differently-capitalised
guess correctly rendered as a *new* uncommitted tail instead of overwriting
the settled word, and stopping still typed the terminal's transcript from
the separate, complete, unstreamed pass — unsubmitted, exactly as without
streaming. That live pass caught a real bug: `whisper-server.exe` survived a
`taskkill` of the whole app, because it had never been placed in a Windows
job object the way `pty::spawn` already contains agent children (see
`pty::job`). Fixed by giving `ServerHandle` its own job and reverified by
force-killing the app again — see D31 for the full account, including why
the `b4938` release tag turned out not to mean one fixed set of bytes.
**Not yet verified with a real human voice** — this feature did not exist
when D29's own ear-test debt was recorded, so both are now owed together;
see next steps.

### Not built — deliberately absent, not stubbed

Preview (§46), mobile PWA (§55), cloud (§59).

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

29. **A table with a comment describing its purpose can still have no writer.**
    `session_events` was declared in migration 1 as "a searchable mirror... for
    §51", and nothing ever inserted into it — a repo-wide check of every
    `INSERT` site confirmed zero rows on every installation this product has
    ever run on. The earlier version of this file said `session_events`
    "exists exactly for this"; it did not. Comments describe intent, not
    behaviour — the same lesson as item 24, one layer earlier: read the
    schema's writers, not its doc comment, before building on top of it.

30. **A `Message` with real text and real usage together is the ordinary shape
    of a reply, not an edge case.** `mirror`'s first pass routed on one
    `match`: a message carrying non-empty usage took the "record usage" arm
    and never reached "record searchable text". Every substantive reply an
    agent ever gave carried both, so every substantive reply was silently
    absent from search — only a plain user-typed message, which never carries
    usage, was ever indexed. Found by running a real Claude Code turn and
    searching for its own answer: only the question came back. See D26.

31. **`cargo build --release` does not produce the binary this product
    ships.** It compiles `jarvis.exe` — the crate's own name — and Tauri's own
    build step is what copies it to `jarvis-desktop.exe` afterward (D9).
    Launching the plain-cargo binary directly showed a window that never
    became visible: correct size, correct title, `IsWindowVisible = false` —
    the frontend's `window_ready` reveal never fired. `pnpm tauri build
    --no-bundle` produces the same binary the installer would, in about the
    same time as a plain `cargo build`, skipping only NSIS — which is worth
    knowing separately, because NSIS is currently the part that hangs (see
    `docs/BLOCKERS.md` B5). Use `--no-bundle` to verify a change against the
    real app; a bare `cargo build` is not that.

32. **A struct carrying the right fields does not mean the row on disk has
    them.** Evidence's `code`/`code_args` (§65) reached `Outcome` and
    `evidence_from` cleanly, and would still have never once reached the
    database: `run_and_record`'s raw `INSERT` named only the columns it was
    written with before those fields existed, so every localisation code
    would have been computed correctly and silently dropped a function call
    later. `confirm_manual` had the identical gap in its own `INSERT`. Same
    lesson as item 17, one call site further from where the value is
    computed — checked before running anything, by reading the SQL rather
    than trusting the type, then confirmed on screen in pt-BR.

33. **A `useCallback` memoized on a list closes over the render it was created
    in, not the render that runs it.** `openProjectById(project.id)` looked up
    the id in `projects` — but the click handler holding that closure had
    already fired before `openFolder()`'s own `refresh()` updated the store, so
    the closure it ran still had the *old* (often empty) `projects` array
    baked in. The lookup returned `undefined`, and the failure was silent: no
    error, just a fall-through to whatever the surface renders by default —
    Mission Control instead of the project just opened. JS does not hot-swap a
    running async closure for a fresher one after a re-render. Found by
    opening a first project from the new Onboarding screen and landing on
    Mission Control instead. Every caller that already holds the `Project`
    object — `openFolder()`'s return value, a row click, `Projects.tsx`'s own
    `pick()` — now calls `openProjectDirect(project)` and skips the lookup
    entirely; `openProjectById(id)` still exists for the callers (Missions,
    Global Search) that genuinely only have an id, and is not safe to use for
    a project that may not be in the store yet.

34. **`whisper-rs` (whisper.cpp linked as an FFI library) is currently broken
    on Windows/MSVC.** Its bindgen step emits *glibc*-specific types
    (`_G_fpos_t`, `_IO_FILE`) for an MSVC target regardless of `--target` —
    confirmed by hitting the identical `attempt to compute N - M, which would
    overflow` compile error on two crate versions (0.14.4 and 0.16.0), not a
    local misconfiguration. `WHISPER_DONT_GENERATE_BINDINGS=1` does not avoid
    it either. Voice dictation (§54, D29) works around this entirely by
    spawning the official prebuilt `whisper-cli.exe` as a subprocess instead
    of linking the library — do not re-attempt the FFI path without checking
    whether this upstream issue has since been fixed.

35. **This development machine has no microphone.** `cpal`'s
    `default_input_device()` returns `None` here, and Device Manager agrees —
    it is not a driver or a privacy-setting problem, the hardware is simply
    absent. **Resolved for a first pass**: Alan connected a real headset and
    dictation worked. It surfaced two real bugs (PSReadLine's bell, and
    UTF-8 chunk-splitting — see item 36), both fixed and proven. What is
    still unverified is a repeat pass with the fixes in place, plus the new
    sound cues, by ear, on a real microphone — see section 7.

36. **Chunking a string by a fixed byte count is not safe for anything
    outside ASCII.** `session::typing::type_text` split dictated text into
    48-byte pieces for pacing (item 11 above), and a raw byte offset can
    land inside a multi-byte UTF-8 character — an accented letter (ção,
    informação, ...) sliced across two separate writes 30ms apart. A plain
    shell's line buffer tends to reassemble that silently; Claude Code's own
    TUI decodes each PTY read independently and renders the split character
    as garbage instead — which is exactly the shape of a real bug report
    ("dictation works in a plain terminal, breaks specifically inside Claude
    Code"). `char_boundary_chunks` now walks a chunk boundary back to the
    nearest real character boundary before writing. Proved against the
    actual failure, not just the isolated function: a real-PTY test spawns
    the genuine `claude` CLI, clears its first-run trust prompt, types an
    accented Portuguese sentence, and reads the captured bytes back —
    `a_multibyte_sentence_survives_claude_codes_own_tui` in
    `session/typing.rs`, `#[ignore]`d because `claude` is not guaranteed to
    be on every machine that runs the test suite.

37. **Deleting `jarvis.db` strands every session log on disk, permanently.**
    The onboarding reset in section 6 (`Remove-Item ...jarvis.db*`) is
    documented as the way to see §13 again, and it is — but it also takes
    every row in `sessions` with it while leaving the directories under
    `%APPDATA%\dev.jarvis.desktop\sessions` exactly where they are. There are
    **42 session directories on this machine and 10 session rows**; the ten
    directories holding real Claude Code conversations, 82 items between them,
    have no row pointing at them any more and nothing will ever read them
    again. This is a **development-environment consequence, not a product
    bug** — the product itself never deletes a project or a session (the only
    `DELETE FROM projects` in the tree is in two tests), and archiving keeps
    the row. It was found by running the Global Search backfill against a copy
    of the real database and getting `0 rows` from 10 sessions, which looked
    exactly like a broken backfill and was not. Two things follow: reach for
    that reset less casually than the section-6 snippet suggests, and do not
    trust "the live database has no conversations in it" as evidence about
    anything except the last reset.

38. **Session logs are never pruned, by anything.** Nothing in this product
    removes a `sessions/<id>/` directory — not archiving a project, not
    closing a session, not uninstalling (M10 preserves user data on purpose).
    2.5 MB across 42 sessions here is nothing, and the growth is unbounded and
    proportional to how much the product is actually used: a heavy month of
    agent work is terminal output measured in hundreds of megabytes. No
    retention policy has been designed and none should be invented casually —
    the log **is** the record (§23), so pruning it is throwing away history
    Conversation View, Analytics and Global Search all read. Flagged here so
    it is a decision someone makes rather than a surprise someone discovers.

39. **A process kept alive for a whole app session needs its own job
    object — one scoped to a single PTY does not cover it.** `pty::spawn`
    already contains every agent CLI child so a `taskkill` of J.A.R.V.I.S.
    never orphans one (see `pty::job`, next to D6/D7), but that containment
    is created fresh per PTY session. `voice::server::ServerHandle` (§54
    streaming, D31) is deliberately *not* scoped to one PTY — it is spawned
    once and kept warm for the app's whole lifetime — and the first version
    simply spawned `whisper-server.exe` as a plain child with none of that
    containment. Found live, not in a test: force-killing the app left
    `whisper-server.exe` running on its own, unowned, exactly the shape rule
    8's job objects exist to prevent. Fixed by giving `ServerHandle` its own
    `pty::job::ProcessJob` (now `pub(crate)` so `voice` can reuse it) and
    reverified by force-killing the app a second time and watching the
    server die with it. The lesson generalises: containment scoped to "this
    session" is not containment scoped to "this app run" — check which one a
    new long-lived child process actually needs.

39. **An embedded browser answers shortcuts you did not take.** Ctrl+F for the
    terminal's scrollback search (§20) was handled inside xterm's
    `attachCustomKeyEventHandler`, which runs only while the terminal literally
    holds DOM focus. Come back from the command palette, or click the tab
    strip, and the key never reached xterm — at which point **WebView2 answered
    it with its own built-in find-in-page**: a Chromium widget floating over
    the app, styled like nothing else in the product, searching the DOM rather
    than the scrollback. This is item 13 a second time with a different
    villain. A shortcut the product owns has to be taken on `window` in the
    **capture** phase, before any widget *or the host browser* gets an opinion;
    `preventDefault` there is what stops WebView2. Guard it on which surface is
    actually visible, or several mounted terminals all answer at once — and
    Ctrl+F must still reach Monaco's own find when the editor is on screen.

40. **`event.key` is `"F"`, not `"f"`, whenever Shift or CapsLock is down.**
    A case-sensitive comparison means a shortcut that silently stops working
    for anyone with CapsLock on. `App.tsx` already spells its own shortcuts
    with `toLowerCase()`; the terminal's did not, and Ctrl+F went to the shell
    as `^F` while nothing opened. This machine had CapsLock on, which is the
    only reason it was found at all.

41. **`max-width` measures the border box, so padding eats the measure.**
    A sentence aligned under a control with `padding-left: 196px` and
    `max-width: 62ch` gets 62ch *minus the padding* to wrap in — it wrapped
    after about thirty characters, a two-line stub beside half a screen of
    empty space. `margin-left` is the alignment that does not steal from the
    measure.

42. **Do not judge a colour from a screenshot; sample the pixels.** A capture
    of the terminal in light theme read as clearly dark to the eye, and a whole
    line of investigation started into a theme bug that did not exist —
    `GetPixel` said `#FFFFFF`. Rendered captures can misread badly. When the
    claim is about colour, `System.Drawing.Bitmap::GetPixel` settles it in one
    command and costs nothing:

    ```powershell
    Add-Type -AssemblyName System.Drawing
    $b = [System.Drawing.Bitmap]::FromFile('shot.png')
    $c = $b.GetPixel(700, 600); '#{0:X2}{1:X2}{2:X2}' -f $c.R, $c.G, $c.B
    ```

    Sample a **background**, not a glyph — the first sample landed on amber
    terminal text inside a blue highlight and read as amber.

43. **WebView2 raises no `paste` event for clipboard *images*.** Only for
    text. This is not a focus problem and not a listener-placement problem —
    both were tried, on the host element and then on `window` in the capture
    phase, and neither fired. The app was made to report what it actually
    received: the Ctrl+V `keydown` arrives at xterm's helper textarea and no
    `paste` event follows it at all, so `clipboardData.items` is never
    reached and there is nothing to read. **Every browser-shaped approach
    fails identically**, which is worth knowing before spending an hour on
    the next one. Image paste (§22) hooks Ctrl+V and has the core read the
    clipboard with `arboard`; on Windows that returns a raw RGBA buffer, not
    a file, so it is encoded to PNG before anything is written.

    The general lesson, and the reason this cost time: **a missing event is
    indistinguishable from a broken handler**. Two rebuilds went into moving
    a listener that was never going to fire. One temporary line that printed
    what the app actually saw settled it immediately — when something does
    not happen, make the program say what *did*.

44. **A truncated UUID is not a unique filename.** `&uuid[..8]` reads like a
    reasonable short id and is not: UUIDv7's leading hex digits *are* a
    millisecond timestamp, so two of them created in the same millisecond are
    byte-identical. Two images pasted quickly produced one file, the second
    silently overwriting the first. Caught by a test that writes twice in a
    row — which is fast enough to land in one millisecond, exactly as a
    person pasting twice is.

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
`testsC:\Users\...` and an "invalid folder name" box. The picker also opens on
whatever folder was browsed last — **screenshot it before typing anything**;
it has landed on one of Alan's real project folders more than once, which the
scratch-folder rule (below) exists to keep you out of.

Onboarding (§13) shows once per install, gated on a `settings` row keyed
`onboarding.seen`. To see it again — after a real change, not just to look —
close the app and delete the whole db file, there is no in-app reset and no
`sqlite3` on this machine to `UPDATE` a single row:

```powershell
Remove-Item "$env:APPDATA\dev.jarvis.desktop\jarvis.db*" -Force
```

(The installed app's data directory is `%LOCALAPPDATA%\J.A.R.V.I.S`, not
`%APPDATA%\dev.jarvis.desktop` — that path is the dev build's own app-data dir,
named after the Tauri identifier. Confirm which binary you're resetting for.)

Voice dictation (§54) needs `resources/whisper/whisper-cli.exe` and its DLLs
on disk — `pnpm tauri build --no-bundle` copies them into
`target/release/whisper/` automatically (verified: `--no-bundle` does copy
declared `bundle.resources`, not just full `tauri build`). The model itself
downloads through the app's own UI on first use; it is **not** checked into
the repo. Chasing the FFI path (item 34 above) installed LLVM, CMake and
Ninja on this machine via `winget` — none of them are needed by the shipped
architecture, so there was nothing to revert, but a future session should
know they are here and why.

`whisper-server.exe` (§54 streaming, D31) is bundled too now, in
`resources/whisper/server/`, deliberately **not** the same directory as
`whisper-cli.exe`. Re-fetching the `b4938` tag for this pass pulled a
materially different build than the one already committed for
`whisper-cli.exe` — different `whisper.dll`/`ggml*.dll` bytes, CPU-dispatch
variants (`ggml-cpu-alderlake.dll` and eight siblings) instead of one
`ggml-cpu.dll`, and it does not need `llama.dll`, `parakeet.dll` or
`SDL2.dll` from that same zip — confirmed by removing them and checking
`whisper-server.exe --help` still ran, not assumed from the file list. See
D31 for the full account. `whisper-stream.exe` (the zip's third binary)
still needs SDL2 for its own microphone capture and remains not a fit — this
codebase owns capture via `cpal`.

---

## 7. Suggested next steps, in priority order

**M6 and M7 are both finished.** Files, editor, Review, Git, worktrees, the
memory layer, Global Search, an agent writing to its own Brain, Onboarding
and voice dictation are all built and verified against real data and a real
agent. Voice dictation has now been tested with a real microphone once (see
section 4) and two real bugs it surfaced are fixed (items 35–36 in section
5). Real-time streaming transcription — the substantial feature request
from that same session — is now also built and verified (D31, section 4,
item 39 in section 5). What is open is one **combined** live ear test
covering everything voice-related that has shipped since the last one:

1. **Re-verify voice dictation end to end, by ear, on a real microphone —
   the sound cues, both earlier bug fixes, and now streaming too.** The
   start/finish chimes (`surfaces/voice/sound.ts`), the PSReadLine-bell and
   UTF-8-chunking fixes (items 35–36 in section 5), and the live-caption
   streaming path (D31) have each been proven mechanically or through a
   synthetic-audio bypass, but nobody has listened to the chimes yet, and no
   build has been driven through a live headset dictation pass — captions
   included — end to end. One pass now covers all of it rather than two
   separate ones.
3. **The rest of M2** — split panes (§20), scrollback search, image paste (§22,
   with a hover preview of the pasted image — reinforced explicitly, make sure
   it lands with the rest rather than as a bare paste). The terminal is the
   hero surface and these are what it still lacks.
3. ~~**Global Search does not backfill.**~~ **Built and verified as a walk; the startup wiring is not yet verified** — see D30 and section 4.
   It is a background task with a bookmark, deliberately *not* a migration:
   migration 10 adds `sessions.events_backfilled_at` and nothing else, and
   `search::backfill` does the walk five seconds after launch, chunked so the
   single mutex-guarded connection is never held long, delete-then-insert per
   session with the bookmark stamped last so a crash halfway through is redone
   rather than doubled. `SessionLogReader::for_each_structured` is what makes
   it affordable — `read_from(0)` would have materialised every PTY frame in
   every log to find the JSON between them.
4. **A manually completed mission is never asked what it learned (D27).**
   The reflection only fires at the end of an Unattended run, because that is
   the one place a driven run holds the seat with nobody else in it (D15).
   Extending it to an attended completion needs an answer to whose seat it
   is — typing a question into a terminal a person is watching mid-conversation
   is a different UX problem than this pass solved.

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
- **B5** `pnpm tauri build`'s NSIS step hangs on this machine — no `makensis`
  process ever appears, zero CPU, for 25+ minutes. `--no-bundle` works fine and
  is what this session used throughout. Not urgent, but real.
