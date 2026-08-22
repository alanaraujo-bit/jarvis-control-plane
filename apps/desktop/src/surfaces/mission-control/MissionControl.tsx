import { useEffect, useMemo } from "react";
import { FolderPlus } from "lucide-react";
import { useT } from "../../app/i18n";
import { Logo } from "../../design/Logo";
import { StatusDot } from "../../design/StatusDot";
import { useMissions, type MissionSummary } from "../missions/useMissions";
import { useProjects } from "../projects/useProjects";
import "./MissionControl.css";

interface MissionControlProps {
  onOpenProject: () => void;
  onOpenMission: (mission: MissionSummary) => void;
}

/**
 * Mission Control (§18) — the home surface.
 *
 * It answers, in priority order: what needs me, what is working, what finished,
 * which projects are active. Sections render only when they have something to
 * say; an empty section is removed rather than shown empty, so the surface
 * stays calm as activity rises and falls.
 */
export function MissionControl({ onOpenProject, onOpenMission }: MissionControlProps) {
  const t = useT();
  const { summaries, refresh } = useMissions();
  const { projects, refresh: refreshProjects } = useProjects();

  useEffect(() => {
    void refresh();
    void refreshProjects();
  }, [refresh, refreshProjects]);

  const groups = useMemo(() => {
    // Blocked, waiting and failed come first: they are the only states where
    // nothing moves until a person does something.
    const needsAttention = summaries.filter((m) =>
      ["blocked", "waiting", "failed"].includes(m.status),
    );
    const working = summaries.filter((m) => ["running", "verifying"].includes(m.status));
    const completed = summaries.filter((m) => m.status === "completed").slice(0, 5);
    return { needsAttention, working, completed };
  }, [summaries]);

  const nothingAtAll =
    summaries.length === 0 && projects.length === 0;

  if (nothingAtAll) {
    return (
      <div className="mc">
        <div className="mc__empty">
          <Logo size={30} className="mc__empty-mark" />
          <h1 className="mc__empty-title">{t("missionControl.empty.title")}</h1>
          <p className="mc__empty-body">{t("missionControl.empty.body")}</p>
          <button type="button" className="mc__empty-action" onClick={onOpenProject}>
            <FolderPlus size={14} strokeWidth={1.9} aria-hidden="true" />
            {t("missionControl.empty.action")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="mc">
      <div className="mc__inner">
        <h1 className="mc__title">{t("missionControl.title")}</h1>

        <Section
          title={t("missionControl.needsAttention")}
          missions={groups.needsAttention}
          onOpen={onOpenMission}
          urgent
        />
        <Section
          title={t("missionControl.working")}
          missions={groups.working}
          onOpen={onOpenMission}
        />
        <Section
          title={t("missionControl.recentlyCompleted")}
          missions={groups.completed}
          onOpen={onOpenMission}
        />

        {projects.length > 0 && (
          <section className="mc__section">
            <h2 className="mc__section-title">{t("missionControl.activeProjects")}</h2>
            <ul className="mc__projects">
              {projects.slice(0, 6).map((project) => (
                <li key={project.id} className="mc__project">
                  <span className="mc__project-name">{project.name}</span>
                  {project.gitBranch && (
                    <span className="mc__project-branch">{project.gitBranch}</span>
                  )}
                </li>
              ))}
            </ul>
          </section>
        )}
      </div>
    </div>
  );
}

function Section({
  title,
  missions,
  onOpen,
  urgent = false,
}: {
  title: string;
  missions: MissionSummary[];
  onOpen: (mission: MissionSummary) => void;
  urgent?: boolean;
}) {
  const t = useT();
  // The section disappears entirely rather than showing an empty container.
  if (missions.length === 0) return null;

  return (
    <section className="mc__section" data-urgent={urgent || undefined}>
      <h2 className="mc__section-title">{title}</h2>
      <ul className="mc__list">
        {missions.map((mission) => (
          <li key={mission.id}>
            <button type="button" className="mc__row" onClick={() => onOpen(mission)}>
              <StatusDot status={mission.status} />
              <span className="mc__row-title">{mission.title}</span>
              <span className="mc__row-project">{mission.projectName}</span>

              <span className="mc__row-meta">
                {mission.status === "blocked" && mission.blockedReason ? (
                  <span className="mc__row-reason">{mission.blockedReason}</span>
                ) : mission.openCriteria > 0 ? (
                  // Surfaced everywhere a mission appears: the gap between
                  // "claimed" and "verified" is the product's whole point (§30).
                  <span className="mc__row-open">
                    {t("missionControl.openCriteria", { count: mission.openCriteria })}
                  </span>
                ) : mission.taskCount > 0 ? (
                  <span className="mc__row-tasks">
                    {mission.tasksDone}/{mission.taskCount}
                  </span>
                ) : null}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
