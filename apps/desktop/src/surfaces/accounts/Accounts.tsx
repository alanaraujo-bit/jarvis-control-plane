import { useEffect, useMemo, useState, type FormEvent } from "react";
import {
  Check,
  Clock3,
  LogIn,
  Pause,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  ShieldAlert,
  Trash2,
  X,
} from "lucide-react";
import type { MessageKey } from "@jarvis/i18n";
import { useI18n, useT } from "../../app/i18n";
import {
  useAccounts,
  type AccountCard,
  type AutoSwitchPolicy,
  type ProviderId,
  type QuotaWindow,
} from "./useAccounts";
import "./Accounts.css";

const PROVIDERS: ProviderId[] = ["claude-code", "codex"];
const POLICIES: AutoSwitchPolicy[] = ["off", "onExhaustion", "onThreshold"];

function formatTokens(value: number, locale: string) {
  return new Intl.NumberFormat(locale, { notation: "compact", maximumFractionDigits: 1 }).format(
    value,
  );
}

function countdown(target: number | null, now: number, t: ReturnType<typeof useT>) {
  if (!target) return null;
  const remaining = Math.max(0, target - now);
  if (remaining === 0) return t("accounts.reset.now");
  const minutes = Math.ceil(remaining / 60_000);
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return hours > 0
    ? t("accounts.reset.hours", { hours, minutes: rest })
    : t("accounts.reset.minutes", { minutes });
}

function WindowMeter({ window, now }: { window: QuotaWindow; now: number }) {
  const t = useT();
  const { locale } = useI18n();
  const reset = countdown(window.resetsAtMs, now, t);
  const nameKey = `accounts.window.${window.window}` as MessageKey;
  const confidenceKey = `accounts.confidence.${window.confidence}` as MessageKey;

  return (
    <div className="accounts__window" data-confidence={window.confidence}>
      <div className="accounts__window-head">
        <span className="accounts__window-name">{t(nameKey)}</span>
        <span className="accounts__window-value">
          {window.percent == null
            ? t("accounts.tokens.used", { tokens: formatTokens(window.tokens, locale) })
            : `${Math.round(window.percent)}%`}
        </span>
      </div>

      {window.percent != null ? (
        <div
          className="accounts__meter"
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(window.percent)}
          aria-label={`${t(nameKey)} — ${t(confidenceKey)}`}
        >
          <span
            className="accounts__meter-fill"
            style={{ width: `${Math.max(1, Math.min(100, window.percent))}%` }}
          />
        </div>
      ) : (
        <div className="accounts__unknown-line" aria-hidden="true" />
      )}

      <div className="accounts__window-foot">
        <span className="accounts__confidence" data-confidence={window.confidence}>
          {t(confidenceKey)}
        </span>
        {window.confidence === "estimated" && window.calibrationSamples > 0 && (
          <span>
            {t("accounts.calibration", { count: window.calibrationSamples })}
          </span>
        )}
        {reset && (
          <span className="accounts__reset">
            <Clock3 size={11} aria-hidden="true" />
            {reset}
          </span>
        )}
      </div>
    </div>
  );
}

function AccountActions({ card, canPause }: { card: AccountCard; canPause: boolean }) {
  const t = useT();
  const { account, quota } = card;
  const activate = useAccounts((state) => state.activate);
  const pause = useAccounts((state) => state.pause);
  const remove = useAccounts((state) => state.remove);
  const rename = useAccounts((state) => state.rename);
  const beginSignIn = useAccounts((state) => state.beginSignIn);
  const [editing, setEditing] = useState(false);
  const [label, setLabel] = useState(account.label);

  const saveRename = async (event: FormEvent) => {
    event.preventDefault();
    if (!label.trim()) return;
    await rename(account.id, label);
    setEditing(false);
  };

  if (editing) {
    return (
      <form className="accounts__rename" onSubmit={(event) => void saveRename(event)}>
        <input
          autoFocus
          value={label}
          onChange={(event) => setLabel(event.target.value)}
          aria-label={t("accounts.rename")}
        />
        <button type="submit" className="accounts__icon-button" aria-label={t("common.confirm")}>
          <Check size={14} />
        </button>
        <button
          type="button"
          className="accounts__icon-button"
          aria-label={t("common.cancel")}
          onClick={() => setEditing(false)}
        >
          <X size={14} />
        </button>
      </form>
    );
  }

  return (
    <div className="accounts__actions">
      {account.checkedAt === null || !account.signedIn ? (
        <button
          type="button"
          className="accounts__button accounts__button--primary"
          onClick={() => void beginSignIn(account.id, account.email ?? undefined)}
        >
          <LogIn size={13} />
          {t("accounts.signIn")}
        </button>
      ) : !account.active && !account.paused && quota.health !== "exhausted" ? (
        <button
          type="button"
          className="accounts__button accounts__button--primary"
          onClick={() => void activate(account.id)}
        >
          <Check size={13} />
          {t("accounts.useAccount")}
        </button>
      ) : null}
      <button
        type="button"
        className="accounts__button"
        onClick={() => void pause(account.id, !account.paused)}
        disabled={!canPause}
        title={!canPause ? t("accounts.pauseNeedsReplacement") : undefined}
      >
        {account.paused ? <Play size={13} /> : <Pause size={13} />}
        {account.paused ? t("accounts.resume") : t("accounts.pause")}
      </button>
      <button
        type="button"
        className="accounts__icon-button"
        aria-label={t("accounts.rename")}
        onClick={() => setEditing(true)}
      >
        <Pencil size={13} />
      </button>
      {!account.adopted && (
        <button
          type="button"
          className="accounts__icon-button accounts__icon-button--danger"
          aria-label={t("accounts.remove")}
          onClick={() => {
            if (
              window.confirm(
                t("accounts.removeConfirm", { name: account.label || account.email || "" }),
              )
            ) {
              void remove(account.id);
            }
          }}
        >
          <Trash2 size={13} />
        </button>
      )}
    </div>
  );
}

