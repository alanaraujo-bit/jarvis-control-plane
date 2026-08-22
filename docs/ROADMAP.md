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

## M5 — Missions (§29–§35)
- [ ] Mission model, states, tasks, acceptance criteria
- [ ] Evidence + Verification (§30)
- [ ] Autonomy profiles + inheritance (§32/§33)
- [ ] Guardrails (§35)
- [ ] Mission Control home (§18)

## M6 — Code surfaces
- [ ] Files explorer (§41)
- [ ] Editor, Monaco (§42)
- [ ] Diff / Review (§43)
- [ ] Git + worktrees (§44/§45)

## M7 — Knowledge
- [ ] Project Brain (§36–§38)
- [ ] Notes (§40)
- [ ] Activity (§48), Global Search (§51), Analytics (§52/§53)

## M8 — Preview / Browser (§46/§47)
## M9 — Onboarding (§13), Settings (§64)
## M10 — Installer (§12) + Updater (§62)
## M11 — Mobile PWA (§55–§58) + Cloud relay (§59)
## M12 — Voice (§54)

---

## Current milestone
**M10 — Installer and updater**, so the product is genuinely installable (§92).

## Next steps
1. NSIS installer with product identity; verify a real install/uninstall
2. Updater plumbing + minisign keypair (signing cert is blocked, see BLOCKERS)
3. Missions (§29) — the central concept, on top of the session core
