import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Check, Pencil, Search, Target, Trash2, X } from "lucide-react";
import { useI18n, useT, type Translate } from "../../app/i18n";
import type { MessageKey } from "@jarvis/i18n";
import { StatusDot } from "../../design/StatusDot";
import type { HistoryEntry } from "../../app/history";
import { useProjects } from "../projects/useProjects";
import { useHistory, type Range } from "./useHistory";
import { SessionPreview } from "./SessionPreview";
import {
  BUCKET_ORDER,
  bucketOf,
  dotFor,
  formatBytes,
  formatDuration,
  providerLabel,
  relative,
  type Bucket,
} from "./format";
import "./History.css";

/**
 * Session History (§88).
 *
 * Every session this machine has ever run: titled, searchable, grouped by when
 * it happened, and openable. `session_list` cannot answer this — it filters to
 * one project and to sessions that have not ended, which is right for the
 * terminal tabs it feeds and is why a finished session was, until this surface,
 * unreachable.
 *
 * The search box searches **what was said**, not only the names. That is the
 * part the reference this was modelled on cannot do, and it costs nothing to
 * offer: the FTS5 index it runs on was built for Global Search (§51) and is
 * already full.
 */
export interface HistoryProps {
  /**
   * Open this session where it lives — the project workspace, as a read-only
   * conversation tab. Routed through the same path Global Search uses, because
   * a second mechanism for reopening a past session is exactly what §23's
   * one-log architecture exists to prevent.
   */
  onOpenSession: (entry: HistoryEntry) => void;
  /** Rejoin a session whose agent is still running. */
  onGoToTerminal: (entry: HistoryEntry) => void;
  /**
   * Continue this conversation in a new agent (§88, D41). Resolves once the
   * new session has started, so the preview can show that it is working.
   */
  onContinue: (entry: HistoryEntry) => Promise<void>;
  onOpenMission: (missionId: string) => void;
}


