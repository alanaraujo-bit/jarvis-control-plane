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

## D5 — Git through the `git` executable, not libgit2
**Date:** 2026-08-22
**Why:** Worktrees (§45) are only fully supported by the CLI, and shelling out
means behaviour matches the user's own Git exactly — their config, hooks,
credential helpers and `safe.directory` rules. A library would silently diverge.
Git is already a required tool (§14), so this adds no dependency.
**Guards:** every invocation sets `GIT_TERMINAL_PROMPT=0` and `GIT_PAGER=cat` so
a subprocess can never hang waiting for input there is no UI to provide.

## D6 — The PTY host answers ConPTY's startup cursor query
**Date:** 2026-08-22
**Why:** Discovered empirically, not from documentation. On Windows, ConPTY
emits a Device Status Report (`ESC [ 6 n`) as a startup handshake and **stalls
the entire session until the terminal replies**. With no reply, a session
produces those four bytes and then nothing, forever.

When a terminal view is attached, xterm.js answers and everything works — which
is exactly why this is dangerous: it would have looked fine in every manual test
and failed precisely in **Unattended mode** (§32), where an agent runs with no
view attached. That is the mode the product most needs to be reliable.

**Decision:** the Rust side tracks whether a view is attached to each session and
answers the query itself when none is. Terminal semantics belong to the core, not
to whether someone happens to be looking.

**Verified:** an isolated reproduction confirmed both halves — no reply stalls the
stream; replying `ESC [ 1 ; 1 R` releases it immediately.

## D7 — Sessions are spawned with the parent agent's environment scrubbed
**Date:** 2026-08-22
**Why:** Found by running the product, not by reasoning about it. When
J.A.R.V.I.S. is itself launched from inside an agent session, that agent's
environment markers (`CLAUDE_CODE_CHILD_SESSION`, `CLAUDECODE`, `CLAUDE_PID`, …)
are in our process environment and are inherited by everything we spawn.

A nested Claude Code sees the child-session marker, concludes it is a
sub-session, and **turns transcript saving off**. That would silently remove the
structured stream Conversation View (§24) and usage reporting (§28) depend on —
the terminal would look perfectly fine while the entire structured half of the
product quietly stopped working.

**Decision:** `pty::spawn` removes these markers for every session kind. A
session launched by J.A.R.V.I.S. is a new top-level session, and a plain shell
should not believe it is running inside an agent either.

**Tested:** a real child process is spawned and asked to echo the marker back.

## D8 — xterm's font family must be a literal stack
**Date:** 2026-08-22
**Why:** `fontFamily: "var(--font-mono), …"` looks reasonable and is silently
wrong. xterm measures glyph width by rendering into a canvas from that string,
where `var(...)` does not resolve; it falls back to a proportional font and
every terminal line renders with visibly uneven letter spacing. Caught by
looking at a screenshot of the real terminal, not by any test.

## D10 — Guardrails enforce through the provider's own pre-tool hook
**Date:** 2026-08-23
**Why:** §35 needs a sensitive operation to be *stopped*, not merely noticed. An
agent CLI running inside a PTY cannot be intercepted from outside — by the time
bytes reach the terminal the command has run. The only real enforcement point is
the callback the provider itself makes before running a tool.

Verified empirically **before** any of it was designed, against Claude Code
2.1.240: a `PreToolUse` hook receives `tool_name` and `tool_input.command` on
stdin, and `permissionDecision: "deny"` genuinely stops the command and hands
the reason to the model. Also verified: a hook that exits non-zero is treated as
having no opinion, which is what makes failing open safe.

**Consequence:** the same executable runs as the hook (`--jarvis-guardrail-hook`),
checked in `main` before Tauri starts. It reads a policy snapshot the app wrote
for that session and never opens the database — a program that runs once per
tool call must not contend with the application for it.

**It cannot write the session log either.** A session has one writer (D2), so
the guard appends its decisions to its own JSONL and the session runtime
projects them into the log as `Approval` frames — the same shape as following a
provider transcript.

## D11 — Guardrails fail open, deliberately
**Date:** 2026-08-23
**Why:** This process runs before **every** tool call in every agent session. A
guard that failed closed would turn any bug in it — a missing file, malformed
JSON, a snapshot from another version — into an agent that cannot work at all.

The cost is real and is not hidden: a rule can silently fail to apply. So the
product never claims this layer is absolute. `ProviderCapabilities::guardrails`
reports what each provider actually enforces, the classifier's own module doc
says a command that does not match has not been proven safe, and the UI reports
what *matched* rather than declaring what a command is.

Where enforcement **is** unconditional is a verification command J.A.R.V.I.S.
runs itself (§30): we own that process, so a refusal there means it truly does
not execute.

## D12 — "Ask" becomes a refusal when nobody is attached
**Date:** 2026-08-23
**Why:** Under Guided or Autonomous someone is looking at the terminal, so the
provider's own prompt reaches a person. Under Unattended (§32) nobody is, and
that same prompt would wait forever — which is precisely the indefinite resource
consumption §34 exists to forbid.

So when a rule says *ask* and no view is attached, the guard **refuses** and the
mission goes to Waiting with a reason. Stopping and explaining beats hanging
quietly. The snapshot's `attended` flag is rewritten on every attach and detach,
because whether a human is present is a fact about *now*, not about when the
session started.

**Verified against real Claude Code:** with no view attached, `rm -rf junk` was
refused, the agent reported it was blocked and explicitly did not try another
deletion method, and the directory was still there afterwards.

## D13 — An operation has exactly one spelling
**Date:** 2026-08-23
**Why:** `Operation` is serialised through `as_str`, not a serde `rename_all`
rule. With `rename_all = "kebab-case"` serde emitted `git-force-push` while
storage, the policy snapshot and the i18n keys all used `git.force-push`.

Nothing failed. The core was correct, every test passed, and the Settings screen
rendered eight raw message keys — found by looking at a screenshot, which is the
fourth time in this codebase that has been the only thing that would have caught
a bug. `an_operation_serialises_as_the_id_used_everywhere_else` now pins it.

## D14 — A turn ends when the provider says so, never when the terminal goes quiet
**Date:** 2026-08-23
**Why:** The autopilot (§32) has to know when the agent has finished a turn, and
the tempting signal — the terminal going quiet — is a guess that is wrong
exactly when it is expensive. A long compile and a finished turn look identical
from outside; interrupting the first is worse than waiting for the second.

Both providers state it outright. Claude Code reports `stop_reason: "end_turn"`,
where `"tool_use"` means it is still mid-loop and will be back; Codex emits
`event_msg/task_complete`.

**Verified before designing anything:** across all 88 Claude Code transcripts on
this machine, 26,928 assistant messages carry a stop reason and 664 turn
boundaries were recovered, in 82 of 88 sessions, none of them a `tool_use`. The
parsers turn both providers' signals into `ConversationItem::TurnEnded` and the
autopilot reacts to that and nothing else.

**Consequence:** it also fixed a real bug found the same way — a
`file-history-snapshot` keeps its timestamp *inside* `snapshot`, unlike every
other entry, so file changes were being stamped at the epoch in the timeline.

## D15 — A driven session has nobody to ask, even with someone watching
**Date:** 2026-08-23
**Why:** The guardrail snapshot originally treated "a view is attached" as
"there is a human to ask". That is wrong, and wrong in the direction that hangs.

A driven session usually **does** have its terminal open with a person reading
along — and that person is not who the provider's permission prompt reaches.
The autopilot is in the seat. The agent would sit on a prompt the autopilot
cannot answer, which is exactly the indefinite consumption §34 forbids, and it
would look from outside like the agent had simply gone quiet.

**Decision:** `Snapshot::can_ask_a_person()` requires a view attached **and** no
autopilot driving. A driven session sets `driven: true`, so a rule set to *ask*
refuses with a reason and the mission goes to Waiting.

## D16 — The autopilot types; it does not paste
**Date:** 2026-08-23
**Why:** Found by driving a real agent and reading the session log — twice, in
two different ways, neither of which any test would have caught.

First, a freshly started agent has never spoken, so there is no turn end to
react to. Without an opening instruction the run sits at "turn 0" with a working
agent doing nothing at all.

Second, sending the instruction as one write **loses characters** (observed on
screen: "so tere is no"), and appending the carriage return to the text leaves
the instruction sitting in the prompt **unsent** — the line editor is still
catching up and swallows it. That failure is the nastiest of the three, because
the terminal shows the words perfectly while the agent has been told nothing.

