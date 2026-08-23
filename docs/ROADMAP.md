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

## M6 — Code surfaces
- [ ] Files explorer (§41)
- [ ] Editor, Monaco (§42)
- [ ] Diff / Review (§43)
- [ ] Git + worktrees (§44/§45)

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
**M5 — Missions: complete.** Guardrails, then agents driving missions
unattended — in that order, because Unattended without guardrails means an
agent doing irreversible things with nobody able to object.

Next is **M6 — Code surfaces**: Files, Editor, Diff/Review.

## Next steps
1. Files, Editor and Diff/Review (§41–§43) — the largest remaining surface
2. Project Brain (§36) and Notes (§40)
3. Finish localising the remaining evidence summaries (§65)
