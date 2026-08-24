/**
 * What is left on the accounts work is currently starting on, always visible.
 *
 * The Accounts screen answers "how much have I got" when you go and ask. This
 * is the part that removes the asking: the reason the question kept getting
 * asked is that the answer was nowhere until you went looking for it, and by
 * then the interesting moment — an agent about to start a long run on an
 * account with forty minutes left in its window — had already passed.
 *
 * It is not a decorative counter, which §7 rules out of the status bar. It
 * reports state that changes what a person does next, it appears only when a
 * provider has actually stated a number, and it disappears rather than showing
 * a placeholder.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import type { MessageKey } from "@jarvis/i18n";
import { useT } from "../app/i18n";
import {
  useAccounts,
  type AccountCard,
  type LiveWindow,
  type ProviderId,
} from "../surfaces/accounts/useAccounts";
import { countdown, nextTickDelay, remaining, severityBand } from "../surfaces/accounts/format";
import "./QuotaChips.css";

/** The window rationing an account right now, if a provider has said. */
function binding(card: AccountCard): LiveWindow | null {
  if (card.quota.live?.state !== "ok") return null;
  const windows = card.quota.live.reading.windows;
  return windows.find((window) => window.binding) ?? windows[0] ?? null;
}

export function QuotaChips({ onOpen }: { onOpen: () => void }) {
  const t = useT();
  const report = useAccounts((state) => state.report);
  const ensureFresh = useAccounts((state) => state.ensureFresh);
  const [now, setNow] = useState(() => Date.now());

  // One reading at startup, then a slow re-check. Quota only moves when the
  // person actually uses an account, so five minutes is far more often than
  // the number changes and still cheap: two CLI startups, off the UI thread.
  useEffect(() => {
    void ensureFresh();
    const timer = window.setInterval(() => void ensureFresh(), 300_000);
    return () => window.clearInterval(timer);
  }, [ensureFresh]);

  // One chip per provider, for the account new work would start on.
  const chips = useMemo(() => {
    const out: { provider: ProviderId; card: AccountCard; window: LiveWindow }[] = [];
    for (const card of report?.accounts ?? []) {
      if (!card.account.active) continue;
      const window = binding(card);
      if (window) out.push({ provider: card.account.provider, card, window });
    }
    return out;
  }, [report]);

  // Wake on the boundary the label flips on, not every second — the same
  // reasoning as the panel's clock, and it matters more here because the
  // status bar is mounted for the whole life of the window.
  const timer = useRef<number | null>(null);
  useEffect(() => {
    const tick = () => {
      setNow(Date.now());
      timer.current = window.setTimeout(
        tick,
        nextTickDelay(Date.now(), chips.map((chip) => chip.window.resetsAtMs)),
      );
    };
    tick();
    return () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    };
  }, [chips]);

  if (chips.length === 0) return null;

  return (
    <>
      {chips.map(({ provider, card, window: quotaWindow }) => {
        const left = Math.round(remaining(quotaWindow.percentUsed));
        const band = severityBand(quotaWindow.severity, quotaWindow.percentUsed);
        const reset = countdown(quotaWindow.resetsAtMs, now, t);
        return (
          <button
            key={card.account.id}
            type="button"
            className="quotachip"
            data-band={band}
            onClick={onOpen}
            title={`${card.account.label || card.account.email || ""} — ${t(
              "accounts.leftPercent",
              { percent: left },
            )}${reset ? ` · ${reset}` : ""}`}
          >
            <span className="quotachip__dot" aria-hidden="true" />
            <span className="quotachip__provider">
              {t(`accounts.provider.${provider}` as MessageKey)}
            </span>
            <span className="quotachip__value">{left}%</span>
            {reset && <span className="quotachip__reset">{reset}</span>}
          </button>
        );
      })}
    </>
  );
}
