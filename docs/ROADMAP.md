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

## M2 — Terminal (hero surface §21)  ✅
- [x] PTY host (portable-pty), resize, lifecycle
- [x] Windows job objects — no orphaned agent processes
- [x] Single-writer session runtime; unattended sessions run with no view (§32)
- [x] Raw-byte IPC channel, coalesced at 16ms
- [x] xterm.js integration, scrollback, per-theme palettes, tabs
- [x] Split panes and layout presets (§20) — up to four terminals at once,
      side by side / stacked / grid. Presets rather than a drag-resizable
      tree, and four rather than unlimited: a fifth pane at this window size
      is narrower than the prompt most agent CLIs draw. Splitting changes
      each pane's **CSS box only** — no terminal is ever re-parented, because
      a remounted terminal has lost its scrollback.
      **Verified in a real build:** two panes with both scrollbacks intact, a
      command typed into each reaching its own shell, all three layouts, a
      third terminal opened mid-split, and closing panes and sessions.
- [x] Search within scrollback (§20) — Ctrl+F, a panel over the terminal
      rather than a dialog over the app, match-case, live counter, and an
      overview ruler shown only while searching so there is no empty gutter
      the rest of the time. Matches are blue, never amber (§6).
      **Verified in a real build**, both themes and both languages, and it
      took four fixes that the suite could not see — the worst being Ctrl+F
      falling through to **WebView2's own find-in-page** whenever the terminal
      did not literally hold DOM focus. See HANDOFF §5 item 39.
- [x] Image paste as first-class attachment (§22) — Ctrl+V in a terminal
      writes the clipboard image into the session's own directory (never the
      user's repository, D23's reasoning) and types the path at the prompt,
      unsubmitted. A chip says an image is attached; hovering shows the
      picture — Alan's own requirement, not a bare filename.
      **The webview cannot see a pasted image at all**: WebView2 raises no
      `paste` event for clipboard image data, only for text, so the core
      reads the clipboard with `arboard`. Found by instrumenting the running
      app after two DOM-shaped attempts produced nothing — see HANDOFF §5
      item 43.
      **Verified end to end in a real build** with a real image on the real
      clipboard: a genuine PNG on disk, the quoted path typed and left
      unsubmitted, the preview showing the actual picture, and `git status`
      in the project still clean.

**M2 is finished.** Every part of the hero surface is built and has been
driven in a real build rather than only compiled.

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
- [x] Global Search finds sessions recorded **before** it existed (D30) — a
      one-time backfill, a background task with a bookmark rather than a
      migration, idempotent across a crash halfway through.
      **Verified twice over:** the walk against this machine's real recorded
      sessions (55 rows out of 10 real logs, and a Portuguese word from a real
      Claude Code run came back attributed to the session it was said in), and
      the *wiring* against a real launch — `search backfill finished` appears
      in the log exactly 5.000s after startup, which is `STARTUP_GRACE`. The
      wiring was checked separately and deliberately: items 23 and 33 in
      HANDOFF §5 are both "the code was right and the wiring never ran". The
      log line is now unconditional for that reason — a background task that
      is silent in its steady state cannot be told apart from one that never
      started.

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

## M8 — Preview / Browser (§46/§47)  ✅
- [x] The dev server's URL is read from the **session's own output** — nothing
      to configure, no port to guess. When an agent runs `npm run dev`, the
      address it printed is already in the log this product keeps (§23).
- [x] Loopback only, enforced in the command and not just the scanner
- [x] Opens in a separate window, not an iframe (see below)
- [x] Reload, for a dev server without hot reload — the "did that actually
      change anything?" button
- [x] Offers a choice when a session serves more than one thing
- [x] Nothing opens by itself; detection is automatic, opening is a click

M8's entry here was a bare heading with no spec, so the first decision was
what Preview *is*. It is **not a browser in a tab**. The loop is
ask → modify → run → **see** → inspect → fix, and every step but *see*
already existed: an agent edits files (§41/§42), runs a server in a real
terminal (§21), and the diff is in Review (§43). The missing step meant
leaving the app and losing the one thing this product knows that a browser
cannot — **which session started this server**.

