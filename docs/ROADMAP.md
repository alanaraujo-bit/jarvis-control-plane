# J.A.R.V.I.S. — Living Roadmap

Status legend: `[ ]` not started · `[~]` in progress · `[x]` done & verified

Rule (§69/§81): milestones ship **finished**, not scaffolded. A milestone is
done only when it passes the §79 Definition of Done, including real visual
inspection (§76).

---

## M0 — Foundation
- [~] Monorepo (pnpm workspaces), Tauri v2 shell, real window on screen
- [ ] Design token system (dark + light, both first-class §8)
- [ ] App chrome: custom titlebar, window controls, DPI, reduce-motion
- [ ] i18n foundation wired from the first string (§65)
**Done when:** app launches, both themes render, screenshot captured & reviewed.

## M1 — Session Core (the load-bearing milestone §23)
- [ ] Append-only session event log (frame codec + index)
- [ ] SQLite schema + migrations
- [ ] Session projections: terminal replay + conversation projection
**Done when:** a session log survives restart and both projections agree.

## M2 — Terminal (hero surface §21)
- [ ] PTY host (portable-pty), resize, lifecycle
- [ ] xterm.js integration, scrollback, search, tabs, panes, layouts
- [ ] Session restore, agent state badges (Working/Waiting/Idle/…)
- [ ] Image paste as first-class attachment (§22)

## M3 — Providers (§26)
- [ ] Provider adapter trait + capability model
- [ ] Claude Code adapter (transcript tail + usage)
- [ ] Codex adapter (rollout tail + rate limits)
- [ ] Environment scan (§14)

## M4 — Conversation View (§24)
- [ ] Structured projection UI, same session as terminal
- [ ] Terminal ↔ Conversation toggle preserving context

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
**M0 — Foundation.**

## Next steps
1. pnpm workspace + Tauri scaffold, window on screen
2. Design tokens, both themes
3. Screenshot + visual review pass
