import { useCallback, useEffect, useState } from "react";
import type { MessageKey } from "@jarvis/i18n";
import { Popover } from "../../design/Popover";
import { useI18n, useT } from "../../app/i18n";
import { invoke, isTauri } from "../../app/platform";
import {
  useAccounts,
  type Account,
  type AccountQuota,
  type Confidence,
} from "../accounts/useAccounts";
import { formatTokens, remaining, resetMoment, span as spanOf } from "../accounts/format";
import "./SessionQuota.css";

/** `accounts::rotation::commands::SessionQuotaSummary`. */
interface SessionQuotaSummary {
  account: Account | null;
  quota: AccountQuota | null;
  etas: { window: "five_hour" | "weekly"; eta: { atMs: number; confidence: Confidence } }[];
  tokensIn: number;
  tokensOut: number;
  tokensCache: number;
  startedAtMs: number;
  segments: { accountId: string; fromMs: number; toMs: number | null }[];
  queue: { account: Account; why: { kind: string; resetsAtMs?: number } }[];
}

interface Shown {
  percent: number | null;
  resetsAtMs: number | null;
  confidence: Confidence;
}

/** The five-hour and weekly windows, from the live reading when there is one. */
function windowsOf(quota: AccountQuota | null): { session: Shown | null; weekly: Shown | null } {
  if (!quota) return { session: null, weekly: null };
  if (quota.live?.state === "ok") {
    const windows = quota.live.reading.windows;
    const pick = (group: string) => {
      const found = windows
        .filter((w) => w.group === group)
        .reduce<(typeof windows)[number] | null>(
          (worst, w) => (!worst || w.percentUsed > worst.percentUsed ? w : worst),
          null,
        );
      return found
        ? { percent: found.percentUsed, resetsAtMs: found.resetsAtMs, confidence: "official" as const }
        : null;
    };
    return { session: pick("session"), weekly: pick("weekly") };
  }
  const pick = (name: string) => {
    const w = quota.windows.find((window) => window.window === name);
    return w ? { percent: w.percent, resetsAtMs: w.resetsAtMs, confidence: w.confidence } : null;
  };
  return { session: pick("five_hour"), weekly: pick("weekly") };
}

const nameOf = (account: Account | null | undefined, fallback: string) =>
  account ? account.label || account.email || fallback : fallback;

/**
 * The quota of the account a session is running on, where the work happens
 * (M26). Compact in the tab strip; one click opens everything else.
 */
