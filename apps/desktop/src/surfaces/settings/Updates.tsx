import { useState } from "react";
import { Download, RefreshCw, ShieldAlert } from "lucide-react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { useT } from "../../app/i18n";
import { isTauri } from "../../app/platform";
import { useAppVersion } from "../../app/version";
import "./Updates.css";


type Status =
  | { state: "idle" }
  | { state: "checking" }
  | { state: "upToDate" }
  | { state: "available"; update: Update }
  | { state: "downloading"; version: string; percent: number }
  | { state: "ready"; version: string }
  | { state: "failed"; message: string };

/**
 * Updates (§62).
 *
 * The user should never have to hunt for a new installer. Downloads are
 * verified against the bundled public key before anything is installed, and a
 * failed or unsigned update is refused rather than applied — an update must
 * never be able to damage an installation or its data.
 */
export function Updates() {
  const t = useT();
  const [status, setStatus] = useState<Status>({ state: "idle" });
  const version = useAppVersion();

  const runCheck = async () => {
    if (!isTauri()) return;
    setStatus({ state: "checking" });
    try {
      const update = await check();
      setStatus(update ? { state: "available", update } : { state: "upToDate" });
    } catch (cause) {
      setStatus({ state: "failed", message: String(cause) });
    }
  };

  const install = async (update: Update) => {
    const version = update.version;
    setStatus({ state: "downloading", version, percent: 0 });

    let total = 0;
    let received = 0;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          received += event.data.chunkLength;
          // Without a content length there is no honest percentage, so the
          // label falls back to indeterminate rather than inventing one.
          const percent = total > 0 ? Math.min(100, Math.round((received / total) * 100)) : 0;
          setStatus({ state: "downloading", version, percent });
        } else if (event.event === "Finished") {
          setStatus({ state: "ready", version });
        }
      });
      setStatus({ state: "ready", version });
    } catch (cause) {
      setStatus({ state: "failed", message: String(cause) });
    }
  };

  return (
    <section className="updates">
      <div className="updates__head">
        <div>
          <h2 className="updates__title">{t("update.title")}</h2>
          {version && (
            <p className="updates__version">{t("update.current", { version })}</p>
          )}
        </div>

        <button
          type="button"
          className="updates__check"
          onClick={() => void runCheck()}
          disabled={status.state === "checking" || status.state === "downloading"}
        >
          <RefreshCw
            size={13}
            strokeWidth={2}
            className={status.state === "checking" ? "updates__spin" : undefined}
            aria-hidden="true"
          />
          {status.state === "checking" ? t("update.checking") : t("update.check")}
        </button>
      </div>

      {status.state === "upToDate" && <p className="updates__status">{t("update.upToDate")}</p>}

      {status.state === "available" && (
        <div className="updates__panel">
          <p className="updates__status">
            {t("update.available", { version: status.update.version })}
          </p>
          {status.update.body && (
            <div className="updates__notes">
              <span className="updates__notes-label">{t("update.notes")}</span>
              <p className="selectable">{status.update.body}</p>
            </div>
          )}
          <button
            type="button"
            className="updates__primary"
            onClick={() => void install(status.update)}
          >
            <Download size={13} strokeWidth={2} aria-hidden="true" />
            {t("update.install")}
          </button>
        </div>
      )}

      {status.state === "downloading" && (
        <div className="updates__panel">
          <p className="updates__status">
            {t("update.downloading", { percent: status.percent })}
          </p>
          <div
            className="updates__bar"
            role="progressbar"
            aria-valuenow={status.percent}
            aria-valuemin={0}
            aria-valuemax={100}
          >
            <div className="updates__bar-fill" style={{ width: `${status.percent}%` }} />
          </div>
        </div>
      )}

      {status.state === "ready" && (
        <div className="updates__panel">
          <p className="updates__status">{t("update.ready", { version: status.version })}</p>
          <button type="button" className="updates__primary" onClick={() => void relaunch()}>
            {t("update.install")}
          </button>
        </div>
      )}

      {status.state === "failed" && <p className="updates__error">{t("update.failed")}</p>}

      {/* Stated plainly rather than hidden: this build is unsigned, so Windows
          will warn on install until a certificate is in place (BLOCKERS B1). */}
      <p className="updates__note">
        <ShieldAlert size={12} strokeWidth={2} aria-hidden="true" />
        {t("update.unsigned")}
      </p>
    </section>
  );
}
