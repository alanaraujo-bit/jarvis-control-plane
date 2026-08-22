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
- [ ] Agents driving missions automatically under Unattended (§32)
- [ ] Guardrails for sensitive operations (§35)
**Verified in the installed app:** created a mission, was refused completion,
ran verification, did the work, verified, completed — then deleted the artifact
and watched completion be revoked.

### Known gaps in this milestone
- Evidence summaries are generated in Rust and are English-only. They should
  carry a structured code the UI localises (§65).

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
**M5 — Missions**, mostly landed. Next is connecting missions to the agents
that work on them.

## Next steps
1. Guardrails for sensitive operations (§35)
2. Files, Editor and Diff/Review (§41–§43)
3. Project Brain (§36) and Notes (§40)
4. Localised evidence summaries (§65)