**Decision:** wait for the prompt to be drawn, write the instruction in small
paced chunks, then send the submit key as a separate write after a pause.

## D17 — Monaco ships without its language services, and is loaded on demand
**Date:** 2026-08-23
**Why:** D4 chose Monaco for the editor (§42) and left the cost unmeasured. The
handoff asked for that number **before** committing, so it was measured rather
than estimated — and the estimate would have been wrong by a factor of four.

Three builds, all real:

| Configuration | `dist` on disk |
|---|---|
| `import * as monaco from "monaco-editor"` — every grammar, every worker | **14.0 MB** |
| Editor core + 21 named grammars + the JSON service | 4.5 MB |
| **Editor core + 21 named grammars, no language services** (shipped) | **4.1 MB** |

What that actually costs the product is smaller than any of those, because
Tauri compresses the frontend into the binary. Measured on the installed
application, not inferred:

| | Before M6 | With Monaco |
|---|---|---|
| `%LOCALAPPDATA%\J.A.R.V.I.S` | 7.49 MB | **8.53 MB** |
| `…_x64-setup.exe` | not measured | 4.13 MB |

**+1.04 MB installed for a full code editor.** That is not a problem, and it is
worth writing down that the 4.1 MB figure from a scratch Vite build is a
ceiling, not an answer — extrapolating from it would have overstated the cost
by 4×.

**The TypeScript, CSS, HTML and JSON language services are excluded**, and not
only for weight. Monaco's TypeScript worker knows nothing about the project's
`tsconfig.json` or its `node_modules`, so it reports missing imports and
phantom errors on code that is perfectly correct. Confidently wrong diagnostics
are worse than none — the same principle §28 applies to numbers. Real
intelligence arrives with the LSP client D4 left room for, which does know those
things. That the worker is also 7 MB on its own, larger than the entire
installed product, settles it.

**Loaded on demand.** The editor is a 3.7 MB chunk and most sessions never open
a file, so it is dynamically imported behind `packages/editor`. Verified in the
release bundle: the eager entry chunk contains no occurrence of `monaco`.

**Also found by looking, as usual.** Monaco's bracket pair colourisation is on
by default and paints nested brackets in saturated gold, pink and blue. It is
decoration — the colour encodes nesting depth, which nobody reads — and the gold
is close enough to the product's amber to read as a state signal (§6). It is
off. Caught in the first screenshot of the real editor, not by any test.

## D9 — The main binary is named `jarvis-desktop`, not `jarvis`
**Date:** 2026-08-22
**Why:** Found by running the real installer on a real machine, not by reading
config. Tauri derives the executable name from the crate, giving `jarvis.exe` —
and this machine already had an unrelated application installed with that exact
binary name.

NSIS detects a running instance **by executable name**, so our installer
displayed "J.A.R.V.I.S is open! Click OK to close it" while pointing at
somebody else's program. Accepting would have terminated an application that has
nothing to do with us.

It failed safe when declined, but the collision is the bug. A distinctive binary
name removes the ambiguity at the source.

**Also affected:** `tools/JarvisWindow.ps1`, which locates the dev window for
screenshots and UI automation. It already filtered by executable *path* for this
same reason — two earlier attempts, matching by window title and then by process
name, each mis-targeted a real unrelated window on this machine.

## D18 — A worktree is a project
**Date:** 2026-08-23
**Why:** §45 could have meant two very different things, and the difference was
worth an hour of checking rather than a week of building the wrong one.

A project in this product is a folder on the machine with a checkout in it
(§16). A worktree is a folder on the machine with a checkout in it. Registering
each worktree as **its own project row** means Files, the editor, Review,
attribution, sessions, missions and guardrails work inside one with no changes
at all.

The alternative — a worktree as a *view* within a single project — would mean
teaching `files::project_root` which tree it is looking at, splitting
`file_changes` attribution across trees, and reworking the path confinement §41
calls the security boundary. All of it to describe something the filesystem
already describes.

**Verified before deciding, not after:** `rev-parse --show-toplevel` run inside
a worktree returns the **worktree's own** path, not the main repository's. That
one fact is what makes `git::locate` answer correctly inside a worktree, and
therefore what makes everything above free. Had it returned the main repository,
opening a worktree would have shown the wrong tree's files and the wrong tree's
diff, silently. `a_worktree_locates_as_its_own_repository_root` pins it.

**Consequence:** `projects.worktree_of` (migration 7) records the relationship,
because a worktree registered as a bare project row is a folder that appears in
the list with no explanation of where it came from — which is precisely how it
looked before the row was taught to say so.

## D19 — A guardrail in front of our own operation names it; it does not classify it
**Date:** 2026-08-23
**Why:** `hold_for_guardrail` classifies a mission's verification command
because that command is text a person wrote and we have to work out what it
does. A button is not text. When Review discards a file, this crate builds the
`git restore` line itself and already knows which operation it is performing.

Round-tripping a command we constructed through the classifier would import
D11's **fail open** into the one place D11 promises enforcement is
unconditional: if the matcher ever stopped recognising our own spelling, a
destructive operation would run silently unguarded and every test would still
pass. `policy::resolve(conn, project, Operation::…)` cannot fail that way.

**Also decided here: no `Pending` row for a surface-initiated operation.** An
agent's tool call is intercepted mid-flight and a verification can be parked and
resumed, so both can wait for somebody to come back. A button cannot — the
person is *right there*. Worse, `pending()` feeds Mission Control's
needs-attention list and `decide_guardrail` can only resume work through a
`criterion_id`, which a Git action does not have: the row would be settled, the
action silently dropped, and the queue left asserting a human is needed forever.

So the surface asks (`check`, recording nothing), shows the §35 choices, and
calls back with the answer. The core **re-resolves** on that second call: the
choice is the human's answer, never the caller's authority. A `Deny` is not
overridable by anything arriving from the webview.
`a_refusal_cannot_be_talked_out_of_by_a_choice` pins that.

## D20 — Discard means HEAD, and it is three commands
**Date:** 2026-08-23
**Why:** Found by checking against real Git 2.55 **before** writing the button,
which is the only reason it was found at all — every wrong spelling here exits
zero.

`git restore <path>` restores the working tree **from the index**. A file
carrying staged content comes back as the *staged* version, not the committed
one. A "Discard" button built on it throws away half a change, reports success,
and leaves nothing on screen to say so. The spelling that reaches the commit is
`restore --source=HEAD --staged --worktree`, and it is also the only one that
survives a **staged deletion**, where plain restore fails outright with
`pathspec did not match any file(s) known to git`.

Two more cases that are not that command at all: an **untracked** file has no
committed version to return to, so discarding it is `git clean -f`; and a
repository with **no commits** has no `HEAD`, so unstaging is `git rm --cached`
rather than `restore --staged`, which dies with `could not resolve HEAD`.

One button, three code paths, each tested against a real repository.

`git.discard-changes` is its own guardrail class for a related reason:
everything else the classifier catches leaves the old commits in the reflog for
thirty days, while uncommitted work has never been written to an object at all.
`git restore`, `git checkout -- <path>` and `git stash drop` were **not**
recognised by the classifier before this, so an agent discarding a person's
uncommitted work was never guarded — the handoff said otherwise and the handoff
was wrong.

## D21 — Knowledge is briefed; a note is not
**Date:** 2026-08-23
**Why:** §36–§40 could have been one table with a flag. Two tables, and the
line between them is a question with one answer: **does an agent need to know
this?**

Knowledge is what stays true about a project — what it is, how it is built,
what will bite you, what a word means here. A note is working memory: a
reminder, a link, a thing to come back to. Handing an agent somebody's todo
list as context is worse than handing it nothing, because it is exactly the
kind of noise that makes a model confidently wrong.

Deciding it by *kind* rather than by a per-item switch means there is nothing
for anyone to forget to set, and nothing to get wrong in a hurry.
`a_note_never_reaches_the_brief` is the test, and `brief::compose` takes only
`&[Knowledge]` — there is no parameter through which a note could arrive.

A note can be **promoted** into knowledge when it turns out to be durable, and
the note is removed rather than copied: the same sentence in two places is two
sentences that will drift apart.

## D22 — Derived facts are recomputed, never stored
**Date:** 2026-08-23
**Why:** The Brain shows two kinds of thing and marks which is which, for the
same reason §28 stamps confidence on every number. *"Deploys go through
staging"* and *"src/app.ts changed nine times"* are not the same kind of claim,
and a surface that renders them identically invites the reader to trust the
weaker one as much as the stronger. They are on **separate tabs**, so the rule
is structural rather than a caption somebody has to read.

