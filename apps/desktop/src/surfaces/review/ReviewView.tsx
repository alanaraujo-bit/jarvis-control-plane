import { useEffect, useState } from "react";
import { GitBranch, Minus, Plus, RefreshCw, RotateCcw, ShieldAlert, Undo2 } from "lucide-react";
import { useT } from "../../app/i18n";
import type { MessageKey } from "@jarvis/i18n";
import type { Choice } from "../guardrails/useGuardrails";
import { DiffView } from "./DiffView";
import { reviewDiffKey, useReview, type ChangeKind, type ReviewFile } from "./useReview";
import "./ReviewView.css";

interface ReviewViewProps {
  projectId: string;
}

/**
 * A row shows one letter, because the list is long and the word would crowd
 * every line. The word travels with it as the tooltip and the accessible name:
 * a code that has to be learned before the surface can be read is not
 * finished, and colour alone must never be the only cue.
 */
const KIND_LABEL: Record<ChangeKind, MessageKey> = {
  added: "review.kind.added",
  modified: "review.kind.modified",
  deleted: "review.kind.deleted",
  renamed: "review.kind.renamed",
  untracked: "review.kind.untracked",
  conflicted: "review.kind.conflicted",
};

const KIND_FULL: Record<ChangeKind, MessageKey> = {
  added: "review.kindFull.added",
  modified: "review.kindFull.modified",
  deleted: "review.kindFull.deleted",
  renamed: "review.kindFull.renamed",
  untracked: "review.kindFull.untracked",
  conflicted: "review.kindFull.conflicted",
};

/**
 * Diff / Review (§43).
 *
 * The question is "what did this agent change?", so the list is ordered by the
 * work rather than by the alphabet: files an agent touched come first, most
 * recent at the top, and each says which session touched it. Files nobody we
 * were watching changed are still listed — that is a fact about the working
 * tree, not an omission.
 *
 * Since §44 it also acts. Staging and unstaging move the index and are
 * ordinary; **discarding goes through the guardrail** (D11), because what it
 * destroys was never committed and nothing anywhere can bring it back.
 */
