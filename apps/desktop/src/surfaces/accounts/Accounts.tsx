/**
 * Accounts & usage (§66, M16).
 *
 * This screen exists to replace a habit: opening the provider's web UI, signing
 * in and out of four accounts, and adding up by hand what is left. So it is
 * built around the three questions that habit is asking, in the order they are
 * asked, and every card answers all three without a click:
 *
 * 1. **How much is left** — headroom, as a dial, because it is the number
 *    decisions are made on.
 * 2. **When does it come back** — a countdown *and* the wall-clock moment,
 *    because "in 4h" and "at 20:20" answer different questions.
 * 3. **Which window is holding me up** — the binding one is the dial; the rest
 *    are bars beneath it. On this machine the five-hour window is at 5% while
 *    the weekly sits at 99%, and a panel that shows both the same size makes
 *    that impossible to see.
 *
 * Where the numbers come from, and how sure we are, is on the card rather than
 * in a footnote (§28): a figure a provider stated a minute ago and one this
 * product inferred from its own history must not look alike.
 */

import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import {
  AlertTriangle,
  Check,
  Clock3,
  LogIn,
  Pause,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  ShieldAlert,
  Ticket,
  Trash2,
  X,
} from "lucide-react";
import type { MessageKey } from "@jarvis/i18n";
import { useI18n, useT } from "../../app/i18n";
import {
  useAccounts,
  type AccountCard,
  type AutoSwitchPolicy,
  type LiveReading,
  type LiveWindow,
  type ProviderId,
  type QuotaWindow,
} from "./useAccounts";
import { Bar, Gauge } from "./Gauge";
import {
  countdown,
  duration,
  formatMoney,
  formatTokens,
  nextTickDelay,
  readingAge,
  remaining,
  resetMoment,
  severityBand,
} from "./format";
import "./Accounts.css";

const PROVIDERS: ProviderId[] = ["claude-code", "codex"];
const POLICIES: AutoSwitchPolicy[] = ["off", "onExhaustion", "onThreshold"];

/**
 * A window's translated name, falling back to the provider's own spelling.
 *
 * Claude Code ships window codenames that rotate between releases, so an
 * untranslated kind is expected rather than a bug. Showing the raw name with
 * its scope label beats both hiding the window and printing a missing-key
 * placeholder — one loses information the person needs, the other looks broken.
 */
function windowName(
  window: LiveWindow,
  t: ReturnType<typeof useT>,
  known: Set<string>,
): string {
  if (window.scopeLabel) {
    return t("accounts.window.scoped", { scope: window.scopeLabel });
  }
  const key = `accounts.window.${window.kind}`;
  return known.has(key) ? t(key as MessageKey) : window.rawKind.replace(/_/g, " ");
}

const KNOWN_WINDOW_KEYS = new Set([
  "accounts.window.session",
  "accounts.window.weekly",
  "accounts.window.weeklyOpus",
  "accounts.window.weeklySonnet",
  "accounts.window.monthly",
  "accounts.window.five_hour",
  "accounts.window.seven_day",
]);

/**
 * Reason ids this build has words for.
 *
 * The catalogue is typed, so a key that is not in it is a compile error rather
 * than a missing string — which is the point of §65 and also why an id coming
 * from a provider cannot be handed to `t` directly. A provider that invents a
 * new reason must degrade to a sentence that is still true, not to a blank.
 */
const LIVE_REASONS = new Set([
  "signedOut",
  "notApplicable",
  "toolMissing",
  "timeout",
  "spawnFailed",
  "providerError",
  "wrongDirectory",
  "unreadable",
  "unsupported",
  "noOutput",
  "noInput",
]);

const SPEND_REASONS = new Set([
  "org_level_disabled",
  "org_level_disabled_until",
  "org_service_level_disabled",
  "out_of_credits",
  "member_zero_credit_limit",
  "user_disabled",
]);

/** Sort so the binding window leads and the rest follow shortest-first. */
function ordered(windows: LiveWindow[]): LiveWindow[] {
  const rank = (window: LiveWindow) =>
    window.group === "session" ? 0 : window.group === "weekly" ? 1 : 2;
  return [...windows].sort((a, b) => {
    if (a.binding !== b.binding) return a.binding ? -1 : 1;
    return rank(a) - rank(b);
  });
}