### Two decisions worth keeping
- **A separate window, not an iframe.** The iframe was the obvious first
  choice and does not work: the app's CSP is `default-src 'self'`. Widening
  it would put a dev server's page in a context adjacent to the surface that
  can invoke every Tauri command here — a real escalation for a layout
  convenience. A separate webview has its own empty capability set.
- **Loopback is a security boundary, not tidiness.** Preview renders inside
  our own window, so pointing it at an arbitrary host because a string
  appeared in terminal output would let any program a session runs choose
  what this product displays. `preview_open` re-checks with `url::Url` and
  the *same* `is_loopback_host` the scanner uses — tested against
  `127.0.0.1.evil.com` and `localhost.evil.com`, which a prefix check
  waves through.

**Verified end to end in a real build**, against a real dev server: `npx
serve` printed Local and Network addresses, Preview offered `localhost:4173`
and silently excluded the LAN address, Open rendered the real page in its own
window, the file was then edited on disk and **Reload showed the change** —
the whole §46 loop, closed. Both themes, both languages, plus the empty
state and the follow-the-active-session behaviour.

## M9 — Onboarding (§13), Settings (§64)  ✅
- [x] Welcome screen shown once per install, gated on a `settings` row
- [x] Reuses the environment scan (§14) and `openFolder`, no bespoke picker
- [x] Window reveal waits on the onboarding check, so there is no flash of
      the wrong screen — and defaults to "already seen" if the check fails,
      so a broken check can never leave the window hidden
- [x] Settings (§64) itself — Appearance, Terminal, Agents, Environment,
      Updates, ordered by how often they are reached rather than
      alphabetically. Autonomy sits above Guardrails on purpose: how much an
      agent does on its own is the broader question, and what it may never do
      is the fence around that answer.
- [x] **A project has settings of its own** — one more area in the project
      workspace, hosting the project-scoped autonomy and guardrail controls.
      `GuardrailPanel` had accepted a `projectId` since §35 and had never once
      been given one, because global Settings renders only when no project is
      open and structurally cannot host a project-scoped control.
- [x] **The audit §64 asks for**, and what it found: three values the product
      used, showed on screen, and gave nobody a way to change — the two
      unreachable levels of the autonomy chain (§33), and the autopilot turn
      budget, which `AutopilotPanel` renders as "turn 3 of 24" while
      `DEFAULT_TURN_BUDGET` was a constant. Terminal type size and scrollback
      were hard-coded too. All now configurable, none of them new features.
- [x] One typed accessor for the `settings` table, replacing per-area SQL:
      **unset is no row**, and a value that will not parse reads as unset
      rather than failing the screen.

### Settings, as built
- **Bounds are enforced in the core, not just drawn in the UI.** The surface
  renders a slider, so an out-of-range value should be impossible — which is
  why it is not trusted: a command reachable from the webview is reachable
  from any bug in it. Out of range is *refused* on the way in, so a stored
  value is always one somebody could have chosen; out of range on the way
  *out* is *clamped*, so an older build's row cannot produce an unusable
  terminal or a zero budget that fails every run before it starts.
- **The bounds are product decisions.** Below 4 turns an unattended run
  cannot finish anything real and every mission would end in Failed; above
  100, "run until done" stops being a budget at all, which is what §34's rule
  against consuming resources indefinitely exists to prevent.
- **A run keeps the budget it started with.** Re-reading each turn would let
  a change in Settings move the finish line under a run already going.
- **Preferences apply in place.** Changing the terminal's type size or
  scrollback never rebuilds it — a rebuild kills the process and throws away
  the scrollback. Changing the size also refits and tells the PTY, or the
  shell keeps wrapping at the old width.