export function ReviewView({ projectId }: ReviewViewProps) {
  const t = useT();
  const report = useReview((state) => state.report[projectId]);
  const loading = useReview((state) => state.loading[projectId]);
  const error = useReview((state) => state.error[projectId]);
  const selected = useReview((state) => state.selected[projectId]);
  const diffs = useReview((state) => state.diffs);
  const diffLoading = useReview((state) => state.diffLoading);
  const refresh = useReview((state) => state.refresh);
  const select = useReview((state) => state.select);
  const act = useReview((state) => state.act);
  const confirming = useReview((state) => state.confirming);
  const confirm = useReview((state) => state.confirm);
  const cancelConfirm = useReview((state) => state.cancelConfirm);
  const refused = useReview((state) => state.refused[projectId]);

  // Re-read on every visit. An agent may have been working the whole time the
  // user was on another surface, and a stale diff is worse than a slow one.
  useEffect(() => {
    void refresh(projectId);
  }, [projectId, refresh]);

  if (report && !report.isRepo) {
    return (
      <div className="review__empty">
        <p className="review__empty-title">{t("review.notARepo.title")}</p>
        <p className="review__empty-body">{t("review.notARepo.body")}</p>
      </div>
    );
  }

  const files = report?.files ?? [];
  const active = files.find((file) => file.path === selected);
  const activeDiff = active ? diffs[reviewDiffKey(projectId, active.path)] : undefined;
  const activeLoading = active ? diffLoading[reviewDiffKey(projectId, active.path)] : false;

  const totals = files.reduce(
    (sum, file) => ({
      insertions: sum.insertions + file.insertions,
      deletions: sum.deletions + file.deletions,
    }),
    { insertions: 0, deletions: 0 },
  );

  return (
    <div className="review">
      <aside className="review__list">
        <header className="review__list-header">
          <span className="review__count">
            {t("review.changedFiles", { count: files.length })}
          </span>
          {files.length > 0 && (
            <span className="review__totals">
              <span className="review__added">+{totals.insertions}</span>
              <span className="review__removed">−{totals.deletions}</span>
            </span>
          )}
          <button
            type="button"
            className="review__refresh"
            onClick={() => void refresh(projectId)}
            aria-label={t("review.refresh")}
            title={t("review.refresh")}
            data-busy={loading || undefined}
          >
            <RefreshCw size={12} strokeWidth={2} aria-hidden="true" />
          </button>
        </header>

        {report?.branch && (
          <p className="review__branch">
            <GitBranch size={11} strokeWidth={2} aria-hidden="true" />
            {t("review.against", { branch: report.branch })}
          </p>
        )}

        {/* A repository with no commits has no HEAD to compare against, and
            saying so beats reporting "no changes" for a folder full of them. */}
        {report && report.isRepo && !report.hasCommits && (
          <p className="review__note">{t("review.noCommits")}</p>
        )}

        {error && <p className="review__error">{error}</p>}

        {files.length === 0 && !loading && report?.hasCommits && (
          <p className="review__note">{t("review.clean")}</p>
        )}

        <ul className="review__files">
          {files.map((file) => (
            <li key={file.path} className="review__row">
              <button
                type="button"
                className="review__file"
                data-selected={file.path === selected || undefined}
                data-staged={file.staged || undefined}
                onClick={() => void select(projectId, file)}
                title={file.path}
              >
                <span className="review__file-head">
                  <span
                    className="review__kind"
                    data-kind={file.kind}
                    // Staged is shown as a chip on this letter, so the word for
                    // it has to travel here too — otherwise the only cue that a
                    // file is staged is a colour.
                    title={kindLabel(t, file)}
                    aria-label={kindLabel(t, file)}
                  >
                    {t(KIND_LABEL[file.kind])}
                  </span>
                  <span className="review__file-name">{basename(file.path)}</span>
                </span>
                <span className="review__file-meta">
                  <span className="review__dir">{dirname(file.path)}</span>
                  {file.binary || file.tooLarge ? (
                    // A row with no counts has to say why. Silence here reads
                    // as "nothing changed" for a file that is entirely new.
                    <span className="review__binary">
                      {file.binary ? t("review.binaryShort") : t("review.tooLargeShort")}
                    </span>
                  ) : (
                    <span className="review__stat">
                      {file.insertions > 0 && (
                        <span className="review__added">+{file.insertions}</span>
                      )}
                      {file.deletions > 0 && (
                        <span className="review__removed">−{file.deletions}</span>
                      )}
                    </span>
                  )}
                </span>
                <Attribution file={file} />
              </button>
              <RowActions
                file={file}
                onAct={(action) => void act(projectId, action, [file])}
              />
            </li>
          ))}
        </ul>

        <CommitBox projectId={projectId} files={files} />
      </aside>

      <section className="review__diff">
        {active ? (
          <>
            <header className="review__diff-header">
              <span className="review__diff-path selectable">
                {active.fromPath && (
                  <span className="review__renamed-from">{active.fromPath} → </span>
                )}
                {active.path}
              </span>
            </header>
            {/* The confirmation sits above the diff rather than in a modal, so
                the change being thrown away is still on screen while the
                decision is made. A dialog that covers the evidence is asking
                someone to decide blind. */}
            {confirming && confirming.projectId === projectId && (
              <DiscardConfirmation
                file={confirming.file}
                command={confirming.command}
                onChoose={(choice) => void confirm(choice)}
                onCancel={cancelConfirm}
              />
            )}
            {refused && (
              <p className="review__refused">
                <ShieldAlert size={13} strokeWidth={1.9} aria-hidden="true" />
                {t("review.refused")}
              </p>
            )}
            <div className="review__diff-body">
              {activeDiff ? (
                <DiffView diff={activeDiff} />
              ) : (
                activeLoading && <p className="review__note">{t("review.loadingDiff")}</p>
              )}
            </div>
          </>
        ) : (
          !loading && (
            <div className="review__empty">
              <p className="review__empty-title">{t("review.empty.title")}</p>
              <p className="review__empty-body">{t("review.empty.body")}</p>
            </div>
          )
        )}
      </section>
    </div>
  );
}