Stated knowledge carries its `source`: an agent's claim about a project is not
the owner's, and flattening the two hides that.

Derived facts are computed on every read and never written down — the choice
Review made (D2), for the reason that a *stored* derived fact can go stale
without anybody noticing. A memory layer that quietly lies is worse than one
that is empty.

What earns a place is a fact that would change what somebody does. Analytics
(§52) already counts tokens and runtime; these answer the narrower question
*"what should I know before I touch this?"* — which files everyone keeps
editing, what has been refused here, whether completions in this project hold.
Completed and revoked are reported **together**: nine completions with six
revocations says something that "nine completed" actively hides (§30).

## D23 — A brief reaches the agent out of band, never through the user's repo
**Date:** 2026-08-23
**Why:** There were two honest ways to give an agent a project's knowledge, and
one of them is invasive.

Writing `CLAUDE.md` or `AGENTS.md` into the project is cheaper — the provider
reads it unprompted, costing nothing extra — and it means J.A.R.V.I.S. writing
into the user's repository. It would appear in `git status`, in their diff, and
in the very Review surface this product also ships; and it would collide with a
`CLAUDE.md` they wrote themselves. §3 says the code is theirs.

So the brief is written beside the guardrail snapshot in **our own** log
directory and passed with `--append-system-prompt-file`. Nothing in their tree,
nothing in their terminal, and it survives them having their own instructions
file.

**Appended, never replacing.** `--system-prompt` differs by one word and would
strip the agent of everything else it knows — an agent that had forgotten how
to be an agent. No test of the brief's *content* would catch that, so
`a_brief_is_appended_rather_than_replacing_the_system_prompt` tests the
argument list.

**Verified twice, and the first time did not count.** `--append-system-prompt-file`
plainly works under `claude -p`; every session this product starts is an
interactive PTY. Assuming those are the same thing is the shape of the Monaco
option that exists, type-checks and does nothing (D17). It was settled by
writing four things into a scratch project's Brain, starting Claude Code from
the app, and asking what it knew without reading files: it answered with all
four, under the brief's own headings, in a folder it had never seen.

Codex 0.147.0 has no equivalent flag, so it reports `OpeningMessage` and is not
handed a file it would never read (§26).

## D24 — A hidden area is not an unmounted one
**Date:** 2026-08-23
**Why:** The project areas are mounted once and hidden with CSS, so returning to
one keeps its open file, its scroll position and its selected diff. That is
worth keeping, and it quietly broke three surfaces at once.

A `useEffect` keyed on the project id fires when the component mounts and never
again. Review carried the comment *"Re-read on every visit. An agent may have
been working the whole time the user was on another surface, and a stale diff is
worse than a slow one"* directly above an effect that did not do that. Nothing
errored; the surface simply showed what had been true the first time it was
opened.

Found by running a real agent, going back to the Brain, and reading "nothing has
happened in this project yet" over a project where something had just happened.

`useVisitRefresh` watches `active` going from false to true, which is the signal
a mounted-and-hidden component has instead of a mount. Note the shape of this
bug: the comment was right and the code was wrong, so reading the code alone
would have confirmed the intention rather than the behaviour.

## D25 — session_events becomes a real mirror, and Global Search stays cross-project
**Date:** 2026-08-23
**Why:** §51 needed the one place D2 promised: `session_events`. Checking
before building found it had no writer since migration 1 — a repo-wide search
of every `INSERT` site turned up nothing. It existed, with zero rows, on every
installation the product has ever run on. HANDOFF's own §7 said otherwise;
HANDOFF was wrong.

**session_events is extended, not recreated.** The table had no reader either,
so nothing depends on the shape migration 1 gave it — but rule 2 at the top of
`migrations.rs` says additive, so migration 9 adds three columns (`project_id`,
`label`, `text`) rather than dropping and recreating the table. `kind` is
repurposed to carry `ConversationItem`'s own tag (message | thinking | toolCall
| toolResult | error) rather than the coarser session-log `EventKind`, which
collapses message, thinking, turnEnded and error together — exactly the
distinction search needs to tell what an agent said from what it only thought.

**The FTS5 index is standalone, not `content=session_events`.** External-content
mode keys against an integer rowid, and `session_events` is `WITHOUT ROWID` with
a composite `(session_id, seq)` key. A second small copy of the handful of
columns a result needs (`session_events_fts`) sidesteps that entirely, at a
cost this table's size will never notice — verified available in the `bundled`
rusqlite feature already in use, with no new Cargo dependency.

**Everything else uses `LIKE`, not FTS.** Knowledge, notes, missions and
activity are small tables — a few hundred rows even for a heavy user — so a
plain parameterised `LIKE '%…%'` is the same choice `activity::list` already
makes. Adding FTS machinery there would be infrastructure this product does
not need yet, the same restraint D22 applies to derived facts.

**Global Search spans every project, not the open one.** The question it
answers — "where did I see that" — has no reason to assume the answer lives in
whatever happens to be open, and scoping to one project would silently hide the
times it does not.

**A past session's conversation had no way to be reopened.**
`session_conversation` already worked for an ended session — `ConversationView`'s
own `live` prop anticipates exactly this — but `ProjectWorkspace` only ever
built a tab for a session it started or found still running (`adopt` filters to
`live`). A conversation match from Global Search had nowhere to go.
`TerminalTab.historical` is the fix: a tab that renders only `ConversationView`,
was never `startSession`-ed, and is never `closeSession`-ed either — closing it
ends nothing, because nothing here was ever attached to begin with.

**Verified against a real Claude Code turn, not just unit tests**, which is the
only reason the next decision exists.

## D26 — usage and searchable text are recorded independently, not as alternatives
**Date:** 2026-08-23
**Why:** Found by running a real Claude Code turn in a scratch project and
searching for its own reply: only the question came back. `mirror`'s first pass
routed on a single `match` — a `Message` carrying non-empty usage took the
"record usage" arm, and a `Message` without it fell through to "record
searchable text". A real assistant reply almost always carries **both** its own
text and the usage the provider billed for producing it, so the ordinary case —
not an edge case — took the usage arm and its text was silently never indexed.
Only a plain user-typed message, which never carries usage, was ever
searchable; the replies were not.

**Decision:** the two inserts are independent statements, not exclusive match
arms. A `Message` with usage writes to `usage_samples`; the same item, if its
text is non-empty, also writes to `session_events` — regardless of what the
usage check decided. `a_reply_with_both_text_and_usage_is_recorded_both_ways`
pins the contract, and
`a_real_claude_code_reply_survives_the_full_pipeline_into_search` runs the
actual line captured from the failing turn through the real parser, not a
hand-built `ConversationItem`, so the whole pipeline is pinned rather than just
`mirror`'s contract in isolation.

## D27 — An agent is asked to reflect only at the end of an unattended run, once, and only into a narrow question

**Date:** 2026-08-23
**Why:** `add_knowledge` has accepted `Source::Agent` since §36–§38 was built
(D21–D23), and nothing ever called it that way — HANDOFF §7 named this as the
open item and named the risk in the same breath: an agent that writes freely
into a project's memory fills it with restatements of its own last task,
which is worse than an empty Brain because `brief::compose` (D23) hands
**every** non-archived knowledge row to **every** future agent that starts in
that project, with no per-row filter. A bad entry here is not clutter sitting
in a tab; it is system-prompt context for every session afterward.

**Where:** `Step::Complete`, right after `mission::store::set_status` reports
the completion actually stuck, and only when the run got there through
`autopilot::driver` (§32). This is the one place a driven run holds the seat
with nobody else in it (D15) — an attended completion is watched by a person,
and typing a question into that terminal mid-conversation is not this run's
conversation to interrupt. **A manually completed mission is never asked.**
That is a real gap, left deliberately: the reusable machinery here is the PTY
write / transcript read pattern the briefing test proved (D23), and applying
it to an attended session raises a UX question — whose seat is it — this pass
did not need to answer.

**What keeps it narrow, all decided up front rather than tuned after the
fact:**
- **One question, asked once per completed mission.** Not "what did you do,"
  which invites a summary of the task — the prompt asks explicitly for what
  would still matter *next time*, forbids restating the task, and offers a
  named escape hatch (`NOTHING TO RECORD`) that is expected to fire most of
  the time. Trusting the agent's own judgement about whether it has anything
  durable to say is the same trust `plan::instruction_for` already extends
  when it lets an agent say "I cannot proceed" instead of guessing (§34).
