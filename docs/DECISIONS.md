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