/**
 * The change type, and whether it is staged, as one readable phrase.
 *
 * "Partly staged" is a real state, not a rounding of one of the others: `MM` in
 * porcelain means some of the change is in the index and some is not, and both
 * buttons are offered for exactly that reason.
 */
function kindLabel(t: ReturnType<typeof useT>, file: ReviewFile): string {
  const kind = t(KIND_FULL[file.kind]);
  if (!file.staged) return kind;
  return `${kind} · ${t(file.unstaged ? "review.partlyStaged" : "review.staged")}`;
}

/**
 * What can be done to one file (§44).
 *
 * Stage and unstage are both offered when a file is partly staged, because it
 * genuinely is both — `MM` in porcelain — and offering only one would hide half
 * the file's state behind a button that then does something unexpected.
 *
 * The discard button carries the word for what it does to *this* file: for a
 * deleted file the same Git command is a recovery, and calling that "discard"
 * would frighten someone out of the one action they want.
 */
function RowActions({
  file,
  onAct,
}: {
  file: ReviewFile;
  onAct: (action: "stage" | "unstage" | "discard") => void;
}) {
  const t = useT();
  const name = basename(file.path);
  const restoring = file.kind === "deleted";

  return (
    <span className="review__actions">
      {file.unstaged && (
        <button
          type="button"
          className="review__action"
          onClick={() => onAct("stage")}
          title={t("review.stage")}
          aria-label={`${t("review.stage")}: ${name}`}
        >
          <Plus size={13} strokeWidth={2.1} aria-hidden="true" />
        </button>
      )}
      {file.staged && (
        <button
          type="button"
          className="review__action"
          onClick={() => onAct("unstage")}
          title={t("review.unstage")}
          aria-label={`${t("review.unstage")}: ${name}`}
        >
          <Minus size={13} strokeWidth={2.1} aria-hidden="true" />
        </button>
      )}
      <button
        type="button"
        className="review__action review__action--discard"
        onClick={() => onAct("discard")}
        title={t(restoring ? "review.restoreTitle" : "review.discardTitle", { file: name })}
        aria-label={t(restoring ? "review.restoreTitle" : "review.discardTitle", {
          file: name,
        })}
      >
        {restoring ? (
          <RotateCcw size={13} strokeWidth={2.1} aria-hidden="true" />
        ) : (
          <Undo2 size={13} strokeWidth={2.1} aria-hidden="true" />
        )}
      </button>
    </span>
  );
}

/**
 * The guardrail asking before a discard (§35, D11).
 *
 * The four answers are §35's, unchanged, because this is the same question the
 * guardrail asks anywhere else and answering it here should mean the same
 * thing. The command is shown verbatim: approving a paraphrase is not
 * approving anything.
 */
function DiscardConfirmation({
  file,
  command,
  onChoose,
  onCancel,
}: {
  file: ReviewFile;
  command: string;
  onChoose: (choice: Choice) => void;
  onCancel: () => void;
}) {
  const t = useT();
  const name = basename(file.path);
  const restoring = file.kind === "deleted";
  const untracked = file.kind === "untracked";

  return (
    <section className="review__confirm" role="alertdialog" aria-label={name}>
      <header className="review__confirm-head">
        <ShieldAlert size={14} strokeWidth={1.9} aria-hidden="true" />
        <h3 className="review__confirm-title">
          {t(restoring ? "review.confirm.restoreTitle" : "review.confirm.discardTitle", {
            file: name,
          })}
        </h3>
      </header>

      <p className="review__confirm-body">
        {t(
          untracked
            ? "review.confirm.bodyUntracked"
            : restoring
              ? "review.confirm.bodyDeleted"
              : "review.confirm.body",
        )}
      </p>

      <p className="review__confirm-willrun">
        {t("review.confirm.willRun")}: <code className="selectable">{command}</code>
      </p>

      <div className="review__confirm-choices">
        {(["allowOnce", "allowForProject", "alwaysAllow", "neverAllow"] as Choice[]).map(
          (choice) => (
            <button
              key={choice}
              type="button"
              className="review__confirm-choice"
              data-choice={choice}
              onClick={() => onChoose(choice)}
            >
              {t(`guardrail.choice.${choice}` as MessageKey)}
            </button>
          ),
        )}
        <button type="button" className="review__confirm-cancel" onClick={onCancel}>
          {t("review.confirm.cancel")}
        </button>
      </div>
    </section>
  );
}