- **The prompt forbids touching anything.** Completion has already been set,
  and `verify_mission` **revokes** it if a later check finds the evidence no
  longer holds (D25's own doc, one milestone earlier). A reflection turn that
  edited a file could be the reason a completed mission silently un-completes
  itself. Pinned by `the_reflection_prompt_forbids_touching_anything`.
- **A hard length cap, truncated rather than rejected.** 500 characters forces
  the one-or-two-sentence answer the prompt asks for; a reply that ignores the
  hint is cut, not thrown away — a long true fact still beats nothing.
- **An exact-duplicate body is not recorded twice.** Several missions in the
  same project are likely to rediscover the same fact; a straight
  case-insensitive equality check against existing knowledge catches the
  common case cheaply. Similarity matching was deliberately not attempted —
  the same restraint D22 applies to derived facts.
- **No per-item "confirm before briefing" flag.** D21 already rejected a
  per-item switch for knowledge-versus-note, for the same reason: something
  for someone to forget to set. Agent-written knowledge renders in amber
  (`brain.source.agent`, already built before this decision) so a person can
  see and archive a bad entry — that is the review mechanism, not a gate in
  front of the brief.

**A real bug found before this shipped, not after:** the first version read
the reflection's reply starting from the cursor left by the work turn's own
`TurnEnded`. `SETTLE`'s own doc comment says a turn's last frames can still be
arriving when `TurnEnded` is seen — that is one turn earlier than where this
looks, but the same fact applies again here: a straggler frame from the
finished work turn can land after the cursor was captured and before the
reflection question is even sent. Left alone, that stray text is read back as
if it were part of the answer, and the recorded knowledge opens with the
agent restating what it just did — the exact failure this decision exists to
avoid, arriving through a side door instead of the front one. Fixed by
re-baselining to the log's current end immediately before `send()`, not by
trusting the cursor handed in. `a_stray_frame_from_the_finished_turn_never_reaches_the_reflection`
pins it with a straggler frame deliberately written between the two points in
time. Found by writing the test the way §3 asks for tests to be written here —
against a real session log on disk, not a mock — before ever running it
against a live agent.

**Verified in the installed app, against a real Claude Code agent, not just
the unit tests above.** A scratch project's README stated one fact a reader
would not get from the code — the dev server listens on 4173 because
something unrelated on the machine already holds 3000. Created a mission
whose only criterion was a file's existence, set it Unattended, and ran it:
the agent wrote the file, the mission completed, and the terminal showed the
exact `REFLECT_PROMPT` text arrive as the agent's *next* input — not folded
into the completed turn, confirming the re-baseline fix actually holds
outside a test. The agent answered `GOTCHA: The dev server runs on port
4173, not the conventional 3000, because port 3000 on this machine is
already held by an unrelated port scanner — anyone expecting localhost:3000
will find nothing there.` The Brain's Gotcha section showed exactly that
sentence under **Um agente registrou isto** in amber, the brief-size counter
moved (`383 caracteres`), and
Activity recorded `Um agente registrou algo que aprendeu` immediately after
`Missão concluída sem supervisão`, both in pt-BR. Nothing in `done.txt`'s own
"Work confirmed complete" line leaked into the recorded knowledge — the
straggler this decision's own fix targets never showed up.

## D28 — Onboarding is a settings flag, shown once, with no in-app reset

**Date:** 2026-08-23
**Why:** §13 needed a first-run screen and no spec document existed for it in
the repo — the scope was inferred from what was already sitting unused: the
`app.name`/`app.tagline` i18n keys, the environment scan (§14) as its own
finished surface, and `Projects.useProjects().openFolder()` as the only way
the product opens a project. Building a second folder picker or a second
summary of the environment scan would have contradicted §6 (Quiet
Intelligence: one calm screen, not a wizard) for no reason — both already
exist and are already right.

**Where the flag lives:** the shared `settings (key TEXT PRIMARY KEY, value
TEXT NOT NULL)` table, one row keyed `onboarding.seen`, read and written by a
new `onboarding` module — not a new table, because there is exactly one
boolean and `mission::store::global_autonomy` already established the pattern
of using `settings` for singleton state.

**The window reveal waits on the check, not the other way round.** §11
already defers the window's reveal to first paint via `window_ready`, to
avoid a flash of the wrong content; the onboarding check is now one more gate
on that same reveal, so the very first frame a person sees is either the
welcome screen or the normal shell, never one flashing into the other.
Fetching the flag can fail — database unreadable, command not yet
registered — and a check that fails must never be the reason a window stays
hidden forever (HANDOFF item 31 is exactly that failure mode from a different
cause). The frontend fetch defaults to `seen: true` on any error, so the
worst case is skipping onboarding once, never a stuck window. Verified by
temporarily removing `onboarding_status` from `invoke_handler!`, rebuilding,
and confirming on screen that the window still appeared and rendered the
normal app rather than staying hidden.

**No UI control to see it again.** This is a first-run screen, not a tour a
person might want to replay — the environment scan it reuses already has its
own always-available copy in Settings. Seeing it again means resetting the
one row by deleting the local database file, which is a deliberate deletion
of app state, not a click a person could reach by accident. Documented in
HANDOFF §6 with the exact path, since this machine has no `sqlite3` to
`UPDATE` the row directly.

**A real bug found while verifying, not designed against in advance:**
opening a brand-new folder from this screen landed on Mission Control instead
of the project just opened. The cause was not in this module — `Onboarding`
called `onOpenProject(project.id)`, and the id-based lookup one level up in
`App.tsx` was a `useCallback` memoized on `projects`, closed over the array
as it stood when the click fired, which was before `openFolder()`'s own
`refresh()` had put the new project into it. See HANDOFF item 33. Fixed by
having `Onboarding` hand back the `Project` object `openFolder()` already
returned, and adding `openProjectDirect(project)` in `App.tsx` for every
caller that already holds one — which turned out to include the pre-existing
`Projects.tsx` row click, carrying the identical latent bug one screen over.

**Verified in a real build, fresh install simulated by deleting the local
database.** The welcome screen showed with the environment scan rendered
inside it; clicking **Abrir pasta**, selecting a scratch project, and
confirming landed directly inside that project's own workspace (Sessões tab,
`Nenhum terminal em execução`) rather than Mission Control. Opening the same
project afterward from the Projects list did the same. Relaunching the app a
second time went straight to Mission Control with no flash of the welcome
screen. Caught and refused mid-verification, not shipped: the native folder
picker opened on its own last-used location, which was one of Alan's real
projects — cancelled without selecting anything, per the standing rule to
never open a real project through a test flow.

## D29 — Voice dictation runs whisper.cpp locally, as a spawned binary, primed with the project's own vocabulary

**Date:** 2026-08-23
**Why:** Alan asked for dictation into the terminal that is genuinely worth a
paid tier — "algo realmente profissional... premium" — and gave full
autonomy on the technology, with one explicit warning from the session
before this one to think carefully rather than assume. Three real
architectures were probed on this machine, not reasoned about in the
abstract, before picking one.

**What was tried and why it lost:**
- **Windows' own dictation (SAPI / OneCore).** `System.Speech.Recognition`
  has **zero installed recognizers** on this machine at all — confirmed by
  calling `InstalledRecognizers()` and getting nothing back, for any
  language. The newer engine behind Win+H voice typing does have a pt-BR
  token (`MS-1046-110-WINMO-DNN`), but it is reachable only through
  `Windows.Media.SpeechRecognition`, a WinRT API built around a live
  microphone session with no documented way to feed it a pre-recorded file
  — which would have made it untestable by this product's own §80 standard
  (real infrastructure, not assumptions) on top of needing COM/WinRT
  interop this codebase has none of.
- **`whisper-rs` (linking whisper.cpp as a library via FFI).** Real crate,
  real Windows build docs — and **currently broken on Windows/MSVC**: its
  bindgen step emits *glibc*-specific types (`_G_fpos_t`, `_IO_FILE`) for an
  MSVC target regardless of `--target`, a confirmed upstream issue hit
  identically on two crate versions (0.14.4 and 0.16.0), not a local
  misconfiguration. Getting even this far needed installing LLVM, CMake and
  Ninja on this machine — none of which the finished feature uses; see
  HANDOFF for the disclosure of what got installed chasing this path.

