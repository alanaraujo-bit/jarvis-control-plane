import { useEffect, useState } from "react";
import { ChevronLeft, GitBranch, Plus, X } from "lucide-react";
import { ConversationView } from "../conversation/ConversationView";
import { Popover } from "../../design/Popover";
import { FilesView } from "../files/FilesView";
import { ReviewView } from "../review/ReviewView";
import { WorktreesView } from "../worktrees/WorktreesView";
import { BrainView } from "../brain/BrainView";
import { HistoricalTabBadge } from "../../shell/GlobalSearch";
import { useT } from "../../app/i18n";
import type { MessageKey } from "@jarvis/i18n";
import { listSessions, type SessionKind } from "../../app/sessions";
import { useEnvironment } from "../environment/useEnvironment";
import { TerminalView } from "../terminal/TerminalView";
import { useTerminals } from "../terminal/useTerminals";
import type { Project } from "../projects/useProjects";
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
}

/**
 * Areas inside a project (§19).
 *
 * These are project-scoped tools, so they live **inside** the project rather
 * than on the global rail — the rail is six destinations and stays six (§85/§87).
 * The list is the same kind of thing `App.tsx` keeps for the rail: an area that
 * is not built is absent from it, never a "coming soon" screen (§81).
 */
export const AREAS = ["sessions", "files", "review", "worktrees", "brain"] as const;
export type Area = (typeof AREAS)[number];

const AREA_LABEL: Record<Area, MessageKey> = {
  sessions: "project.sessions",
  files: "project.files",
  review: "project.review",
  worktrees: "project.worktrees",
  brain: "project.brain",
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
}: ProjectWorkspaceProps) {
  const t = useT();
  const { report } = useEnvironment();
  const { tabs, activeTab, openTerminal, openHistorical, closeTerminal, setActive, adopt, error } =
    useTerminals();

  const projectTabs = tabs[project.id] ?? [];
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

  const goToArea = (next: Area) => {
    setVisited((seen) => (seen.has(next) ? seen : new Set(seen).add(next)));
    setArea(next);
  };

  const activeTab_ = projectTabs.find((tab) => tab.sessionId === active);

  // Reattach to sessions that are still running from a previous visit.
  useEffect(() => {
    void listSessions(project.id).then((sessions) => adopt(project.id, sessions));
  }, [project.id, adopt]);

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

        <div className="workspace__body">
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
              data-visible={tab.sessionId === active || undefined}
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
                    data-visible={view === "terminal" || undefined}
                  >
                    <TerminalView
                      sessionId={tab.sessionId}
                      // Also gated on the area: a terminal that is off screen
                      // behind Files or Review must not hold the keyboard.
                      autoFocus={
                        area === "sessions" && tab.sessionId === active && view === "terminal"
                      }
                    />
                  </div>
                  {view === "conversation" && (
                    <div className="workspace__projection" data-visible>
                      <ConversationView sessionId={tab.sessionId} live />
                    </div>
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
    </div>
  );
}
