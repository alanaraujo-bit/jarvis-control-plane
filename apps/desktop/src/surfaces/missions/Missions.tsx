import { useEffect, useState } from "react";
import { Plus } from "lucide-react";
import { useT } from "../../app/i18n";
import { StatusDot } from "../../design/StatusDot";
import { useProjects } from "../projects/useProjects";
import { MissionDetailView } from "./MissionDetailView";
import { NewMissionForm } from "./NewMissionForm";
import { useMissions } from "./useMissions";
import "./Missions.css";

/**
 * Missions (§29).
 *
 * A flat list across every project. Each row shows the one thing that decides
 * whether the mission is really finished: how many required criteria are still
 * unverified (§30).
 */
export function Missions({
  initialMissionId,
  onLaunchAgent,
  onOpenSession,
}: {
  initialMissionId?: string;
  onLaunchAgent?: (projectId: string, missionId: string) => void;
  onOpenSession?: (projectId: string, sessionId: string) => void;
}) {
  const t = useT();
  const { summaries, refresh, error } = useMissions();
  const { projects, refresh: refreshProjects } = useProjects();
  const [selected, setSelected] = useState<string | null>(initialMissionId ?? null);
  const [creating, setCreating] = useState(false);

  useEffect(() => {
    void refresh();
    void refreshProjects();
  }, [refresh, refreshProjects]);

  useEffect(() => {
    if (initialMissionId) setSelected(initialMissionId);
  }, [initialMissionId]);

  if (selected) {
    return (
      <MissionDetailView
        missionId={selected}
        onBack={() => setSelected(null)}
        onLaunchAgent={onLaunchAgent}
        onOpenSession={onOpenSession}
      />
    );
  }

  if (creating) {
    return (
      <NewMissionForm
        projects={projects}
        onCancel={() => setCreating(false)}
        onCreated={(mission) => {
          setCreating(false);
          setSelected(mission.id);
        }}
      />
    );
  }

  return (
    <div className="missions">
      <div className="missions__inner">
        <header className="missions__header">
          <h1 className="missions__title">{t("nav.missions")}</h1>
          <button
            type="button"
            className="missions__primary"
            onClick={() => setCreating(true)}
            // A mission belongs to a project, so there is nothing to create
            // until one exists.
            disabled={projects.length === 0}
          >
            <Plus size={14} strokeWidth={2} aria-hidden="true" />
            {t("mission.new")}
          </button>
        </header>

        {error && <p className="missions__error">{error}</p>}

        {summaries.length === 0 ? (
          <div className="missions__empty">
            <p className="missions__empty-title">{t("mission.empty.title")}</p>
            <p className="missions__empty-body">{t("mission.empty.body")}</p>
          </div>
        ) : (
          <ul className="missions__list">
            {summaries.map((mission) => (
              <li key={mission.id}>
                <button
                  type="button"
                  className="missions__row"
                  onClick={() => setSelected(mission.id)}
                >
                  <StatusDot status={mission.status} />
                  <span className="missions__row-title">{mission.title}</span>
                  <span className="missions__row-project">{mission.projectName}</span>
                  <span className="missions__row-state">
                    {mission.openCriteria > 0 ? (
                      <span className="missions__open">
                        {t("missionControl.openCriteria", { count: mission.openCriteria })}
                      </span>
                    ) : mission.status === "completed" ? (
                      <span className="missions__verified">{t("state.completed")}</span>
                    ) : (
                      t(`state.${mission.status}` as never)
                    )}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