**Verified in a real build:** changed the type size and watched a *running*
shell grow, keep its scrollback, and run the next command; restarted the app
and the preference was still there; Reset returned it to 13 and the label
went back to "Default". Both themes, both languages — including `20.000
linhas` with the Brazilian separator. Control alignment was checked by
sampling pixels rather than by eye: all five settings controls share one
left edge at x=581.
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
## M11 — Mobile PWA (§55–§58) + Cloud relay (§59)  ~
- [x] The relay — three endpoints on Vercel, a **blind mailbox** rather than
      a server that knows anything. The desktop pushes a summary and collects
      queued commands; the phone reads and queues. Turn it off and the desktop
      is untouched, which is the test of whether §3 survived the cloud.
- [x] Pairing without an account — a six-character code, single-use, five
      minutes, attempts counted. The relay stores only a **hash** of each
      token.
- [x] The desktop half — one background thread, off unless chosen, every
      failure logged and swallowed.
- [x] The PWA — freshness first, approvals, what is running. No framework and
      no build step.
- [x] The pairing surface in Settings, saying what is sent **before** you
      connect.
- [x] Disconnect actually revokes — found by testing, see below.
- [ ] **Not verified in a browser.** The Chrome extension is not connected in
      this session, so the PWA has never been looked at on a phone-sized
      viewport. Its logic is tested and its transport is proven end to end,
      but how it *looks* is unverified. This is why M11 is `~` and not `✅`.
- [ ] Starting a mission from the phone. Declined rather than ignored: it
      needs the same Unattended checks `autopilot_start` makes, including the
      untrusted-folder refusal (HANDOFF §5 item 25).
- [ ] Push notification. Needs a developer account and is its own scope.

### The decisions worth keeping
- **Vercel, not Railway, and for cost rather than taste.** Railway already
  bills monthly on this account (ten projects, next invoice USD 18.21);
  Vercel has no contract and no recorded costs. The directive asks for no
  recurring cost where an adequate free path exists — for a relay there is.
- **A summary, never a mirror.** What needs a person, and nothing else. No
  file contents, no terminal output, no conversation text. A relay holding a
  copy of the work would be the second store §23 exists to prevent.
- **Only `AllowOnce` is reachable from a phone.** §35 offers four answers and
  three of them write a lasting policy — including `NeverAllow`. A phone has
  a small screen and a thumb, so it gets the one answer that expires with the
  thing it answers. A refusal from a phone is simply not approving; the
  guardrail already refuses by default.
- **Everything expires, enforced on read.** A serverless relay has no process
  to sweep in, and an expired value must never be served even if nothing has
  swept it. Commands expire far sooner than snapshots.

### Four things found by running it, not by reading it
1. **Disconnect did not disconnect.** Clearing the local pairing left the
   mailbox standing, and a paired phone kept reading. A switch that leaves a
   live token working is worse than no switch.
2. **The `.ts` import suffix does not survive the Vercel build** — 500 in
   production from code that typechecked.
3. **`workspace:*` cannot resolve in a subdirectory deploy.** Worked locally,
   failed in the build.
4. **A default export gets `(req, res)`, not a `Request`.** Named `GET` /
   `POST` exports are what select the web-standard signature.

And one found by reading: `pair.ts` declared an attempts counter, checked it,
and never incremented it — a limit that could not be reached, and one of the
three things making a six-character code safe.

**Verified end to end against real infrastructure**, including the step that
matters most: after pairing from the real desktop UI, the background thread
published on its own and the mailbox came back carrying `deviceName: PHANT0MX` — this machine's
real hostname, with nobody driving it.

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
- [x] **Real-time streaming transcription** (Alan's explicit follow-up
      request, 2026-08-23) — live captions while speaking, VS Code/Cursor-
      style, with an animated treatment (each word's own entrance, a
      breathing volatile tail), not the record-then-type-once flow this
      started as. A warm `whisper-server.exe` polled every second or so,
      folded through a LocalAgreement-style commit/tail split so the
      caption only ever grows and never rewrites a settled word. What
      actually gets typed is untouched — still one complete, unstreamed
      pass on stop. See D31.