**What won: a bundled `whisper-cli.exe`, spawned as a subprocess.**
`ggml-org/whisper.cpp`'s own GitHub release ships a prebuilt Windows
binary. Running it directly sidesteps the FFI/bindgen breakage entirely and
matches a pattern this product already trusts completely — spawning and
reading back an external tool (Claude Code, Codex, `git`) rather than
linking it in, `CREATE_NO_WINDOW` included so it never flashes a console.
It is also the one path that was actually *measurable* here: whisper.cpp
takes a WAV file, so quality could be probed with real audio today, where
the WinRT path could not.

**Local over cloud, deliberately, not by default:** no API key to obtain or
store, no per-utterance network cost, and it keeps voice data inside the
same local-first boundary (§3) as everything else a session touches — a
person's voice never leaves the machine, which is a stronger privacy story
than most competitors' "AI dictation" features can make, and worth saying
in the product itself (see `voice.download.body`). The trade is a real
one-time cost: no billing system exists in this product to actually gate a
"paid tier" on (there is nothing to build there yet), and a ~490MB model
download the first time a person turns this on — never bundled in the
installer, so the base install footprint (§62, currently 7.3MB) is
untouched by a feature most people may never enable.

**Vocabulary priming is the actual product idea, not a footnote.** Proven
with a real, repeatable probe, not assumed: the same pt-BR TTS sentence
("Roda o comando pnpm tauri build... jarvis-desktop.exe... target/release"),
transcribed twice with `ggml-small.bin`. Unprimed, whisper mangled every
proper noun it did not recognise — `tauri build` became "talibiu",
`jarvis-desktop.exe` became "jarvisifn desktop.errisse". Primed with
whisper.cpp's own `--prompt` argument, built from the shared baseline (this
product's own tools and name), the project's current branch, and its
top-level file and directory names (a single non-recursive `read_dir`, not
a full-tree walk — see rule 9 in HANDOFF on what a slow repo-wide scan
costs here), `jarvis-desktop.exe` came back **exactly right** and `Tauri`
was recognised on its own. That gap, measured on the *same* audio, is the
whole argument for local whisper.cpp over a generic dictation tool: this
product already knows a project's own vocabulary, and nothing else speaking
into this terminal does.

**The transcript is typed, never submitted.** `voice_stop_recording` calls
`session::typing::type_text` and stops there — no `submit`. It lands in the
prompt exactly where a person's own typing would sit, for them to read, fix
or discard with Ctrl+C, never auto-run. The paced/chunked write itself was
extracted out of `autopilot::driver::send` into `session::typing` for this
— D16's "typed, not pasted" finding was never autopilot-specific, and
voice dictation needed the identical protection against the same line-editor
character loss.

**A microphone cannot be tested on this machine — it has none.**
`cpal::default_host().default_input_device()` returns `None` here,
confirmed, not assumed (no recording device appears in Device Manager
either — the machine genuinely lacks one, not merely a blocked one). Every
part of the pipeline downstream of a captured buffer was verified for real:
the resample/downmix math has direct unit tests, and the full
record → resample → WAV → whisper-cli → type-into-terminal path was run
end to end using a temporary, fully-reverted bypass that fed the pipeline
one second of silence instead of a live device (see the capture module's
git history / HANDOFF for the exact revert). It worked — whisper.cpp
hallucinated `[MÚSICA DE FUNDO]` on the silence, in Portuguese, correctly
matching the pt-BR locale mapping, and it landed unsubmitted in the real
terminal prompt. What was **not** verified, because it cannot be on this
hardware, is a real human voice through a real microphone. That is the one
step handed back — see HANDOFF.

**Model integrity is checked like the updater checks its own artifacts.**
`voice::model::download` writes to a `.part` file, hashes every chunk as it
streams, and only renames into place if the SHA-256 matches a pinned
constant — the same bar §62 already holds signed installer artifacts to,
applied here because nothing else verifies a ~490MB file pulled from
Hugging Face on first use. A hash mismatch deletes the `.part` and reports
failure; nothing `is_present` would trust is ever left behind.

**Addendum, after a real headset test surfaced two real bugs.** Alan
connected a real microphone and dictated live: the feature worked and a
correct transcript landed unsubmitted in the prompt, closing the one gap
above (a real human voice through a real microphone). That same test
surfaced two problems, both root-caused and fixed rather than patched
around:

*The "irritating high-pitched sound" was never this feature's own audio* —
nothing in §54 played a sound before this addendum. A real session log,
inspected byte-for-byte, showed literal BEL (0x07) bytes sitting in the PTY
output stream right where typed text landed. The cause is PSReadLine's
default `BellStyle: Audible`: it writes a BEL for ordinary line-editor
redraws, which happens on any long or fast-arriving input, not something
specific to dictation. Fixed at the source — `session::commands::default_shell`
now launches PowerShell with `-Command "Set-PSReadLineOption -BellStyle
None"` — rather than trying to filter or intercept the byte downstream,
since xterm.js (v5) has no bell playback of its own to disable in the first
place; whatever played the sound was Windows' own console host reacting to
the byte before this app ever read it back out of the pty. Verified live:
launching PowerShell with the exact args the app uses and reading back
`(Get-PSReadLineOption).BellStyle` returns `None`, and a fresh session's log
shows no more bare BEL bytes after typing through it. This only changes
shells jarvis itself spawns — a person's own `$PROFILE` is untouched.

*The Claude-Code-specific garbling had nothing to do with voice or
whisper.cpp at all* — it was a latent bug in `session::typing::type_text`,
shared by dictation and the autopilot (D16), that dictation's own Portuguese
accents simply had the bad luck to trigger. The chunker split text into
48-byte pieces by raw offset, for the pacing item 11 in HANDOFF documents;
a raw byte offset can land inside a multi-byte UTF-8 character, and an
accented letter split across two writes 30ms apart is exactly what "ção",
"informação" and similar words produce. A plain shell's line buffer happens
to reassemble the two halves silently; Claude Code's own TUI decodes each
PTY read independently and renders the invalid half as replacement-character
garbage instead — which is the entire explanation for "works in a normal
terminal, breaks specifically in Claude Code" with no other moving part
involved. Fixed by walking a chunk boundary back to the nearest real
character boundary (`char_boundary_chunks` in `session/typing.rs`) rather
than trusting a fixed byte count. This was proven against the actual
regression, not just the isolated chunker: a real-PTY test spawns the
genuine `claude` CLI, clears its own first-run "trust this folder?" prompt,
types an accented Portuguese sentence through the real `type_text`, and
reads the captured PTY bytes back — the accented words render intact. That
test is `#[ignore]`d by default (it needs `claude` on `PATH`, which CI
cannot promise) but is real, not a mock, and was run by hand to confirm the
fix against the exact reported symptom before considering it closed.

**Also added in the same pass, not yet re-verified by ear:** soft two-note
start/finish chimes, synthesized with the Web Audio API
(`surfaces/voice/sound.ts`) rather than shipped as audio files — this app's
CSP has no `media-src`/`data:` allowance for audio, and an oscillator needs
neither. Both cues are pure sine tones with gain ramped up and back down
rather than switched, since an oscillator toggled at full volume produces
an audible click from the waveform discontinuity — a small version of the
exact harshness this was written to avoid. The start chime is explicitly
awaited to finish *before* the microphone opens (`useVoice.startRecording`
awaits `playStartChime()` then an explicit `CHIME_DURATION_MS` pause before
calling `voice_start_recording`), so a recording never captures its own
cue and transcribes it back — the same self-recording risk a chime-after-open
ordering would have reintroduced. Tuned by eye against the numbers, not by
ear against a speaker; Alan is the one who can actually confirm these sound
right, which had not happened as of this addendum.

**What is next for §54, from Alan's own follow-up in the same
conversation:** real-time streaming transcription — text appearing
incrementally while speaking, VS Code/Cursor-style, rather than today's
record-the-whole-utterance-then-type-once flow, with a deliberately
designed animated treatment for text arriving live. Investigated but not
built: whisper.cpp's own GitHub release (`b4938`) ships `whisper-server.exe`
alongside the already-bundled `whisper-cli.exe` — an HTTP server that loads
the model once and stays warm across requests, with `/inference` accepting
`prompt` and `language` fields per call, which is what would make repeated
polling of a rolling audio window cheap enough versus `whisper-cli`'s
pay-the-model-load-cost-every-time shape. whisper.cpp's own `stream`
example was also examined and rejected: it needs SDL2 for its own
microphone capture, which this codebase does not want since `cpal` already
owns capture here. Nothing about this has been built yet — see HANDOFF §7.

---

## D30 — The Global Search backfill is a background task with a bookmark, not a migration

