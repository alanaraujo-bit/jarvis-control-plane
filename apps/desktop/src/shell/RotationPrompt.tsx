import { useEffect, useState } from "react";
import { ArrowLeftRight, X } from "lucide-react";
import type { MessageKey } from "@jarvis/i18n";
import { useI18n, useT } from "../app/i18n";
import { invoke, isTauri } from "../app/platform";
import { useAccounts } from "../surfaces/accounts/useAccounts";
import { countdown, resetMoment } from "../surfaces/accounts/format";
import "./RotationPrompt.css";

/** `accounts::rotation::engine::RotationEvent`. */
type Confidence = "official" | "observed" | "estimated" | "unknown";
export type RotationEvent =
  | {
      kind: "switched" | "ask";
      decisionId: string;
      fromAccountId: string;
      toAccountId: string;
      sessionIds: string[];
      reason: "nearLimit" | "exhausted" | "manual" | "resumeAfterWait";
      percent: number | null;
      confidence: Confidence;
      resetsAtMs: number | null;
    }
  | {
      kind: "allExhausted";
      decisionId: string;
      sessionIds: string[];
      nextAccountId: string | null;
      nextResetMs: number | null;
    }
  | { kind: "waiting"; decisionId: string; untilMs: number }
  | { kind: "resumed"; decisionId: string; accountId: string }
  | { kind: "failed"; decisionId: string; code: string };

/** How long an announcement stays; a question stays until answered. */
const LINGER_MS = 7_000;
const ANNOUNCEMENTS = new Set(["switched", "resumed", "failed"]);

/** Fold a new event into what is on screen: one card per decision. */
function fold(items: RotationEvent[], event: RotationEvent): RotationEvent[] {
  const others = items.filter((item) => item.decisionId !== event.decisionId);
  return [...others, event];
}

/**
 * What account rotation tells the person (M26): a small card at the top of
 * the window, never a modal that takes it over. Questions — "switch?",
 * "resume when quota is back?" — stay until answered; announcements fade.
 */
