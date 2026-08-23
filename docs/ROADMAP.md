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
- [ ] Image paste as first-class attachment (§22)

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

### Known gaps in this milestone
- Evidence summaries generated in Rust are still English-only, **except** the
  guardrail refusal, which now carries a structured code the UI localises. The
  `code`/`code_args` columns and the rendering path exist; the remaining
  summaries need converting one at a time (§65).

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

## M7 — Knowledge  ~
- [x] Activity log (§48) — recorded at the moments worth knowing, filterable
- [x] Analytics (§52) — tokens by provider/model/project/day, confidence-aware
- [x] Human leverage (§53) — measured from real interaction, not inferred
- [ ] Project Brain (§36–§38)
- [ ] Notes (§40)
- [ ] Global Search (§51)

### Notes on the analytics design
Bars use one hue because each row is already named beside it: the bar carries
magnitude, the label carries identity. Colouring by rank would double-encode
length as hue. A single-category breakdown drops its bar entirely — a full-width
rectangle restating the number next to it is not a chart.

## M8 — Preview / Browser (§46/§47)
## M9 — Onboarding (§13), Settings (§64)
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
## M12 — Voice (§54)

---

## Current milestone
**M7 — Knowledge.** Activity, Analytics and human leverage are done; Project
Brain (§36–§38), Notes (§40) and Global Search (§51) are what remain.

**M6 is complete.** Files, the editor, Review, Git write operations and
worktrees are all built and verified in the installed app.

## Next steps
1. Project Brain (§36) and Notes (§40) — the memory layer
2. Onboarding (§13) — the environment scan already provides its data
3. Finish localising the remaining evidence summaries (§65)
4. The rest of M2: split panes (§20), scrollback search, image paste (§22)
