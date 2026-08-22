import { useState } from "react";
import { Plus, X } from "lucide-react";
import { useT } from "../../app/i18n";
import type { Project } from "../projects/useProjects";
import { useMissions, type Mission, type NewCriterionInput, type Verification } from "./useMissions";
import "./NewMissionForm.css";

type CheckType = "command" | "fileExists" | "manual";

interface DraftCriterion {
  description: string;
  required: boolean;
  checkType: CheckType;
  /** Command line, or path, depending on `checkType`. */
  value: string;
}

function toVerification(draft: DraftCriterion): Verification {
  switch (draft.checkType) {
    case "command":
      return { type: "command", command: draft.value, cwd: null, expectExit: 0 };
    case "fileExists":
      return { type: "fileExists", path: draft.value };
    case "manual":
      return { type: "manual" };
  }
}

/**
 * Creating a mission.
 *
 * The form asks for acceptance criteria at the same moment as the title, on
 * purpose. Criteria added afterwards are criteria written to fit whatever was
 * built; criteria written up front are what "done" actually means (§30).
 */
export function NewMissionForm({
  projects,
  onCancel,
  onCreated,
}: {
  projects: Project[];
  onCancel: () => void;
  onCreated: (mission: Mission) => void;
}) {
  const t = useT();
  const { createMission } = useMissions();

  const [projectId, setProjectId] = useState(projects[0]?.id ?? "");
  const [title, setTitle] = useState("");
  const [goal, setGoal] = useState("");
  const [criteria, setCriteria] = useState<DraftCriterion[]>([
    { description: "", required: true, checkType: "command", value: "" },
  ]);
  const [busy, setBusy] = useState(false);

  const update = (index: number, patch: Partial<DraftCriterion>) =>
    setCriteria((list) => list.map((c, i) => (i === index ? { ...c, ...patch } : c)));

  const submit = async () => {
    if (!title.trim() || !projectId) return;
    setBusy(true);

    const prepared: NewCriterionInput[] = criteria
      // A criterion with no description is an empty row the user never filled
      // in, not an intention.
      .filter((c) => c.description.trim())
      .map((c) => ({
        description: c.description.trim(),
        required: c.required,
        verification: toVerification(c),
      }));

    const mission = await createMission({
      projectId,
      title: title.trim(),
      goal: goal.trim() || undefined,
      criteria: prepared,
    });
    setBusy(false);
    if (mission) onCreated(mission);
  };

  return (
    <div className="nmf">
      <div className="nmf__inner">
        <h1 className="nmf__title">{t("mission.new")}</h1>

        <label className="nmf__field">
          <span className="nmf__label">{t("nav.projects")}</span>
          <select
            className="nmf__select"
            value={projectId}
            onChange={(e) => setProjectId(e.target.value)}
          >
            {projects.map((project) => (
              <option key={project.id} value={project.id}>
                {project.name}
              </option>
            ))}
          </select>
        </label>

        <label className="nmf__field">
          <span className="nmf__label">{t("mission.title")}</span>
          <input
            className="nmf__input"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder={t("mission.titlePlaceholder")}
            autoFocus
          />
        </label>

        <label className="nmf__field">
          <span className="nmf__label">{t("mission.goal")}</span>
          <input
            className="nmf__input"
            value={goal}
            onChange={(e) => setGoal(e.target.value)}
            placeholder={t("mission.goalPlaceholder")}
          />
        </label>

        <section className="nmf__criteria">
          <span className="nmf__label">{t("mission.criteria")}</span>

          {criteria.map((criterion, index) => (
            <div key={index} className="nmf__criterion">
              <input
                className="nmf__input"
                value={criterion.description}
                onChange={(e) => update(index, { description: e.target.value })}
                placeholder={t("mission.criterionPlaceholder")}
              />

              <div className="nmf__criterion-row">
                <select
                  className="nmf__select nmf__select--small"
                  value={criterion.checkType}
                  onChange={(e) => update(index, { checkType: e.target.value as CheckType })}
                >
                  <option value="command">{t("mission.checkType.command")}</option>
                  <option value="fileExists">{t("mission.checkType.fileExists")}</option>
                  <option value="manual">{t("mission.checkType.manual")}</option>
                </select>

                {criterion.checkType !== "manual" && (
                  <input
                    className="nmf__input nmf__input--mono"
                    value={criterion.value}
                    onChange={(e) => update(index, { value: e.target.value })}
                    placeholder={
                      criterion.checkType === "command"
                        ? t("mission.commandPlaceholder")
                        : "dist/app.js"
                    }
                  />
                )}

                <label className="nmf__required">
                  <input
                    type="checkbox"
                    checked={criterion.required}
                    onChange={(e) => update(index, { required: e.target.checked })}
                  />
                  {t("mission.required")}
                </label>

                {criteria.length > 1 && (
                  <button
                    type="button"
                    className="nmf__remove"
                    onClick={() => setCriteria((list) => list.filter((_, i) => i !== index))}
                    aria-label={t("mission.cancel")}
                  >
                    <X size={12} strokeWidth={2.2} aria-hidden="true" />
                  </button>
                )}
              </div>
            </div>
          ))}

          <button
            type="button"
            className="nmf__add"
            onClick={() =>
              setCriteria((list) => [
                ...list,
                { description: "", required: true, checkType: "command", value: "" },
              ])
            }
          >
            <Plus size={12} strokeWidth={2} aria-hidden="true" />
            {t("mission.addCriterion")}
          </button>
        </section>

        <div className="nmf__actions">
          <button type="button" className="nmf__cancel" onClick={onCancel}>
            {t("mission.cancel")}
          </button>
          <button
            type="button"
            className="nmf__submit"
            onClick={() => void submit()}
            disabled={!title.trim() || !projectId || busy}
          >
            {t("mission.create")}
          </button>
        </div>
      </div>
    </div>
  );
}