// ---------------------------------------------------------------------------
// One window, drawn two ways
// ---------------------------------------------------------------------------

function ResetLine({
  resetsAtMs,
  now,
  showMoment,
}: {
  resetsAtMs: number | null;
  now: number;
  showMoment: boolean;
}) {
  const t = useT();
  const { locale } = useI18n();
  const label = countdown(resetsAtMs, now, t);
  if (!label) return null;
  return (
    <span className="accounts__reset">
      <Clock3 size={11} aria-hidden="true" />
      <span>{label}</span>
      {showMoment && resetsAtMs !== null && (
        <span className="accounts__reset-moment">{resetMoment(resetsAtMs, now, locale)}</span>
      )}
    </span>
  );
}

/** The binding window: the dial, its name, and when it comes back. */
function BindingWindow({ window, now }: { window: LiveWindow; now: number }) {
  const t = useT();
  const left = remaining(window.percentUsed);
  const band = severityBand(window.severity, window.percentUsed);
  const name = windowName(window, t, KNOWN_WINDOW_KEYS);

  return (
    <div className="accounts__binding">
      <Gauge
        percent={left}
        band={band}
        value={String(Math.round(left))}
        unit="%"
        caption={t("accounts.left")}
        label={t("accounts.gaugeLabel", { window: name, percent: Math.round(left) })}
      />
      <div className="accounts__binding-detail">
        <div className="accounts__binding-name">
          <span className="accounts__window-name">{name}</span>
          <span className="accounts__binding-tag" data-source={window.bindingSource}>
            {window.bindingSource === "provider"
              ? t("accounts.binding.stated")
              : t("accounts.binding.inferred")}
          </span>
        </div>
        <span className="accounts__binding-used">
          {t("accounts.usedPercent", { percent: Math.round(window.percentUsed) })}
        </span>
      </div>
      {/* "When does it come back" was one of the three things Alan said he
          could not see, so on the window that is actually rationing him it gets
          its own column and its own type size rather than a line of caption. */}
      <ResetPanel resetsAtMs={window.resetsAtMs} now={now} />
    </div>
  );
}

/** The countdown, at a size you can read from across the desk. */
function ResetPanel({ resetsAtMs, now }: { resetsAtMs: number | null; now: number }) {
  const t = useT();
  const { locale } = useI18n();
  if (resetsAtMs === null) {
    return (
      <div className="accounts__resetpanel" data-empty="true">
        <span className="accounts__resetpanel-label">{t("accounts.resetPanel.label")}</span>
        <span className="accounts__resetpanel-unknown">{t("accounts.resetPanel.unknown")}</span>
      </div>
    );
  }
  const remainingMs = resetsAtMs - now;
  const { days, hours, minutes } = duration(remainingMs);
  return (
    <div className="accounts__resetpanel">
      <span className="accounts__resetpanel-label">{t("accounts.resetPanel.label")}</span>
      <span className="accounts__resetpanel-value">
        {remainingMs <= 0 ? (
          t("accounts.reset.now")
        ) : days > 0 ? (
          <>
            {days}
            <em>d</em> {hours}
            <em>h</em>
          </>
        ) : hours > 0 ? (
          <>
            {hours}
            <em>h</em> {minutes}
            <em>m</em>
          </>
        ) : (
          <>
            {minutes}
            <em>m</em>
          </>
        )}
      </span>
      <span className="accounts__resetpanel-moment">{resetMoment(resetsAtMs, now, locale)}</span>
    </div>
  );
}

/** Every other window: one row, one bar. */
function WindowRow({ window, now }: { window: LiveWindow; now: number }) {
  const t = useT();
  const left = remaining(window.percentUsed);
  const band = severityBand(window.severity, window.percentUsed);
  const name = windowName(window, t, KNOWN_WINDOW_KEYS);

  return (
    <li className="accounts__row">
      <div className="accounts__row-head">
        <span className="accounts__window-name">{name}</span>
        <span className="accounts__row-value" data-band={band}>
          {t("accounts.leftPercent", { percent: Math.round(left) })}
        </span>
      </div>
      <Bar
        percent={left}
        band={band}
        label={t("accounts.gaugeLabel", { window: name, percent: Math.round(left) })}
      />
      <ResetLine resetsAtMs={window.resetsAtMs} now={now} showMoment={false} />
    </li>
  );
}