/**
 * Committing what is staged (§44).
 *
 * Absent entirely when nothing is staged, rather than present and disabled
 * (§81/§18): an empty box with a dead button is a permanent reminder of a thing
 * that is not happening. Staging a file makes it appear, which is also the
 * clearest possible feedback that staging worked — the diff beside it does not
 * change, because Review compares against `HEAD` with the index and the working
 * tree together (§43).
 */
function CommitBox({ projectId, files }: { projectId: string; files: ReviewFile[] }) {
  const t = useT();
  const [message, setMessage] = useState("");
  const commit = useReview((state) => state.commit);
  const committing = useReview((state) => state.committing[projectId]);

  const staged = files.filter((file) => file.staged);
  if (staged.length === 0) return null;

  const send = async () => {
    if (!message.trim() || committing) return;
    if (await commit(projectId, message)) setMessage("");
  };

  return (
    <form
      className="review__commit"
      onSubmit={(event) => {
        event.preventDefault();
        void send();
      }}
    >
      <label className="review__commit-label" htmlFor={`commit-${projectId}`}>
        {t("review.commit.title")}
        <span className="review__commit-count">
          {t("review.commit.staged", { count: staged.length })}
        </span>
      </label>
      <textarea
        id={`commit-${projectId}`}
        className="review__commit-message"
        value={message}
        placeholder={t("review.commit.placeholder")}
        rows={2}
        onChange={(event) => setMessage(event.target.value)}
        onKeyDown={(event) => {
          // The convention every commit box uses. Enter alone has to stay a
          // newline: a commit message has a body.
          if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
            event.preventDefault();
            void send();
          }
        }}
      />
      <button
        type="submit"
        className="review__commit-action"
        disabled={!message.trim() || committing}
      >
        {t("review.commit.action")}
      </button>
    </form>
  );
}

/**
 * Who changed this file.
 *
 * Amber, because agent work is amber everywhere in this product (§6). A file
 * with no attribution simply says nothing — an absent line is quieter than
 * "changed by: nobody", and this list is long.
 */
function Attribution({ file }: { file: ReviewFile }) {
  const t = useT();
  if (file.sessions.length === 0) return null;

  const first = file.sessions[0];
  const label = first.missionTitle ?? first.title ?? providerName(first.provider);
  const others = file.sessions.length - 1;

  return (
    <span className="review__attribution">
      <span className="review__agent-dot" aria-hidden="true" />
      <span className="review__agent">{label}</span>
      {others > 0 && (
        <span className="review__agent-more">{t("review.andOthers", { count: others })}</span>
      )}
    </span>
  );
}

function providerName(provider: string): string {
  if (provider === "claude-code") return "Claude Code";
  if (provider === "codex") return "Codex";
  return provider;
}

/**
 * The trailing `/` is stripped first, so a path that names a directory still
 * has a name. Git no longer sends one (see `changed_files`), and a row with a
 * blank label is bad enough that it is worth not depending on that.
 */
function basename(path: string): string {
  const clean = path.replace(/\/+$/, "");
  return clean.split("/").pop() || clean || path;
}

function dirname(path: string): string {
  const clean = path.replace(/\/+$/, "");
  const cut = clean.lastIndexOf("/");
  return cut <= 0 ? "" : clean.slice(0, cut);
}
