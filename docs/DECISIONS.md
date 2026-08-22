# Decision Log

## D1 — Tauri v2 over Electron
**Date:** 2026-08-22
**Why:** §11 makes performance part of design and §12 demands a real Windows
installer. Tauri ships a native-size binary, a first-party NSIS installer, a
signed updater, and a Rust core capable of real PTY/git/fs work. Electron would
cost ~10x binary size and put the systems layer in Node.
**Alternatives:** Electron (heavier), pure-native WinUI (loses web UI velocity
and the mobile PWA code sharing).
**Verified:** Rust + MSVC links on this machine; Tauri CLI 2.11.4; WebView2 present.

## D2 — A session is an append-only event log
**Date:** 2026-08-22
**Why:** §23 requires Terminal and Conversation to be one session. Modeling them
as two components with separate state makes that impossible without constant
sync. One ordered log with two projections makes it structural — and §39
(timeline), §48 (activity), §30 (evidence), §55–57 (mobile deltas) fall out of
the same log for free.
**Consequence:** raw PTY bytes go to on-disk frame logs, not SQLite rows.

## D3 — Read provider transcripts instead of scraping ANSI
**Date:** 2026-08-22
**Why:** Verified that Claude Code and Codex both write structured JSONL while
running interactively. Scraping terminal output would be fragile and would lose
token usage entirely. Reading transcripts gives official usage data (§28) and a
faithful Conversation View (§24).
**Verified against:** real transcripts on this machine (see ARCHITECTURE.md).

## D4 — Monaco for the editor, behind a boundary
**Date:** 2026-08-22
**Why:** §42 wants multiple cursors, breadcrumbs, go-to-definition, diagnostics —
mostly free in Monaco. Kept behind `packages/editor` so an LSP client can land
later without touching surfaces.
