import { useCallback, useEffect, useState } from "react";
import { Smartphone } from "lucide-react";

import { useT } from "../../app/i18n";
import { invoke, isTauri } from "../../app/platform";
import "./CompanionPanel.css";

/** Where the companion is served from. Shown so it can be typed on a phone. */
const RELAY_HOST = "jarvis-desktop-relay.vercel.app";

interface RelayStatus {
  enabled: boolean;
  paired: boolean;
  code: string | null;
  codeExpiresAt: string | null;
}

/**
 * The mobile companion, in Settings (§59).
 *
 * This panel is the only way pairing is reachable, so it carries the weight of
 * explaining a feature that sends anything anywhere at all. Two things are
 * said plainly rather than buried:
 *
 * - **nothing is sent until you connect a device** — the companion is off
 *   unless chosen, because a local-first product does not start talking to a
 *   server because it was installed (§3);
 * - **what is sent**, in one sentence, before you connect rather than after.
 *   "Which missions need attention and which approvals are waiting. Never your
 *   files, terminal output or conversations." A privacy claim that is only
 *   true in the source is not a claim the user can act on.
 */
export function CompanionPanel() {
  const t = useT();
  const [status, setStatus] = useState<RelayStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!isTauri()) return;
    try {
      setStatus(await invoke<RelayStatus>("relay_status"));
    } catch {
      // The panel simply shows nothing rather than an error: not being able to
      // read a local setting is not something to alarm anyone about.
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const connect = async () => {
    setBusy(true);
    setError(null);
    try {
      setStatus(await invoke<RelayStatus>("relay_pair"));
    } catch {
      setError(t("companion.failed"));
    } finally {
      setBusy(false);
    }
  };

  const disconnect = async () => {
    setBusy(true);
    try {
      setStatus(await invoke<RelayStatus>("relay_unpair"));
    } catch {
      setError(t("companion.failed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="companion">
      <header>
        <h2 className="companion__title">
          <Smartphone size={15} strokeWidth={1.75} aria-hidden="true" />
          {t("companion.title")}
        </h2>
        <p className="companion__subtitle">{t("companion.subtitle")}</p>
      </header>

      <p className="companion__state">
        {status?.paired ? t("companion.paired") : t("companion.off")}
      </p>

      {/* The code, while one is live. Monospace and widely spaced because it
          is read off this screen and typed on another — the same reason the
          alphabet excludes I, O, 0 and 1. */}
      {status?.code && (
        <div className="companion__code-block">
          <span className="companion__code-label">{t("companion.code")}</span>
          <span className="companion__code selectable">{status.code}</span>
          <p className="companion__hint">{t("companion.codeHint", { url: RELAY_HOST })}</p>
        </div>
      )}

      <div className="companion__actions">
        {status?.paired ? (
          <button
            type="button"
            className="companion__button"
            disabled={busy}
            onClick={() => void disconnect()}
          >
            {t("companion.disconnect")}
          </button>
        ) : (
          <button
            type="button"
            className="companion__button companion__button--primary"
            disabled={busy}
            onClick={() => void connect()}
          >
            {t("companion.connect")}
          </button>
        )}
      </div>

      {error && <p className="companion__error">{error}</p>}

      {/* Said before connecting, not after. */}
      <p className="companion__privacy">{t("companion.whatIsSent")}</p>
    </section>
  );
}
