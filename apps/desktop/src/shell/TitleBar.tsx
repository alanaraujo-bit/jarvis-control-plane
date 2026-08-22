import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Search } from "lucide-react";
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
export function TitleBar({ onOpenPalette }: { onOpenPalette: () => void }) {
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
      <button type="button" className="titlebar__palette" onClick={onOpenPalette}>
        <Search size={13} strokeWidth={2} />
        <span className="titlebar__palette-label">{t("window.search")}</span>
        <kbd className="titlebar__kbd">Ctrl K</kbd>
      </button>

      <div className="titlebar__controls">
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
