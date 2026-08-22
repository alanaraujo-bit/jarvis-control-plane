import { useEffect, useState } from "react";
import { Bot, Check, ChevronLeft, CircleDashed, Play, ShieldCheck, TriangleAlert, X } from "lucide-react";
import { useT } from "../../app/i18n";
import { missionSessions, type SessionInfo } from "../../app/sessions";
import { StatusDot } from "../../design/StatusDot";
import {
  useMissions,
  type AcceptanceCriterion,
  type MissionDetail,
  type Verification,
} from "./useMissions";
import "./MissionDetailView.css";

/** A one-line, human rendering of how a criterion is checked. */
function describeCheck(verification: Verification): string {
  switch (verification.type) {
    case "command":
      return verification.command;
    case "fileExists":
      return verification.path;
    case "fileContains":
      return `${verification.path} contains "${verification.text}"`;
    case "manual":
      return "";
  }
}

/**
 * Mission detail.
 *
 * The layout puts acceptance criteria and their evidence above tasks on
 * purpose. Tasks are what someone intends to do; criteria and evidence are what
 * decides whether it is done (§30).
 */
export function MissionDetailView({
  missionId,
  onBack,
  onLaunchAgent,
  onOpenSession,
}: {
  missionId: string;
  onBack: () => void;
  /** Start an agent in this mission's project, tagged with the mission (§86). */
  onLaunchAgent?: (projectId: string, missionId: string) => void;
  onOpenSession?: (projectId: string, sessionId: string) => void;
}) {
  const t = useT();
  const { detail, verify, setStatus, confirmCriterion, setTaskDone } = useMissions();
  const [mission, setMission] = useState<MissionDetail | null>(null);
  const [verifying, setVerifying] = useState(false);
  const [refusal, setRefusal] = useState<string | null>(null);
  const [agents, setAgents] = useState<SessionInfo[]>([]);

  const load = async () => {
    setMission(await detail(missionId));
    setAgents(await missionSessions(missionId));
  };

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [missionId]);

  if (!mission) return <div className="md" />;

  const active = mission.criteria.filter((c) => c.removedAt === null);
  const withdrawn = mission.criteria.filter((c) => c.removedAt !== null);
  const openRequired = active.filter((c) => c.required && c.status !== "verified");

  const runVerification = async () => {
    setVerifying(true);
    setRefusal(null);
    const next = await verify(missionId);
    if (next) setMission(next);
    setVerifying(false);
  };

  const tryComplete = async () => {
    const error = await setStatus(missionId, "completed");
    // The core refuses when evidence is missing; show why rather than failing
    // silently or, worse, pretending it worked.
    setRefusal(error);
    if (!error) await load();
  };

  return (
    <div className="md">
      <div className="md__inner">
        <button type="button" className="md__back" onClick={onBack}>
          <ChevronLeft size={14} strokeWidth={2} aria-hidden="true" />
          {t("nav.missions")}
        </button>

        <header className="md__head">
          <div className="md__identity">
            <StatusDot status={mission.status} size={8} />
            <h1 className="md__title">{mission.title}</h1>
          </div>
          {mission.goal && <p className="md__goal">{mission.goal}</p>}

          <div className="md__facts">
            <span className="md__fact">
              {t("mission.autonomy")}: {t(`mission.autonomy.${mission.effectiveAutonomy}` as never)}
              {mission.autonomy === null && (
                <span className="md__inherited"> · {t("mission.autonomy.inherited")}</span>
              )}
            </span>
          </div>
        </header>

        {mission.status === "blocked" && mission.blockedReason && (
          <div className="md__blocked">
            <TriangleAlert size={14} strokeWidth={2} aria-hidden="true" />
            <div>
              <span className="md__blocked-label">{t("mission.blockedReason")}</span>
              <p className="selectable">{mission.blockedReason}</p>
            </div>
          </div>
        )}

        <div className="md__actions">
          {mission.status === "ready" && (
            <button
              type="button"
              className="md__action"
              onClick={async () => {
                await setStatus(missionId, "running");
                await load();
              }}
            >
              <Play size={13} strokeWidth={2} aria-hidden="true" />
              {t("mission.start")}
            </button>
          )}

          <button
            type="button"
            className="md__action"
            onClick={() => void runVerification()}
            disabled={verifying}
          >
            <ShieldCheck size={13} strokeWidth={2} aria-hidden="true" />
            {verifying ? t("mission.verifying") : t("mission.verify")}
          </button>

          {mission.status !== "completed" && (
            <button
              type="button"
              className="md__action md__action--primary"
              onClick={() => void tryComplete()}
              // Not disabled: the refusal explains itself, which teaches the
              // rule better than a dead button does.
            >
              <Check size={13} strokeWidth={2} aria-hidden="true" />
              {t("mission.complete")}
            </button>
          )}
        </div>

        {refusal && (
          <p className="md__refusal">
            {t("mission.notVerified", { count: openRequired.length })}
          </p>
        )}

        {/* ---- Agents (§86) ------------------------------------------------
            The thread from a mission to the agent doing it. Clicking through
            lands in that agent's terminal, which is the same session as its
            conversation (§23). */}
        <section className="md__section">
          <div className="md__section-head">
            <h2 className="md__section-title">{t("mission.agents")}</h2>
            {onLaunchAgent && (
              <button
                type="button"
                className="md__action"
                onClick={() => onLaunchAgent(mission.projectId, missionId)}
              >
                <Bot size={13} strokeWidth={1.9} aria-hidden="true" />
                {t("mission.launchAgent")}
              </button>
            )}
          </div>

          {agents.length === 0 ? (
            <p className="md__note">{t("mission.noAgents")}</p>
          ) : (
            <ul className="md__agents">
              {agents.map((session) => (
                <li key={session.id}>
                  <button
                    type="button"
                    className="md__agent"
                    onClick={() => onOpenSession?.(session.projectId, session.id)}
                    disabled={!onOpenSession}
                  >
                    <StatusDot status={session.live ? session.state : "idle"} />
                    <span className="md__agent-provider">{session.provider}</span>
                    <span className="md__agent-cwd">{session.cwd}</span>
                    {onOpenSession && (
                      <span className="md__agent-open">{t("mission.openSession")}</span>
                    )}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>

        {/* ---- Acceptance criteria ---------------------------------------- */}
        <section className="md__section">
          <h2 className="md__section-title">{t("mission.criteria")}</h2>

          {active.length === 0 ? (
            <p className="md__note">{t("mission.noCriteria")}</p>
          ) : (
            <ul className="md__criteria">
              {active.map((criterion) => (
                <CriterionRow
                  key={criterion.id}
                  criterion={criterion}
                  evidence={mission.evidence.filter((e) => e.criterionId === criterion.id)}
                  onConfirm={async () => {
                    await confirmCriterion(criterion.id, "you");
                    await load();
                  }}
                />
              ))}
            </ul>
          )}

          {withdrawn.length > 0 && (
            <ul className="md__withdrawn">
              {withdrawn.map((criterion) => (
                <li key={criterion.id} className="md__withdrawn-row">
                  <span className="md__withdrawn-tag">{t("mission.withdrawn")}</span>
                  <span className="md__withdrawn-text">{criterion.description}</span>
                  <span className="md__withdrawn-why">
                    {t("mission.withdrawnBy", {
                      who: criterion.removedBy ?? "?",
                      reason: criterion.removedReason ?? "",
                    })}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </section>

        {/* ---- Tasks -------------------------------------------------------- */}
        {mission.tasks.length > 0 && (
          <section className="md__section">
            <h2 className="md__section-title">{t("mission.tasks")}</h2>
            <ul className="md__tasks">
              {mission.tasks.map((task) => (
                <li key={task.id}>
                  <label className="md__task">
                    <input
                      type="checkbox"
                      checked={task.done}
                      onChange={async (e) => {
                        await setTaskDone(task.id, e.target.checked);
                        await load();
                      }}
                    />
                    <span data-done={task.done || undefined}>{task.description}</span>
                  </label>
                </li>
              ))}
            </ul>
          </section>
        )}
      </div>
    </div>
  );
}

function CriterionRow({
  criterion,
  evidence,
  onConfirm,
}: {
  criterion: AcceptanceCriterion;
  evidence: MissionDetail["evidence"];
  onConfirm: () => void;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const latest = evidence[0];
  const check = describeCheck(criterion.verification);

  return (
    <li className="md__criterion" data-status={criterion.status}>
      <div className="md__criterion-head">
        <span className="md__criterion-icon">
          {criterion.status === "verified" ? (
            <Check size={12} strokeWidth={2.6} />
          ) : criterion.status === "failed" ? (
            <X size={12} strokeWidth={2.6} />
          ) : (
            <CircleDashed size={12} strokeWidth={2} />
          )}
        </span>

        <span className="md__criterion-text">{criterion.description}</span>

        {!criterion.required && <span className="md__criterion-tag">{t("mission.optional")}</span>}

        {criterion.verification.type === "manual" && criterion.status !== "verified" && (
          <button type="button" className="md__confirm" onClick={onConfirm} title={t("mission.confirmManual")}>
            {t("mission.confirm")}
          </button>
        )}
      </div>

      {check && <code className="md__criterion-check selectable">{check}</code>}

      {latest && (
        <button
          type="button"
          className="md__evidence-toggle"
          onClick={() => setOpen((o) => !o)}
          aria-expanded={open}
          data-ok={latest.ok || undefined}
        >
          {latest.summary}
        </button>
      )}

      {/* The command's own output. Verification that cannot be inspected is
          just another claim. */}
      {open && latest?.detail && <pre className="md__evidence-detail selectable">{latest.detail}</pre>}
    </li>
  );
}
