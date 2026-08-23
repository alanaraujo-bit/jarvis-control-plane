import { Hand, Radar } from "lucide-react";
import { useT } from "../../app/i18n";
import type { MessageKey } from "@jarvis/i18n";
import type { RunStatus } from "./useAutopilot";
import "./AutopilotPanel.css";

/**
 * Running a mission unattended (§32).
 *
 * Offered only when the mission's autonomy actually is Unattended. Autonomy is
 * the user's statement about how involved they want to be (§33), and putting an
 * "run it unsupervised" button on a Guided mission would invite them to
 * contradict a setting they already made — so the panel explains rather than
 * tempting.
 *
 * While a run is going, the turn count is shown plainly. An agent working
 * through turn 9 of 24 is information; a spinner is not.
 */
export function AutopilotPanel({
  unattended,
  run,
  refusal,
  onStart,
  onStop,
}: {
  unattended: boolean;
  run: RunStatus | null;
  refusal: string | null;
  onStart: () => void;
  onStop: () => void;
}) {
  const t = useT();
  const active = run !== null && run.state !== "finished";

  return (
    <section className="autopilot" data-active={active || undefined}>
      <div className="autopilot__head">
        <Radar
          size={14}
          strokeWidth={1.75}
          aria-hidden="true"
          className={active ? "autopilot__icon autopilot__icon--live" : "autopilot__icon"}
        />
        <h3 className="autopilot__title">
          {active ? t("autopilot.running") : t("autopilot.title")}
        </h3>

        {active ? (
          <span className="autopilot__progress">
            {t("autopilot.turn", { turns: run.turns, budget: run.budget })}
            <span className="autopilot__state">
              {" · "}
              {t(`autopilot.state.${run.state}` as MessageKey)}
            </span>
          </span>
        ) : null}
      </div>

      {!active && <p className="autopilot__description">{t("autopilot.description")}</p>}

      {/* Refusals explain themselves rather than leaving a dead button (§34). */}
      {refusal && <p className="autopilot__refusal">{t(refusal as MessageKey)}</p>}

      <div className="autopilot__actions">
        {active ? (
          <button type="button" className="autopilot__action" onClick={onStop}>
            <Hand size={13} strokeWidth={2} aria-hidden="true" />
            {t("autopilot.stop")}
          </button>
        ) : (
          <button
            type="button"
            className="autopilot__action autopilot__action--primary"
            onClick={onStart}
            // Not disabled when the mission is not Unattended: pressing it
            // explains what to change, which teaches the model better than a
            // greyed-out control does.
          >
            <Radar size={13} strokeWidth={2} aria-hidden="true" />
            {t("autopilot.start")}
          </button>
        )}

        {!unattended && !active && (
          <span className="autopilot__hint">{t("autopilot.requiresUnattended")}</span>
        )}
      </div>
    </section>
  );
}
