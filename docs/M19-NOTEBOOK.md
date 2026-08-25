# M19 — The Notebook (§NEW)

Working record, so the work survives an interrupted session.

## What was asked for

Alan keeps his prompts in WhatsApp messages to himself. He asked for two things
that are one thing: a notepad he can reach without leaving the terminal — a
modal, openable while an agent is working — and a **folder system for prompts
and ideas** so the library lives inside the product.

## What makes it belong here rather than being Notion

A note in this product can be **handed to the agent you are watching**. That is
the whole difference, and everything else follows from it.

## What already exists, and why this is not it

`project_notes` (§40) is Notes, and it stays. It is working memory **about one
project**: it lives in that project's Brain tab, it can be promoted into
knowledge that gets briefed to an agent, and it is deliberately temporary —
`brain::delete_note`'s own docstring says a note "is a scratchpad entry whose
whole purpose is to be temporary".

The Notebook is the opposite on every axis: it is the person's own library, it
belongs to no project (a prompt scoped to one project is useless — you want it
everywhere), it is never briefed, and it is hoarded for months rather than
discarded. Making `project_notes.project_id` nullable to serve both would have
forced every existing reader — including `promote_note`, which needs to know
*which* project's knowledge to write into — to handle a note belonging to no
project. Two names for two things beats one name for two behaviours.

## The measurement that decided the feature

`session::typing::type_text` — the path the autopilot and voice dictation use —
**flattens every newline to a space**, and its docstring says why: a `\n`
reaching an agent CLI's line editor submits the fragment before it. Prompts are
multi-paragraph. Sent through that path, Alan's library would arrive as run-on
lines and would *look* like it had worked.

So it was measured rather than assumed, in the real app against a real Claude
Code 2.1.245 session: a three-line string pasted into the prompt **arrived with
its line structure intact and unsent** (`shots/nb-07-paste-again.png`). Claude
Code's TUI honours bracketed paste (DECSET 2004); without it, the first `\r`
would have submitted line one on its own.

**So delivery does not go through Rust at all.** `xterm`'s own `Terminal.paste()`
already reads `decPrivateModes.bracketedPasteMode` for *that* terminal and
wraps or does not wrap accordingly, then raises `onData`, which `TerminalView`
already writes to the PTY. It is the same path a real Ctrl+V takes, which is the
most-tested path there is. A Rust-side implementation would have had to guess at
a mode the frontend already knows.

## Shape

- **Store:** migration 16. `notebooks` (flat, one level) and `notebook_notes`
  (`notebook_id` nullable = unfiled).
- **Deleting a notebook never deletes notes** — `ON DELETE SET NULL` drops them
  into Unfiled, and the confirm says so. A library someone has hoarded for
  months must not lose forty prompts to one click.
- **No `kind` column.** A prompt is a note you send. A per-item switch is a
  switch to forget (D21's lesson).
- **One level of folders**, chosen not defaulted: nesting buys recursive
  rendering, cycle prevention on move and a cascade decision, for a structure
  most people never build. Same call `MAX_SLOTS` made for split panes.
- **Surface:** an overlay, Ctrl+Shift+N (capture phase — Monaco and the terminal
  claim keys first, which is why Ctrl+K and Ctrl+Shift+F are already registered
  that way) plus a titlebar button passed in as a prop, never imported.
- **Autosave**, debounced, flushed on close and on switching notes.

## What the build found

Four things that were not visible from the code.

1. **Sending into a shell would have executed the prompt.** `paste()` brackets
   only when that terminal set DECSET 2004; a shell does not, so a twenty-line
   prompt is twenty commands, run. The gate became `term.modes.bracketedPasteMode`
   rather than the kind of session — which also covers an agent CLI that has
   started but not yet drawn its prompt. Verified on screen: the same note
   refused with a sentence, and the shell prompt untouched.
2. **The overlay left focus behind its own scrim.** `pasteIntoSession` focuses
   the terminal so the words land where they belong; leaving the overlay open
   meant every keystroke after that went invisibly into the PTY. Send now
   flushes and closes — which is also what somebody would have done next.
3. **The folder actions were unreachable by keyboard.** `display: none` cannot
   take focus, so Tab skipped them *and* the `:focus-within` rule meant to
   reveal them could never fire. Now `opacity`, which also stops the row
   twitching under the pointer.
4. **The note list could reshuffle itself.** Caught by a flaky test: with
   `updated_at` and `created_at` tied — which duplicating a note guarantees —
   the order was undefined. `id` (UUIDv7) is now the final tiebreak.

And one the product caught on its own: migration 16 needed its fingerprint in
`SHIPPED`, exactly as `a_shipped_migration_is_never_edited` exists to demand.

## Verified in the real build

- Ctrl+Shift+N opens it **while the terminal holds focus** — the capture-phase
  registration doing its job.
- A folder created, a note created inside it, a five-line prompt typed and
  autosaved, surviving the overlay closing and reopening (a real SQLite round
  trip) and a theme change.
- **Sent into a live Claude Code 2.1.245**, which showed
  `[Pasted text #1 +5 lines]` — its own collapsed-paste affordance — and left
  it unsent in the prompt. `shots/nb-16-sent.png`.
- Refused into a shell with a sentence, nothing written. `shots/nb-20-shell-refused.png`.
- Dark and light, pt-BR and English, 1442px and 786px (folder column drops,
  search still reaches everything).
- Escape while naming a folder cancels the folder, **not** the notebook — the
  capture-phase guard. Three open/close cycles create nothing.
- Delete a note: confirmed, and the count moved 2 → 1.
- `cargo test` 553 passing, run three times for the flake; `pnpm typecheck` and
  the JS suites clean.

## Known limitation, chosen rather than missed

**Global Search (Ctrl+Shift+F) does not find notebook notes.** It finds
`project_notes` — there is a `SearchKind::Note`, a `note_matches` query and a
StickyNote icon — so somebody with two hundred prompts in here who reaches for
the product's own global search finds nothing and concludes search is broken.

It was deferred on purpose rather than forgotten, and the cost is why: it needs
a new `SearchKind` parsed and serialised in `search/mod.rs` plus entries in
`GROUP_ICON`, `GROUP_ORDER`, `subtitle()` and both catalogues — **and** a
navigation special case, because `onSelect` in `App.tsx` opens a project
workspace and a notebook note belongs to no project. That last part is what
stops it being free, and it is a protocol change that should not ride along
with a new surface. In-notebook search covers the day-to-day; this covers the
moment somebody forgets where they put something.

## State

- [x] The typing measurement, in the real app
- [x] Migration 16 + `notebook` module + 7 tests
- [x] i18n, both catalogues
- [x] Overlay surface
- [x] Wiring: shortcut, titlebar, palette entry, terminal registry
- [x] Seen working in the real build, both themes, both widths
- [x] D47, ROADMAP, HANDOFF
