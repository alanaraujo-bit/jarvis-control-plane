/**
 * Local model (§92).
 *
 * The sibling of Accounts, for a provider that has no account. Accounts asks
 * "how much allowance is left"; there is no allowance here, so this screen asks
 * the three questions that actually decide whether a local session will be any
 * good, in the order somebody asks them:
 *
 * 1. **Is it there** — a server answering, a runner installed, a model chosen.
 * 2. **Will it be fast** — is the model entirely in VRAM, what context window
 *    was it really loaded with, and is the card being held back right now.
 * 3. **What will a session be allowed to do** — sandbox, approvals, how long
 *    the weights stay resident.
 *
 * Everything shown is measured. Where a number could not be obtained it is an
 * em dash, never a plausible stand-in: the entire reason this screen exists is
 * that a local model has no billing page to check the numbers against.
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { Check, Cpu, HardDrive, RefreshCw, Wrench } from "lucide-react";
import { useI18n, useT } from "../../app/i18n";
import { isTauri } from "../../app/platform";
import { GpuPanel } from "./GpuPanel";
import {
  effectiveContext,
  gigabytes,
  residentFor,
  useLocalRuntime,
  type ApprovalPolicy,
  type InstalledModel,
  type SandboxMode,
} from "./useLocalRuntime";
import "./LocalRuntime.css";

/** Fast enough to feel live while a model loads, slow enough to be free. */
const POLL_MS = 2_000;

const KEEP_ALIVE_CHOICES = [0, 5, 30, 120, -1];
const SANDBOXES: SandboxMode[] = ["read-only", "workspace-write", "danger-full-access"];
const APPROVALS: ApprovalPolicy[] = ["untrusted", "on-failure", "on-request", "never"];

const SERVER_LABELS: Record<string, "localAi.contextLengthLabel" | "localAi.flashAttentionLabel" | "localAi.kvCacheLabel"> = {
  OLLAMA_CONTEXT_LENGTH: "localAi.contextLengthLabel",
  OLLAMA_FLASH_ATTENTION: "localAi.flashAttentionLabel",
  OLLAMA_KV_CACHE_TYPE: "localAi.kvCacheLabel",
};