export function SessionQuota({ sessionId }: { sessionId: string }) {
  const t = useT();
  const { locale } = useI18n();
  const report = useAccounts((state) => state.report);
  const [summary, setSummary] = useState<SessionQuotaSummary | null>(null);
  const [anchor, setAnchor] = useState<HTMLButtonElement | null>(null);
  const [open, setOpen] = useState(false);
  const [switching, setSwitching] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());

  const refresh = useCallback(() => {
    if (!isTauri()) return;
    void invoke<SessionQuotaSummary>("session_quota_summary", { sessionId })
      .then(setSummary)
      .catch(() => setSummary(null));
  }, [sessionId]);

  // Every minute, and whenever the core says work moved between accounts.
  useEffect(() => {
    refresh();
    const timer = window.setInterval(() => {
      setNow(Date.now());
      refresh();
    }, 60_000);
    let unlisten: (() => void) | undefined;
    if (isTauri()) {
      void import("@tauri-apps/api/event").then(({ listen }) =>
        listen("accounts:rotation", () => refresh()).then((stop) => (unlisten = stop)),
      );
    }
    return () => {
      window.clearInterval(timer);
      unlisten?.();
    };
  }, [refresh]);

  if (!summary) return null;
  if (!summary.account) {
    return <span className="session-quota session-quota--none">{t("session.quota.noAccount")}</span>;
  }

  const unnamed = t("accounts.unnamed");
  const { session, weekly } = windowsOf(summary.quota);
  const eta = summary.etas.find((e) => e.window === "five_hour") ?? summary.etas[0];
  const at = (ms: number) => resetMoment(ms, now, locale);
  const nameById = (id: string) =>
    nameOf(report?.accounts.find((card) => card.account.id === id)?.account, unnamed);
  const span = (ms: number) => spanOf(ms, t);
  const ranFor = span(now - summary.startedAtMs);

  const switchTo = async (accountId: string) => {
    setSwitching(accountId);
    try {
      await invoke("session_switch_account", { sessionId, accountId });
    } catch {
      // The rotation card says what went wrong (the core emits `failed`).
    } finally {
      setSwitching(null);
      setOpen(false);
      refresh();
    }
  };

  const band = (percent: number | null) =>
    percent === null ? "unknown" : percent >= 100 ? "exhausted" : percent >= 85 ? "critical" : percent >= 70 ? "warning" : "normal";

  return (
    <>
      <button
        ref={setAnchor}
        type="button"
        className="session-quota"
        aria-expanded={open}
        aria-label={t("session.quota.label")}
        onClick={() => {
          if (!open) refresh(); // the minute timer may be up to 60 s behind
          setOpen(!open);
        }}
      >
        <span className="session-quota__account">{nameOf(summary.account, unnamed)}</span>
        {session?.percent != null && (
          <span className="session-quota__figure" data-band={band(session.percent)} data-confidence={session.confidence}>
            {t("session.quota.session")} {Math.round(session.percent)}%
            {session.resetsAtMs !== null && session.resetsAtMs > now && (
              <span className="session-quota__muted"> · {at(session.resetsAtMs)}</span>
            )}
          </span>
        )}
        {weekly?.percent != null && (
          <span className="session-quota__figure" data-band={band(weekly.percent)} data-confidence={weekly.confidence}>
            {t("session.quota.weekly")} {Math.round(weekly.percent)}%
          </span>
        )}
        {eta && (
          <span className="session-quota__eta" data-confidence="estimated">
            {t("session.quota.eta", { time: at(eta.eta.atMs) })}
          </span>
        )}
      </button>

      <Popover anchor={anchor} open={open} onClose={() => setOpen(false)} align="end">
        <div className="session-quota__panel">
          {(["session", "weekly"] as const).map((kind) => {
            const shown = kind === "session" ? session : weekly;
            if (!shown || shown.percent === null) return null;
            const windowEta = summary.etas.find(
              (e) => e.window === (kind === "session" ? "five_hour" : "weekly"),
            );
            return (
              <div key={kind} className="session-quota__window">
                <div className="session-quota__row">
                  <span>{t(kind === "session" ? "session.quota.session" : "session.quota.weekly")}</span>
                  <span>
                    {t("session.quota.free", { percent: Math.round(remaining(shown.percent)) })}
                    <span className="session-quota__badge" data-confidence={shown.confidence}>
                      {t(`accounts.confidence.${shown.confidence}` as MessageKey)}
                    </span>
                  </span>
                </div>
                <div className="session-quota__bar" data-band={band(shown.percent)}>
                  <span style={{ width: `${Math.min(100, shown.percent)}%` }} />
                </div>
                <div className="session-quota__row session-quota__muted">
                  <span>
                    {shown.resetsAtMs !== null
                      ? t("session.quota.resetsIn", { time: at(shown.resetsAtMs), countdown: span(shown.resetsAtMs - now) })
                      : ""}
                  </span>
                  <span>
                    {windowEta
                      ? t("session.quota.etaIn", {
                          time: at(windowEta.eta.atMs),
                          countdown: span(windowEta.eta.atMs - now),
                        })
                      : t("session.quota.etaUnknown")}
                  </span>
                </div>
              </div>
            );
          })}

          <div className="session-quota__section">
            <div className="session-quota__row">
              <span>{t("session.quota.tokens")}</span>
              <span className="session-quota__badge" data-confidence="observed">
                {t("accounts.confidence.observed")}
              </span>
            </div>
            <div className="session-quota__tokens">
              <span>
                <b>{formatTokens(summary.tokensIn, locale)}</b> {t("session.quota.in")}
              </span>
              <span>
                <b>{formatTokens(summary.tokensOut, locale)}</b> {t("session.quota.out")}
              </span>
              <span>
                <b>{formatTokens(summary.tokensCache, locale)}</b> {t("session.quota.cache")}
              </span>
            </div>
            <div className="session-quota__muted">{t("session.quota.duration", { duration: ranFor })}</div>
          </div>

          {summary.segments.length > 1 && (
            <div className="session-quota__section">
              <div className="session-quota__row">{t("session.quota.segments")}</div>
              <ol className="session-quota__segments">
                {summary.segments.map((segment) => (
                  <li key={`${segment.accountId}-${segment.fromMs}`}>
                    <span>{nameById(segment.accountId)}</span>
                    <span className="session-quota__muted">
                      {at(segment.fromMs)}
                      {segment.toMs !== null ? ` – ${at(segment.toMs)}` : ""}
                    </span>
                  </li>
                ))}
              </ol>
            </div>
          )}

          <div className="session-quota__section">
            <div className="session-quota__row">{t("session.quota.switch")}</div>
            {summary.queue.length === 0 ? (
              <div className="session-quota__muted">{t("session.quota.noQueue")}</div>
            ) : (
              summary.queue.map((entry) => (
                <button
                  key={entry.account.id}
                  type="button"
                  className="popover__item session-quota__destination"
                  disabled={switching !== null}
                  onClick={() => void switchTo(entry.account.id)}
                >
                  <span>{nameOf(entry.account, unnamed)}</span>
                  <span className="session-quota__muted">
                    {switching === entry.account.id
                      ? t("session.quota.switching")
                      : entry.why.resetsAtMs
                        ? t("session.quota.resets", { time: at(entry.why.resetsAtMs) })
                        : ""}
                  </span>
                </button>
              ))
            )}
          </div>
        </div>
      </Popover>
    </>
  );
}
