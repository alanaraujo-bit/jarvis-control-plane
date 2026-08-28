# M25 — Persistent multi-project workspace orchestration

## Implementation status

Implemented in the desktop application:

- attachment ownership and conditional stale-detach protection;
- an atomic log barrier joining replay to live output without a missing span;
- a canonical workspace store backed by the local SQLite settings table;
- durable project dock, active area, terminal selection, ordering and split;
- global navigation and Back hide workspaces instead of discarding them;
- background projects have no mounted xterm/WebGL tree while their Rust
  sessions continue.

Cross-project side-by-side rendering and an out-of-process session supervisor
remain optional follow-up milestones. The dock already supports simultaneous
work across multiple projects without requiring either feature.

## Outcome

J.A.R.V.I.S. must behave as a workbench, not as a sequence of disposable
pages. Opening a project creates a persistent project workspace. Global
navigation may cover that workspace, but must not close it. Returning to a
project restores the exact area, terminal, view and pane layout the person was
using without requiring another project-tree lookup.

This milestone deliberately separates three kinds of state:

1. **Process state** — the PTY and agent continue in the Rust core.
2. **Workspace state** — which projects, sessions, areas and layouts are open.
3. **Presentation state** — which workspace or global surface is visible now.

No navigation action may be mistaken for a process lifecycle action.

## Current failure model

### Only one project can exist in the presentation model

`App.tsx` owns a single `openProject: Project | null`. The Back button and
every global rail destination set it to `null`. The keyed `<main>` then
unmounts `ProjectWorkspace`, including every `TerminalView`. The core session
usually survives, but the user's workbench does not.

This model cannot represent two open projects. Opening project B necessarily
replaces project A, so there is nowhere for A's active area, focused session or
layout to remain.

### Workspace details are component-local and disposable

`ProjectWorkspace` keeps `area`, `visited`, `view`, focus requests and popover
state in component state. `useTerminals` keeps tabs, active tabs and split
slots only in an in-memory frontend store. None of this is a durable workspace
record, and remount/adoption chooses the first live session rather than the
session the person was using.

### Attach/detach has a stale-detach race

The Rust `LiveSession` stores one optional UI channel. `session_attach`
replaces it, while `session_detach` clears it unconditionally. During a rapid
unmount/remount, this ordering is possible:

1. old view begins asynchronous detach;
2. new view attaches and becomes the current sink;
3. old detach reaches Rust and clears the new sink.

The new terminal remains mounted but receives no output. Closing or remounting
another tab may appear to repair it by attaching again.

There is also a replay gap: `TerminalView` first calls `session_replay` and only
then calls `session_attach`. Output produced between those calls is present in
the log but absent from both the returned replay and the live channel.

### Rendering every terminal does not scale to multiple projects

Within one mounted project, all terminal tabs keep an xterm and potentially a
WebGL context alive so scrollback is not lost. Extending that strategy to every
open project would multiply DOM, GPU and resize-observer cost. Process
continuity and renderer continuity must not be treated as the same thing.

## Target interaction model

### Project dock

A persistent project dock sits above the content surface and contains every
open project. Each item shows project name, branch and compact activity state:
working, waiting for input, failed or idle.

- Opening a project that is already docked activates it; it never duplicates
  the project.
- Opening another project adds it beside the existing projects and activates
  it.
- Clicking a docked project returns directly to its last area and session.
- The Back button opens the Projects surface without closing the workspace.
- A separate close action removes a project from the dock after explaining
  that its running sessions continue. Stopping sessions remains an explicit,
  separate action.
- Global rail destinations temporarily become the visible surface while the
  dock remains available. Returning is one click.

The first milestone switches between project workspaces. Side-by-side projects
are a later layout option; they must reuse the same workspace model rather than
introduce a second navigation system.

### Sessions inside a project

Session tabs and pane layout are independent concepts:

- `sessionOrder` controls the tab strip;
- `activeSessionId` controls the selected tab and keyboard target;
- `paneSessionIds` controls what is simultaneously visible;
- `focusedPaneId` is always one of the visible panes;
- `viewModeBySession` controls terminal/conversation per session;
- closing a pane does not stop or close its session;
- closing a session tab is the only tab action that requests process closure.

Selecting a tab that is not in the current split replaces the focused pane.
Starting a new session either joins a split with capacity or replaces its
focused pane. An active session must never be hidden.

## State model

The frontend gets a single `WorkspaceStore`:

```text
presentation:
  visible: global(surfaceId) | project(projectId)
  lastGlobalSurface

workspace:
  openProjectIds[]
  activeProjectId?
  projects[projectId]:
    area
    visitedAreas[]
    sessionOrder[]
    activeSessionId?
    paneSessionIds[]
    focusedPaneId?
    splitDirection
    viewModeBySession
```

