# M15 — Session History (§88)

> **Status: in progress.** This file is the working record: what was measured,
> what was decided and why, and what is left. It is written the way
> `M13-ACCOUNTS.md` and `M14-NOTIFICATIONS.md` are — findings first, because
> the findings are what the design rests on.

## 1. What is being built, and what it is not

Every session this product has ever run, reachable from one place: searchable,
titled, grouped by when it happened, openable read-only, renameable, and
deletable. The reference Alan gave is the session list in VS Code's Claude Code
extension. The bar is to be better than it, not to copy it.

**Where it is already different, deliberately:**

| VS Code's list | Here |
|---|---|
| Only the current workspace | Every project on this machine, filterable to one |
| Searches **titles** | Searches **what was actually said**, through the FTS5 index §51 already built, and shows the matching line |
| One provider | Claude Code, Codex and plain shells, side by side |
| A title and a relative time | Title, project, provider, state, duration, turns, tokens, and the mission it was working on (§86) |
| Delete removes it from a list | Delete removes the row, its search index rows **and** its log directory, and says how many bytes that frees |

**Not built, deliberately (§81):** the screenshot's **Web** tab. Those are
Claude's *cloud* sessions. This product is local-first (§3) and does not have
them; a tab that is empty, or that signs the user into somebody's cloud, is
worse than its honest absence.

## 2. The gap, stated exactly

Two facts about the code as it stood, both verified by reading it:

1. **`sessions.title` was always `NULL`.** The column has existed since
   migration 1. A repo-wide search for a writer found none — only three readers
   (`session_list`, `mission_sessions`, and the `SessionInfo` struct). Every
   session in the database is untitled.
2. **`session_list` filters `ended_at IS NULL`.** That is correct for what it
   is — the query the project workspace adopts live tabs from — and it means a
   closed session is unreachable from anywhere in the product except by
   stumbling on one of its own sentences in Global Search.

So there was no history, and there was nothing to label a history row with.
Titles are the load-bearing half and were built first.

## 3. Findings — measured on this machine, not assumed

### 3.1 Claude Code writes its own AI-generated title

Real, and it is the best thing in this feature. Verbatim from a transcript on
this machine:

```json
{"type":"ai-title","aiTitle":"hello.txt file creation","sessionId":"a7e2f226-…"}
```

**89 of the 124 Claude Code transcripts here carry one.** The line has no
`timestamp` and no `uuid`, which matters for the parser: it cannot be
timestamped from its own content.

`providers::claude::is_internal_noise` already listed `"ai-title"` — as **noise
to drop**. That was right for Conversation View, which is what it was written
for (§24: a transcript is full of machinery). It is exactly wrong for a title,
so the title is lifted *before* the noise filter runs, rather than by loosening
the filter.

### 3.2 Codex does not

`set_thread_title` appears in exactly one of the 48 Codex rollouts here, and it
is a **tool definition inside the instructions**, not an event Codex emitted:

```
"name":"set_thread_title","description":"Rename a Codex thread in the background."
```

No rollout on this machine records a title for itself. Enumerating every
`"type"` across a day of rollouts confirms it — there is no title event.

This is a genuine capability difference and it is expressed as one (§26), not
papered over: `TitleSupport::Provider` for Claude Code, `TitleSupport::Derived`
for Codex, `TitleSupport::None` for a plain shell. A test fails if the two
providers ever describe themselves identically.

## 4. Decisions

### D36 — A title has a source, and a person's rename outranks a machine's

Three sources, precedence **user > provider > derived**:

| Source | Where it comes from |
|---|---|
| `user` | Somebody renamed it here. Nothing ever overwrites this. |
| `provider` | The provider named it itself — Claude Code's `ai-title`. |
| `derived` | The first thing the person typed in that session, truncated. |

Stored as a `title_source` column beside the title, and rendered as a label, for
the same reason a usage figure carries its confidence (§28): a title Claude Code
wrote and a title we cut out of the first sentence are not the same claim, and a
surface that shows them identically is asserting something it does not know.

A provider title arriving later must never clobber a rename — a person who names
a session and watches the name change ten seconds later has been told the
product does not respect their input.

### D37 — The title travels through the log, like everything else

`ConversationItem::Title` is a new variant, logged as a `Lifecycle` frame — it
is a fact about the session, not something anybody said, and filing it as a
`Message` would put a stray bubble in Conversation View. It goes through the log
because §23 means everything does: the same frame that updates the row is on
disk, so a title survives a database rebuilt from the logs.

### D38 — Backfill reads the index, not the logs

D30's backfill had to walk every session log because the content it wanted was
only there. This one does not: `session_events` already holds every user message
of every session on this machine, put there by that same backfill. A derived
title is therefore one SQL statement per session, not a filesystem walk.

Old sessions get a *derived* title only. Their `ai-title` is still in Claude
Code's own transcript on disk, but our log never recorded it — and re-reading
124 provider transcripts to recover titles for sessions that already have a
usable one is work bought at a price out of proportion to it. Stated here so it
is a decision rather than an oversight.

### D39 — Delete is the whole session, and nothing else pretends to prune

`session_events_fts` is a **standalone** FTS5 table, not `content=`-linked
(migration 9's own comment says why: `session_events` is `WITHOUT ROWID`).
So there is no trigger and no cascade. `ON DELETE CASCADE` clears
`session_events`; the FTS index would keep its rows forever, and Global Search
would go on returning hits for a conversation that no longer exists. Delete
therefore removes, explicitly and in one transaction: the FTS rows, the
`session_events` rows, and the `sessions` row (cascading `usage_samples` and
`file_changes`), and then the `sessions/<id>/` directory on disk.

And that is *all* the pruning there is. HANDOFF item 38 flags automatic
retention as a decision nobody should invent casually — the log **is** the
record (§23). What this surface adds is the missing half of that: it shows how
much disk the logs actually take, so the person can decide. No background
pruner, no age limit, no "clean up" button that decides for them.

A live session cannot be deleted. Taking an agent's log out from under it is not
a delete, it is a crash.

## 5. Plan

- [ ] Migration 12 — `sessions.title_source`, a global `sessions (created_at DESC)`
      index (the existing one is `(project_id, created_at DESC)` and does not
      serve a cross-project query), and the derived-title backfill bookmark.
- [ ] `TitleSupport` in the capability model, and the §26 test.
- [ ] `ConversationItem::Title` + `claude::parse_line` lifting `ai-title`.
- [ ] `session::title` — precedence-respecting write, derivation, backfill.
- [ ] `history` — list (keyset-paginated), search (FTS), rename, delete, storage.
- [ ] The surface: rail entry, grouped rows, search, filters, inline rename,
      delete confirmation.
- [ ] Both i18n catalogues.
- [ ] Verified in a real build, by looking at it, in both themes.