function AccountCardView({
  card,
  now,
  canPause,
}: {
  card: AccountCard;
  now: number;
  canPause: boolean;
}) {
  const t = useT();
  const { locale } = useI18n();
  const { account, quota } = card;
  const verification =
    account.checkedAt === null
      ? "unverified"
      : account.signedIn
        ? quota.health
        : "signedOut";

  return (
    <article className="accounts__card" data-health={verification} data-active={account.active || undefined}>
      <header className="accounts__card-head">
        <div className="accounts__identity">
          <div className="accounts__name-line">
            <h2>{account.label || account.email || t("accounts.unnamed")}</h2>
            {account.active && <span className="accounts__active-badge">{t("accounts.active")}</span>}
          </div>
          {account.email && account.email !== account.label ? (
            <p>{account.email}</p>
          ) : !account.email ? (
            <p>{t("accounts.identityMissing")}</p>
          ) : null}
          <div className="accounts__meta">
            {account.plan && <span>{account.plan}</span>}
            {account.orgName && <span title={account.orgName}>{account.orgName}</span>}
            {account.adopted && <span>{t("accounts.machineAccount")}</span>}
          </div>
        </div>
        <span className="accounts__health" data-health={verification}>
          <span className="accounts__health-dot" />
          {t(`accounts.health.${verification}` as MessageKey)}
        </span>
      </header>

      <div className="accounts__windows">
        {quota.windows.map((window) => (
          <WindowMeter key={window.window} window={window} now={now} />
        ))}
      </div>

      <div className="accounts__facts">
        <span>
          {t("accounts.tokens.today", { tokens: formatTokens(quota.tokensToday, locale) })}
        </span>
        {quota.liveSessions > 0 && (
          <span className="accounts__live-note">
            {t("accounts.liveSessions", { count: quota.liveSessions })}
          </span>
        )}
      </div>

      {account.provider === "claude-code" && !account.adopted && (
        <div className="accounts__trust-note" data-warning={card.folderTrusted === false || undefined}>
          <ShieldAlert size={14} aria-hidden="true" />
          <span>
            {card.folderTrusted === false
              ? t("accounts.folderUntrusted")
              : t("accounts.trustIsPerAccount")}
          </span>
        </div>
      )}

      {quota.refusalDetail && <blockquote className="accounts__refusal">{quota.refusalDetail}</blockquote>}
      <AccountActions card={card} canPause={canPause} />
    </article>
  );
}