All transitions go through named actions such as `openProject`,
`showGlobalSurface`, `activateSession`, `openInPane`, `closePane`,
`closeSession` and `closeWorkspace`. Components render this state; they do not
invent parallel navigation state locally.

Durable fields are written through one debounced core command into SQLite.
Ephemeral UI such as open popovers, drag previews and search text is not
persisted. On startup, the core returns the workspace snapshot together with
the current live-session list; the frontend reconciles rather than overwrites
it.

Reconciliation rules:

- remove references to projects that no longer exist;
- retain archived projects only when explicitly reopened through History;
- remove dead sessions from live panes but retain their transcript tabs when
  the snapshot says they were open;
- choose the persisted active session when valid, otherwise the most recent
  valid pane, otherwise the most recent live session;
- normalize every layout so active/focused sessions are visible and pane IDs
  are unique.

## Race-free session view protocol

Every attachment receives an opaque `attachmentId`. Detach includes both the
session and attachment IDs; Rust clears the sink only when the ID still owns
it. A stale cleanup can therefore never detach a newer view.

Replay and live output become one cursor-based subscription rather than two
unrelated calls. Each PTY output frame has a monotonically increasing sequence.
Opening a view requests output after its last rendered sequence and receives an
ordered replay followed by live frames from the same subscription. The UI
deduplicates by sequence, making rapid project/session switching lossless.

The same protocol supports future secondary windows without weakening the
single-keyboard-owner rule. If multiple read-only subscribers are later needed,
the core can fan out frames while accepting input and resize from only the
foreground attachment.

## Performance policy

- PTYs, logging, transcript following and notifications stay in the Rust core
  regardless of which project is visible.
- Only visible panes own attached xterm renderers.
- The most recently hidden renderer may remain hot briefly for instant Back;
  older renderers are disposed and reconstructed from the cursor/log on demand.
- Reattachment replays only the configured scrollback budget, never an entire
  unbounded log.
- Resize events are emitted only for visible panes and are coalesced.
- Background projects never own focus handlers, WebGL animation or active
  `ResizeObserver`s.
- Project/session activity badges are driven by core state, not by mounted
  terminal components.

This gives process continuity without paying the GPU and DOM cost of every open
terminal at once.

## Application restart boundary

Navigation, minimizing and hiding the window preserve live processes. A full
desktop-process exit currently ends the in-process PTYs. M25 restores the
workspace and transcripts after relaunch and offers **Continue** for ended
agents; it does not pretend those processes survived.

True survival across application upgrades or crashes requires a separate local
session-supervisor process with authenticated IPC. That is a follow-up
milestone and must not be mixed into the navigation refactor.

## Delivery order

### Phase 1 — correctness of the transport

- add attachment identities and conditional detach;
- replace replay-then-attach with cursor-based subscription;
- add rapid attach/detach and no-gap backend tests.

### Phase 2 — canonical workspace state

- introduce `WorkspaceStore` and pure transition/normalization functions;
- move project area, active session and pane layout out of component-local
  state;
- add SQLite workspace snapshot and startup reconciliation.

### Phase 3 — project dock and navigation

- render persistent open-project items;
- make Back/global navigation hide rather than close workspaces;
- route Projects, History, Mission Control, notifications and global search
  through the same `openProject` transition.

### Phase 4 — renderer lifecycle and performance

- mount only visible terminal panes;
- restore them through the cursor subscription;
- add a small measured hot-renderer cache if profiling proves it useful;
- expose background activity without mounting terminals.

### Phase 5 — optional cross-project layouts

- allow two project workspaces side by side on sufficiently large windows;
- enforce one keyboard-focused pane globally;
- preserve the same dock and state model.

## Acceptance scenarios

1. Open project A, start Claude Code, visit Settings, and return to A with one
   click. The same session, scrollback, area and input focus are present.
2. Open A and B. Both remain in the project dock. Switching between them never
   recreates a process and each restores its own selected terminal.
3. Open two sessions in A. Clicking either tab always reveals that session;
   neither requires closing the other.
4. Split two sessions, open a third, switch tabs repeatedly and close one pane.
   The active session is always visible and exactly one pane owns the keyboard.
5. Switch A → B → global surface → A while both agents emit output. Every byte
   appears once and in order after returning.
6. Repeat rapid navigation hundreds of times. A stale detach never removes the
   latest attachment.
7. Relaunch the application. Open projects, selected areas and pane layouts are
   restored; sessions that ended with the process are clearly marked and can
   be continued.
8. With many projects and sessions open, background projects have no xterm,
   WebGL or resize-observer cost while their PTYs and notifications continue.

## Required tests

- property tests for workspace normalization invariants;
- reducer tests for every named transition;
- Rust concurrency tests for attachment ownership and output sequencing;
- component tests for active-tab visibility and split replacement;
- end-to-end tests covering two projects, two sessions per project, global
  navigation, rapid switching and relaunch restoration;
- performance budget test recording mounted xterms, active observers, memory
  and project-switch latency.