D25 shipped Global Search forward-only: `session::transcript::mirror` indexes
conversation content as it arrives, so anything said in a session recorded
before that build is on disk in the session log and absent from
`session_events`. The user-facing shape of that gap is worse than a missing
feature — searching for something you know you said returns an empty list
that is indistinguishable from "no match".

**The obvious place to fix it is a migration, and that is the wrong place.**
A migration runs inside one transaction with its own version record, on the
startup path, before the window exists. Walking every session log on the
machine is unbounded work over on-disk data of unknown size, and a failure
partway through is precisely the situation rule 9 in `docs/HANDOFF.md` was
written about — a database that already recorded a version it does not
actually have. There are 42 session directories on this development machine
and no reason to believe that is the ceiling.

So the split is: **migration 10 adds a column and nothing else** — the
bookmark, `sessions.events_backfilled_at` — and `search::backfill` does the
walk afterwards, on its own thread, five seconds after launch, one session
at a time with a rest in between. The first seconds after launch belong to
the window, the project list and any session being restored; search is
simply more complete a moment later than it was. A backfill that saturates
the disk to finish four seconds sooner is a worse product than one nobody
notices running.

**Rows go in chunked, because the database is one connection behind one
mutex.** `db::Database` is deliberately a single connection (its own doc
comment says why), so a transaction held for the length of a whole session
is a stall in every other surface. 500 rows per transaction holds the lock
for milliseconds at a time instead.

**Idempotence is the load-bearing property, not a nicety.** A session is
backfilled inside a delete-then-insert, and its bookmark is stamped *last*.
A process killed mid-session therefore leaves a NULL bookmark and half its
rows, and the next launch clears those rows before writing again — so the
answer to "what happens if this dies at the worst moment" is "it is redone",
not "it is doubled". The FTS index has to be cleared alongside the table or
every result from that session appears twice, which is the specific mistake
`an_interrupted_backfill_is_redone_without_doubling_anything` exists to
catch: it reproduces exactly that crash (run, then blank the bookmark, then
run again) and asserts on both the row count and the number of search hits.

**A session created from this build on is stamped at insert time.** Its live
transcript tailer already indexes it, so leaving it NULL would mean the
backfill re-reads a log whose contents are already in the table. NULL means
"recorded before search existed", and that is never true of a new session.

**A session whose log directory is gone is stamped too, not skipped.** There
is nothing to index and there never will be, so leaving it pending would
mean reopening the same question on every launch for the rest of the
product's life.

### The reader this needed

`SessionLogReader::read_from` returns a `Vec`, which is right for a
projection reading a bounded tail and wrong for a one-time pass over every
session ever recorded: a real agent log is overwhelmingly PTY output, so
materialising all of it to pick out a few kilobytes of JSON would allocate
hundreds of megabytes. That is the same mistake `replay_pty` already carries
a comment about avoiding, one caller along.

`for_each_structured` walks frames by header, steps over terminal payloads
without ever reading them, and hands the caller one structured event at a
time. `walking_structured_frames_skips_the_terminal_bytes_entirely` writes
400KB of PTY output around 40 bytes of JSON and asserts fewer than 1KB was
read into memory — the cost, not just the result. Returning `false` from the
visitor stops the walk where it stands, so a caller asked to shut down does
not have to finish a large log first.

### What this does not do

It does not backfill `usage_samples` or `file_changes`. Those have had
writers since long before D25 and are not missing anything; `session_events`
was the one table with no writer at all (HANDOFF §5 item 29).

---

## D31 — Live captions replay a warm HTTP server, not whisper.cpp's own streaming example, and never touch what gets typed

Alan's ask, from the same conversation that closed D29's addendum: dictation
should look like VS Code/Cursor's — text appearing while speaking, not the
record-the-whole-utterance-then-type-once flow §54 shipped with — "com
animação na transcrição", to the same premium bar as the rest of the feature.

**Whisper is not a streaming model.** Every call re-reads its audio from the
start and can revise words it produced on an earlier call once more context
arrives. That single fact drove every choice below.

**What the caption shows is not what gets typed — deliberately, and this is
the load-bearing decision.** Two designs were on the table: (a) a
J.A.R.V.I.S.-owned caption surface that previews live, with the terminal
receiving one complete transcript on stop, exactly as today; or (b) typing
committed segments into the terminal progressively, append-only. (b) reads
closer to Alan's own description, but it means sending corrections —
backspaces, or worse, silently-wrong text nobody asked to revise — into a
live agent CLI's own line editor, which is precisely the class of bug D16 and
item 11 in HANDOFF exist to prevent. (a) was chosen: `voice_stop_recording`
is untouched, still one call to the complete, unstreamed `whisper-cli.exe`
pass, still typed once, still never submitted. Streaming only ever feeds a
preview surface that has no path to the terminal at all. A caption can be
wrong for a second without cost; a terminal cannot.

**A warm `whisper-server.exe`, polled, beats `whisper-cli.exe` run
repeatedly.** `whisper-cli` pays the full model-load cost — measured at
several seconds against `ggml-small.bin` — on every invocation, which rules
out calling it every second or two. `whisper-server.exe` ships in the same
`b4938` release already bundled for `whisper-cli`, loads the model once, and
answers over HTTP for as long as it stays up. It is spawned lazily on first
dictation and kept alive for the whole app session rather than per-recording,
so only the very first utterance in a session pays the cold-start cost.

**The b4938 release tag is not one fixed artifact.** Re-downloading it for
this pass produced a materially different build from the one already
committed for `whisper-cli.exe` — a newer `whisper.dll`/`ggml*.dll` set,
CPU-dispatch variants (`ggml-cpu-alderlake.dll` and friends) in place of one
`ggml-cpu.dll`, and new dependencies (`llama.dll`, `parakeet.dll`) that a
direct test proved `whisper-server.exe` does not actually need — removing
them and confirming `--help` still ran was the empirical check, not an
assumption from the file list. The two builds' same-named DLLs are not
interchangeable, so `whisper-server.exe` and its own matched set live in
`resources/whisper/server/`, never merged into `resources/whisper/` where
`whisper-cli.exe` already lives. Worth knowing before assuming a version tag
pins bytes.

**The actual HTTP contract was reverse-engineered against the real binary,
not recalled from memory.** `whisper-server.exe --help`, run for real,
supplied the flags (`--host`, `--port`, `--model`); the request shape
(`multipart/form-data`, a `file` field, `response_format`, `language`,
`prompt`) was confirmed by starting the server against the real downloaded
model and posting a synthesized pt-BR sentence through `curl.exe`, reading
back real segment/word timestamps and a `detected_language_probability`
before any Rust was written. `response_format=json` (plain `{"text": "..."}`)
was chosen over `verbose_json` deliberately: the word arrays `verbose_json`
adds are whisper's own **sub-word tokens** — the sample response split
"falo" across two word entries — which is the wrong granularity for anything
comparing words across polls. The assembled top-level `text` field is
already correctly spaced and punctuated, so whitespace-splitting it is safe
where splitting the word array would not be.

**Full buffer, adaptive cadence — not a trailing window.** Every poll sends
everything captured so far, from the start, and the next poll fires only
after the previous one returns (with a floor sleep so an empty buffer does
not spin). A trailing window was the other real option, and was rejected for
v1: whisper's own hypothesis for a window boundary shifts as the boundary
itself slides, which breaks simple prefix comparison between polls and
demands tracking audio-time offsets to realign words across windows. Full
buffer keeps every hypothesis anchored at sample zero, so two consecutive
polls' word lists are directly comparable. The honest cost: latency grows
with utterance length, since whisper re-reads from the start every time —
measured at roughly 0.56x realtime for `ggml-small.bin` on this machine's
CPU, so a 20s utterance costs an ~11s poll. Adaptive cadence means that never
compounds into a backlog; a long dictation just updates less often, which is
a graceful degradation for the realistic case here (a sentence or two spoken
into an agent prompt, not continuous long-form dictation).

**Committing text needs two polls to agree, not one, and never retracts.**
`voice::stream::AgreementState` is a simplified LocalAgreement policy: a word
enters the committed caption only once it sits in the common prefix of the
current hypothesis *and* the previous one, and once committed it is never
removed even if a later poll disagrees. One poll's guess is not evidence; two
consecutive polls landing on the same words is. Never retracting is the same
philosophy as the terminal-typing decision one level up — a caption a person
already read must not un-say itself, even at the cost of an occasional
committed word that turns out to have been wrong (harmless here, since the
final typed text never reads from this state at all).

