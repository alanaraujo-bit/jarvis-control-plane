import { ShieldAlert } from "lucide-react";
import { useT } from "../../app/i18n";
import type { MessageKey } from "@jarvis/i18n";
import type { Choice, GuardrailEvent } from "./useGuardrails";
import "./PendingApprovals.css";

const CHOICES: Choice[] = ["allowOnce", "allowForProject", "alwaysAllow", "neverAllow"];

/**
 * Approvals a guardrail is holding (§35).
 *
 * Rendered where the work is — inside the mission — rather than as a modal.
 * A modal would interrupt whatever the person is doing to ask about something
 * that is already stopped and can wait, and §18 says the interface should carry
 * what needs attention rather than shout it.
 *
 * The section disappears entirely when the queue is empty. An "all clear" panel
 * would be a permanent reminder of a thing that is not happening.
 */
export function PendingApprovals({
  events,
  onDecide,
}: {
  events: GuardrailEvent[];
  onDecide: (eventId: string, choice: Choice) => void;
}) {
  const t = useT();
  if (events.length === 0) return null;

  return (
    <section className="approvals">
      <header className="approvals__header">
        <ShieldAlert size={14} strokeWidth={1.75} aria-hidden="true" />
        <h3 className="approvals__title">{t("guardrail.pending.title")}</h3>
        <span className="approvals__count">
          {t("guardrail.pending.body", { count: events.length })}
        </span>
      </header>

      <ul className="approvals__list">
        {events.map((event) => (
          <li key={event.id} className="approvals__item">
            <div className="approvals__what">
              <span className="approvals__operation">
                {t(`guardrail.op.${event.operation}` as MessageKey)}
              </span>
              <span className="approvals__origin">
                {t(`guardrail.origin.${event.origin}` as MessageKey)}
              </span>
            </div>

            {/* The command verbatim. Paraphrasing what is about to run would
                make the decision unreviewable. */}
            <code className="approvals__command">{event.command}</code>

            <p className="approvals__matched">
              {t("guardrail.matched")}: <code>{event.fragment}</code>
            </p>

            <div className="approvals__choices">
              {CHOICES.map((choice) => (
                <button
                  key={choice}
                  type="button"
                  className="approvals__choice"
                  data-choice={choice}
                  onClick={() => onDecide(event.id, choice)}
                >
                  {t(`guardrail.choice.${choice}` as MessageKey)}
                </button>
              ))}
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** Guardrail history, newest first. */
export function GuardrailHistory({ events }: { events: GuardrailEvent[] }) {
  const t = useT();
  if (events.length === 0) return null;

  return (
    <section className="approvals approvals--history">
      <h3 className="approvals__title">{t("guardrail.history")}</h3>
      <ul className="approvals__log">
        {events.map((event) => (
          <li key={event.id} className="approvals__entry" data-status={event.status}>
            <span className="approvals__entry-status">
              {t(`guardrail.status.${event.status}` as MessageKey)}
            </span>
            <span className="approvals__entry-operation">
              {t(`guardrail.op.${event.operation}` as MessageKey)}
            </span>
            <code className="approvals__entry-command">{event.command}</code>
            <span className="approvals__entry-reason">
              {t(`guardrail.reason.${event.reason}` as MessageKey)}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}
