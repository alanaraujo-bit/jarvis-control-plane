import { useEffect } from "react";
import { ShieldCheck } from "lucide-react";
import { useT } from "../../app/i18n";
import type { MessageKey } from "@jarvis/i18n";
import { useGuardrails, type Decision, type Operation, type PolicyView } from "./useGuardrails";
import "./GuardrailPanel.css";

const DECISIONS: Decision[] = ["ask", "allow", "deny"];

/**
 * Guardrail policy (§35).
 *
 * One row per operation, each a segmented control — the same control Settings
 * already uses for theme and language, because this is the same kind of choice
 * and should not look like a new mechanism.
 *
 * The operation list comes from the core. A new operation appears here without
 * a component being touched, which is the same reason capabilities are data
 * (§26): the UI renders what exists rather than a list it keeps in step by hand.
 */
export function GuardrailPanel({ projectId }: { projectId?: string }) {
  const t = useT();
  const { policies, loading, error, loadPolicies, setPolicy } = useGuardrails();

  useEffect(() => {
    void loadPolicies(projectId);
  }, [loadPolicies, projectId]);

  return (
    <section className="guardrails">
      <header className="guardrails__header">
        <div>
          <h2 className="guardrails__title">
            <ShieldCheck size={15} strokeWidth={1.75} aria-hidden="true" />
            {t("guardrail.title")}
          </h2>
          <p className="guardrails__subtitle">{t("guardrail.subtitle")}</p>
        </div>
      </header>

      {error && <p className="guardrails__error">{error}</p>}

      <ul className="guardrails__list">
        {loading && policies.length === 0
          ? Array.from({ length: 4 }, (_, i) => (
              <li key={i} className="guardrails__row guardrails__row--skeleton" aria-hidden="true">
                <span className="guardrails__skeleton guardrails__skeleton--name" />
                <span className="guardrails__skeleton guardrails__skeleton--control" />
              </li>
            ))
          : policies.map((policy) => (
              <PolicyRow
                key={policy.operation}
                policy={policy}
                onChange={(decision) => void setPolicy(policy.operation, decision, projectId)}
              />
            ))}
      </ul>

      {/* The honest footnote. A guardrail that overstates its reach is worse
          than none, because it is trusted for things it does not cover. */}
      <p className="guardrails__note">{t("guardrail.coverage.note")}</p>
    </section>
  );
}

function PolicyRow({
  policy,
  onChange,
}: {
  policy: PolicyView;
  onChange: (decision: Decision | null) => void;
}) {
  const t = useT();
  const name = `guardrail.op.${policy.operation}` as MessageKey;
  const detail = `guardrail.op.${policy.operation}.detail` as MessageKey;

  // A project row that follows the global rule shows it as inherited rather
  // than as if it had been chosen here (§28's instinct: never present a derived
  // value as an authored one).
  const inherited = policy.scope !== "project" && policy.inherited !== null;

  return (
    <li className="guardrails__row" data-decision={policy.decision}>
      <div className="guardrails__label">
        <span className="guardrails__name">{t(name)}</span>
        <span className="guardrails__detail">{t(detail)}</span>
      </div>

      <div className="guardrails__controls">
        <div
          className="guardrails__segmented"
          role="radiogroup"
          aria-label={t(name)}
          data-inherited={inherited || undefined}
        >
          {DECISIONS.map((decision) => (
            <button
              key={decision}
              type="button"
              role="radio"
              aria-checked={policy.decision === decision}
              data-active={policy.decision === decision || undefined}
              data-decision={decision}
              className="guardrails__segment"
              onClick={() => onChange(decision)}
            >
              {t(`guardrail.decision.${decision}` as MessageKey)}
            </button>
          ))}
        </div>

        {/* Clearing is only offered where there is something to clear, and it
            says what it does: fall back, rather than "reset". */}
        {policy.scope === "project" ? (
          <button type="button" className="guardrails__clear" onClick={() => onChange(null)}>
            {t("guardrail.clear")}
          </button>
        ) : (
          <span className="guardrails__scope">{t(`guardrail.scope.${policy.scope}` as MessageKey)}</span>
        )}
      </div>
    </li>
  );
}

/** Shared by the pending banner and the history list. */
export function operationLabelKey(operation: Operation): MessageKey {
  return `guardrail.op.${operation}` as MessageKey;
}
