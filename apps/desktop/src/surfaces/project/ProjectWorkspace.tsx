import { useEffect, useState } from "react";
import { ChevronLeft, Columns2, GitBranch, Grid2x2, Plus, Rows2, X } from "lucide-react";
import { ConversationView } from "../conversation/ConversationView";
import { Popover } from "../../design/Popover";
import { FilesView } from "../files/FilesView";
import { ReviewView } from "../review/ReviewView";
import { WorktreesView } from "../worktrees/WorktreesView";
import { BrainView } from "../brain/BrainView";
import { PreviewView } from "../preview/PreviewView";
import { GuardrailPanel } from "../guardrails/GuardrailPanel";
import { AutonomyPanel } from "../settings/AutonomyPanel";
import { HistoricalTabBadge } from "../../shell/GlobalSearch";
import { useI18n } from "../../app/i18n";
import type { MessageKey } from "@jarvis/i18n";
import { listSessions, type SessionKind } from "../../app/sessions";
import { setVisibleSessions } from "../../app/notifications";
import { useEnvironment } from "../environment/useEnvironment";
import { TerminalView } from "../terminal/TerminalView";
import { useTerminals, type SplitDirection } from "../terminal/useTerminals";
import type { Project } from "../projects/useProjects";
import { VoiceButton } from "./VoiceButton";
import "./ProjectWorkspace.css";

interface ProjectWorkspaceProps {
  project: Project;
  onBack: () => void;
  /** Opening a worktree is opening a project — see §45. */
  onOpenProject: (projectId: string) => void;
  /** Where Global Search (§51) wants this project opened. */
  focusArea?: Area;
  /** A past session Global Search found a conversation match in. Opened
   * read-only: this tab was never started here, and never should be closable
   * back into a live agent (§51). */
  focusSessionId?: string;
  focusSessionProvider?: SessionKind;
  focusSessionTitle?: string;
  /**
   * Bumped every time somebody asks to be taken somewhere, even if it is the
   * same somewhere as last time.
   *
   * Without it, arriving from a notification (§49) into a project that is
   * *already open* did nothing at all: the area is component state read once at
   * mount, and `App` keys this component on the project id, so re-opening the
   * same project is not a remount and the new `focusArea` was never applied.
   * Found by clicking a toast while its own project was on screen, and landing
   * back on the file tree.
   */
  focusToken?: number;
}

/**
 * Areas inside a project (§19).
 *
 * These are project-scoped tools, so they live **inside** the project rather
 * than on the global rail — the rail is six destinations and stays six (§85/§87).
 * The list is the same kind of thing `App.tsx` keeps for the rail: an area that
 * is not built is absent from it, never a "coming soon" screen (§81).
 */
export const AREAS = ["sessions", "files", "review", "preview", "worktrees", "brain", "settings"] as const;
export type Area = (typeof AREAS)[number];

const AREA_LABEL: Record<Area, MessageKey> = {
  sessions: "project.sessions",
  files: "project.files",
  review: "project.review",
  preview: "preview.title",
  worktrees: "project.worktrees",
  brain: "project.brain",
  settings: "project.settings",
};

/**
 * The three ways panes can share the screen (§20).
 *
 * Presets, not a resizable tree. A draggable split tree is a large amount of
 * machinery — drag state, minimum sizes, persistence, a resize storm through
 * every PTY — in service of a choice most people make once. Three layouts and
 * a four-pane ceiling cover what a person actually does with several agents,
 * and can be read at a glance from the icon.
 */
const SPLIT_LAYOUTS = [
  { id: "columns", Icon: Columns2 },
  { id: "rows", Icon: Rows2 },
  { id: "grid", Icon: Grid2x2 },
] as const;

const SPLIT_LABEL: Record<SplitDirection, MessageKey> = {
  columns: "terminal.split.columns",
  rows: "terminal.split.rows",
  grid: "terminal.split.grid",
};

/** Which agents can actually be launched, from the real environment scan (§14). */
const AGENT_TOOL_ID: Record<Exclude<SessionKind, "shell">, string> = {
  "claude-code": "claude",
  codex: "codex",
};

/**
 * The project cockpit (§19).
 *
 * Everything here is scoped to one project. Only surfaces that exist are shown;
 * the rest arrive with the milestones that build them (§81).
 */