function AddAccount({ provider, onClose }: { provider: ProviderId; onClose: () => void }) {
  const t = useT();
  const create = useAccounts((state) => state.create);
  const [label, setLabel] = useState("");
  const [email, setEmail] = useState("");
  const [working, setWorking] = useState(false);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setWorking(true);
    try {
      await create(provider, label, provider === "claude-code" ? email : undefined);
      onClose();
    } finally {
      setWorking(false);
    }
  };

  return (
    <form className="accounts__add" onSubmit={(event) => void submit(event)}>
      <div>
        <h2>{t("accounts.add.title")}</h2>
        <p>{t("accounts.add.body")}</p>
      </div>
      <label>
        <span>{t("accounts.label")}</span>
        <input
          autoFocus
          value={label}
          onChange={(event) => setLabel(event.target.value)}
          placeholder={t("accounts.labelPlaceholder")}
        />
      </label>
      {provider === "claude-code" && (
        <label>
          <span>{t("accounts.emailOptional")}</span>
          <input
            type="email"
            value={email}
            onChange={(event) => setEmail(event.target.value)}
            placeholder="name@example.com"
          />
        </label>
      )}
      <div className="accounts__add-actions">
        <button type="button" className="accounts__button" onClick={onClose}>
          {t("common.cancel")}
        </button>
        <button type="submit" className="accounts__button accounts__button--primary" disabled={working}>
          {working ? t("accounts.openingLogin") : t("accounts.continueToLogin")}
        </button>
      </div>
    </form>
  );
}

export function Accounts({ projectId = null }: { projectId?: string | null }) {
  const t = useT();
  const report = useAccounts((state) => state.report);
  const loading = useAccounts((state) => state.loading);
  const refreshing = useAccounts((state) => state.refreshing);
  const error = useAccounts((state) => state.error);
  const load = useAccounts((state) => state.load);
  const setAutoSwitch = useAccounts((state) => state.setAutoSwitch);
  const [provider, setProvider] = useState<ProviderId>("claude-code");
  const [adding, setAdding] = useState(false);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    void load(true, projectId);
  }, [load, projectId]);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);

  const cards = useMemo(
    () => report?.accounts.filter((card) => card.account.provider === provider) ?? [],
    [provider, report],
  );

  return (
    <div className="accounts">
      <div className="accounts__inner">
        <header className="accounts__header">
          <div>
            <h1>{t("accounts.title")}</h1>
            <p>{t("accounts.subtitle")}</p>
          </div>
          <div className="accounts__header-actions">
            <button
              type="button"
              className="accounts__icon-button"
              aria-label={t("accounts.refresh")}
              onClick={() => void load(true)}
              disabled={refreshing}
            >
              <RefreshCw size={15} className={refreshing ? "accounts__spin" : undefined} />
            </button>
            <button
              type="button"
              className="accounts__button accounts__button--primary"
              onClick={() => setAdding(true)}
            >
              <Plus size={14} />
              {t("accounts.add")}
            </button>
          </div>
        </header>

        <div className="accounts__provider-tabs" role="tablist">
          {PROVIDERS.map((value) => (
            <button
              key={value}
              type="button"
              role="tab"
              aria-selected={provider === value}
              data-active={provider === value || undefined}
              onClick={() => {
                setProvider(value);
                setAdding(false);
              }}
            >
              {t(`accounts.provider.${value}` as MessageKey)}
              <span>{report?.accounts.filter((card) => card.account.provider === value).length ?? 0}</span>
            </button>
          ))}
        </div>

        {report && (
          <section className="accounts__automation">
            <div>
              <h2>{t("accounts.autoSwitch.title")}</h2>
              <p>
                {report.autoSwitch === "onThreshold"
                  ? t("accounts.autoSwitch.estimatedDisclosure", {
                      percent: report.thresholdPercent,
                    })
                  : t("accounts.autoSwitch.body")}
              </p>
            </div>
            <div className="accounts__policy" role="radiogroup">
              {POLICIES.map((policy) => (
                <button
                  key={policy}
                  type="button"
                  role="radio"
                  aria-checked={report.autoSwitch === policy}
                  data-active={report.autoSwitch === policy || undefined}
                  onClick={() => void setAutoSwitch(policy)}
                >
                  {t(`accounts.autoSwitch.${policy}` as MessageKey)}
                </button>
              ))}
            </div>
          </section>
        )}

        {error && <div className="accounts__error">{t("accounts.error")}</div>}
        {adding && <AddAccount provider={provider} onClose={() => setAdding(false)} />}

        {loading && !report ? (
          <div className="accounts__loading">{t("common.loading")}</div>
        ) : cards.length === 0 && !adding ? (
          <div className="accounts__empty">
            <p>{t("accounts.empty.title")}</p>
            <span>{t("accounts.empty.body")}</span>
          </div>
        ) : (
          <div className="accounts__grid">
            {cards.map((card) => (
              <AccountCardView
                key={card.account.id}
                card={card}
                now={now}
                canPause={
                  card.account.paused ||
                  !card.account.active ||
                  cards.some(
                    (candidate) =>
                      candidate.account.id !== card.account.id &&
                      candidate.account.signedIn &&
                      !candidate.account.paused,
                  )
                }
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
