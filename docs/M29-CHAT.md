# M29 — Chat mode

**State: built and tested against the real CLIs (2026-10-18). Seeing it run
in the app is still pending — see Handoff below.** Local commits only; nothing
pushed.

Design (pt-BR, the source for every decision here):
[`docs/superpowers/specs/2026-10-18-chat-mode-design.md`](superpowers/specs/2026-10-18-chat-mode-design.md).
Plan: [`docs/superpowers/plans/2026-10-18-chat-mode.md`](superpowers/plans/2026-10-18-chat-mode.md).
Decision: D54.

## Why

Alan asked for a second way to work with the agents, for people who should
never have to read a terminal: "algo mais intuitivo… igual do Claude Code,
igual do ChatGPT… Para pessoas leigas é mais fácil". The terminal stays the
default. He left the design to me, "sempre pensando na experiência do usuário
final".

## What it is

The same agent (Claude Code or Codex) in the same project, with the same
account, configuration, guardrail and session log — worked with through a chat:

- the person's messages as bubbles; the agent's replies as Markdown that
  streams in; the work in between folded into one line per turn, which opens
  into plain-language steps ("Read `src/app.ts`", "Ran `pnpm test`");
- a permission or a question is a card with buttons (Allow / Always in this
  chat / Deny; options to pick), in the waiting colour;
- the model and effort are picked inside the text box, from the CLI's own list;
- Stop interrupts the turn; dictation and the Notebook drop text into the box;
- the person chooses Chat or Terminal in the + menu, and Settings → Appearance
  decides which comes first.

## What was measured before building (2026-10-18)

Claude Code 2.1.302 and Codex 0.163.0, in scratch repositories.

**Claude Code, `claude -p --input-format stream-json --output-format stream-json`:**
one process for the whole conversation; the transcript is written exactly as
for an interactive session, so the existing tailer needs no change.
`--permission-prompt-tool stdio` is required: without it every permission is
denied on the spot and `AskUserQuestion` does not exist. `set_model` and
`apply_flag_settings {effortLevel}` change the next turn, and `get_settings`
reports what is applied (`applied.model`, `applied.effort`). The `initialize`
answer lists the models with their supported efforts. A message sent mid-turn
is folded into that turn — two messages, one `result` — so busy follows the
CLI's own `queued_turn_count`. Turns draw from the same five-hour and weekly
windows as the terminal (`rate_limit_event`, `isUsingOverage: false`).
`-p` skips the "do you trust this folder?" question.

**Codex, `codex app-server`:** `thread/start {cwd}` answers with the thread id,
the rollout path, the model and the effort; the rollout is the envelope
`codex::parse_line` already reads. Model and effort ride `turn/start` and hold
for later turns. A message mid-turn goes in with `turn/steer`. Approval
requests list the decisions they accept — a command offered `accept`, an
execpolicy amendment and `cancel`, with neither `acceptForSession` nor
`decline` — so the card offers exactly those. In Default mode
`request_user_input` is unavailable and Codex asks inside the message instead
(`agentMessage.questions`), which the chat turns into quick replies.

Three parser bugs surfaced and are fixed for the terminal too: a `/model` echo
rendered as "You" bubbles; 0.163.0's command argument lists left every Codex
Shell row empty; and a message sent while Claude works is recorded only as a
`queued_command` attachment — 42 of them on this machine, every one missing
from Conversation View and search until now.

Folder trust was read from the wrong file without an account
(`~/.claude/.claude.json`, no projects; Claude reads `~/.claude.json`), and a
config with no `projects` counted as unknown. Both let a chat start unasked;
both are fixed and tested.

## How it is built

- `sessions.mode` (migration 23) and `ProviderCapabilities::chat`
  (`StreamJson`, `AppServer`, `None` — the local runner was never measured).
- `LiveSession` holds a PTY or a protocol pipe (`chat::process`, contained in
  a job object). `write()` refuses a chat, which is what keeps dictation, the
  Notebook, the autopilot and the rotation relay from typing into a protocol.
- `chat::claude` and `chat::codex` are pure drivers: protocol lines in,
  effects out (`Emit`, `Write`, `Log`, `State`), tested line by line against
  what the real CLIs sent.
- `chat::ChatRuntime` applies effects in order under one lock, writes stdin on
  a thread of its own, streams events to the attached screen, and raises an
  **Official** "waiting for you" notification for a request nobody is looking
  at (the terminal can only offer Observed, D35).
- The conversation on screen is the session log (`session_conversation`),
  exactly as for Conversation View. Requests, answers, model/effort changes and
  interrupts are logged as `Approval` and `Lifecycle` frames. The raw protocol
  stream is not logged.
- A chat only starts in a folder the provider already trusts; trust is read,
  never written (D51). The screen explains and opens that agent's terminal,
  where the person answers the provider's own question.
- "Always in this chat" is session-scoped: Claude's suggestions are rewritten
  to `destination: "session"` (it suggests `localSettings`, which would write
  into the repository); Codex gets `acceptForSession` only when offered.
- The UI renders agent Markdown from data — never `innerHTML` — and only
  `http(s)` links, opened in the browser.

## Verified

- `cargo test --lib`: 789 passed (24 ignored). An independent review of the
  branch found eleven issues; all were fixed with tests, among them a closed
  Codex chat leaving its process tree alive (the test fails without the fix),
  a card showing only part of a compound command, and quadratic Markdown
  parsing.
- `real_claude_chat_turn` and `real_codex_chat_turn` (ignored; run with
  `cargo test --lib chat::real_tests -- --ignored --test-threads=1`): a file
  created after an allowed request; two back-to-back messages ending busy
  once; a model switch confirmed by the CLI and leaving `~/.claude/settings.json`
  byte-identical; the transcript on disk where the tailer looks; a Codex steer
  into a running turn; a model switch carried by the next `turn/start`.
- `pnpm --filter @jarvis/desktop test`: 28 passed (Markdown, activity, thread).
- `pnpm typecheck`, i18n tests, lint (no new warnings in the chat files).

## Not built, deliberately

Switching a session between chat and terminal; rotation (M26) moving a chat
(the engine skips chats, and `replace_process_with` refuses one); chat for the
local runner; split panes with a chat; image attachments; continuing a Codex
chat from History (Codex resumes in place, D41); remembering a model choice
across chats; the Codex brief through `developerInstructions` (not measured).

## Handoff

**Objective:** see the chat running in the real app and fix what looks wrong.

**Why it stopped:** the QA build was up (`pnpm tauri dev --config
'{"identifier":"dev.jarvis.desktop.m29qa"}'`, data in
`%APPDATA%\dev.jarvis.desktop.m29qa`), but Alan was presenting his screen in a
video call whose floating window sat over the app. Synthetic clicks could have
landed on the call's controls, so nothing was clicked; the window was
minimised.

**Next action:** with the machine free, run the spec §9 app list in a scratch
project on a long path (one exists:
`…\scratchpad\chat-qa`, a session-scoped folder — make a new one if gone):
trust card on a fresh folder → trust in the terminal → open Claude and Codex
chats; streaming; Allow / Always / Deny; a question; model and effort changes
visible in the next turn; Stop; dictation and the Notebook into the box; the
notification with the window unfocused; History and search find the chat;
Continue a finished Claude chat; a guardrail *ask* reaching the chat as a card.
Dark and light, en and pt-BR, pixels sampled for the working dot and the
waiting card.