**Verified on this machine, including a real microphone once and a full
synthetic-audio pass through streaming.** The model downloads and
hash-verifies through the app's own UI; the whole capture → transcribe →
type pipeline was run for real end to end, first with a temporary
synthetic-audio bypass and then with a real headset; a missing microphone
is reported honestly rather than hanging. Streaming's own command → poll →
caption path was driven live through the installed app the same
bypass way (this machine still has no microphone) and caught a real bug —
`whisper-server.exe` outliving a `taskkill`'d app, fixed with the same
Windows job-object containment `pty::spawn` already trusts. What remains is
one live pass confirming the sound cues, both earlier bug fixes, *and*
streaming's captions together, by ear, on a real microphone.

---

## M14 — Notifications (§49)  ✅

The feature Alan asked for by name, against ORCA: be told when any agent stops
working — because it finished, or because it needs a decision — with a preview
of what it wants.

- [x] The rule the whole thing turns on: **notify only about what the person is
      not already looking at**, where watching means the window has focus *and*
      that session is on screen. A suppressed notification is dropped entirely,
      not stored and marked read, so the centre is a list of what you *missed*.
      Decided in the core, in one place, and tested.
- [x] Five sources, two confidences (§28). A finished turn, a guardrail
      decision, a mission completing and a run stopping are **Official** —
      something stated them. A question read off a terminal is **Observed**.
- [x] `notify::detect` — the part nothing existing could do. Claude Code asking
      whether it may write a file is stated nowhere: the guardrail hook returns
      early for anything that is not a classified `Bash` command, and the
      transcript records the answer rather than the question. Built from four
      recordings of real CLIs (`notify::capture`), keyed on the one invariant
      that survives all four — a numbered choice list with exactly one cursor
      glyph — never on wording.
- [x] `notify::render`, because these TUIs emit cursor-forward escapes where a
      person sees a space, and a conventional `strip_ansi` turns
      `Do you want to create hello.txt?` into `Doyouwanttocreatehello.txt?`.
- [x] `notify::watch` — the conjunction that makes a match trustworthy: the
      shape holds, the terminal has been quiet, and nothing has been typed
      since. One thread per agent session, parked on a channel from the pump.
- [x] Migration 12, read state, dedup keyed on the question rather than the
      session, and a prune at startup.
- [x] `notify::bus`, so a mission changing status does not need a webview
      handle to say so.
- [x] The surface: a bell in the titlebar, a centre under it, an in-app toast
      stack, a Windows toast when the window is behind something else, a
      taskbar flash, and a short synthesised sound.
- [x] Three switches in Settings and a test button that fires the real path.
- [x] i18n complete in en and pt-BR. The title is ours and translated; the
      preview is the agent's own words and is deliberately not.

### What a Windows toast can and cannot do, measured

Fired from the installed build before the surface was written:

* it **appears, attributed to us** — our own mark and name — on a machine that
  also carries an unrelated `jarvis.exe` with its own Start Menu shortcut;
* it does **not** appear from `pnpm dev`, because the plugin only sets the
  AppUserModelID for a packaged binary and an unpackaged one has no shortcut to
  be identified by;
* **clicking it does nothing, and cannot**: `tauri-plugin-notification` 2.3.3
  exposes activation callbacks on mobile only.

So the desktop toast is an *alert* and the centre is the thing you click. Built
the other way round, this would have shipped a toast that looks interactive and
is not.

### Verified in a real build, against a real agent

A Claude Code 2.1.241 agent in a scratch project, both themes, pt-BR.

Its folder-trust question was read off its own terminal and shown as
**Lido do terminal** (Observed); its reply after the next turn was shown as
**Informado pelo agente** (Official) with the reply itself as the preview. While
that session was the one on screen, **nothing was raised at all** — the moment
the workspace moved to Files, the same class of event produced a toast and a
badge. Clicking a row went to the session, and the toast for a session stood
down as soon as that session came on screen.

### The one that would have shipped broken

