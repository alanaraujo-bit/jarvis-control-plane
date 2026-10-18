import { useEffect, useState } from "react";
import { Cloud, CloudOff, RefreshCw } from "lucide-react";
import type { MessageKey } from "@jarvis/i18n";
import { useT } from "../../app/i18n";
import { invoke } from "../../app/platform";

/** `identity::commands::SyncStatus`. */
interface SyncStatus {
  signedIn: boolean;
  connected: boolean;
  ok: boolean | null;
  at: number | null;
  error: string | null;
}

const SYNC_ERRORS: Record<string, MessageKey> = {
  "identity.serverUnreachable": "identity.sync.error.unreachable",
  "identity.cloudSessionExpired": "identity.sync.error.expired",
  "identity.serverRefused": "identity.sync.error.refused",
};

/**
 * Whether this machine is actually saving to the account (M27). Sync used to
 * fail silently, and a deleted server went unnoticed: this says what is true.
 */
export function SyncLine({ onSignIn }: { onSignIn: () => void }) {
  const t = useT();
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const load = () => void invoke<SyncStatus>("identity_sync_status").then(setStatus).catch(() => undefined);
    load();
    const timer = window.setInterval(load, 30_000);
    return () => window.clearInterval(timer);
  }, []);

  if (!status) return null;
  const syncNow = async () => {
    setBusy(true);
    try {
      setStatus(await invoke<SyncStatus>("identity_sync_now"));
    } finally {
      setBusy(false);
    }
  };
  const when = status.at ? new Date(status.at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : "";

  if (!status.connected) {
    return (
      <div className="account__sync" data-state="disconnected" role="status">
        <CloudOff size={14} strokeWidth={1.8} aria-hidden="true" />
        <span>{t("identity.sync.disconnected")}</span>
        <button type="button" className="account__ghost" onClick={onSignIn}>
          {t("identity.sync.signInAgain")}
        </button>
      </div>
    );
  }
  const failed = status.ok === false;
  return (
    <div className="account__sync" data-state={failed ? "failed" : "ok"} role="status">
      {failed ? (
        <CloudOff size={14} strokeWidth={1.8} aria-hidden="true" />
      ) : (
        <Cloud size={14} strokeWidth={1.8} aria-hidden="true" />
      )}
      <span>
        {failed
          ? t("identity.sync.failed", {
              reason: t(SYNC_ERRORS[status.error ?? ""] ?? "identity.sync.error.unknown"),
              time: when,
            })
          : status.at
            ? t("identity.sync.saved", { time: when })
            : t("identity.sync.pending")}
      </span>
      <button type="button" className="account__ghost" disabled={busy} onClick={() => void syncNow()}>
        <RefreshCw size={12} strokeWidth={1.9} aria-hidden="true" />
        {busy ? t("identity.sync.syncing") : t("identity.sync.now")}
      </button>
    </div>
  );
}
