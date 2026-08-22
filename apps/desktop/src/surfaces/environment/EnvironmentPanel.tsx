import { useState } from "react";
import { Check, Copy, ExternalLink, RefreshCw } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useT } from "../../app/i18n";
import { isTauri } from "../../app/platform";
import { useEnvironment } from "./useEnvironment";
import type { ToolReport, ToolState } from "@jarvis/protocol";
import type { MessageKey } from "@jarvis/i18n";
import "./EnvironmentPanel.css";

const STATE_LABEL: Record<ToolState, MessageKey> = {
  ready: "env.ready",
  missing: "env.missing",
  degraded: "env.degraded",
};

const IMPORTANCE_LABEL = {
  required: "env.required",
  recommended: "env.recommended",
  optional: "env.optional",
} as const satisfies Record<string, MessageKey>;

/**
 * Environment scan (§14).
 *
 * When something is missing, the panel hands the user the exact command to fix
 * it rather than pointing at external documentation.
 */
export function EnvironmentPanel() {
  const t = useT();
  const { report, loading, error, scan } = useEnvironment();

  return (
    <section className="env">
      <header className="env__header">
        <div>
          <h2 className="env__title">{t("env.title")}</h2>
          {report && (
            <p className="env__subtitle">
              {report.ready ? t("env.allReady") : t("env.someMissing")}
            </p>
          )}
        </div>
        <button
          type="button"
          className="env__rescan"
          onClick={() => void scan(true)}
          disabled={loading}
        >
          <RefreshCw
            size={13}
            strokeWidth={2}
            className={loading ? "env__spin" : undefined}
            aria-hidden="true"
          />
          {t("env.rescan")}
        </button>
      </header>

      {error && <p className="env__error">{error}</p>}

      <ul className="env__list">
        {loading && !report
          ? Array.from({ length: 6 }, (_, i) => (
              <li key={i} className="env__row env__row--skeleton" aria-hidden="true">
                <span className="env__skeleton env__skeleton--dot" />
                <span className="env__skeleton env__skeleton--name" />
                <span className="env__skeleton env__skeleton--meta" />
              </li>
            ))
          : report?.tools.map((tool) => (
              <ToolRow key={tool.id} tool={tool} stateLabel={t(STATE_LABEL[tool.state])} />
            ))}
      </ul>
    </section>
  );
}

function ToolRow({ tool, stateLabel }: { tool: ToolReport; stateLabel: string }) {
  const t = useT();
  const [copied, setCopied] = useState(false);

  const copyHint = async () => {
    if (!tool.installHint) return;
    try {
      await navigator.clipboard.writeText(tool.installHint);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      // Clipboard can be denied; the command stays visible and selectable.
    }
  };

  const open = (url: string) => {
    void (isTauri() ? openUrl(url) : window.open(url, "_blank", "noopener"));
  };

  return (
    <li className="env__row" data-state={tool.state}>
      <span className="env__dot" aria-hidden="true" />

      <div className="env__identity">
        <span className="env__name">{tool.name}</span>
        {tool.importance === "required" && (
          <span className="env__tag">{t(IMPORTANCE_LABEL[tool.importance])}</span>
        )}
      </div>

      <div className="env__detail">
        {tool.version && <span className="env__version mono">{tool.version}</span>}
        {tool.authenticated !== null && (
          <span className="env__auth" data-on={tool.authenticated || undefined}>
            {tool.authenticated ? t("env.signedIn") : t("env.signedOut")}
          </span>
        )}
        {tool.path && (
          <span className="env__path selectable" title={tool.path}>
            {tool.path}
          </span>
        )}
        {tool.detail && <span className="env__message">{tool.detail}</span>}
      </div>

      <span className="env__state">{stateLabel}</span>

      {tool.state !== "ready" && (
        <div className="env__actions">
          {tool.installHint && (
            <button type="button" className="env__action" onClick={() => void copyHint()}>
              {copied ? <Check size={12} strokeWidth={2.2} /> : <Copy size={12} strokeWidth={2} />}
              <code className="env__command">{tool.installHint}</code>
            </button>
          )}
          {tool.installUrl && (
            <button
              type="button"
              className="env__action env__action--icon"
              onClick={() => open(tool.installUrl!)}
              aria-label={t("env.learnMore")}
              title={t("env.learnMore")}
            >
              <ExternalLink size={12} strokeWidth={2} />
            </button>
          )}
        </div>
      )}
    </li>
  );
}
