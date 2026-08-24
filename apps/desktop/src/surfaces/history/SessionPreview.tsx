import { ArrowLeft, FolderGit2, Play, Target, TerminalSquare } from "lucide-react";
import { useI18n, useT, type Translate } from "../../app/i18n";
import type { MessageKey } from "@jarvis/i18n";
import { ConversationView } from "../conversation/ConversationView";
import { StatusDot } from "../../design/StatusDot";
import type { HistoryEntry } from "../../app/history";
import { formatBytes, formatDuration, dotFor, providerLabel } from "./format";
import "./SessionPreview.css";

/**
 * A past session, read before deciding what to do with it (§88, D41).
 *
 * ## Why this is a step rather than a jump
 *
 * The first version of History opened the project workspace the moment a row
 * was clicked. That is the right *destination* and the wrong *first move*: the
 * question somebody has in front of a history list is "is this the one?", and
 * answering it should not require leaving the list, loading a workspace and
 * finding your way back.
 *
 * So a row opens here — the conversation itself, in full, read-only, with the
 * way onward stated plainly. Nothing has been started, nothing has been
 * navigated away from, and Back really does go back.
 *
 * ## The way onward
 *
 * A **live** session is not resumed, it is rejoined: the agent is still
 * running and its terminal is where it always was.
 *
 * A **finished** session is continued — a new agent process, handed this
 * conversation, in a new tab named after it. That is the whole point of the
 * feature: history that you can only read is an archive, and this product is
 * for getting work done.
 *
 * Where continuing is impossible the offer is **absent and explained**, never a
 * button that would fail (§81). Two reasons it can be: the provider resumes in
 * a way this build cannot follow, or its own id for this conversation was never
 * recorded. Both are stated rather than left as a mystery.
 */
export interface SessionPreviewProps {
  entry: HistoryEntry;
  onBack: () => void;
  /** Rejoin a session that is still running. */
  onGoToTerminal: (entry: HistoryEntry) => void;
  /** Start a new agent carrying this conversation. */
  onContinue: (entry: HistoryEntry) => void;
  onOpenProject: (entry: HistoryEntry) => void;
  onOpenMission: (missionId: string) => void;
  /** Set while the continuation is being started, so it cannot be double-fired. */
  starting: boolean;
}

export function SessionPreview({
  entry,
  onBack,
  onGoToTerminal,
  onContinue,
  onOpenProject,
  onOpenMission,
  starting,
}: SessionPreviewProps) {
  const t = useT();
  const { locale } = useI18n();
  const now = Date.now();
  const duration = formatDuration(entry, now);
  const name = entry.title ?? t("history.untitled");

  return (
    <div className="preview">
      <header className="preview__bar">
        <button type="button" className="preview__back" onClick={onBack}>
          <ArrowLeft size={15} strokeWidth={1.75} aria-hidden="true" />
          {t("history.back")}
        </button>

        <div className="preview__actions">
          <button
            type="button"
            className="preview__action"
            onClick={() => onOpenProject(entry)}
          >
            <FolderGit2 size={14} strokeWidth={1.75} aria-hidden="true" />
            {entry.projectName}
          </button>

          {entry.live ? (
            // Still running. Rejoining is not resuming — there is nothing to
            // hand back, the agent never stopped.
            <button
              type="button"
              className="preview__action preview__action--primary"
              onClick={() => onGoToTerminal(entry)}
            >
              <TerminalSquare size={14} strokeWidth={1.75} aria-hidden="true" />
              {t("history.goToTerminal")}
            </button>
          ) : !entry.projectExists ? (
            // The conversation is still perfectly readable; there is just
            // nowhere to run. Said plainly rather than left as a Continue that
            // the core would refuse — see `session.cwdMissing`, and the real
            // agent that started in somebody's home directory before it existed.
            <span className="preview__cannot" title={t("history.folderGone.hint")}>
              {t("history.folderGone")}
            </span>
          ) : entry.resumable ? (
            <button
              type="button"
              className="preview__action preview__action--primary"
              onClick={() => onContinue(entry)}
              disabled={starting}
            >
              <Play size={14} strokeWidth={1.75} aria-hidden="true" />
              {starting ? t("history.continuing") : t("history.continue")}
            </button>
          ) : (
            // Absent and explained, rather than present and broken (§81).
            <span className="preview__cannot" title={t("history.cannotContinue.hint")}>
              {t("history.cannotContinue")}
            </span>
          )}
        </div>
      </header>

      <div className="preview__head">
        <div className="preview__title-line">
          <StatusDot status={dotFor(entry)} />
          <h1 className="preview__title" data-untitled={entry.title ? undefined : true}>
            {name}
          </h1>
        </div>

        <div className="preview__meta">
          <span>{providerLabel(entry.provider, t)}</span>
          <Dot />
          <span>{new Date(entry.createdAt).toLocaleString(locale)}</span>
          {duration && (
            <>
              <Dot />
              <span>{duration}</span>
            </>
          )}
          {entry.turns > 0 && (
            <>
              <Dot />
              <span>{t("history.turns", { count: entry.turns })}</span>
            </>
          )}
          {entry.tokens !== null && entry.tokens > 0 && (
            <>
              <Dot />
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
              <Dot />
              <span>{formatBytes(entry.bytes, locale)}</span>
            </>
          )}
        </div>

        {entry.missionId && entry.missionTitle && (
          <button
            type="button"
            className="preview__mission"
            onClick={() => onOpenMission(entry.missionId!)}
          >
            <Target size={12} strokeWidth={1.75} aria-hidden="true" />
            {entry.missionTitle}
          </button>
        )}

        {/* A continuation says so. Without it, two rows in the list look like
            two unrelated pieces of work rather than one thread (§86's own
            reasoning, applied to sessions). */}
        {entry.resumedFrom && (
          <p className="preview__note">{t("history.continuedFromEarlier")}</p>
        )}
      </div>

      {/* The same component the live session uses — same log, same projection,
          same rendering (§23/§24). A preview that rendered the conversation its
          own way would be a second implementation free to disagree with the
          real one. `live` is passed through so a running session's preview
          keeps updating while it is being read. */}
      <div className="preview__body">
        <ConversationView sessionId={entry.id} live={entry.live} />
      </div>
    </div>
  );
}

function Dot() {
  return (
    <span className="preview__dot" aria-hidden="true">
      ·
    </span>
  );
}

/** Re-exported so `History` and this share one vocabulary. */
export type { Translate, MessageKey };
