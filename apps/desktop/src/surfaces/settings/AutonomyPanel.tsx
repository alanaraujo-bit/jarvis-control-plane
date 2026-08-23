import { Gauge } from "lucide-react";
import type { MessageKey } from "@jarvis/i18n";

import { useT } from "../../app/i18n";
import { useAutonomy, type Autonomy } from "./useAutonomy";
import "./AutonomyPanel.css";

const LEVELS: Autonomy[] = ["guided", "autonomous", "unattended"];

interface AutonomyPanelProps {
  /**
   * Scope. Given a project, the panel offers that project's own default and
   * shows what it inherits; without one, it offers the global default.
   */
  projectId?: string;
}

/**
 * Autonomy defaults (§33).
 *
 * The rule has always been mission → project → global, and until now only the
 * mission level had a surface: Mission Detail rendered the word "Inherited"
 * against two values nobody could read or change. This is the other two
 * levels, and it is deliberately the *same* segmented control the mission
 * detail, the theme picker and the guardrail policies already use — this is
 * the same kind of choice and must not look like a new mechanism.
 *
 * The project scope offers a fourth option the global scope does not:
 * **Inherit**. That is not symmetry for its own sake. At the global level
 * "nothing chosen" and "Guided" behave identically, so a fourth state would be
 * a distinction the user cannot observe. At the project level clearing means
 * "follow the global default", which is observably different the moment the
 * global default changes — so it earns a control.
 */
export function AutonomyPanel({ projectId }: AutonomyPanelProps) {
  const t = useT();
  const { chain, error, setGlobal, setProject } = useAutonomy(projectId);

  const scoped = projectId !== undefined;
  // Nothing chosen globally *is* Guided — `resolve_autonomy` falls back to it,
  // so showing Guided selected is the truth rather than a friendly default.
  const globalSelected: Autonomy = chain?.global ?? "guided";
  const selected: Autonomy | null = scoped ? (chain?.project ?? null) : globalSelected;

  const label = (level: Autonomy) => t(`mission.autonomy.${level}` as MessageKey);

  const choose = (level: Autonomy | null) => {
    if (scoped) void setProject(level);
    else if (level) void setGlobal(level);
  };

  return (
    <section className="autonomy">
      <header className="autonomy__header">
        <h2 className="autonomy__title">
          <Gauge size={15} strokeWidth={1.75} aria-hidden="true" />
          {t("autonomy.title")}
        </h2>
        <p className="autonomy__subtitle">{t("autonomy.subtitle")}</p>
      </header>

      {error && <p className="autonomy__error">{error}</p>}

      <div className="autonomy__field">
        <span className="autonomy__label">
          {scoped ? t("autonomy.project") : t("autonomy.default")}
        </span>

        <div
          className="autonomy__segmented"
          role="radiogroup"
          aria-label={scoped ? t("autonomy.project") : t("autonomy.default")}
          // Until the core answers, the control has nothing true to show and
          // must not render a guess as a setting.
          aria-busy={chain === null || undefined}
        >
          {scoped && (
            <button
              type="button"
              role="radio"
              aria-checked={selected === null}
              data-active={selected === null || undefined}
              className="autonomy__segment"
              disabled={chain === null}
              onClick={() => choose(null)}
            >
              {t("autonomy.inherit")}
            </button>
          )}
          {LEVELS.map((level) => (
            <button
              key={level}
              type="button"
              role="radio"
              aria-checked={selected === level}
              data-active={selected === level || undefined}
              className="autonomy__segment"
              disabled={chain === null}
              title={t(`autonomy.${level}.help` as MessageKey)}
              onClick={() => choose(level)}
            >
              {label(level)}
            </button>
          ))}
        </div>
      </div>

      <p className="autonomy__note">
        {scoped && selected === null
          ? // The word "inherit" is only useful if it says what from. This is
            // the sentence that was missing when Mission Detail said
            // "Inherited" and stopped there.
            t("autonomy.inherits", { 0: label(globalSelected) })
          : t("autonomy.appliesTo")}
      </p>
    </section>
  );
}