**`isFocused()` returns `true` for a minimised window on Windows.** The feed
asked it the instant a notification arrived, concluded the person was sitting
right there, drew an in-app toast onto a window nobody could see, and never
fired the desktop toast — which is the entire point of the feature. Nothing in
the interface looked wrong: the notification appeared correctly in the centre.
Caught by asking *Windows* what it had actually been handed
(`ToastNotificationManager::History`), and finding nothing from the run that had
just happened. The fix is not a better call but no call: the surface now reads
the focus state `onFocusChanged` last reported — the same signal the core was
given — and checks `isMinimized()` beside it. See HANDOFF §5 item 55.

**Then verified properly:** window minimised, a real Claude Code agent finishing
a real turn, and Windows showed *Claude Code terminou / This is a throwaway
scratch project used to verify notifications end to end.* — the agent's own
reply, from the installed build.

### Four more found by running it, not by reading it

1. **The count badge covered the bell.** A 13px badge on a 14px icon in a 38px
   button; visible only by zooming into a real screenshot.
2. **The toast's close button sat level with the middle of the agent's own
   question** and read as part of the text.
3. **Opening the centre erased the thing it was opened to see.** Marking
   everything read is right, and it also cleared the marker showing which rows
   were new — so two new notifications rendered exactly like two old ones.
4. **Clicking a notification for an already-open project did nothing.** The
   area is state read once at mount, and `App` keys the workspace on the
   project id, so re-opening the same project is not a remount.

### One found by reading, not by running

**An unattended run would have notified once per turn.** Twenty notifications
under the default budget, while the person was away by choice, and then a
twenty-first when the run actually stopped — the only one they had asked for,
at the bottom of a pile. Setting a mission to Unattended is asking not to watch
it, so a driven session’s finished turns are dropped. Its *questions* are not:
a driven agent that stops to ask is the one thing about an unattended run worth
interrupting somebody for, because it cannot continue until they answer. See
D35’s addendum.

### Deliberately not in M14

Push to a phone. `ROADMAP` already called it its own scope, and it needs a
developer account (B6). The relay snapshot (§59) already carries what needs a
person, which is the same question answered on a different screen.


## Current milestone
**M14 — Notifications (§49) is complete and verified in a real build.** An
agent that stops — finished, or waiting on a decision — now reaches the person
who walked away, with a preview of what it wants and an honest label for where
that preview came from. See the section above, and D35.