export function RotationPrompt() {
  const t = useT();
  const { locale } = useI18n();
  const report = useAccounts((state) => state.report);
  const load = useAccounts((state) => state.load);
  const [items, setItems] = useState<RotationEvent[]>([]);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/event").then(({ listen }) =>
      listen<RotationEvent>("accounts:rotation", ({ payload }) => {
        setItems((current) => fold(current, payload));
        void load("cached");
      }).then((stop) => (unlisten = stop)),
    );
    return () => unlisten?.();
  }, [load]);

  // Announcements leave on their own; the waiting pill counts down.
  useEffect(() => {
    const timers = items
      .filter((item) => ANNOUNCEMENTS.has(item.kind))
      .map((item) =>
        window.setTimeout(
          () => setItems((current) => current.filter((other) => other !== item)),
          LINGER_MS,
        ),
      );
    const tick = items.some((item) => item.kind === "waiting")
      ? window.setInterval(() => setNow(Date.now()), 30_000)
      : undefined;
    return () => {
      timers.forEach(window.clearTimeout);
      if (tick) window.clearInterval(tick);
    };
  }, [items]);

  if (items.length === 0) return null;

  const name = (id: string | null) => {
    const account = report?.accounts.find((card) => card.account.id === id)?.account;
    return account ? account.label || account.email || t("accounts.unnamed") : t("accounts.unnamed");
  };
  const at = (ms: number) => resetMoment(ms, now, locale);
  const drop = (decisionId: string) =>
    setItems((current) => current.filter((item) => item.decisionId !== decisionId));
  const answer = (command: string, decisionId: string, accept: boolean) => {
    drop(decisionId);
    void invoke(command, { decisionId, accept }).catch(() => undefined);
  };

  return (
    <div className="rotation-prompts" role="region" aria-live="polite" aria-label={t("settings.rotation")}>
      {items.map((item) => {
        switch (item.kind) {
          case "ask": {
            const estimated = item.confidence === "estimated";
            return (
              <div key={item.decisionId} className="rotation-card" data-kind="ask" role="alertdialog">
                <ArrowLeftRight size={15} aria-hidden="true" className="rotation-card__icon" />
                <div className="rotation-card__text">
                  <strong>
                    {item.reason === "exhausted"
                      ? t("rotation.ask.titleExhausted", { from: name(item.fromAccountId) })
                      : t("rotation.ask.title", {
                          from: name(item.fromAccountId),
                          percent: Math.round(item.percent ?? 0),
                        })}
                    {estimated && <em className="rotation-card__estimated"> · {t("rotation.estimated")}</em>}
                  </strong>
                  <span>
                    {item.resetsAtMs !== null
                      ? t("rotation.ask.body", { to: name(item.toAccountId), time: at(item.resetsAtMs) })
                      : t("rotation.ask.bodyReserve", { to: name(item.toAccountId) })}
                  </span>
                  <div className="rotation-card__actions">
                    <button type="button" className="rotation-card__primary" onClick={() => answer("rotation_answer", item.decisionId, true)}>
                      {t("rotation.ask.accept")}
                    </button>
                    <button type="button" onClick={() => answer("rotation_answer", item.decisionId, false)}>
                      {t("rotation.ask.decline")}
                    </button>
                  </div>
                </div>
              </div>
            );
          }
          case "allExhausted":
            return (
              <div key={item.decisionId} className="rotation-card" data-kind="exhausted" role="alertdialog">
                <ArrowLeftRight size={15} aria-hidden="true" className="rotation-card__icon" />
                <div className="rotation-card__text">
                  <strong>{t("rotation.allOut.title")}</strong>
                  <span>
                    {item.nextResetMs !== null
                      ? t("rotation.allOut.body", { time: at(item.nextResetMs), account: name(item.nextAccountId) })
                      : t("rotation.allOut.bodyUnknown")}
                  </span>
                  <div className="rotation-card__actions">
                    <button type="button" className="rotation-card__primary" onClick={() => answer("rotation_accept_wait", item.decisionId, true)}>
                      {t("rotation.allOut.accept")}
                    </button>
                    <button type="button" onClick={() => answer("rotation_accept_wait", item.decisionId, false)}>
                      {t("rotation.allOut.decline")}
                    </button>
                  </div>
                </div>
              </div>
            );
          case "waiting":
            return (
              <div key={item.decisionId} className="rotation-card rotation-card--pill" data-kind="waiting">
                <span>
                  {t("rotation.waiting", { time: at(item.untilMs) })}
                  <span className="rotation-card__muted"> · {countdown(item.untilMs, now, t)}</span>
                </span>
                <button
                  type="button"
                  onClick={() => {
                    drop(item.decisionId);
                    void invoke("rotation_cancel_wait", { decisionId: item.decisionId }).catch(() => undefined);
                  }}
                >
                  {t("rotation.waiting.cancel")}
                </button>
              </div>
            );
          case "switched":
            return (
              <div key={item.decisionId} className="rotation-card" data-kind="switched" role="status">
                <ArrowLeftRight size={15} aria-hidden="true" className="rotation-card__icon" />
                <div className="rotation-card__text">
                  <strong>{t("rotation.switched", { to: name(item.toAccountId) })}</strong>
                  <span>
                    {t(`rotation.switched.${item.reason === "resumeAfterWait" ? "exhausted" : item.reason}` as MessageKey, {
                      from: name(item.fromAccountId),
                      percent: Math.round(item.percent ?? 0),
                    })}
                    {item.sessionIds.length > 1 && ` · ${t("rotation.switched.sessions", { count: item.sessionIds.length })}`}
                  </span>
                </div>
                <Dismiss onClick={() => drop(item.decisionId)} label={t("rotation.dismiss")} />
              </div>
            );
          case "resumed":
            return (
              <div key={item.decisionId} className="rotation-card" data-kind="switched" role="status">
                <ArrowLeftRight size={15} aria-hidden="true" className="rotation-card__icon" />
                <div className="rotation-card__text">
                  <strong>{t("rotation.resumed", { account: name(item.accountId) })}</strong>
                </div>
                <Dismiss onClick={() => drop(item.decisionId)} label={t("rotation.dismiss")} />
              </div>
            );
          case "failed":
            return (
              <div key={item.decisionId} className="rotation-card" data-kind="failed" role="status">
                <ArrowLeftRight size={15} aria-hidden="true" className="rotation-card__icon" />
                <div className="rotation-card__text">
                  <strong>{t("rotation.failed")}</strong>
                </div>
                <Dismiss onClick={() => drop(item.decisionId)} label={t("rotation.dismiss")} />
              </div>
            );
        }
      })}
    </div>
  );
}

function Dismiss({ onClick, label }: { onClick: () => void; label: string }) {
  return (
    <button type="button" className="rotation-card__dismiss" onClick={onClick} aria-label={label}>
      <X size={12} aria-hidden="true" />
    </button>
  );
}
