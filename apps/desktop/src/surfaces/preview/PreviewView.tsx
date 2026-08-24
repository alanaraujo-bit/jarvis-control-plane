import type { MessageKey } from "@jarvis/i18n";
import { ExternalLink, MonitorPlay, RefreshCw, X } from "lucide-react";

import { useT } from "../../app/i18n";
import { usePreview } from "./usePreview";
import "./PreviewView.css";

interface PreviewViewProps {
  /** The session whose output is watched for a dev server. */
  sessionId?: string;
  /** False while another area is on screen — see `usePreview`. */
  active: boolean;
}

/** Localise a stable code from the core, or say plainly that it failed (§65). */
function message(t: (key: MessageKey) => string, code: string): string {
  const known = ["notLocal", "invalidUrl"];
  const tail = code.split(".").pop() ?? "";
  return known.includes(tail) ? t(`preview.${tail}` as MessageKey) : t("preview.failed");
}

/**
 * Preview (§46) — the *see* step of ask → modify → run → see → inspect → fix.
 *
 * The surface is deliberately small. It is not a browser: it is the shortest
 * path from "the agent started a server" to "I am looking at it", and every
 * control here serves that or is absent.
 *
 * **Nothing opens on its own.** The URL is detected automatically because the
 * product already has the output to read it from, but opening a window is
 * always a click. An agent restarting a dev server must never yank a window
 * onto someone's screen, and a surface that opens things unbidden is one people
 * learn to distrust.
 */
export function PreviewView({ sessionId, active }: PreviewViewProps) {
  const t = useT();
  const { detected, open, error, openPreview, reload, close } = usePreview(sessionId, active);

  // Everything after the scheme: the scheme is noise once you know it is a
  // local address, and the port is the part a person actually recognises.
  const short = (url: string) => url.replace(/^https?:\/\//, "");

  return (
    <div className="preview">
      <div className="preview__inner">
        <header className="preview__header">
          <h2 className="preview__title">
            <MonitorPlay size={15} strokeWidth={1.75} aria-hidden="true" />
            {t("preview.title")}
          </h2>
          <p className="preview__subtitle">{t("preview.openWindow")}</p>
        </header>

        {error && <p className="preview__error">{message(t, error)}</p>}

        {detected.url ? (
          <>
            <div className="preview__found">
              <span className="preview__url selectable">{short(detected.url)}</span>
              <div className="preview__actions">
                <button
                  type="button"
                  className="preview__primary"
                  onClick={() => void openPreview(detected.url as string)}
                >
                  <ExternalLink size={13} strokeWidth={1.9} aria-hidden="true" />
                  {t("preview.open")}
                </button>
                {/* Reload and Close appear only while a window is actually
                    open — offering to reload nothing is a dead control. */}
                {open && (
                  <>
                    <button type="button" className="preview__action" onClick={() => void reload()}>
                      <RefreshCw size={13} strokeWidth={1.9} aria-hidden="true" />
                      {t("preview.reload")}
                    </button>
                    <button type="button" className="preview__action" onClick={() => void close()}>
                      <X size={13} strokeWidth={2} aria-hidden="true" />
                      {t("preview.close")}
                    </button>
                  </>
                )}
              </div>
            </div>

            {/* Only when there is a genuine choice to make. A repository
                running an API and a web app in one terminal prints two
                addresses, and picking one silently would be picking wrong
                half the time. */}
            {detected.all.length > 1 && (
              <div className="preview__others">
                <p className="preview__others-title">{t("preview.choose")}</p>
                <ul className="preview__list">
                  {detected.all
                    .filter((url) => url !== detected.url)
                    .map((url) => (
                      <li key={url}>
                        <button
                          type="button"
                          className="preview__other"
                          onClick={() => void openPreview(url)}
                        >
                          {short(url)}
                        </button>
                      </li>
                    ))}
                </ul>
              </div>
            )}
          </>
        ) : (
          /* Not an error and not a spinner: a session with no server running
             is the ordinary state, and this says what to do about it. */
          <div className="preview__empty">
            <p className="preview__empty-title">{t("preview.searching")}</p>
            <p className="preview__empty-body">{t("preview.hint", { name: "J.A.R.V.I.S." })}</p>
          </div>
        )}
      </div>
    </div>
  );
}
