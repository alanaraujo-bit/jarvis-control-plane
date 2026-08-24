import { CircleDot, TriangleAlert } from "lucide-react";
import { useT } from "../app/i18n";
import { isTauri } from "../app/platform";
import { useEnvironment } from "../surfaces/environment/useEnvironment";
import { useAppVersion } from "../app/version";
import { QuotaChips } from "./QuotaChips";
import "./StatusBar.css";

/**
 * Status bar.
 *
 * Reports only things that are true right now and worth a glance: whether the
 * environment is usable, and whether this is the real desktop shell or the
 * browser preview harness. It is not a place to park decorative counters (§7).
 */
export function StatusBar({ onOpenAccounts }: { onOpenAccounts: () => void }) {
  const t = useT();
  const { report, loading } = useEnvironment();
  const version = useAppVersion();

  const missingRequired =
    report?.tools.filter((tool) => tool.importance === "required" && tool.state !== "ready") ?? [];

  return (
    <footer className="statusbar">
      <div className="statusbar__side">
        {loading ? (
          <span className="statusbar__item statusbar__item--muted">{t("env.scanning")}</span>
        ) : missingRequired.length > 0 ? (
          <span className="statusbar__item statusbar__item--danger">
            <TriangleAlert size={12} strokeWidth={2} aria-hidden="true" />
            {t("env.someMissing")}
          </span>
        ) : (
          <span className="statusbar__item">
            <CircleDot size={12} strokeWidth={2} className="statusbar__dot" aria-hidden="true" />
            {t("env.allReady")}
          </span>
        )}
      </div>

      <div className="statusbar__side statusbar__side--end">
        <QuotaChips onOpen={onOpenAccounts} />
        {!isTauri() && (
          // Honest marker: this build is rendering against fixtures, not the
          // Rust core. It must never be mistaken for the real integration (§80).
          <span className="statusbar__item statusbar__badge">Preview — fixtures</span>
        )}
        {version && <span className="statusbar__item statusbar__item--muted">{version}</span>}
      </div>
    </footer>
  );
}
