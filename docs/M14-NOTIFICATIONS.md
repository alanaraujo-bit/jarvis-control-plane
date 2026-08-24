# M14 — Notifications (§49)

> **Live working document.** Written as the work happens so an interrupted
> session can be picked up from here. The finished account moves into
> `ROADMAP.md` and `DECISIONS.md` when the milestone closes.

## What is being built

A notification when **an agent stops working** — because it finished, or
because it is waiting on a person — carrying a short, honest preview of what it
wants.

## Status

| Step | State |
|---|---|
| 0. Spike: does a Windows toast fire from this app at all? | plugin wired, `cargo check` green; not yet fired |
| 1. Signals: where "the agent stopped" actually comes from | decided (below) |
| 2. The detector for the provider's own permission prompt | evidence captured |
| 3. Storage + read state | not started |
| 4. Dispatch and the suppression rule | not started |
| 5. The Notification Center surface | not started |
| 6. Settings | not started |
| 7. Tests | not started |
| 8. Verified in a real build against a real agent | not started |

---

## 1. Where "the agent stopped" actually comes from

Three sources, and they are **not** of equal standing. The confidence model
§28 already uses for usage applies here unchanged.

| Source | Signal | Confidence |
|---|---|---|
| Provider transcript | `ConversationItem::TurnEnded` — Claude Code's `stop_reason: end_turn`, Codex's `task_complete` | **Official** |
| Guardrail | a held operation (`Status::Pending`) or one handed to the provider's own prompt (`Status::Asked`) | **Official** |
| Autopilot | a run that stopped for one of the §34 reasons | **Official** |
| Session lifecycle | `Lifecycle::Exited` | **Official** |
| Terminal output | the provider drew a question and is waiting for an answer | **Observed** |

The last one is the one that matters most in practice and the only one that
had to be built from scratch. `guardrail::guard` returns early for any tool
that is not `Bash`, and again for a `Bash` command that classifies as nothing
sensitive — so **the ordinary case**, "Claude Code is asking whether it may
write a file", is invisible to every existing mechanism. Without it the
feature would miss the moment it exists for.

## 2. What an agent CLI actually draws — captured, not guessed

`notify/capture.rs` spawns a real agent CLI in a real PTY and records every
byte. Four captures, against Claude Code 2.1.241 and Codex 0.149.1 on this
machine.

Two things the harness itself had to learn first:

- **The first run captured four bytes.** `\x1b[6n` and nothing else: Claude
  Code will not draw a character until something answers ConPTY's
  cursor-position query. Same trap as D6, from the other side.
- **The second run captured an agent in auto mode that never asked anything.**
  It had inherited `CLAUDE_CODE_CHILD_SESSION` and friends from the agent
  session running the test. Those are stripped now, and the permission mode is
  passed explicitly.

### What the captures show

**Claude Code — writing a file**

```
Create file
 hello.txt
────────────────────────────────────────────
Do you want to create hello.txt?
❯ 1. Yes
  2. No
 Esc to cancel · Tab to amend
```

**Claude Code — running a command**

```
Bash command
   git --version
   Check git version
This command requires approval
Do you want to proceed?
❯ 1. Yes
  2. Yes, and don't ask again for: git *
  3. No
 Esc to cancel · Tab to amend · ctrl+e to explain
```

**Claude Code — first-run theme picker** (a question with *no* footer at all)

```
Choose the text style that looks best with your terminal
  1. Auto (match terminal)
❯ 2. Dark mode ✔
  3. Light mode
```

**Codex — folder trust**

```
  Do you trust the contents of this directory? …
› 1. Yes, continue
  2. No, quit
  Press enter to continue
```

### The invariant that survives all four

Not the wording, not the footer — the theme picker has no footer, and
"Do you want to proceed?" carries no information about *what*.

> **A numbered choice list, of which exactly one row carries a cursor glyph.**

`❯` for Claude Code, `›` for Codex. That, and nothing else, is common to every
capture. The wording is used for the *preview*; it is never what decides that a
prompt is on screen.

### One thing that would have broken a naive reader

These TUIs emit **`CSI n C` (cursor-forward) where a person would expect
spaces**. A conventional `strip_ansi` — the one `preview/detect.rs` already
has — turns `Do you want to create hello.txt?` into
`Doyouwanttocreatehello.txt?`. The detector therefore renders the tail rather
than stripping it: cursor-forward becomes spaces, absolute cursor moves become
line breaks.

### A useful second-order signal

Claude Code sets the terminal title (OSC 0) to a spinner glyph plus a label for
what it is doing: `◐ Claude Code`, then `◑ hello.txt file creation`, then
`✳ hello.txt file creation`. Good material for a preview when the question
itself is generic. Recorded here; not load-bearing.

## Log

- Orientation: read the session core, providers, guardrail, autopilot,
  activity, settings, the design tokens and the i18n catalogue.
- `tauri-plugin-notification` added (Rust + JS + capability). `cargo check` green.
- `notify/capture.rs` written; four captures taken against real CLIs.