export function LocalRuntime() {
  const t = useT();
  const { locale } = useI18n();
  const report = useLocalRuntime((state) => state.report);
  const gpu = useLocalRuntime((state) => state.gpu);
  const busyModel = useLocalRuntime((state) => state.busyModel);
  const notice = useLocalRuntime((state) => state.notice);
  const refresh = useLocalRuntime((state) => state.refresh);
  const save = useLocalRuntime((state) => state.save);
  const load = useLocalRuntime((state) => state.load);
  const unload = useLocalRuntime((state) => state.unload);
  const dismissNotice = useLocalRuntime((state) => state.dismissNotice);

  useEffect(() => {
    if (!isTauri()) return;
    void refresh();
    const timer = window.setInterval(() => void refresh(), POLL_MS);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const config = report?.config;
  const context = useMemo(() => effectiveContext(report), [report]);

  const update = useCallback(
    (patch: Partial<NonNullable<typeof config>>) => {
      if (!config) return;
      void save({ ...config, ...patch });
    },
    [config, save],
  );

  return (
    <div className="local">
      <div className="local__inner">
        <header className="local__header">
          <div>
            <h1>{t("localAi.title")}</h1>
            <p>{t("localAi.subtitle")}</p>
          </div>
          <button type="button" className="local__ghost" onClick={() => void refresh()}>
            <RefreshCw size={13} strokeWidth={1.9} aria-hidden="true" />
            {t("common.retry")}
          </button>
        </header>

        {notice && (
          <p className="local__notice" role="status">
            {t(notice as "localAi.restartNeeded")}
            <button type="button" onClick={dismissNotice}>
              {t("common.dismiss")}
            </button>
          </p>
        )}

        <section className="local__card">
          <header className="local__card-head">
            <h2>{t("localAi.server")}</h2>
            <span className="local__state" data-online={report?.reachable || undefined}>
              {report?.reachable ? t("localAi.online") : t("localAi.offline")}
            </span>
          </header>

          <dl className="local__facts">
            <div className="local__fact">
              <dt>{t("localAi.endpoint")}</dt>
              <dd>
                <strong>{config?.endpoint ?? "—"}</strong>
                <span>{t("localAi.endpointHelp")}</span>
              </dd>
            </div>
            <div className="local__fact">
              <dt>{t("localAi.version")}</dt>
              <dd>
                <strong>{report?.serverVersion ?? "—"}</strong>
              </dd>
            </div>
            <div className="local__fact">
              <dt>{t("localAi.effectiveContext")}</dt>
              <dd>
                <strong>
                  {context ? new Intl.NumberFormat(locale).format(context.tokens) : "—"}
                </strong>
                {/* Which of the two answers this is matters: one was measured
                    on the process that will serve the request, the other is a
                    setting that has not been proven to apply to anything. */}
                <span>
                  {context?.source === "runner"
                    ? t("localAi.contextFromRunner")
                    : context?.source === "server"
                      ? t("localAi.contextFromServer")
                      : t("localAi.contextUnknown")}
                </span>
              </dd>
            </div>
          </dl>

          {report && !report.reachable && (
            <p className="local__warn">
              {t("localAi.unreachableHelp", { endpoint: config?.endpoint ?? "" })}
            </p>
          )}
          {report && !report.runnerInstalled && (
            <p className="local__warn">{t("localAi.runnerMissing")}</p>
          )}
        </section>

        <GpuPanel gpu={gpu} locale={locale} />

        <section className="local__card">
          <header className="local__card-head">
            <h2>{t("localAi.models")}</h2>
          </header>

          {report && report.models.length === 0 ? (
            <p className="local__muted">{t("localAi.noModels")}</p>
          ) : (
            <ul className="local__models">
              {report?.models.map((model) => (
                <ModelRow
                  key={model.name}
                  model={model}
                  locale={locale}
                  active={config?.model === model.name}
                  resident={report.resident.find((entry) => entry.name === model.name) ?? null}
                  busy={busyModel === model.name}
                  onUse={() => update({ model: model.name })}
                  onLoad={() => void load(model.name)}
                  onUnload={() => void unload(model.name)}
                />
              ))}
            </ul>
          )}

          <p className="local__muted local__muted--tight">{t("localAi.noAccount")}</p>
        </section>

        <section className="local__card">
          <header className="local__card-head">
            <h2>{t("localAi.session")}</h2>
          </header>

          <div className="local__settings">
            <Choice
              label={t("localAi.keepAlive")}
              help={t("localAi.keepAliveHelp")}
              value={String(config?.keepAliveMinutes ?? 30)}
              options={KEEP_ALIVE_CHOICES.map((minutes) => ({
                value: String(minutes),
                label:
                  minutes < 0
                    ? t("localAi.keepAliveForever")
                    : minutes === 0
                      ? t("localAi.keepAliveImmediate")
                      : t("localAi.keepAliveFor", { count: minutes }),
              }))}
              onChange={(value) => update({ keepAliveMinutes: Number.parseInt(value, 10) })}
            />

            <Choice
              label={t("localAi.sandbox")}
              value={config?.sandbox ?? "workspace-write"}
              options={SANDBOXES.map((mode) => ({
                value: mode,
                label: t(
                  mode === "read-only"
                    ? "localAi.sandbox.readOnly"
                    : mode === "workspace-write"
                      ? "localAi.sandbox.workspaceWrite"
                      : "localAi.sandbox.dangerFullAccess",
                ),
              }))}
              onChange={(value) => update({ sandbox: value as SandboxMode })}
            />

            <Choice
              label={t("localAi.approval")}
              value={config?.approval ?? "on-request"}
              options={APPROVALS.map((policy) => ({
                value: policy,
                label: t(
                  policy === "untrusted"
                    ? "localAi.approval.untrusted"
                    : policy === "on-failure"
                      ? "localAi.approval.onFailure"
                      : policy === "on-request"
                        ? "localAi.approval.onRequest"
                        : "localAi.approval.never",
                ),
              }))}
              onChange={(value) => update({ approval: value as ApprovalPolicy })}
            />

            <label className="local__toggle">
              <input
                type="checkbox"
                checked={config?.preloadOnStart ?? true}
                onChange={(event) => update({ preloadOnStart: event.target.checked })}
              />
              <span>
                {t("localAi.preload")}
                <em>{t("localAi.preloadHelp")}</em>
              </span>
            </label>
          </div>

          <p className="local__path">
            <span>{t("localAi.configRoot")}</span>
            <code>{report?.codexHome ?? "—"}</code>
            <em>{t("localAi.configRootHelp")}</em>
          </p>
        </section>

        <ServerSettings />
      </div>
    </div>
  );
}

function ModelRow({
  model,
  locale,
  active,
  resident,
  busy,
  onUse,
  onLoad,
  onUnload,
}: {
  model: InstalledModel;
  locale: string;
  active: boolean;
  resident: ReturnType<typeof residentFor>;
  busy: boolean;
  onUse: () => void;
  onLoad: () => void;
  onUnload: () => void;
}) {
  const t = useT();
  const tools = model.capabilities.includes("tools");
  const gpuShare = resident && resident.sizeBytes > 0 ? resident.sizeVramBytes / resident.sizeBytes : null;

  return (
    <li className="local__model" data-active={active || undefined}>
      <div className="local__model-head">
        <strong>{model.name}</strong>
        <span className="local__tags">
          {model.parameterSize && <span className="local__tag">{model.parameterSize}</span>}
          {model.quantization && <span className="local__tag">{model.quantization}</span>}
          {model.capabilities.map((capability) => (
            <span key={capability} className="local__tag local__tag--soft">
              {capability}
            </span>
          ))}
        </span>
      </div>

      <div className="local__model-facts">
        <span>
          <HardDrive size={12} strokeWidth={1.8} aria-hidden="true" />
          {t("localAi.onDisk")} {gigabytes(model.sizeBytes, locale)}
        </span>
        {model.maxContext != null && (
          <span>
            {t("localAi.maxContext")} {new Intl.NumberFormat(locale).format(model.maxContext)}
          </span>
        )}
        <span className="local__residency" data-resident={resident ? true : undefined}>
          <Cpu size={12} strokeWidth={1.8} aria-hidden="true" />
          {resident ? t("localAi.resident") : t("localAi.notResident")}
        </span>
      </div>

      {/* The reading that predicts a bad session before it starts. */}
      {resident && gpuShare != null && (
        <p className="local__spill" data-spilled={gpuShare < 1 || undefined}>
          {gpuShare >= 1
            ? t("localAi.fullyOnGpu")
            : t("localAi.spilled", { percent: Math.round(gpuShare * 100) })}
        </p>
      )}

      {!tools && <p className="local__warn local__warn--inline">{t("localAi.notAnAgent")}</p>}

      <div className="local__model-actions">
        <button
          type="button"
          className="local__ghost"
          disabled={active || !tools}
          onClick={onUse}
          // A model with no tool support is not offered for sessions at all:
          // it would produce an agent that narrates edits it never makes.
          title={tools ? undefined : t("localAi.notAnAgent")}
        >
          {active ? <Check size={13} strokeWidth={2} aria-hidden="true" /> : null}
          {active ? t("localAi.inUse") : t("localAi.use")}
        </button>
        <button
          type="button"
          className="local__ghost"
          disabled={busy}
          onClick={resident ? onUnload : onLoad}
        >
          {busy ? t("localAi.working") : resident ? t("localAi.unload") : t("localAi.load")}
        </button>
      </div>
    </li>
  );
}

/**
 * The three knobs that are **not** ours to set per session.
 *
 * Ollama reads them once, at startup, and they apply to every client of the
 * server. Presenting them beside the per-session settings without saying so
 * would imply this app can change the context window for its own sessions,
 * which it cannot.
 */
function ServerSettings() {
  const t = useT();
  const report = useLocalRuntime((state) => state.report);
  const setServerEnv = useLocalRuntime((state) => state.setServerEnv);
  const [drafts, setDrafts] = useState<Record<string, string>>({});

  if (!report) return null;

  return (
    <section className="local__card">
      <header className="local__card-head">
        <h2>{t("localAi.serverSettings")}</h2>
      </header>
      <p className="local__muted local__muted--tight">{t("localAi.serverSettingsHelp")}</p>

      <div className="local__settings">
        {report.serverEnv.map((entry) => {
          const labelKey = SERVER_LABELS[entry.key];
          const draft = drafts[entry.key] ?? entry.value ?? "";
          return (
            <label key={entry.key} className="local__field">
              <span>
                {labelKey ? t(labelKey) : entry.key}
                <code>{entry.key}</code>
              </span>
              <span className="local__field-row">
                <input
                  value={draft}
                  placeholder={entry.fallback ?? ""}
                  onChange={(event) =>
                    setDrafts((current) => ({ ...current, [entry.key]: event.target.value }))
                  }
                />
                <button
                  type="button"
                  className="local__ghost"
                  disabled={draft.trim() === (entry.value ?? "").trim() || draft.trim() === ""}
                  onClick={() => void setServerEnv(entry.key, draft.trim())}
                >
                  <Wrench size={13} strokeWidth={1.9} aria-hidden="true" />
                  {t("common.confirm")}
                </button>
              </span>
              {!entry.value && entry.fallback && (
                <em>{t("localAi.notSet", { value: entry.fallback })}</em>
              )}
              {/* Where the number came from, which is the most this app can
                  honestly say. It cannot see the environment of a server that
                  started before it did, so an inherited value is reported as
                  inherited rather than presented as a saved setting. */}
              {entry.value && !entry.persisted && <em>{t("localAi.inherited")}</em>}
            </label>
          );
        })}
      </div>
    </section>
  );
}

function Choice({
  label,
  help,
  value,
  options,
  onChange,
}: {
  label: string;
  help?: string;
  value: string;
  options: { value: string; label: string }[];
  onChange: (value: string) => void;
}) {
  return (
    <label className="local__field">
      <span>{label}</span>
      <select value={value} onChange={(event) => onChange(event.target.value)}>
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
      {help && <em>{help}</em>}
    </label>
  );
}