**The entrance animation needed no state of its own.** `committed` only ever
grows, so `LiveCaption` renders it as one `<span>` per word keyed by array
index — React mounts a fresh span only for a newly-agreed word and leaves
every earlier one alone, which is what makes the CSS entrance animation play
exactly once, on the word that just settled, with no manual "what changed"
tracking anywhere in the component.

**Verified live, not only in unit tests — and it caught a real bug.** This
development machine still has no microphone (see D29), so the full
command → poll-thread → `Channel` → `LiveCaption` path was driven through
the real, installed app with a temporary synthetic-audio generator standing
in for `cpal`'s device stream — same shape as D29's own silence bypass,
fully reverted afterward, never gated on anything a real install would set.
It worked: a real commit/tail split rendered on screen (`whisper-server`
hallucinated `[Música]` on the synthetic tone; two consecutive matching
polls committed it, and a third poll's slightly different capitalisation
showed up correctly as a *new*, uncommitted tail rather than overwriting the
settled word), and the final `whisper-cli` pass still typed its own,
separate result into the terminal on stop, unsubmitted, exactly as without
streaming. That same pass caught a real bug before it shipped:
`whisper-server.exe` survived a `taskkill` of the whole app. Unlike the
agent-CLI children `pty::spawn` contains (see `pty::job`, next to D6/D7),
`ServerHandle` had never been assigned to a Windows job object with
`KILL_ON_JOB_CLOSE` — it was kept alive for a whole app session, not scoped
to one PTY, and nothing carried the same guarantee. Fixed by giving
`ServerHandle` its own job, exactly the mechanism `pty::spawn` already
trusts, and reverified by force-killing the app again and watching
`whisper-server.exe` die with it. `pty::job` moved from `pty`-private to
`pub(crate)` to make that reuse possible — a one-word visibility change, not
a rewrite of anything D6/D7 depends on.

**What is still open:** a repeat verification with a real human voice
through a real microphone has not happened on this build — the ear test D29
already owed (see HANDOFF §7) covers the sound cues and the two dictation
bug fixes, not this feature, since it did not exist when that debt was
recorded. Both are now owed together.

---

## D32 — Preview is a separate window pointed at loopback, and the URL comes from the session's own output

M8's roadmap entry was a bare heading with no specification, so the first
decision was what Preview *is*. The answer that shaped everything else: it is
**not a browser in a tab**. The loop §46 exists to close is
ask → modify → run → **see** → inspect → fix, and every step but *see*
already existed here — an agent edits files (§41/§42), runs a dev server in a
real terminal (§21), and the diff is in Review (§43). What was missing was
looking at the result, which meant leaving the application.

A browser embedded in the app would technically close that loop and would add
nothing this product knows. **What it knows is which session started the
server**, because the PTY output is already in the log (§23). So `detect`
reads the URL out of the same stream the terminal is drawing: no port to
configure, no setting to forget, and no chance of previewing a different
project's server that happens to be running.

### Reading output, not watching ports

Enumerating listening sockets was the alternative. It finds *a* server and
cannot say which one this session started, and a developer's machine usually
has several — a stale `next dev`, a database, something a colleague's script
left running. The output is the honest source: this text was printed by this
session's own process. Ports 5432/3306/6379/27017/9229 are excluded anyway,
because a Postgres banner has the same shape as a dev server's and is not a
web page.

### A separate window, not an iframe

The iframe is the obvious first choice and does not work: this app's CSP is
`default-src 'self'`, so a `localhost:5173` frame is blocked outright.

The important part is what *not* to do about that. Widening the CSP to allow
it would put the dev server's page — code the agent just wrote, or a
dependency it installed — in a context adjacent to the surface that can invoke
every Tauri command in this application. That is a real escalation in exchange
for a layout convenience. A separate `WebviewWindow` is its own webview with
its own (empty) capability set, which is the correct boundary, and it happens
to give people what they actually want anyway: the preview *beside* the
editor rather than squeezed inside it.

### Loopback is a security boundary

Preview renders whatever it is pointed at inside a window this application
owns. Terminal output is **not trustworthy input** — a file an agent prints, a
dependency's postinstall banner, an error message quoting a URL. If any string
in that stream could choose what Preview displays, then any program a session
runs could.

So `detect` only ever offers loopback, and `preview_open` **re-checks the URL
it is handed** rather than trusting the webview that sent it: the renderer got
it from `preview_detect` in the ordinary case, but a command that opens a
window must not depend on its caller having been careful. It parses with
`url::Url` rather than matching substrings, and applies the *same*
`is_loopback_host` the scanner filtered with — two spellings of "is this
local" that disagree is exactly how a check gets bypassed.
`127.0.0.1.evil.com` and `localhost.evil.com` are both in the test.

### Nothing opens by itself

Detection is automatic; opening is always a click. An agent restarting a dev
server must never yank a window onto someone's screen, and a surface that
opens things unbidden is one people learn to distrust. The same reasoning as
§54's "never auto-submitted".

### Reload exists because hot reload is not universal

A dev server with HMR updates itself and the button is redundant. One without
it — a static server, a Rust binary, a Python app — does not, and without a
reload the preview silently shows the previous version. That is worse than no
preview: it is a preview you cannot trust. The button is the difference.

---

## D33 — A preference is validated in the core, and "unset" is the absence of a row

§64 asks for an audit of the settings scattered through the product. What it
found was not scattered settings — it was the opposite. Three values the
product used, **showed on screen**, and gave nobody a way to change:

- the global and project levels of the autonomy chain (§33), against which
  Mission Detail had been rendering the word "Inherited";
- the autopilot turn budget, which `AutopilotPanel` renders as "turn 3 of 24"
  while `DEFAULT_TURN_BUDGET` was a constant read in three places;
- the terminal's type size and scrollback depth, hard-coded at 13px and
  20,000 lines.

None of these is a new feature. Each is the missing half of something already
built, which is the bar for going beyond the roadmap.

### Validation belongs in the core

Every one of these is bounded, and the surface renders a slider — so an
out-of-range value should be impossible. **That is exactly why it is not
trusted.** A `#[tauri::command]` is reachable from the webview, which means it
is reachable from any bug in the webview, and a preference that can be set to
zero is a run that fails before it starts.

The two directions are deliberately different:

- **On the way in, out of range is refused.** A stored value is then always
  one somebody could have chosen, and the database never holds a number the
  product would have to defend against later.
- **On the way out, out of range is clamped.** An older build's row, a
  hand-edited database, or a bound that has since tightened must not produce
  an unusable terminal — and `settings_set_preference` already refused
  anything new, so a bad row means history rather than a live mistake.

The key list is closed for the same reason: a typo in the webview should not
be able to write an arbitrary row into the settings table.

### The bounds are product decisions, not round numbers

Below **4 turns** an unattended run cannot finish anything real, so every
mission would end in `Failed` — a setting whose only effect is to break the
feature is not a setting. Above **100**, "run until done" stops being a budget
at all, and §34's rule against consuming resources indefinitely is the whole
reason the number exists.

### A run keeps the budget it started with

`turn_budget` is read once, when the driver's thread starts, and held. Reading
it each turn would let a change in Settings move the finish line under a run
already in progress: a mission could pass the budget it began under and fail
on a number nobody ever applied to it. A budget is part of the terms a run
started with; the next run gets the new value.

### Unset is the absence of a row

`clear` deletes rather than storing an empty string or a sentinel, so
`Option<T>` from a read means exactly what it says and no reader has to know
which flavour of empty it is looking at. `set_global_autonomy` had already
made this choice; the accessor makes it the rule.

And **a value that will not parse reads as unset**, never as an error. A
preference that cannot be understood is a preference nobody chose, so the
default applies. The alternative — propagating the failure — would let one
corrupt row stop the whole settings screen from rendering, which is a much
worse outcome than silently using the value the product would have used
anyway.

### Why an accessor at all, for two call sites

`mission::store` and `onboarding` each wrote their own SQL against `settings`
and both were correct. Two duplicates is not a problem; the *shape* is,
because §64 adds more, and each new site re-decides how unset is spelled and
what a malformed value means. They drift quietly, because nothing forces them
to agree.

The existing call sites are deliberately **left alone**. They work, they are
tested, and rewriting them to prove a point is churn — §9 asks for an audit of
inconsistency, not for every duplicate to be hunted down the day it is found.

### Preferences apply in place

Changing the terminal's type size or scrollback never rebuilds the terminal. A
rebuild kills the process running in it and throws away the scrollback — the
same constraint that shaped split panes (§20). Both are live `options` on
xterm, so they are simply assigned. The font size also changes how many
columns fit, so the view is refitted and the PTY told; without that the shell
keeps wrapping at the old width and every long line breaks in the wrong place.