/**
 * The fallback ladder, shown only when no provider answered.
 *
 * This is M13's original view and it is still the honest one when a probe
 * cannot run — an account mid-login, a CLI being upgraded, a machine offline.
 * It is kept visually quieter than a live card on purpose: an inferred
 * percentage that looks identical to a stated one is precisely what §28 forbids.
 */
function DerivedWindow({ window, now }: { window: QuotaWindow; now: number }) {
  const t = useT();
  const { locale } = useI18n();
  const confidenceKey = `accounts.confidence.${window.confidence}` as MessageKey;
  const nameKey = `accounts.window.${window.window}` as MessageKey;
  const left = window.percent === null ? null : remaining(window.percent);

  return (
    <li className="accounts__row accounts__row--derived" data-confidence={window.confidence}>
      <div className="accounts__row-head">
        <span className="accounts__window-name">{t(nameKey)}</span>
        <span className="accounts__row-value">
          {left === null
            ? t("accounts.tokens.used", { tokens: formatTokens(window.tokens, locale) })
            : t("accounts.leftPercent", { percent: Math.round(left) })}
        </span>
      </div>
      {left === null ? (
        <div className="accounts__unknown-line" aria-hidden="true" />
      ) : (
        <Bar
          percent={left}
          band={severityBand("", window.percent ?? 0)}
          label={t("accounts.gaugeLabel", {
            window: t(nameKey),
            percent: Math.round(left),
          })}
        />
      )}
      <div className="accounts__row-foot">
        <span className="accounts__confidence" data-confidence={window.confidence}>
          {t(confidenceKey)}
        </span>
        {window.confidence === "estimated" && window.calibrationSamples > 0 && (
          <span>{t("accounts.calibration", { count: window.calibrationSamples })}</span>
        )}
        <ResetLine resetsAtMs={window.resetsAtMs} now={now} showMoment={false} />
      </div>
    </li>
  );
}

// ---------------------------------------------------------------------------
// Spend
// ---------------------------------------------------------------------------

/**
 * Paid overage above the subscription — the "monthly limit" a refusal actually
 * names.
 *
 * The provider's own refusal sentence on this machine read "You've hit your
 * monthly spend limit", and a panel showing only five-hour and weekly windows
 * cannot explain that sentence. The currency comes from the provider (BRL here)
 * rather than being assumed.
 */
