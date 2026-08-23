# J.A.R.V.I.S. — Living Roadmap

Status legend: `[ ]` not started · `[~]` in progress · `[x]` done & verified

Rule (§69/§81): milestones ship **finished**, not scaffolded. A milestone is
done only when it passes the §79 Definition of Done, including real visual
inspection (§76).

---

## M0 — Foundation  ✅
- [x] Monorepo (pnpm workspaces), Tauri v2 shell, real window on screen
- [x] Design token system (dark + light, both first-class §8)
- [x] App chrome: custom titlebar, caption buttons, reduce-motion
- [x] i18n foundation wired from the first string (§65)
- [x] Command palette with subsequence ranking (§50)
**Verified:** launched and screenshotted in both themes.

## M1 — Session Core (the load-bearing milestone §23)  ✅
- [x] Append-only session event log (frame codec + sparse index)
- [x] Crash recovery from a torn trailing frame
- [x] Bounded two-pass replay (never buffers a whole session)
- [x] SQLite WAL + forward-only migrations
- [x] Projects over real folders, Git detection via the git binary
**Verified:** 28 tests, including a real git repository.

## M2 — Terminal (hero surface §21)  ~
- [x] PTY host (portable-pty), resize, lifecycle
- [x] Windows job objects — no orphaned agent processes
- [x] Single-writer session runtime; unattended sessions run with no view (§32)
- [x] Raw-byte IPC channel, coalesced at 16ms
- [x] xterm.js integration, scrollback, per-theme palettes, tabs
- [ ] Split panes and layout presets (§20)
- [ ] Search within scrollback
- [ ] Image paste as first-class attachment (§22) — pasted into the terminal,
      with a hover preview of the image rather than a bare filename/placeholder
      (Alan's own requirement, 2026-08-23; not yet scoped further)

## M3 — Providers (§26)  ✅
- [x] Provider adapter trait + capability model (capabilities as data)
- [x] Claude Code adapter — deterministic correlation via --session-id
- [x] Codex adapter — file-watch correlation, stated honestly as weaker
- [x] JSONL tailer: partial lines, split UTF-8, truncation, CRLF
- [x] Environment scan (§14)
**Verified:** the parser was run over all 79 real transcripts on this machine —
49,905 conversation items, 19,582 carrying official usage.

## M4 — Conversation View (§24)  ✅
- [x] Structured projection: turns, tool calls with results, thinking, errors
- [x] Official token usage per turn, never estimates dressed as facts (§28)
- [x] Terminal ↔ Conversation toggle over one session, process untouched
**Verified:** a real Claude Code session in a real repo, shown in both views.

## M5 — Missions (§29–§35)  ~
- [x] Mission model, states, tasks, acceptance criteria
- [x] Evidence + Verification by running real checks (§30)
- [x] Completion refused without evidence, and **revoked** when evidence stops holding
- [x] Withdrawing a criterion is recorded with a reason, never deleted (§31)
- [x] Autonomy profiles + inheritance across mission/project/global (§33)
- [x] Blocked always reachable, and must explain itself (§34)
- [x] Mission Control home, sections that disappear when empty (§18)
- [x] Missions linked to the agents working on them (§86)
- [x] Agents driving missions automatically under Unattended (§32)
- [x] Guardrails for sensitive operations (§35)
**Verified in the installed app:** created a mission, was refused completion,
ran verification, did the work, verified, completed — then deleted the artifact
and watched completion be revoked.

**Verified in the installed app:** set a policy in Settings, ran a mission whose
acceptance criterion was `npm publish`, watched the check be **held** rather than
run, answered the approval, and saw the mission leave Waiting. Separately, the
guard was run against real Claude Code 2.1.240: a force push was refused before
it executed, and under Unattended an `rm -rf` was stopped with the directory
still there afterwards.

### Unattended, as built
- The autopilot takes the human's seat: after every turn it verifies, then
  either sends the next instruction or stops with a reason.
- **The provider says when a turn ended; we never infer it.** Claude Code
  reports `stop_reason: "end_turn"` (`"tool_use"` means it is still working);
  Codex emits `event_msg/task_complete`. Checked against all 88 transcripts on
  this machine — 664 turn boundaries, none of them a `tool_use`.
- Three endings, never "keep going and hope" (§34): Completed with evidence,
  Waiting/Blocked for a person, or Failed on the turn budget or a stall.
- A driven session reports `driven: true` to guardrails, so a rule set to *ask*
  refuses instead of parking the agent on a prompt the autopilot cannot answer.

### Guardrails, as built
- Eight operation classes, matched by a tokenising classifier that respects
  quoting — `--force-with-lease` and `echo "rm -rf x"` deliberately do not match.
- Policy resolves project → global → default (Ask), the §33 shape.
- Enforcement differs by provider and the capability model says so (§26):
  Claude Code is stopped pre-execution; Codex has the same hook mechanism but
  will not run it until the user trusts it, so it is reported as such.
- Under Unattended, "ask" becomes a refusal rather than a prompt nobody can
  answer, and the mission goes to Waiting with a reason (§34).

### Evidence summaries, fully localised (§65)
Every sentence J.A.R.V.I.S. authors as evidence now carries a `code` and
`code_args` alongside its English `summary` fallback — the same shape the
guardrail refusal pioneered, extended to the other ten: a command's pass,
fail, timeout and spawn-failure; a file existing, missing, containing or not
containing text, or being unreadable; a manual criterion's "needs a person"
and "confirmed by {who}". Command/file **output** in `detail` (stdout,
stderr, an OS error string) is deliberately left with no code — it is the
tool speaking, not a sentence J.A.R.V.I.S. wrote, and translating it would be
inventing words nobody said.

A real bug caught before it shipped: `Outcome` grew `code`/`code_args` and
`evidence_from` threaded them through cleanly, but the raw `INSERT` in
`run_and_record` — a different place entirely — still named only the
original columns, so every code would have been computed correctly and then
silently dropped on the way into the database. The same gap existed
separately in `confirm_manual`'s own `INSERT`. Both are the same lesson as
item 17 in HANDOFF's list: a correct struct is not correct bytes on disk;
each is pinned by a test that reads the evidence back through the real
database, not just checks the in-memory value.

**Verified in the installed app:** ran a Command criterion built to fail and
watched the row read exactly `` `exit 1` saiu com código 1, esperado 0 `` in
pt-BR, with the command and both exit codes substituted and no `{placeholder}`
left visible; confirmed a Manual criterion and watched it read `Confirmado
por you`. Both exercise a different one of the two `INSERT` sites this pass
fixed.

## M6 — Code surfaces  ✅
- [x] Files explorer (§41)
- [x] Editor, Monaco (§42)
- [x] Diff / Review (§43)
- [x] Git write operations (§44) — stage, unstage, discard, restore, commit
- [x] Worktrees (§45)

Files, the editor, Review and Worktrees live **inside a project**, not on the
rail: they are project-scoped tools and the rail stays six destinations
(§85/§87). The project header carries an underlined
**Sessions · Files · Review · Worktrees** navigation, visually distinct from the
Terminal/Conversation pill so that two segmented controls never stack.

**Verified in the installed app:** browsed the tree with `node_modules` and
`build.log` dimmed as Git-ignored and `.git` absent; opened a file in Monaco,
edited it, saved with Ctrl+S and confirmed on disk that the content changed and
the LF line endings were **not** rewritten; ran a real Claude Code agent in a
scratch repository, watched it create a file, and saw Review put that file at
the top of the list attributed to **Claude Code**; renamed a file and saw the
diff render as one changed line rather than as a new file. Both themes
screenshotted.

### As built
- **Path confinement is the security boundary.** The webview only ever names
  paths relative to the project root, and the root itself is read from the
  database rather than accepted from the caller. Two independent checks: no
  `..`, root or drive components in the request, then the resolved path
  re-checked against the filesystem so a symlink pointing out of the project is
  caught. Tested against a real temporary directory, including Windows verbatim
  (`\\?\`) paths and case folding.
- **Git decides what changed, always** (D5). `status --porcelain=v1 -z` for the
  list, `diff -M HEAD` for the hunks. We parse; we never compute a diff
  ourselves and hope it agrees with the user's own `git`.
- **Attribution is a join, not an invention.** `file_changes` already records
  what each session touched, so Review answers "what did this agent change?"
  from the same append-only log everything else reads (D2).
- **Monaco is loaded on demand and ships without its language services** (D17).
  +1.04 MB installed, measured on the real installer rather than estimated.
- **A save will not silently overwrite an agent.** The editor remembers the
  file's modified time and the core refuses a write when the file on disk is no
  longer the one that was opened — nothing is written, the buffer is untouched,
  and the surface says what happened and what the two ways out are. Saving
  again overwrites deliberately. This matters here more than in an ordinary
  editor: the product exists so an agent can be working in one tab while a
  person reads in another, which makes the conflict the normal case.
- **Closing a tab with unsaved work asks first.** Closing disposes the model,
  so the undo history goes with it; a single click must not be able to destroy
  an edit the dirty dot was advertising.

### Deliberately not in M6
- **Language intelligence.** Monaco's own TypeScript/CSS/HTML/JSON workers are
  excluded because they would be confidently wrong without project context
  (D17). Real diagnostics arrive with the LSP client D4 left room for.

### Git and worktrees, as built (§44/§45)
- **Discard is the only guarded action**, as `git.discard-changes` (D20).
  Staging and unstaging move the index and are reversible; asking about them
  would teach the user to switch the guardrail off, and then the discard is
  unguarded too.
- **The gate names its operation rather than classifying a string it just
  built** (D19), and writes no `Pending` row — the person is at the screen, so
  the question is asked and answered there.
- **Every spelling was checked against real Git before it was written down**
  (D20). Plain `git restore` restores from the *index*, so a discard built on
  it silently keeps staged content.
- **A worktree is a project** (D18). Opening one opens it everywhere: Review
  inside a worktree compares against that worktree's branch, with no
  worktree-specific code anywhere in §41–§44.
- **Removing a dirty worktree asks twice, for two different things.** First Git
  refuses because there is uncommitted work — that is information, answered
  with a plain "remove it anyway". Only the forced removal is a guarded
  operation (`fs.recursive-delete`), and only then are the §35 choices offered.
  `--force` is never passed speculatively.

**Verified in the installed app:** staged and unstaged a file; discarded one
carrying both staged and working-tree changes and confirmed **on disk** that it
reached `HEAD` rather than stopping at the index; restored a deleted file;
committed three staged files; set **Never allow** and watched the next discard
be refused with the working tree untouched. Created a worktree for
`agent/login-form`, saw the slash become a dash rather than a nested directory,
opened it as a project and read its own diff in Review, then removed it — first
refused by Git for having work in it, then held by the guardrail, then done,
with its project row archived. Both themes, both languages.

### One behaviour change worth knowing, still open
**Ctrl+K is resolved globally, before any widget sees it.** Monaco treats it as
a chord prefix and called `stopPropagation`, so the command palette silently
stopped opening whenever the editor had focus — while the titlebar went on
advertising the shortcut. It is now handled in the capture phase. The side
effect is that Ctrl+K no longer reaches a shell as readline's
kill-to-end-of-line, which is the same trade VS Code and Cursor make.

It touches the terminal, which is the hero surface (§21), so it is Alan's call
and it is **reversible**: `App.tsx` is the only place that decides, and letting
the terminal keep the key means either dropping to the bubble phase (and losing
the palette while the editor has focus) or exempting the terminal's DOM subtree
from the capture handler. The second is the one worth building if he wants it
back.

## M7 — Knowledge  ✅
- [x] Activity log (§48) — recorded at the moments worth knowing, filterable
- [x] Analytics (§52) — tokens by provider/model/project/day, confidence-aware
- [x] Human leverage (§53) — measured from real interaction, not inferred
- [x] Project Brain (§36–§38) — knowledge, briefed to every agent that starts
- [x] Project history (§39) — the project's own story, not the global feed
- [x] Notes (§40) — working memory, never sent anywhere
- [x] Global Search (§51) — knowledge, notes, missions, activity and
      conversation content, across every project, from Ctrl+Shift+F
- [x] An agent writes to its own Brain (§36–§38, D27) — once, at the end of
      an Unattended run, asked one narrow question it can decline to answer
- [~] Global Search finds sessions recorded **before** it existed (D30) — a
      one-time backfill, a background task with a bookmark rather than a
      migration, idempotent across a crash halfway through. The walk itself is
      verified against this machine's real recorded sessions; what is **not**
      yet verified is the eight lines in `setup()` that start it, which have
      never run on an actual launch. Items 23 and 33 in HANDOFF §5 are both
      "the code was right and the wiring never ran", so this stays `[~]` until
      a real build logs `search backfill complete` with a nonzero row count.

### The memory layer, as built
- **Knowledge is briefed; a note is not** (D21). One question decides which a
  thing is, so there is no per-item switch to forget. `brief::compose` takes
  only knowledge — a note cannot arrive through it.
- **Stated and derived live on separate tabs** (D22), so §28's rule is
  structural rather than a caption. Stated knowledge carries who said it;
  derived facts are recomputed on every read and never stored.
- **The brief goes out of band** (D23): written beside the guardrail snapshot
  in our own log directory and passed with `--append-system-prompt-file`.
  Nothing is written into the user's repository — a context file in a working
  tree would show up in the Review surface this product also ships.
- **Codex is reported honestly** (§26). It has no equivalent flag on 0.147.0,
  so it is `OpeningMessage` and is not handed a file it would never read.

**Verified in the installed app:** wrote four things into a scratch project's
Brain, started Claude Code from the app, and asked what it knew without reading
files. It answered with all four, grouped under the brief's own headings, in a
folder it had never seen before — and said plainly it had no other context
without reading the repo. The brief file was 484 bytes in our own data
directory, matching the size the panel reported, and `git status` in the
project was empty. Promoted a note into knowledge and watched it leave the
notes list. Both themes, both languages.

### Two bugs this milestone found in earlier work
- **Every project area was showing stale data** (D24). Areas are mounted once
  and hidden with CSS, so an effect keyed on the project id never fires again.
  Review's comment said it re-read on every visit; it did not.
- **An unattended run in an untrusted folder would have hung forever.** Claude
  Code asks "is this a project you trust?" the first time it opens any folder.
  A worktree is a brand-new folder, so §45 had just made this reachable.
  `autopilot_start` now refuses rather than starting something that cannot
  proceed (§34).

### Global Search, as built (§51)
- **`session_events` had no writer since migration 1** (D25) — HANDOFF's own
  claim that it "exists exactly for this" was checked before building on it and
  found false. Extended additively rather than recreated, and wired into the
  transcript tailer that already mirrors usage and file changes.
- **A standalone FTS5 index**, not `content=session_events`: the table is
  `WITHOUT ROWID` with a composite key, which external-content mode cannot key
  against. Knowledge, notes, missions and activity stay on plain `LIKE` —
  small tables, the same choice `activity::list` already makes.
- **Every project, always** — the question is "where did I see that", and
  scoping to whichever project is open would silently hide the answer whenever
  it lives elsewhere.
- **A past session gets a read-only tab it never started** (D25). Global
  Search is the first thing that can open a session's conversation after the
  session ended and its tab was closed; the tab is never `startSession`-ed and
  never `closeSession`-ed, because nothing here was ever attached to it.
- **Usage and searchable text are independent, not alternatives** (D26). Found
  by running a real Claude Code turn and searching for its own reply: only the
  question came back. A reply's usage and its own text used to be routed by
  one `match`, so a message carrying both — the ordinary shape of a reply —
  took the usage arm and its text was never indexed.

**Verified in the installed app:** searched "guardrail" from Mission Control
and landed on a real mission and its activity, translated and timestamped;
searched a real Brain entry's own words and it opened straight into that
project's Memória tab; started a real Claude Code turn in a scratch project,
searched for its reply and found only the question — fixed the usage/text bug,
rebuilt, same probe again, and both sides of the exchange came back labelled
**Você** and **Agente**. Closed the session and searched a third time: Global
Search opened it back up as a read-only conversation tab, model name and token
counts intact, and closing that tab touched nothing on the backend. Both
themes, both languages.

### An agent writes to its own Brain, as built (§36–§38, D27)
- **Only at the end of an Unattended run, once, and only into one narrow
  question.** `Step::Complete` in `autopilot::driver` is the one place a
  driven run holds the seat with nobody else in it (D15) — a manually
  completed mission is never asked, because typing a question into a
  terminal a person is watching is a different conversation to interrupt.
- **The prompt asks for what would still matter *next time*, not a summary
  of the task**, forbids touching anything (a stray edit here could be why a
  later re-verify revokes the completion it just set), and offers a named
  escape hatch (`NOTHING TO RECORD`) expected to fire most of the time.
- **A real bug caught before it shipped:** the reflection's reply window
  originally started at the cursor the work turn's own `TurnEnded` left
  behind — the same tail-frame risk `SETTLE` exists for one turn earlier.
  Fixed by re-baselining to the log's current end immediately before the
  question is sent, pinned by a test with a deliberately planted straggler
  frame, then reconfirmed against a real agent.

**Verified in the installed app:** a scratch project's README stated one
non-obvious fact — the dev server listens on 4173 because something
unrelated already holds port 3000. Ran a trivial mission Unattended against
a real Claude Code agent: on completion the reflection question arrived as
the agent's next input, it answered `GOTCHA: The dev server runs on port
4173, not the conventional 3000, because port 3000 on this machine is
already held by an unrelated port scanner — anyone expecting localhost:3000
will find nothing there.`, and the Brain's Gotcha tab showed exactly that
sentence under **Um agente registrou isto** in amber, briefed-size counter
moved, Activity recorded in pt-BR right after the mission's own completion.

### Notes on the analytics design
Bars use one hue because each row is already named beside it: the bar carries
magnitude, the label carries identity. Colouring by rank would double-encode
length as hue. A single-category breakdown drops its bar entirely — a full-width
rectangle restating the number next to it is not a chart.

## M8 — Preview / Browser (§46/§47)
## M9 — Onboarding (§13), Settings (§64)  ~
- [x] Welcome screen shown once per install, gated on a `settings` row
- [x] Reuses the environment scan (§14) and `openFolder`, no bespoke picker
- [x] Window reveal waits on the onboarding check, so there is no flash of
      the wrong screen — and defaults to "already seen" if the check fails,
      so a broken check can never leave the window hidden
- [ ] Settings (§64) itself
**Verified:** fresh install simulated by deleting the local db; the welcome
screen showed, opening a folder from it landed inside that project's own
workspace, and relaunching afterward skipped straight to Mission Control.
## M10 — Installer (§12) + Updater (§62)  ✅
- [x] NSIS installer with product identity, OS-language auto-detection
- [x] Per-user install — no administrator prompt
- [x] Updater with minisign verification; signed artifacts produced
- [x] Single-instance enforcement — one owner for the database and logs
- [x] Silent install/upgrade path (what the updater uses)
- [x] Uninstall removes the program and **preserves user data**
**Verified on this machine:** installed, launched, upgraded, uninstalled and
reinstalled. Install footprint is 7.3 MB. Signing certificate is blocked (B1).
## M11 — Mobile PWA (§55–§58) + Cloud relay (§59)
## M12 — Voice (§54)  ~
- [x] Microphone input that transcribes into the terminal — dictation as an
      input method for a running session, not a separate voice-command
      surface (Alan's own requirement, 2026-08-23)
- [x] Fully local: whisper.cpp bundled and spawned, no cloud API, no key
- [x] Primed with the project's own vocabulary (its files, its branch) —
      verified to fix exactly the proper nouns a generic dictation tool gets
      wrong, on the same recorded sentence, twice (D29)
- [x] Typed into the prompt, never auto-submitted
- [x] Verified against a **real** microphone — Alan tested with a real
      headset; worked, and surfaced two real bugs (PSReadLine's audible
      bell, UTF-8 chunk-splitting breaking Claude Code's own TUI on
      accented text), both fixed and proven against real infrastructure
      (see HANDOFF §5 items 35–36)
- [x] Pleasant start/finish sound cues — soft two-note Web Audio chimes,
      not yet re-verified by ear against the latest rebuild
- [ ] **Real-time streaming transcription** (Alan's explicit follow-up
      request, 2026-08-23) — text should appear incrementally while
      speaking, VS Code/Cursor-style, with a considered visual treatment
      ("animação na transcrição"), not today's record-then-type-once flow.
      Architecture investigated (`whisper-server.exe` from the same
      whisper.cpp release, kept warm, polled on a rolling audio window)
      but not yet built — see HANDOFF §7 item 1.
**Verified on this machine, including a real microphone once.** The model
downloads and hash-verifies through the app's own UI; the whole
capture → transcribe → type pipeline was run for real end to end, first
with a temporary synthetic-audio bypass and then with a real headset; a
missing microphone is reported honestly rather than hanging. What remains
is streaming transcription itself, plus one more live pass confirming the
sound-cue and bug fixes together by ear.

---

## Current milestone
**M6 and M7 are both complete.** Files, the editor, Review, Git write
operations, worktrees, the memory layer, Global Search, and an agent writing
to its own Brain are all built and verified against real data and a real
agent. Onboarding (§13) is also built and verified; Settings (§64) is what
remains of M9. Voice dictation (§54, M12) has been tested with a real
microphone once — it works, and two real bugs it surfaced (an irritating
PSReadLine bell, and dictated accents garbling specifically inside Claude
Code's TUI) are fixed and proven. Open on M12: real-time streaming
transcription, a substantial follow-up feature request, plus a second live
pass confirming the new sound cues and both fixes together by ear.

## Next steps
1. Build real-time streaming transcription for voice dictation (§54) — text
   appearing incrementally while speaking, with a considered animated
   treatment, matching VS Code/Cursor's own dictation UX. See HANDOFF §7
   item 1 for the investigated architecture (`whisper-server.exe`, kept
   warm, polled on a rolling window) and what is not yet built.
2. Re-verify voice dictation's sound cues and both bug fixes by ear on a
   real microphone — small, but not yet done against the latest rebuild.
   See HANDOFF §7 item 2.
3. The rest of M2: split panes (§20), scrollback search, image paste (§22,
   with a hover preview of the pasted image)
4. ~~Global Search does not backfill~~ — **done** (D30). `search::backfill`
   walks every session log once, off the startup path, one session per
   transaction, resumable and idempotent. Verified against this machine's own
   recorded sessions: 55 rows out of 10 real logs, and a Portuguese word from
   a real Claude Code run came back as a Conversation hit attributed to the
   session it was said in.
5. A manually completed mission is never asked what it learned (D27) — the
   reflection only fires at the end of an Unattended run, deliberately.