**M6 and M7 are both complete.** Files, the editor, Review, Git write
operations, worktrees, the memory layer, Global Search, and an agent writing
to its own Brain are all built and verified against real data and a real
agent. Onboarding (§13) is also built and verified; Settings (§64) is what
remains of M9. Voice dictation (§54, M12), including real-time streaming
transcription, is now fully built: tested with a real microphone once, two
real bugs that pass surfaced are fixed and proven (an irritating PSReadLine
bell, and dictated accents garbling specifically inside Claude Code's TUI),
and streaming's own command → poll → caption path is proven through the
installed app via a synthetic-audio bypass (D31) — including a real
orphaned-process bug the same live pass caught and fixed. Open on M12: one
combined live ear-test pass confirming the sound cues, both bug fixes, and
streaming's captions together on a real microphone.

## M13 — Accounts and quota (§66)  ✅

The feature Alan named the most important one in the project: four Claude Pro
subscriptions, each with its own five-hour allowance, and work that moves to the
next account rather than stopping.

- [x] Migration 11 — `provider_accounts`, `account_limit_events`,
      `sessions.account_id`, `usage_samples.account_id`.
- [x] The registry and the identity model (`accounts/mod.rs`). An account **is**
      a provider configuration directory (`CLAUDE_CONFIG_DIR` / `CODEX_HOME`);
      the account already signed in on this machine is adopted, never copied,
      and nothing here ever rewrites the user's own credentials. Rewriting one
      global credential file would log the user out of the session they are
      sitting in front of, and could not let a running session finish on the old
      account while new work starts on the next one — which is the point.
- [x] The quota model (`accounts/quota.rs`). Established empirically against
      115 real transcripts: **Claude Code has no live gauge** — a rejection is
      Official and exact to the second, everything before it is Observed at
      best, and a percentage needs an allowance learned from this machine's own
      past refusals. **Codex does** state its consumption every turn, and the
      adapter currently reads the wrong field name for the reset time.
- [x] Prove `CLAUDE_CONFIG_DIR` / `CODEX_HOME` against the real CLI, in an
      `#[ignore]`d test. Everything else rests on it.
- [x] Config dir and per-session transcript root plumbed through the launcher.
- [x] The Accounts surface, the quota panel, manual switch, automatic switch.
- [x] Agent continuity across a switch, through the Brain brief rather than
      `--resume`, which cannot cross configuration directories.

Full account, including everything measured and every trap:
[`docs/M13-ACCOUNTS.md`](M13-ACCOUNTS.md). Signing in to accounts 2–4 needs an
interactive browser login and is **B6**.

## M15 — Session History (§88)  ✅

Every session this machine has ever run, in one place: titled, searchable,
grouped by when it happened, openable read-only, renameable and deletable.

Two facts made this the gap it was. `sessions.title` had existed since migration
1 and **nothing had ever written to it**, so every session in the database was
untitled; and `session_list` filters `ended_at IS NULL`, which is right for the
terminal tabs it feeds and meant a finished session was unreachable from
anywhere in the product except by stumbling on one of its own sentences in
Global Search.

- [x] **Titles that mean something (D36/D37).** Claude Code names its own
      sessions and nobody was reading it: `ai-title`, in 89 of the 124
      transcripts on this machine. It is lifted before the noise filter that
      correctly drops it for Conversation View. Codex states no title at all —
      a real capability difference, expressed as `TitleSupport` with a test that
      fails if the two providers ever describe themselves identically (§26).
      Precedence is user > provider > derived, enforced in SQL.
- [x] **A derived title for everything older (D38)**, backfilled off the startup
      path from the search index rather than from the logs, with the ordering
      against `search::backfill` made safe rather than assumed.
- [x] **The surface.** Grouped by day, keyset-paginated (never `OFFSET`), filters
      for range, provider and project, inline rename, inline delete confirmation.
- [x] **Search over what was actually said**, through the FTS5 index §51 already
      built — the part the reference this was modelled on cannot do. A title
      match sorts above a body match; a body match shows the line it matched.
- [x] **Delete that really deletes (D39)** — rows, FTS index entries, and the log
      directory — and says how many bytes it freed. A live session is refused.
- [x] **Storage made visible.** No retention policy was invented (HANDOFF item
      38 flags that as a decision nobody should make casually); the surface just
      says what the logs cost, so it is a decision somebody *can* make.
- [x] Verified in a real build against a real Claude Code 2.1.241 session, in
      both themes and both languages.

Not built, deliberately (§81): the **Web** tab in the reference screenshot.
Those are Claude's cloud sessions; this product is local-first (§3) and does not
have them.

Two bugs the looking found: every row was forty pixels too tall because a
four-item grid had three columns and the fourth wrapped onto an invisible second
row; and clicking a row for an **archived** project did nothing at all — a
silent fall-through that turned out to have been in Global Search since §51
(D40).

- [x] **Continuing a session (D41).** A row opens a **preview** -- the
      conversation, read-only -- and the preview offers the way back to the
      terminal: a live session is rejoined, a finished one is continued by a new
      agent handed the old conversation, in a tab named after it. Both CLIs
      resume by opposite mechanisms and only one can be followed, so
      `ResumeSupport` says which and the offer is absent with a reason where it
      cannot work. Verified by planting a fact, closing the session, continuing
      it and asking: **"Amber."**
- [x] **A session starts where it says it starts (D42).** Found by continuing
      into a deleted folder and watching a real agent open in the user's home
      directory. True of every launch into a moved project; refused now.

Full account: [`docs/M15-HISTORY.md`](M15-HISTORY.md).

## M16 — Live, official quota per account (§66)  ✅

M13 shipped complete and correct, and Alan opened it and said he still could not
see when a window resets, how much he had left, or which quota was the one
holding him up. He was right, and the reason is worth keeping: M13 read all 115
transcripts on this machine, established that Claude Code states quota **only in
the turn it refuses**, and built an impeccably honest panel that therefore said
"allowance unknown" everywhere — sending him back to the web UI the feature
existed to replace. The finding was true about *transcripts*, not about the
providers.

- [x] Both CLIs answer a live usage question on their own supported protocol —
      `get_usage` on Claude Code's stream-json control channel,
      `account/rateLimits/read` on `codex app-server`. Measured against the real
      binaries, not remembered: no HTTP endpoint, no credential read, no token
      spent, no transcript written.
- [x] **The check that decided the scope**, before anything was built: run each
      probe against an *empty* configuration directory. Both read the account in
      the directory they are pointed at, and both fail by saying so instead of
      returning the ambient account's numbers under the wrong name. Kept as an
      `#[ignore]`d safety test — if it ever passes as `Ok`, the feature is
      misattributing allowances and must be turned off.
- [x] Migration 15 — `account_live_readings`, one row per account. A cache, and
      it says so by holding exactly one row; the append-only record stays in
      `account_limit_events`, which every live reading is folded into so the
      switch, the calibration and `build_window` needed no knowledge of probing.
- [x] `implied_allowance` — the unpublished allowance learned from *any*
      official percentage rather than only from a refusal, so the Estimated
      fallback stops needing the failure it exists to prevent.
- [x] The surface rebuilt around the three questions actually being asked: a
      dial showing what is **left**, a countdown *and* the wall-clock moment,
      and the binding window drawn larger than the ones that are not. Paid
      overage in the provider's own currency. Provenance and reading age on the
      card, not in a footnote.
- [x] Status-bar chips (D44) — the answer where the question gets asked.
- [x] Codex identity via `account/read`: this build stopped writing
      `id_token_claims` into `auth.json` and every Codex account had silently
      gone nameless.

Full account, including everything the looking found:
[`docs/M16-QUOTA.md`](M16-QUOTA.md). See D43 and D44.

## Next steps

1. Re-verify voice dictation end to end by ear on a real microphone — the
   sound cues, both earlier bug fixes, and now streaming's live captions
   too. One pass now covers everything voice-related that has shipped
   since the last one. See HANDOFF §7 item 1.
2. ~~The rest of M2~~ — **done.** Split panes (§20), scrollback search (§20)
   and image paste (§22) are all built and all verified in a real build.
   **M2 is finished.**
3. ~~Global Search does not backfill~~ — **done** (D30). `search::backfill`
   walks every session log once, off the startup path, one session per
   transaction, resumable and idempotent. Verified against this machine's own
   recorded sessions: 55 rows out of 10 real logs, and a Portuguese word from
   a real Claude Code run came back as a Conversation hit attributed to the
   session it was said in.
4. A manually completed mission is never asked what it learned (D27) — the
   reflection only fires at the end of an Unattended run, deliberately.
5. **The notification sound has never been heard.** `present.chime` synthesises
   two sine tones through `AudioContext` — quiet by construction, with an
   exponential envelope so it cannot click — and it is on by default. Every
   other channel was verified by looking; this one cannot be, and nobody has
   listened to it yet. It shares the ear-test debt with the voice cues in item
   1, and should be judged in the same pass: is it audible from another room,
   and is it annoying at the desk? If it is either wrong, the switch is already
   in Settings.
6. **Notifications, one thing left open (§49).** The detector reads a question
   off the terminal for Claude Code and Codex, and its evidence is four real
   captures from those two. A third agent CLI would need its own capture
   before anyone should assume it is covered — `notify::capture` is the
   harness, and the invariant it keys on is a shape rather than a sentence, so
   the honest expectation is *probably*, not *certainly*.
7. **Push to a phone is still its own scope.** The relay snapshot already
   carries what needs a person; delivering it as a push needs a developer
   account (B6).