export function ProjectWorkspace({
  project,
  onBack,
  onOpenProject,
  focusArea,
  focusSessionId,
  focusSessionProvider,
  focusSessionTitle,
  focusToken,
}: ProjectWorkspaceProps) {
  const { t, locale } = useI18n();
  const { report } = useEnvironment();
  const {
    tabs,
    activeTab,
    slots,
    direction,
    openTerminal,
    openHistorical,
    closeTerminal,
    setActive,
    adopt,
    addToSplit,
    removeFromSplit,
    setDirection,
    error,
  } = useTerminals();

  const projectTabs = tabs[project.id] ?? [];
  // Only sessions that are still tabs, **in tab order**. Filtering here rather
  // than trusting the store means a stale id can never render an empty pane.
  //
  // Tab order, not the order they were added to the split: on screen the first
  // attempt put the active terminal on the left and the one it was split with
  // on the right, so the panes read `Shell 2 | Shell` while the strip above
  // read `Shell | Shell 2`. Two orderings of the same four things, and the
  // tab strip is the one the user is already reading. Found by splitting in
  // the real app and having to look twice to tell which pane was which.
  const split = projectTabs
    .map((tab) => tab.sessionId)
    .filter((id) => (slots[project.id] ?? []).includes(id));
  const splitting = split.length > 1;
  const layout = direction[project.id] ?? "columns";
  const active = activeTab[project.id];
  const [menuOpen, setMenuOpen] = useState(false);
  const [menuAnchor, setMenuAnchor] = useState<HTMLButtonElement | null>(null);
  const [view, setView] = useState<"terminal" | "conversation">("terminal");
  const [area, setArea] = useState<Area>(focusArea ?? "sessions");
  // Files and Review are mounted the first time they are opened and stay
  // mounted after that, so returning to one keeps its open files, its scroll
  // position and its selected diff. Sessions is always mounted: unmounting it
  // would tear down every terminal in the project.
  const [visited, setVisited] = useState<Set<Area>>(
    () => new Set<Area>(focusArea ? ["sessions", focusArea] : ["sessions"]),
  );

  // Global Search (§51) landed here pointing at a specific past session. This
  // project is a fresh mount whenever it opens (`App.tsx` keys on the project
  // id), so a mount-time effect is enough — no cleanup needed for a value that
  // cannot change under this component without it being torn down first.
  useEffect(() => {
    if (focusSessionId && focusSessionProvider) {
      openHistorical(project.id, focusSessionId, focusSessionProvider, focusSessionTitle);
      setView("conversation");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project.id, focusSessionId, focusSessionProvider, focusSessionTitle]);

  // Somebody asked to be taken somewhere. See `focusToken`.
  //
  // The area applies immediately; the session has to wait for a tab to exist.
  // Arriving from a notification into a project that is not already open is a
  // fresh mount, and `adopt` — which rebuilds the tabs for sessions still
  // running — resolves a round trip later. Activating a session that is not yet
  // a tab would leave the workspace pointing at nothing, so the request is held
  // and applied when its tab shows up.
  const [wantedSession, setWantedSession] = useState<string | undefined>();
  useEffect(() => {
    if (!focusToken || !focusArea) return;
    setVisited((seen) => (seen.has(focusArea) ? seen : new Set(seen).add(focusArea)));
    setArea(focusArea);
    setWantedSession(focusSessionId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focusToken]);

  useEffect(() => {
    if (!wantedSession) return;
    // Only a session this project actually has a tab for. One it no longer
    // has — closed since the notification was raised — is simply not selected,
    // and the person lands on Sessions, which is still where they were going.
    if (projectTabs.some((tab) => tab.sessionId === wantedSession)) {
      setActive(project.id, wantedSession);
      setWantedSession(undefined);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wantedSession, projectTabs.length]);

  const goToArea = (next: Area) => {
    setVisited((seen) => (seen.has(next) ? seen : new Set(seen).add(next)));
    setArea(next);
  };

  const activeTab_ = projectTabs.find((tab) => tab.sessionId === active);

  // Reattach to sessions that are still running from a previous visit.
  useEffect(() => {
    void listSessions(project.id).then((sessions) => adopt(project.id, sessions));
  }, [project.id, adopt]);

  // Which sessions are genuinely on screen, for the notification suppression
  // rule (§49). This component is the only place that knows: the store knows
  // the tabs and the split, but not that the workspace is showing Files
  // instead of Sessions, and an agent finishing behind the file tree is one
  // nobody is watching.
  //
  // A split shows several at once, so this is a list. The cleanup is what
  // makes leaving the project — or closing the window on it — report that
  // nothing is being watched any more.
  const onScreen = area === "sessions" ? (splitting ? split : active ? [active] : []) : [];
  const onScreenKey = onScreen.join(" ");
  useEffect(() => {
    setVisibleSessions(onScreenKey === "" ? [] : onScreenKey.split(" "));
    return () => setVisibleSessions([]);
  }, [onScreenKey]);

  const launch = (kind: SessionKind) => {
    setMenuOpen(false);
    // A sensible starting geometry; the view corrects it on first fit.
    void openTerminal(project.id, kind, { cols: 120, rows: 30 });
  };

  const toolReady = (kind: Exclude<SessionKind, "shell">) =>
    report?.tools.find((tool) => tool.id === AGENT_TOOL_ID[kind])?.state === "ready";

  return (
    <div className="workspace">
      <header className="workspace__header">
        <button type="button" className="workspace__back" onClick={onBack}>
          <ChevronLeft size={15} strokeWidth={1.9} aria-hidden="true" />
          <span className="sr-only">{t("projects.back")}</span>
        </button>

        <div className="workspace__identity">
          <span className="workspace__name">{project.name}</span>
          {project.isGit && project.gitBranch && (
            <span className="workspace__branch">
              <GitBranch size={11} strokeWidth={2} aria-hidden="true" />
              {project.gitBranch}
            </span>
          )}
        </div>

        {/* Areas of the project. Deliberately *not* a segmented pill: the
            Terminal/Conversation switch below is one, and two pills stacked
            read as the same kind of control. This is navigation — it changes
            what you are looking at — so it is rendered as navigation. */}
        <nav className="workspace__areas" aria-label={project.name}>
          {AREAS.map((id) => (
            <button
              key={id}
              type="button"
              className="workspace__area"
              data-active={area === id || undefined}
              aria-current={area === id ? "page" : undefined}
              onClick={() => goToArea(id)}
            >
              {t(AREA_LABEL[id])}
            </button>
          ))}
        </nav>

        <span className="workspace__path selectable" title={project.path}>
          {project.path}
        </span>
      </header>

      <div className="workspace__area-body" data-visible={area === "sessions" || undefined}>
        <div className="workspace__tabs">
        {projectTabs.map((tab) => (
          <div
            key={tab.sessionId}
            className="workspace__tab"
            data-active={tab.sessionId === active || undefined}
          >
            <button
              type="button"
              className="workspace__tab-label"
              onClick={() => setActive(project.id, tab.sessionId)}
              // Alt+click puts this tab on screen beside what is already
              // there, the same gesture as the split button but aimed at a
              // specific terminal rather than the next one along. Discovered
              // through the tooltip rather than guessed at.
              title={tab.historical ? undefined : t("terminal.split.addThis")}
              onMouseDown={(event) => {
                if (event.altKey && !tab.historical) {
                  event.preventDefault();
                  addToSplit(project.id, tab.sessionId);
                }
              }}
            >
              <span className="workspace__tab-dot" data-kind={tab.kind} aria-hidden="true" />
              {tab.title}
              {tab.historical && <HistoricalTabBadge title={t("search.historicalTab")} />}
            </button>
            <button
              type="button"
              className="workspace__tab-close"
              onClick={() => void closeTerminal(project.id, tab.sessionId)}
              aria-label={t("terminal.close")}
              title={t("terminal.close")}
            >
              <X size={11} strokeWidth={2.2} aria-hidden="true" />
            </button>
          </div>
        ))}

        <div className="workspace__new">
          {/* No native title: the OS tooltip renders over the menu this button
              opens, and its styling is outside our control (§9). */}
          <button
            ref={setMenuAnchor}
            type="button"
            className="workspace__new-button"
            onClick={() => setMenuOpen((open) => !open)}
            aria-label={t("terminal.new")}
            aria-expanded={menuOpen}
          >
            <Plus size={13} strokeWidth={2} aria-hidden="true" />
          </button>

          {/* A popover, not a modal: launching a terminal does not warrant
              taking over the window (§84). It is portalled because the tab
              strip scrolls and would otherwise clip it entirely. */}
          <Popover anchor={menuAnchor} open={menuOpen} onClose={() => setMenuOpen(false)}>
            <button
              type="button"
              role="menuitem"
              className="popover__item"
              onClick={() => launch("shell")}
            >
              {t("terminal.shell")}
            </button>
            {(["claude-code", "codex"] as const).map((kind) => {
              const ready = toolReady(kind);
              const label = kind === "claude-code" ? t("terminal.claudeCode") : t("terminal.codex");
              return (
                <button
                  key={kind}
                  type="button"
                  role="menuitem"
                  className="popover__item"
                  disabled={!ready}
                  onClick={() => launch(kind)}
                  // Explain the disabled state rather than leaving a dead item.
                  title={ready ? undefined : t("terminal.notInstalled", { name: label })}
                >
                  {label}
                  {!ready && <span className="popover__note">—</span>}
                </button>
              );
            })}
          </Popover>
        </div>

        {/* Terminal and Conversation are the same session, not two sessions
            (§23). This switches how it is rendered; nothing is restarted, the
            process keeps running, and the terminal keeps its scrollback.
            A historical tab has no terminal to toggle to — there is nothing
            running to attach a PTY view to — so it skips the pill entirely
            and always shows as a conversation below. */}
        {activeTab_ && !activeTab_.historical && (
          <VoiceButton projectId={project.id} sessionId={activeTab_.sessionId} locale={locale} />
        )}

        {/* Split controls (§20).
            Shown only with something to split — a layout control beside a
            single terminal is an option that cannot do anything. Splitting
            is offered while there is room; the direction control replaces it
            once panes are on screen, because that is the choice that is
            actually live at that point. */}
        {!splitting && projectTabs.length > 1 && activeTab_ && !activeTab_.historical && (
          <button
            type="button"
            className="workspace__split-button"
            onClick={() => {
              // The obvious partner: the next tab along, wrapping. Splitting
              // should be one click, not a click and then a chooser.
              const index = projectTabs.findIndex((tab) => tab.sessionId === active);
              const next = projectTabs[(index + 1) % projectTabs.length];
              if (next) addToSplit(project.id, next.sessionId);
            }}
            aria-label={t("terminal.split.add")}
            title={t("terminal.split.add")}
          >
            <Columns2 size={13} strokeWidth={1.9} aria-hidden="true" />
          </button>
        )}

        {splitting && (
          <div
            className="workspace__split-controls"
            role="radiogroup"
            aria-label={t("terminal.split.layout")}
          >
            {SPLIT_LAYOUTS.map(({ id, Icon }) => (
              <button
                key={id}
                type="button"
                role="radio"
                aria-checked={layout === id}
                data-active={layout === id || undefined}
                className="workspace__split-option"
                onClick={() => setDirection(project.id, id)}
                aria-label={t(SPLIT_LABEL[id])}
                title={t(SPLIT_LABEL[id])}
              >
                <Icon size={13} strokeWidth={1.9} aria-hidden="true" />
              </button>
            ))}
          </div>
        )}

        {/* Terminal/Conversation is meaningless in a split — see the note by
            the conversation projection below. */}
        {activeTab_ && !activeTab_.historical && !splitting && (
          <div className="workspace__view-toggle" role="radiogroup" aria-label={t("view.terminal")}>
            {(["terminal", "conversation"] as const).map((mode) => (
              <button
                key={mode}
                type="button"
                role="radio"
                aria-checked={view === mode}
                data-active={view === mode || undefined}
                className="workspace__view-option"
                onClick={() => setView(mode)}
              >
                {mode === "terminal" ? t("view.terminal") : t("view.conversation")}
              </button>
            ))}
          </div>
        )}
        </div>

        {/* Split panes (§20).
            `data-split` switches `.workspace__body` from a stack of absolutely
            positioned panes to a grid. Each pane keeps its **place in the DOM**
            either way — only its CSS box changes. Re-parenting a terminal to
            move it into a layout would remount it, and remounting is how you
            lose a session's whole scrollback. */}
        <div className="workspace__body" data-split={splitting ? layout : undefined}>
        {error && <p className="workspace__error">{error}</p>}

        {projectTabs.length === 0 ? (
          <div className="workspace__empty">
            <p className="workspace__empty-title">{t("terminal.empty.title")}</p>
            <p className="workspace__empty-body">{t("terminal.empty.body")}</p>
            <button type="button" className="workspace__empty-action" onClick={() => launch("shell")}>
              {t("terminal.shell")}
            </button>
          </div>
        ) : (
          // Every tab stays mounted and only the active one is shown. Unmounting
          // would tear down the terminal and lose its scrollback on every switch.
          projectTabs.map((tab) => (
            <div
              key={tab.sessionId}
              className="workspace__pane"
              // In a split, every session in the layout is visible at once and
              // `active` narrows to which one holds the keyboard. Outside one,
              // visible and active are the same thing.
              data-visible={
                (splitting ? split.includes(tab.sessionId) : tab.sessionId === active) || undefined
              }
              // Which slot this pane occupies, so the grid can order panes by
              // the layout rather than by tab order — the two differ as soon
              // as you split with anything but the leftmost tabs.
              style={
                splitting && split.includes(tab.sessionId)
                  ? { order: split.indexOf(tab.sessionId) }
                  : undefined
              }
              // Only meaningful with more than one pane on screen: it says
              // which of them the keyboard is going to.
              data-focused={(splitting && tab.sessionId === active) || undefined}
              // An odd last pane in the grid spans both columns rather than
              // sitting at half width beside an empty cell. Computed here
              // because CSS counts hidden siblings and would pick the wrong
              // element — every tab stays mounted (see the note above).
              data-last-odd={
                (layout === "grid" &&
                  split.length % 2 === 1 &&
                  split.indexOf(tab.sessionId) === split.length - 1) ||
                undefined
              }
              // Clicking anywhere in a pane moves focus to it, which is what
              // every split terminal does and what the click was for anyway.
              onMouseDown={() => {
                if (splitting && tab.sessionId !== active) setActive(project.id, tab.sessionId);
              }}
            >
              {tab.historical ? (
                // Opened by Global Search (§51) against a session this window
                // never started. There is no PTY to attach `TerminalView` to
                // — only the log `session_conversation` already reads — so
                // this is the one place a tab is conversation-only.
                <div className="workspace__projection" data-visible>
                  <ConversationView sessionId={tab.sessionId} live={false} />
                </div>
              ) : (
                <>
                  {/* The terminal stays mounted even when the conversation is
                      showing: unmounting it would discard its scrollback, and
                      switching views must never cost the user their history. */}
                  <div
                    className="workspace__projection"
                    data-visible={view === "terminal" || splitting || undefined}
                  >
                    <TerminalView
                      sessionId={tab.sessionId}
                      // Also gated on the area: a terminal that is off screen
                      // behind Files or Review must not hold the keyboard.
                      // In a split several terminals are visible and exactly
                      // one is focused, so this stays `=== active` — four
                      // panes all claiming focus would fight over every
                      // keystroke, and Ctrl+F would open four find bars.
                      autoFocus={
                        area === "sessions" &&
                        tab.sessionId === active &&
                        (view === "terminal" || splitting)
                      }
                    />
                  </div>
                  {/* Conversation is a single-session reading view, so a
                      split shows terminals only. Rendering four conversations
                      side by side would be four columns of prose too narrow
                      to read — the toggle is hidden in a split for the same
                      reason. */}
                  {view === "conversation" && !splitting && (
                    <div className="workspace__projection" data-visible>
                      <ConversationView sessionId={tab.sessionId} live />
                    </div>
                  )}
                  {splitting && (
                    <button
                      type="button"
                      className="workspace__pane-close"
                      // Closes the *pane*, never the session: the terminal
                      // stays a tab and keeps running. Conflating the two
                      // would make tidying a layout kill an agent mid-task.
                      onClick={() => removeFromSplit(project.id, tab.sessionId)}
                      aria-label={t("terminal.split.remove")}
                      title={t("terminal.split.remove")}
                    >
                      <X size={11} strokeWidth={2.2} aria-hidden="true" />
                    </button>
                  )}
                </>
              )}
            </div>
          ))
        )}
        </div>
      </div>

      {visited.has("files") && (
        <div className="workspace__area-body" data-visible={area === "files" || undefined}>
          <FilesView projectId={project.id} />
        </div>
      )}

      {visited.has("review") && (
        <div className="workspace__area-body" data-visible={area === "review" || undefined}>
          <ReviewView projectId={project.id} active={area === "review"} />
        </div>
      )}

      {/* Preview (§46) — the *see* step, right after Review's *inspect*.
          It watches the **active session's** output for a dev server, which
          is what makes it this product's preview rather than a browser: the
          URL comes from the log we already keep, not from something typed. */}
      {visited.has("preview") && (
        <div className="workspace__area-body" data-visible={area === "preview" || undefined}>
          <PreviewView sessionId={active} active={area === "preview"} />
        </div>
      )}

      {visited.has("worktrees") && (
        <div className="workspace__area-body" data-visible={area === "worktrees" || undefined}>
          <WorktreesView
            projectId={project.id}
            active={area === "worktrees"}
            onOpenProject={onOpenProject}
          />
        </div>
      )}

      {visited.has("brain") && (
        <div className="workspace__area-body" data-visible={area === "brain" || undefined}>
          <BrainView projectId={project.id} active={area === "brain"} />
        </div>
      )}

      {/* Project-scoped settings (§64).
          Both panels here were already written to take a `projectId` and had
          nowhere to be given one: `App.tsx` renders global Settings only when
          no project is open, so it structurally cannot host a project-scoped
          control. Guardrail policy per project has been implemented and
          unreachable since §35 for exactly that reason. This is the host. */}
      {visited.has("settings") && (
        <div className="workspace__area-body" data-visible={area === "settings" || undefined}>
          <div className="workspace__settings">
            <AutonomyPanel projectId={project.id} />
            <GuardrailPanel projectId={project.id} />
          </div>
        </div>
      )}
    </div>
  );
}