export function History({
  onOpenSession,
  onGoToTerminal,
  onContinue,
  onOpenMission,
}: HistoryProps) {
  // The session being read before deciding what to do with it. Local rather
  // than in the store: it is a view state, and it should not survive leaving
  // the surface -- coming back to History means coming back to the list.
  const [selected, setSelected] = useState<HistoryEntry | null>(null);
  const [starting, setStarting] = useState(false);
  const t = useT();
  // The app's own locale, not the browser's: somebody running the product in
  // pt-BR wants 2,5 MB, whatever Windows was installed as.
  const { locale } = useI18n();
  const projects = useProjects((state) => state.projects);
  const {
    entries,
    hasMore,
    searched,
    loading,
    loadingMore,
    error,
    filters,
    providers,
    storage,
    renaming,
    confirmingDelete,
    load,
    loadMore,
    setFilters,
    beginRename,
    rename,
    askDelete,
    remove,
  } = useHistory();

  // The search box is local and the store's filter is debounced off it: a
  // controlled input that waited for a round trip per keystroke would drop
  // characters on a slow query, which is the one thing a search box may not do.
  const [text, setText] = useState(filters.text);
  useEffect(() => {
    const timer = window.setTimeout(() => {
      if (text !== filters.text) setFilters({ text });
    }, 180);
    return () => window.clearTimeout(timer);
  }, [text, filters.text, setFilters]);

  useEffect(() => {
    void load();
    // Deliberately once. This surface is unmounted when you leave it (unlike a
    // project area — see `useVisitRefresh` for the case where that is not
    // true), so a mount *is* a visit.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // A single clock for the whole list. Every row computes a relative time, and
  // `Date.now()` per row would have rows in one render disagree by a tick.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, []);

  // "Load more" on scroll rather than a button: the observer fires while the
  // sentinel is still below the fold, so the next page is usually there before
  // anybody reaches the end of this one.
  const sentinel = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const node = sentinel.current;
    if (!node || !hasMore) return;
    const observer = new IntersectionObserver(
      (records) => {
        if (records.some((record) => record.isIntersecting)) void loadMore();
      },
      { rootMargin: "400px" },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [hasMore, loadMore, entries.length]);

  const grouped = useMemo(() => {
    const groups = new Map<Bucket, HistoryEntry[]>();
    for (const entry of entries) {
      const bucket = bucketOf(entry.createdAt, now);
      const list = groups.get(bucket);
      if (list) list.push(entry);
      else groups.set(bucket, [entry]);
    }
    return BUCKET_ORDER.filter((bucket) => groups.has(bucket)).map(
      (bucket) => [bucket, groups.get(bucket)!] as const,
    );
  }, [entries, now]);

  const projectOptions = useMemo(
    () => [...projects].sort((a, b) => a.name.localeCompare(b.name)),
    [projects],
  );

  // Reading a session comes before deciding what to do with it (§88, D41).
  // The list is replaced rather than squeezed beside a panel: a conversation
  // is the widest thing this surface shows, and Back really does go back.
  if (selected) {
    return (
      <SessionPreview
        entry={selected}
        starting={starting}
        onBack={() => setSelected(null)}
        onGoToTerminal={onGoToTerminal}
        onOpenProject={onOpenSession}
        onOpenMission={onOpenMission}
        onContinue={(entry) => {
          // Guarded rather than merely disabled: the button is disabled while
          // this runs, and a double activation from the keyboard would
          // otherwise start two agents on one conversation.
          if (starting) return;
          setStarting(true);
          void onContinue(entry).finally(() => setStarting(false));
        }}
      />
    );
  }

  return (
    <div className="hist">
      <div className="hist__inner">
        <header className="hist__header">
          <div className="hist__heading">
            <h1 className="hist__title">{t("history.title")}</h1>
            {storage && (
              <p className="hist__storage">
                {t("history.storage", {
                  count: storage.sessions,
                  size: formatBytes(storage.bytes, locale),
                })}
              </p>
            )}
          </div>

          <div className="hist__search">
            <Search size={14} strokeWidth={1.75} aria-hidden="true" />
            <input
              type="search"
              className="hist__search-input"
              value={text}
              placeholder={t("history.searchPlaceholder")}
              aria-label={t("history.searchPlaceholder")}
              onChange={(event) => setText(event.target.value)}
            />
          </div>
        </header>

        <div className="hist__filters">
          <Segmented
            label={t("history.filter.range")}
            value={filters.range}
            options={(["all", "today", "week", "month"] as Range[]).map((range) => ({
              value: range,
              label: t(`history.range.${range}` as MessageKey),
            }))}
            onChange={(range) => setFilters({ range: range as Range })}
          />

          {providers.length > 1 && (
            <Segmented
              label={t("history.filter.provider")}
              value={filters.provider ?? ""}
              options={[
                { value: "", label: t("history.filter.any") },
                ...providers.map((provider) => ({
                  value: provider,
                  label: providerLabel(provider, t),
                })),
              ]}
              onChange={(provider) => setFilters({ provider: provider || null })}
            />
          )}

          {projectOptions.length > 1 && (
            <label className="hist__select">
              <span className="sr-only">{t("history.filter.project")}</span>
              <select
                value={filters.projectId ?? ""}
                onChange={(event) => setFilters({ projectId: event.target.value || null })}
              >
                <option value="">{t("history.filter.allProjects")}</option>
                {projectOptions.map((project) => (
                  <option key={project.id} value={project.id}>
                    {project.name}
                  </option>
                ))}
              </select>
            </label>
          )}
        </div>

        {error && <p className="hist__error">{error}</p>}

        {entries.length === 0 && !loading ? (
          <div className="hist__empty">
            <p className="hist__empty-title">
              {searched ? t("history.empty.noMatch.title") : t("history.empty.title")}
            </p>
            <p className="hist__empty-body">
              {searched ? t("history.empty.noMatch.body") : t("history.empty.body")}
            </p>
          </div>
        ) : (
          <div className="hist__groups" aria-busy={loading || undefined}>
            {grouped.map(([bucket, rows]) => (
              <section key={bucket} className="hist__group">
                <h2 className="hist__group-title">
                  {t(`history.bucket.${bucket}` as MessageKey)}
                  <span className="hist__group-count">{rows.length}</span>
                </h2>
                <ul className="hist__list">
                  {rows.map((entry) => (
                    <Row
                      key={entry.id}
                      entry={entry}
                      now={now}
                      locale={locale}
                      t={t}
                      renaming={renaming === entry.id}
                      confirming={confirmingDelete === entry.id}
                      onOpen={() => setSelected(entry)}
                      onOpenMission={onOpenMission}
                      onBeginRename={() => beginRename(entry.id)}
                      onCancelRename={() => beginRename(null)}
                      onRename={(name) => void rename(entry.id, name)}
                      onAskDelete={() => askDelete(entry.id)}
                      onCancelDelete={() => askDelete(null)}
                      onDelete={() => void remove(entry.id)}
                    />
                  ))}
                </ul>
              </section>
            ))}

            {/* Search returns one capped set; only browsing pages. */}
            {hasMore && !searched && (
              <div ref={sentinel} className="hist__more">
                {loadingMore ? t("history.loadingMore") : ""}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

interface RowProps {
  entry: HistoryEntry;
  now: number;
  locale: string;
  t: Translate;
  renaming: boolean;
  confirming: boolean;
  onOpen: () => void;
  onOpenMission: (missionId: string) => void;
  onBeginRename: () => void;
  onCancelRename: () => void;
  onRename: (name: string) => void;
  onAskDelete: () => void;
  onCancelDelete: () => void;
  onDelete: () => void;
}

function Row({
  entry,
  now,
  locale,
  t,
  renaming,
  confirming,
  onOpen,
  onOpenMission,
  onBeginRename,
  onCancelRename,
  onRename,
  onAskDelete,
  onCancelDelete,
  onDelete,
}: RowProps) {
  const duration = formatDuration(entry, now);
  const exact = new Date(entry.createdAt).toLocaleString(locale);

  // A session nobody and nothing ever named. Shown as a stated absence rather
  // than as a blank line or as its own id, which is not a name (§81).
  const name = entry.title ?? t("history.untitled");

  return (
    <li className="hist__row" data-live={entry.live || undefined}>
      <button
        type="button"
        className="hist__open"
        onClick={onOpen}
        // Nothing else in the row is a link, so the whole row is the target and
        // the actions sit above it.
        aria-label={t("history.open", { name })}
      />

      <StatusDot status={dotFor(entry)} />

      <div className="hist__body">
        {renaming ? (
          <RenameField initial={entry.title ?? ""} t={t} onCancel={onCancelRename} onSave={onRename} />
        ) : (
          <div className="hist__name-line">
            <span className="hist__name" data-untitled={entry.title ? undefined : true}>
              {name}
            </span>
            {/* Where the name came from (D36). A title Claude Code chose and one
                cut from the first sentence are not the same claim, so the row
                says which — and says nothing at all for a name a person typed,
                because they already know. */}
            {entry.titleSource === "provider" && (
              <span className="hist__source" title={t("history.source.provider.hint")}>
                {t("history.source.provider")}
              </span>
            )}
            {entry.titleSource === "derived" && (
              <span className="hist__source" title={t("history.source.derived.hint")}>
                {t("history.source.derived")}
              </span>
            )}
          </div>
        )}

        <div className="hist__meta">
          <span className="hist__project">{entry.projectName}</span>
          <span className="hist__dot" aria-hidden="true">
            ·
          </span>
          <span>{providerLabel(entry.provider, t)}</span>
          {entry.turns > 0 && (
            <>
              <span className="hist__dot" aria-hidden="true">
                ·
              </span>
              <span>{t("history.turns", { count: entry.turns })}</span>
            </>
          )}
          {duration && (
            <>
              <span className="hist__dot" aria-hidden="true">
                ·
              </span>
              <span>{duration}</span>
            </>
          )}
          {/* Absent rather than zero: a session no provider measured has not
              been measured, and drawing "0" would say it cost nothing (§28). */}
          {entry.tokens !== null && entry.tokens > 0 && (
            <>
              <span className="hist__dot" aria-hidden="true">
                ·
              </span>
              <span>
                {t("history.tokens", {
                  count: entry.tokens,
                  value: entry.tokens.toLocaleString(locale),
                })}
              </span>
            </>
          )}
          {entry.bytes > 0 && (
            <>
              <span className="hist__dot" aria-hidden="true">
                ·
              </span>
              <span>{formatBytes(entry.bytes, locale)}</span>
            </>
          )}
        </div>

        {entry.missionId && entry.missionTitle && (
          <button
            type="button"
            className="hist__mission"
            onClick={(event) => {
              event.stopPropagation();
              onOpenMission(entry.missionId!);
            }}
          >
            <Target size={11} strokeWidth={1.75} aria-hidden="true" />
            {entry.missionTitle}
          </button>
        )}

        {/* Only on a search hit: the line that actually matched. This is the
            agent's or the person's own words, never text this app composed. */}
        {entry.snippet && <p className="hist__snippet">{entry.snippet}</p>}
      </div>

      <time className="hist__when" dateTime={new Date(entry.createdAt).toISOString()} title={exact}>
        {entry.live ? t("history.running") : relative(entry.createdAt, now, t)}
      </time>

      {confirming ? (
        <div className="hist__confirm">
          <span className="hist__confirm-text">{t("history.delete.confirm")}</span>
          <button type="button" className="hist__confirm-yes" onClick={onDelete}>
            {t("history.delete.yes")}
          </button>
          <button type="button" className="hist__confirm-no" onClick={onCancelDelete}>
            {t("common.cancel")}
          </button>
        </div>
      ) : (
        !renaming && (
          <div className="hist__actions">
            <button
              type="button"
              className="hist__action"
              onClick={onBeginRename}
              title={t("history.rename")}
              aria-label={t("history.rename")}
            >
              <Pencil size={13} strokeWidth={1.75} aria-hidden="true" />
            </button>
            {/* A running agent is writing to that log. Removing it is a crash,
                not a delete — the core refuses it too, so this is not the only
                thing standing in the way. */}
            {!entry.live && (
              <button
                type="button"
                className="hist__action hist__action--danger"
                onClick={onAskDelete}
                title={t("history.delete")}
                aria-label={t("history.delete")}
              >
                <Trash2 size={13} strokeWidth={1.75} aria-hidden="true" />
              </button>
            )}
          </div>
        )
      )}
    </li>
  );
}

function RenameField({
  initial,
  t,
  onCancel,
  onSave,
}: {
  initial: string;
  t: Translate;
  onCancel: () => void;
  onSave: (name: string) => void;
}) {
  const [value, setValue] = useState(initial);
  const input = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);

  const save = useCallback(() => {
    const trimmed = value.trim();
    // An empty name is a cancel, not a clear. The core refuses it as well —
    // there is no gesture here that means "make this untitled again".
    if (!trimmed) onCancel();
    else onSave(trimmed);
  }, [value, onCancel, onSave]);

  return (
    <div className="hist__rename">
      <input
        ref={input}
        className="hist__rename-input"
        value={value}
        maxLength={72}
        aria-label={t("history.rename")}
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={(event) => {
          // Stopped here so Escape does not also reach whatever else on the
          // window listens for it, and Enter does not submit an outer form.
          event.stopPropagation();
          if (event.key === "Enter") save();
          if (event.key === "Escape") onCancel();
        }}
        // Clicking away is a save, not a discard: the field was opened by a
        // deliberate gesture and losing what was typed to a stray click is the
        // more annoying of the two failures.
        onBlur={save}
      />
      <button type="button" className="hist__action" onMouseDown={save} title={t("common.save")}>
        <Check size={13} strokeWidth={1.75} aria-hidden="true" />
      </button>
      <button
        type="button"
        className="hist__action"
        // `onMouseDown` so it lands before the input's own blur handler saves.
        onMouseDown={(event) => {
          event.preventDefault();
          onCancel();
        }}
        title={t("common.cancel")}
      >
        <X size={13} strokeWidth={1.75} aria-hidden="true" />
      </button>
    </div>
  );
}

function Segmented({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: { value: string; label: string }[];
  onChange: (value: string) => void;
}) {
  return (
    <div className="hist__segmented" role="radiogroup" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          role="radio"
          aria-checked={value === option.value}
          data-active={value === option.value || undefined}
          className="hist__segment"
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