function SpendBlock({ reading }: { reading: LiveReading }) {
  const t = useT();
  const { locale } = useI18n();
  const spend = reading.spend;
  if (!spend) return null;

  // Codex reports a credit balance rather than a spend limit; there is no
  // "used of limit" to draw, so it is stated as a balance and nothing else.
  //
  // A balance of zero on an account with no credits is not a fact worth a row:
  // it looked, on screen, like a broken value rather than an absence. The free
  // resets Codex also grants are already stated beside the reading's age.
  if (spend.currency === "credits") {
    if (!spend.enabled && spend.limit <= 0) return null;
    return (
      <div className="accounts__spend">
        <span className="accounts__spend-label">{t("accounts.spend.credits")}</span>
        <span className="accounts__spend-value">{spend.limit}</span>
      </div>
    );
  }

  if (!spend.enabled) {
    return (
      <div className="accounts__spend" data-off="true">
        <span className="accounts__spend-label">{t("accounts.spend.title")}</span>
        <span className="accounts__spend-value">
          {spend.disabledReason && SPEND_REASONS.has(spend.disabledReason)
            ? t(`accounts.spend.reason.${spend.disabledReason}` as MessageKey)
            : t("accounts.spend.off")}
        </span>
      </div>
    );
  }

  const percent =
    spend.percentUsed ?? (spend.limit > 0 ? (spend.used / spend.limit) * 100 : 0);
  return (
    <div className="accounts__spend" data-reached={spend.limitReached || undefined}>
      <div className="accounts__row-head">
        <span className="accounts__spend-label">{t("accounts.spend.title")}</span>
        <span className="accounts__spend-value">
          {t("accounts.spend.ofLimit", {
            used: formatMoney(spend.used, spend.currency, spend.decimalPlaces, locale),
            limit: formatMoney(spend.limit, spend.currency, spend.decimalPlaces, locale),
          })}
        </span>
      </div>
      <Bar
        percent={remaining(percent)}
        band={severityBand("", percent)}
        label={t("accounts.spend.title")}
      />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Card
// ---------------------------------------------------------------------------

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

/**
 * The line that says where a card's numbers came from and how old they are.
 *
 * Load-bearing rather than decorative: without it a card that failed to refresh
 * shows yesterday's percentage with today's confidence, which is the most
 * expensive kind of wrong this screen can be.
 */
function LiveStamp({ card, now }: { card: AccountCard; now: number }) {
  const t = useT();
  const live = card.quota.live;
  const busy = useAccounts((state) => state.refreshingAccountId) === card.account.id;
  const refresh = useAccounts((state) => state.refreshAccount);

  const retry = (
    <button
      type="button"
      className="accounts__stamp-retry"
      onClick={() => void refresh(card.account.id)}
      disabled={busy}
    >
      <RefreshCw size={11} className={busy ? "accounts__spin" : undefined} aria-hidden="true" />
      {t("accounts.live.check")}
    </button>
  );

  if (live === null) {
    return (
      <div className="accounts__stamp" data-tone="idle">
        <span>{t("accounts.live.neverChecked")}</span>
        {retry}
      </div>
    );
  }

  if (live.state === "ok") {
    return (
      <div className="accounts__stamp" data-tone="live" data-stale={card.quota.liveStale || undefined}>
        <span className="accounts__stamp-official">{t("accounts.confidence.official")}</span>
        <span>{readingAge(live.reading.readAtMs, now, t)}</span>
        {live.reading.resetCredits > 0 && (
          <span className="accounts__credit">
            <Ticket size={11} aria-hidden="true" />
            {t("accounts.live.resetCredits", { count: live.reading.resetCredits })}
          </span>
        )}
        {retry}
      </div>
    );
  }

  // A settled "nothing here" is already the body of the card, so repeating the
  // sentence in the stamp would say the same thing twice on a card that has
  // almost nothing else on it. The stamp keeps only what the body cannot: when
  // it was asked, and the way to ask again.
  if (live.state === "unavailable") {
    return (
      <div className="accounts__stamp" data-tone="idle">
        <span>{readingAge(live.readAtMs, now, t)}</span>
        {retry}
      </div>
    );
  }

  return (
    <div className="accounts__stamp" data-tone="warn">
      <AlertTriangle size={11} aria-hidden="true" />
      <span>
        {LIVE_REASONS.has(live.reason)
          ? t(`accounts.live.reason.${live.reason}` as MessageKey)
          : t("accounts.live.failedGeneric")}
      </span>
      {retry}
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
  // A provider that just told us nobody is signed in here has said more than
  // "we have not checked". Seen on screen: a card whose body read "not signed
  // in" wearing a "Not verified" badge, which are two different claims about
  // the same directory.
  const verification =
    quota.live?.state === "unavailable" && quota.live.reason === "signedOut"
      ? "signedOut"
      : account.checkedAt === null
        ? "unverified"
        : account.signedIn
          ? quota.health
          : "signedOut";

  const reading = quota.live?.state === "ok" ? quota.live.reading : null;
  const windows = reading ? ordered(reading.windows) : [];
  const binding = windows.find((window) => window.binding) ?? windows[0] ?? null;
  const rest = windows.filter((window) => window !== binding);

  // A *settled* negative: the provider was asked and said there is no quota
  // here. Distinct from "the probe failed" and from "we have not asked", both
  // of which still deserve whatever the history can offer.
  const settled = quota.live?.state === "unavailable" ? quota.live.reason : null;

  return (
    <article
      className="accounts__card"
      data-health={verification}
      data-active={account.active || undefined}
    >
      <header className="accounts__card-head">
        <div className="accounts__identity">
          <div className="accounts__name-line">
            <h2>{account.label || account.email || t("accounts.unnamed")}</h2>
            {account.active && (
              <span className="accounts__active-badge">{t("accounts.active")}</span>
            )}
          </div>
          {account.email && account.email !== account.label ? (
            <p>{account.email}</p>
          ) : !account.email ? (
            <p>{t("accounts.identityMissing")}</p>
          ) : null}
          <div className="accounts__meta">
            {(reading?.plan ?? account.plan) && <span>{reading?.plan ?? account.plan}</span>}
            {account.orgName && <span title={account.orgName}>{account.orgName}</span>}
            {account.adopted && <span>{t("accounts.machineAccount")}</span>}
          </div>
        </div>
        {/* The status dot takes its colour from the binding window when there
            is a live reading. Seen on screen: a card at 97% used had a red dial
            beside an amber pill, because health has one "nearing" band and the
            dial has two. Two colours for one fact reads as a bug even when both
            sentences are true. */}
        <span
          className="accounts__health"
          data-health={verification}
          data-band={
            binding && verification !== "signedOut" && verification !== "unverified"
              ? severityBand(binding.severity, binding.percentUsed)
              : undefined
          }
        >
          <span className="accounts__health-dot" />
          {t(`accounts.health.${verification}` as MessageKey)}
        </span>
      </header>

      {binding ? (
        <>
          <BindingWindow window={binding} now={now} />
          {rest.length > 0 && (
            <ul className="accounts__rows">
              {rest.map((window) => (
                <WindowRow key={window.rawKind} window={window} now={now} />
              ))}
            </ul>
          )}
        </>
      ) : settled ? (
        /* The provider answered and the answer was "nothing to report here".
           Drawing the derived ladder anyway put two empty meters reading
           "allowance unknown" above the one sentence that explains why — which
           is the same uninformative emptiness this whole milestone exists to
           remove, reappearing on the cards where the answer is actually known. */
        <p className="accounts__settled">
          {LIVE_REASONS.has(settled)
            ? t(`accounts.live.reason.${settled}` as MessageKey)
            : t("accounts.live.unavailableGeneric")}
        </p>
      ) : (
        <ul className="accounts__rows">
          {quota.windows.map((window) => (
            <DerivedWindow key={window.window} window={window} now={now} />
          ))}
        </ul>
      )}

      {reading && <SpendBlock reading={reading} />}

      <div className="accounts__facts">
        <span>{t("accounts.tokens.today", { tokens: formatTokens(quota.tokensToday, locale) })}</span>
        {quota.liveSessions > 0 && (
          <span className="accounts__live-note">
            {t("accounts.liveSessions", { count: quota.liveSessions })}
          </span>
        )}
      </div>

      {account.provider === "claude-code" && !account.adopted && (
        <div
          className="accounts__trust-note"
          data-warning={card.folderTrusted === false || undefined}
        >
          <ShieldAlert size={14} aria-hidden="true" />
          <span>
            {card.folderTrusted === false
              ? t("accounts.folderUntrusted")
              : t("accounts.trustIsPerAccount")}
          </span>
        </div>
      )}

      {quota.refusalDetail && (
        <blockquote className="accounts__refusal">{quota.refusalDetail}</blockquote>
      )}

      <LiveStamp card={card} now={now} />
      <AccountActions card={card} canPause={canPause} />
    </article>
  );
}

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------

/**
 * The one line to read before reading anything else.
 *
 * Which account new work starts on, how much headroom that account has, and —
 * when it has none — which of the others does. This is the answer to the actual
 * complaint, which was never about any single card: it was about having to
 * assemble the picture by hand across four of them.
 */
function Summary({ cards, now }: { cards: AccountCard[]; now: number }) {
  const t = useT();
  const active = cards.find((card) => card.account.active);
  const bindingOf = (card: AccountCard): LiveWindow | null => {
    if (card.quota.live?.state !== "ok") return null;
    const windows = card.quota.live.reading.windows;
    return windows.find((window) => window.binding) ?? windows[0] ?? null;
  };

  const activeBinding = active ? bindingOf(active) : null;
  const activeLeft = activeBinding ? remaining(activeBinding.percentUsed) : null;

  // The best place to go next, offered only when there is somewhere better.
  const alternative = cards
    .filter(
      (card) =>
        card.account.id !== active?.account.id &&
        card.account.signedIn &&
        !card.account.paused &&
        card.quota.health !== "exhausted",
    )
    .map((card) => ({ card, window: bindingOf(card) }))
    .filter((entry): entry is { card: AccountCard; window: LiveWindow } => entry.window !== null)
    .sort((a, b) => a.window.percentUsed - b.window.percentUsed)[0];

  const suggest =
    alternative && activeLeft !== null && remaining(alternative.window.percentUsed) > activeLeft + 15
      ? alternative
      : null;

  if (!active) return null;
  const band = activeBinding
    ? severityBand(activeBinding.severity, activeBinding.percentUsed)
    : "normal";

  return (
    <section className="accounts__summary" data-band={band}>
      <div className="accounts__summary-main">
        <span className="accounts__summary-label">{t("accounts.summary.newWork")}</span>
        <strong className="accounts__summary-name">
          {active.account.label || active.account.email || t("accounts.unnamed")}
        </strong>
        {activeLeft !== null ? (
          <span className="accounts__summary-figure">
            {t("accounts.summary.headroom", { percent: Math.round(activeLeft) })}
          </span>
        ) : (
          <span className="accounts__summary-figure accounts__summary-figure--soft">
            {t("accounts.summary.noReading")}
          </span>
        )}
        {activeBinding && (
          <ResetLine resetsAtMs={activeBinding.resetsAtMs} now={now} showMoment />
        )}
      </div>
      {suggest && (
        <p className="accounts__summary-hint">
          {t("accounts.summary.better", {
            name:
              suggest.card.account.label ||
              suggest.card.account.email ||
              t("accounts.unnamed"),
            percent: Math.round(remaining(suggest.window.percentUsed)),
          })}
        </p>
      )}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Adding
// ---------------------------------------------------------------------------

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
        <button
          type="submit"
          className="accounts__button accounts__button--primary"
          disabled={working}
        >
          {working ? t("accounts.openingLogin") : t("accounts.continueToLogin")}
        </button>
      </div>
    </form>
  );
}

// ---------------------------------------------------------------------------
// The screen
// ---------------------------------------------------------------------------

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
  const [now, setNow] = useState(() => Date.now());

  // Open with what is on disk so the screen is never a spinner over an empty
  // card, then ask the providers. The probe is a CLI startup per account and
  // is far too slow to block the first paint on.
  useEffect(() => {
    void load(false, projectId).then(() => load(true, projectId));
  }, [load, projectId]);

  /**
   * Tick only when a label would actually change.
   *
   * A one-second interval behind four cards repaints several gauges a second
   * for a number that changes once a minute. This wakes at the next minute (or
   * hour, for a multi-day countdown) boundary instead — the same trick the
   * reference implementation on this machine uses, and the reason the panel can
   * stay open without spinning a core.
   */
  const resets = useMemo(() => {
    const out: (number | null)[] = [];
    for (const card of report?.accounts ?? []) {
      if (card.quota.live?.state === "ok") {
        for (const window of card.quota.live.reading.windows) out.push(window.resetsAtMs);
      }
      for (const window of card.quota.windows) out.push(window.resetsAtMs);
    }
    return out;
  }, [report]);

  const timer = useRef<number | null>(null);
  const schedule = useCallback(() => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    const at = Date.now();
    timer.current = window.setTimeout(() => {
      setNow(Date.now());
      schedule();
    }, nextTickDelay(at, resets));
  }, [resets]);

  useEffect(() => {
    setNow(Date.now());
    schedule();
    return () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    };
  }, [schedule]);

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
              className="accounts__button"
              onClick={() => void load(true)}
              disabled={refreshing}
            >
              <RefreshCw size={14} className={refreshing ? "accounts__spin" : undefined} />
              {refreshing ? t("accounts.checking") : t("accounts.refresh")}
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
              <span>
                {report?.accounts.filter((card) => card.account.provider === value).length ?? 0}
              </span>
            </button>
          ))}
        </div>

        {/* With a single account the strip would restate the card directly
            below it. It earns its place only once there is a choice to make. */}
        {cards.length > 1 && <Summary cards={cards} now={now} />}

        {report && (
          <section className="accounts__automation">
            <div>
              <h2>{t("accounts.autoSwitch.title")}</h2>
              <p>
                {report.autoSwitch === "onThreshold"
                  ? t("accounts.autoSwitch.thresholdDisclosure", {
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
