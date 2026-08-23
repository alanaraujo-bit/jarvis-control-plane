import { useEffect, useState } from "react";
import { FolderGit2, GitBranch, Lock, Plus, ShieldAlert, Trash2 } from "lucide-react";
import { useT } from "../../app/i18n";
import type { MessageKey } from "@jarvis/i18n";
import type { Choice } from "../guardrails/useGuardrails";
import { useWorktrees, type BranchMode, type WorktreeView } from "./useWorktrees";
import "./WorktreesView.css";

interface WorktreesViewProps {
  projectId: string;
  /** Opening a worktree is opening a project — that is the whole design. */
  onOpenProject: (projectId: string) => void;
}

/**
 * Worktrees (§45).
 *
 * A worktree is a second checkout of the same repository, on its own branch, in
 * its own folder. It is what lets an agent work on something without touching
 * the tree the person is reading.
 *
 * Each one is **a project**, so opening it is opening a project and everything
 * else — Files, the editor, Review, sessions, missions — works inside it
 * already. That is why this surface is a list and three buttons rather than a
 * new half of the application.
 */
export function WorktreesView({ projectId, onOpenProject }: WorktreesViewProps) {
  const t = useT();
  const report = useWorktrees((s) => s.report[projectId]);
  const loading = useWorktrees((s) => s.loading[projectId]);
  const error = useWorktrees((s) => s.error[projectId]);
  const busy = useWorktrees((s) => s.busy[projectId]);
  const pending = useWorktrees((s) => s.pending);
  const refresh = useWorktrees((s) => s.refresh);
  const remove = useWorktrees((s) => s.remove);

  useEffect(() => {
    void refresh(projectId);
  }, [projectId, refresh]);

  if (report && !report.isRepo) {
    return (
      <div className="wt__empty">
        <p className="wt__empty-title">{t("worktree.notARepo.title")}</p>
        <p className="wt__empty-body">{t("worktree.notARepo.body")}</p>
      </div>
    );
  }

  const trees = report?.trees ?? [];

  return (
    <div className="wt">
      <header className="wt__header">
        <h2 className="wt__title">{t("worktree.title")}</h2>
        <p className="wt__subtitle">{t("worktree.subtitle")}</p>
      </header>

      {error && <ErrorLine error={error} />}

      <ul className="wt__list">
        {trees.map((tree) => (
          <li key={tree.path} className="wt__item" data-current={tree.isCurrent || undefined}>
            <div className="wt__identity">
              <span className="wt__branch">
                <GitBranch size={12} strokeWidth={2} aria-hidden="true" />
                {tree.branch ?? t("worktree.detached")}
              </span>
              {tree.isMain && <span className="wt__tag">{t("worktree.main")}</span>}
              {tree.isCurrent && (
                <span className="wt__tag wt__tag--current">{t("worktree.current")}</span>
              )}
              {tree.locked && (
                <span className="wt__tag" title={tree.lockReason ?? undefined}>
                  <Lock size={10} strokeWidth={2} aria-hidden="true" />
                  {t("worktree.locked")}
                </span>
              )}
              {/* Git thinks the folder is gone. Saying so beats offering to
                  open something that is not there (§81). */}
              {tree.prunable && <span className="wt__tag wt__tag--gone">{t("worktree.gone")}</span>}
            </div>

            <p className="wt__path selectable">{tree.path}</p>

            <div className="wt__actions">
              {tree.projectId && !tree.isCurrent && (
                <button
                  type="button"
                  className="wt__action"
                  onClick={() => onOpenProject(tree.projectId!)}
                >
                  {t("worktree.open")}
                </button>
              )}
              {/* A worktree Git knows about that we have never opened. Saying
                  what it is beats a button that would do nothing. */}
              {!tree.projectId && !tree.isMain && (
                <span className="wt__note">{t("worktree.notOpened")}</span>
              )}
              {!tree.isMain && (
                <button
                  type="button"
                  className="wt__action wt__action--remove"
                  disabled={busy}
                  onClick={() => void remove(projectId, tree, false)}
                >
                  <Trash2 size={12} strokeWidth={2} aria-hidden="true" />
                  {t("worktree.remove")}
                </button>
              )}
            </div>
          </li>
        ))}
      </ul>

      {pending && pending.projectId === projectId && <RemovalConfirmation pending={pending} />}

      {!loading && <NewWorktree projectId={projectId} />}
    </div>
  );
}

/**
 * A guardrail refusal arrives as a reason code so it can be localised; anything
 * else is Git talking, and Git's own words are the useful ones.
 */
