import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { NotebookPen, Search } from "lucide-react";
import { Logo } from "../design/Logo";
import { useT } from "../app/i18n";
import { isTauri } from "../app/platform";
import "./TitleBar.css";

/**
 * The window titlebar (§82).
 *
 * The window is undecorated so this strip can do real work: it carries product
 * identity, the current location, and the command palette — the primary
 * keyboard entry point into the whole product (§50).
 */
export function TitleBar({
  onOpenPalette,
  onOpenNotebook,
  notifications,
}: {
  /** Absent while the auth screen is up: the palette is gated there, and a
      control that reads as a search field and answers nothing is exactly the
      "stubbed to look finished" this product does not do (§81). */
  onOpenPalette?: () => void;
  /** The Notebook (M19) — a callback rather than the component, for the same
      reason the bell is passed in: the titlebar draws window chrome and should
      not have to know what a notebook is. */
  onOpenNotebook?: () => void;
  /** The notification bell (§49), passed in rather than reached for: the
      titlebar draws window chrome and should not have to know what a
      notification is. */
  notifications?: React.ReactNode;
}) {
  const t = useT();
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!isTauri()) return;
    const win = getCurrentWindow();
    void win.isMaximized().then(setMaximized);
    const unlisten = win.onResized(() => {
      void win.isMaximized().then(setMaximized);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  const control = (action: "minimize" | "toggle" | "close") => async () => {
    if (!isTauri()) return;
    const win = getCurrentWindow();
    if (action === "minimize") return win.minimize();
    if (action === "close") return win.close();
    const next = await win.isMaximized();
    await (next ? win.unmaximize() : win.maximize());
    setMaximized(!next);
  };

  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="titlebar__identity" data-tauri-drag-region>
        <Logo size={15} className="titlebar__mark" />
        <span className="titlebar__wordmark" data-tauri-drag-region>
          J.A.R.V.I.S.
        </span>
      </div>

      {/* The palette trigger reads as a search field but is a button: it opens
          the palette rather than accepting inline text, so focus never gets
          trapped in the chrome. */}
      {onOpenPalette ? (
        <button type="button" className="titlebar__palette" onClick={onOpenPalette}>
          <Search size={13} strokeWidth={2} />
          <span className="titlebar__palette-label">{t("window.search")}</span>
          <kbd className="titlebar__kbd">Ctrl K</kbd>
        </button>
      ) : (
        // The strip still has to hold its shape, or the caption buttons slide
        // left and the window chrome visibly reflows on the way in and out.
        <span className="titlebar__palette-spacer" data-tauri-drag-region />
      )}

      <div className="titlebar__controls">
        {/* Beside the bell, because both are the same kind of thing: a place
            that is always one click away regardless of where you are. Absent
            during onboarding, exactly as the bell is — a door to a library of
            prompts, on a screen that exists to ask one question. */}
        {onOpenNotebook && (
          <button
            type="button"
            className="titlebar__notebook"
            onClick={onOpenNotebook}
            aria-label={t("notebook.open")}
            title={`${t("notebook.open")} — ${t("notebook.shortcut")}`}
          >
            <NotebookPen size={14} strokeWidth={1.9} aria-hidden="true" />
          </button>
        )}
        {notifications}
        <button
          type="button"
          className="titlebar__control"
          onClick={control("minimize")}
          aria-label={t("window.minimize")}
          title={t("window.minimize")}
        >
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <rect x="0" y="4.5" width="10" height="1" fill="currentColor" />
          </svg>
        </button>
        <button
          type="button"
          className="titlebar__control"
          onClick={control("toggle")}
          aria-label={maximized ? t("window.restore") : t("window.maximize")}
          title={maximized ? t("window.restore") : t("window.maximize")}
        >
          {maximized ? (
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
              <rect x="0.5" y="2.5" width="7" height="7" fill="none" stroke="currentColor" />
              <path d="M2.5 2.5V0.5H9.5V7.5H7.5" fill="none" stroke="currentColor" />
            </svg>
          ) : (
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
              <rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" />
            </svg>
          )}
        </button>
        <button
          type="button"
          className="titlebar__control titlebar__control--close"
          onClick={control("close")}
          aria-label={t("window.close")}
          title={t("window.close")}
        >
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <path d="M0.5 0.5L9.5 9.5M9.5 0.5L0.5 9.5" stroke="currentColor" fill="none" />
          </svg>
        </button>
      </div>
    </header>
  );
}