Verified by changing the size against a *running* shell: the type grew, the
scrollback was still there, and the next command ran in the same session.

---

## D34 — Uma conta de agente é um config dir; continuidade automática é um relé pelo Brain

**Decisão:** contas Claude Code e Codex são diretórios persistentes e isolados,
selecionados por `CLAUDE_CONFIG_DIR` e `CODEX_HOME`. A conta já presente na
máquina é adotada com a variável ausente; nenhuma credencial é copiada ou
reescrita. O isolamento de estado e transcripts foi comprovado contra os dois
CLIs reais antes de ligar a feature às sessões.

Uma sessão guarda a conta com que nasceu. Troca manual afeta somente sessões
novas; tentar reautenticar um processo vivo seria mentir sobre o que ocorreu.
Troca automática também inicia um processo novo, mas religa a run de autopilot:
o estado persistido da missão, um novo brief do Brain e
`plan::opening_instruction` reconstituem o contexto. O driver antigo para de
dirigir e a sessão antiga permanece viva para inspeção ou tomada manual.

`--resume` foi rejeitado deliberadamente: o transcript da conta A não existe no
config dir da conta B. Copiar um arquivo interno do provedor entre contas seria
frágil e faria o produto manipular um formato que não possui. O relé nunca copia
transcript, preserva orçamento/progresso da run e checa antes a confiança da
pasta na conta destino.

**Cotas:** nenhum endpoint HTTP lembrado entra no desenho. Claude Code fornece
uma recusa e seu reset, não um medidor ao vivo; tokens são Observed, uma
porcentagem calibrada por recusas é Estimated e a ausência de franquia é
Unknown. Codex fornece percentuais/reset Official e mantém as duas janelas.
Confiança cruza o limite Tauri junto de todo número e muda a apresentação: barra
Official sólida, Estimated hachurada, Unknown sem barra. Um reset já passado
ainda ancora o cálculo histórico, mas não aparece como countdown atual.

**Capacidades:** `account_switching` só passou a `true` nos adaptadores depois
que registro, sessão, transcript, troca, relé, paridade Codex e superfície
estavam implementados e testados.

---

## D35 — A notification is raised only for what the person is not already looking at, and a question read off a terminal is Observed, never Official

The ask was ORCA's behaviour: be told when any agent stops working, whether it
finished or is waiting for a decision, with a preview of what it wants.

**The rule is the feature.** `TurnEnded` fires on every turn, and a person
sitting in front of a terminal sees one every minute or two. A product that
raises each of those is unusable inside ten minutes, and — worse — it teaches
the reader to dismiss the toast that actually mattered. So the core suppresses
anything about a session the person is currently watching, where *watching*
means the window has focus **and** that session is on screen. Both halves are
required: a visible terminal in a window behind something else is not being
watched, and a focused window on Mission Control is not watching anything.

A suppressed notification is **dropped entirely**, not stored and marked read.
That makes the notification centre a list of things you *missed* rather than a
transcript of things you watched happen, which is what makes it worth opening.
What happened, permanently, is `activity` (§48) — and the two bars genuinely
disagree in both directions: a finished turn is worth a notification and is
deliberately not worth an activity row, while a quota threshold crossed at 3am
is worth a row and is not worth waking anybody.

**The decision lives in the core, not in the surface.** The surface knows what
is on screen so it keeps `Attention` updated; but "is this worth raising" is
answered in one place, at the moment of raising, and is tested. Splitting it —
core stores, surface decides — would have meant hundreds of rows per session
and a race between the event and the frontend's idea of what was visible.

### The gap that made this a milestone rather than a wiring job

Everything J.A.R.V.I.S. already knew about a stopped agent came from something
a provider *stated*: a finished turn in the transcript, a guardrail decision,
an autopilot stop reason. But **the most ordinary "waiting for you" moment of
all is stated nowhere.** `guardrail::guard` returns early for any tool that is
not `Bash`, and again for a `Bash` command that classifies as nothing
sensitive; the transcript records the answer, never the question. Claude Code
asking whether it may write a file exists only on the screen — and the session
log already holds every byte of it (§23).

So `notify::detect` reads it off the screen, and labels it **Observed** rather
than Official. Same discipline as usage figures (§28): the confidence travels
with the fact, and a reading is never presented as a statement.

### What the captures actually showed

`notify::capture` spawns real agent CLIs in real PTYs and records every byte.
Four captures against Claude Code 2.1.241 and Codex 0.149.1. Three findings,
none of which would have come from reasoning about it:

**The harness had to stand in for a terminal.** The first run captured exactly
four bytes — the cursor-position query `ESC [ 6 n` — because Claude Code draws
nothing until something answers it. D6's trap, met from the other side.

**The harness inherited the session running it.** The second run captured an
agent in auto mode that never asked anything: it had picked up
`CLAUDE_CODE_CHILD_SESSION` and friends from the Claude Code session running
the test. Stripped now, permission mode passed explicitly.

**These TUIs emit cursor-forward escapes where a person sees a space.** A
conventional `strip_ansi` — the one `preview/detect.rs` already has — turns
`Do you want to create hello.txt?` into `Doyouwanttocreatehello.txt?`. Every
pattern written against the readable form matches nothing at all. Hence
`notify::render`, which translates cursor-forward into that many spaces and
absolute cursor moves into line breaks. It is not a terminal emulator and must
never become one: it answers one question, which is what words are on screen.

### Why the detector keys on shape, not on wording

The four captures phrase themselves completely differently — `Do you want to
create hello.txt?`, `Do you want to proceed?`, `Choose the text style that
looks best with your terminal` (no question mark, no footer), and Codex's own
`Do you trust the contents of this directory?` with a different cursor glyph.
One thing survives all four:

> a numbered choice list, of which exactly one row carries a cursor glyph.

That is the whole test. The wording is used only to write the preview. Matching
on "Do you want" would have missed the theme picker and would break the day a
provider rewords a sentence, which providers do and are entitled to.

A plain `>` was in the glyph set and was removed. It is the quotation marker in
every markdown file and diff on earth, so a quoted numbered list — an entirely
ordinary thing for an agent to print — parsed as a live selection with the
first row chosen. The quiet conjunction hides that mid-turn and does not hide
it when such a list is the last thing on screen as a turn ends.

**A match alone is never enough.** A notification that says an agent is waiting
when it is working is worse than no notification. So `watch` requires the match
to hold *and* the terminal to have been quiet for 1.4s *and* nothing to have
been typed since. A list that scrolls past inside a turn fails the second; an
answered question fails the third.

### What the Windows toast can and cannot do

Verified against a real installed build **before** the surface was written,
and two of three answers were not the obvious ones:

* **It appears, attributed to us** — our own mark and name — on a machine that
  also carries an unrelated `jarvis.exe` with its own Start Menu shortcut. The
  AUMID comes from the bundle identifier and NSIS writes it onto our shortcut.
* **It does not appear from `pnpm dev`.** The plugin sets the AUMID only when
  the executable is not under `target/debug` or `target/release`, and an
  unpackaged binary has no shortcut to be identified by. A platform fact, not
  a bug to chase.
* **Clicking it does nothing, and cannot.** `tauri-plugin-notification` 2.3.3
  exposes activation callbacks on mobile only. So the desktop toast is an
  **alert**, and the in-app centre is the thing you click. Built the other way
  round, this would have shipped a toast that looks interactive and is not.

### Two things deliberately not raised

A guardrail set to *ask* is not raised: Claude Code then draws its own question
on the terminal, which the watcher already sees with a better preview. A
completed unattended run is not raised: `mission::store::set_status` has
already announced the completion it caused. Both would have been one event
notified twice, which is the noise that makes people stop reading.

### `notify::bus` is a global, and almost nothing else here is

Notifications are raised from the transcript tailer, the terminal watcher, the
guardrail decision log, a mission changing status and a run giving up. Half are
background threads several calls below any command handler, and none of them is
*about* notifications. Threading a database handle, an `Attention` and a webview
channel through all of that would have put this feature's plumbing into
`mission::store`'s signature. Not installed is a no-op, so tests and the
guardrail hook subprocess never have to care.

### The preview is the agent's own words, and is not translated

`kind` and `reason` are stable identifiers the surface localises (§65).
`preview` is the exception, deliberately: it is what the agent wrote on its own
screen or in its own reply, and translating an agent's question would be
inventing one. It is the only untranslated string the surface shows, and it is
the one worth reading.