function ErrorLine({ error }: { error: string }) {
  const t = useT();
  const refused = error.startsWith("refused:");
  return (
    <p className="wt__error" data-refused={refused || undefined}>
      {refused ? t("worktree.refused") : error}
    </p>
  );
}

/**
 * Removing a worktree that has work in it.
 *
 * Two different questions, and they are not merged. First Git says the tree has
 * uncommitted work — that is information, and the answer is a plain "remove it
 * anyway". Only that second attempt is a guarded operation, and only then are
 * the §35 choices offered. Showing "Always allow" before anyone has said
 * anything would be offering to switch off a guardrail nobody had met yet.
 */
function RemovalConfirmation({
  pending,
}: {
  pending: { projectId: string; tree: WorktreeView; command: string; guarded: boolean };
}) {
  const t = useT();
  const remove = useWorktrees((s) => s.remove);
  const cancel = useWorktrees((s) => s.cancelPending);
  const name = pending.tree.branch ?? pending.tree.path;

  return (
    <section className="wt__confirm" role="alertdialog">
      <header className="wt__confirm-head">
        <ShieldAlert size={14} strokeWidth={1.9} aria-hidden="true" />
        <h3 className="wt__confirm-title">{t("worktree.confirm.title", { branch: name })}</h3>
      </header>
      <p className="wt__confirm-body">{t("worktree.confirm.body")}</p>
      <p className="wt__confirm-willrun">
        {t("review.confirm.willRun")}: <code className="selectable">{pending.command}</code>
      </p>

      <div className="wt__confirm-choices">
        {pending.guarded ? (
          (["allowOnce", "allowForProject", "alwaysAllow", "neverAllow"] as Choice[]).map(
            (choice) => (
              <button
                key={choice}
                type="button"
                className="wt__confirm-choice"
                data-choice={choice}
                onClick={() => void remove(pending.projectId, pending.tree, true, choice)}
              >
                {t(`guardrail.choice.${choice}` as MessageKey)}
              </button>
            ),
          )
        ) : (
          <button
            type="button"
            className="wt__confirm-choice wt__confirm-choice--danger"
            onClick={() => void remove(pending.projectId, pending.tree, true)}
          >
            {t("worktree.confirm.anyway")}
          </button>
        )}
        <button type="button" className="wt__confirm-cancel" onClick={cancel}>
          {t("review.confirm.cancel")}
        </button>
      </div>
    </section>
  );
}

/**
 * Creating one.
 *
 * The person names a **branch**, never a folder. The directory is derived in
 * the core and shown back to them — this is the one operation in the product
 * that writes outside a project root, and letting the webview choose where
 * would be handing it the arbitrary directory creation path confinement exists
 * to prevent (§41).
 */
function NewWorktree({ projectId }: { projectId: string }) {
  const t = useT();
  const [branch, setBranch] = useState("");
  const [mode, setMode] = useState<BranchMode>("create");
  const add = useWorktrees((s) => s.add);
  const busy = useWorktrees((s) => s.busy[projectId]);

  return (
    <form
      className="wt__new"
      onSubmit={(event) => {
        event.preventDefault();
        if (!branch.trim() || busy) return;
        void add(projectId, branch.trim(), mode).then((created) => {
          if (created) setBranch("");
        });
      }}
    >
      <label className="wt__new-label" htmlFor={`wt-branch-${projectId}`}>
        {t("worktree.new.title")}
      </label>
      <div className="wt__new-row">
        <input
          id={`wt-branch-${projectId}`}
          className="wt__new-input"
          value={branch}
          placeholder={t("worktree.new.placeholder")}
          onChange={(event) => setBranch(event.target.value)}
        />
        <div className="wt__new-mode" role="group" aria-label={t("worktree.new.title")}>
          {(["create", "existing"] as BranchMode[]).map((option) => (
            <button
              key={option}
              type="button"
              className="wt__new-modeButton"
              data-active={mode === option || undefined}
              aria-pressed={mode === option}
              onClick={() => setMode(option)}
            >
              {t(`worktree.new.${option}` as MessageKey)}
            </button>
          ))}
        </div>
        <button type="submit" className="wt__new-submit" disabled={!branch.trim() || busy}>
          <Plus size={12} strokeWidth={2.2} aria-hidden="true" />
          {t("worktree.new.action")}
        </button>
      </div>
      <p className="wt__new-hint">
        <FolderGit2 size={11} strokeWidth={2} aria-hidden="true" />
        {t("worktree.new.hint")}
      </p>
    </form>
  );
}
