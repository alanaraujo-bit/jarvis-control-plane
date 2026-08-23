import { useEffect } from "react";
import { GitBranch, RefreshCw } from "lucide-react";
import { useT } from "../../app/i18n";
import type { MessageKey } from "@jarvis/i18n";
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
 * Read-only. Staging, discarding and restoring are destructive Git operations
 * and belong behind the guardrail (D11), which is Git's own milestone (§44),
 * not this one. §81 applies: they are absent rather than disabled decoration.
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
            <li key={file.path}>
              <button
                type="button"
                className="review__file"
                data-selected={file.path === selected || undefined}
                onClick={() => void select(projectId, file)}
                title={file.path}
              >
                <span className="review__file-head">
                  <span
                    className="review__kind"
                    data-kind={file.kind}
                    title={t(KIND_FULL[file.kind])}
                    aria-label={t(KIND_FULL[file.kind])}
                  >
                    {t(KIND_LABEL[file.kind])}
                  </span>
                  <span className="review__file-name">{basename(file.path)}</span>
                </span>
                <span className="review__file-meta">
                  <span className="review__dir">{dirname(file.path)}</span>
                  {file.binary ? (
                    <span className="review__binary">{t("review.binaryShort")}</span>
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
            </li>
          ))}
        </ul>
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

function basename(path: string): string {
  return path.split("/").pop() ?? path;
}

function dirname(path: string): string {
  const cut = path.lastIndexOf("/");
  return cut <= 0 ? "" : path.slice(0, cut);
}
