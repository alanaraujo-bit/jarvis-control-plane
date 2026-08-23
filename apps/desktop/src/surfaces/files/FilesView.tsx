import { Suspense, lazy, useCallback, useEffect, useState } from "react";
import { RotateCcw, Save, X } from "lucide-react";
import { Popover } from "../../design/Popover";
import { useT } from "../../app/i18n";
import { useTheme } from "../../app/theme";
import { FileTree } from "./FileTree";
import { isDirty, useFiles } from "./useFiles";
import "./FilesView.css";

/**
 * Monaco arrives in its own chunk, on demand.
 *
 * `React.lazy` over the boundary package rather than a direct import: the
 * editor is ~3.7 MB and most sessions never open a file, so putting it in the
 * initial bundle would delay the first paint of every launch for a surface
 * nobody asked for (§11). Measured in D17.
 */
const CodeEditor = lazy(async () => {
  const { CodeEditor } = await import("@jarvis/editor");
  return { default: CodeEditor };
});

interface FilesViewProps {
  projectId: string;
}

/** Files and the editor (§41/§42), side by side inside one project. */
export function FilesView({ projectId }: FilesViewProps) {
  const t = useT();
  const resolved = useTheme((state) => state.resolved);

  const open = useFiles((state) => state.open[projectId]);
  const activePath = useFiles((state) => state.active[projectId]);
  const openFile = useFiles((state) => state.openFile);
  const closeFile = useFiles((state) => state.closeFile);
  const setActive = useFiles((state) => state.setActive);
  const edit = useFiles((state) => state.edit);
  const save = useFiles((state) => state.save);
  const reload = useFiles((state) => state.reload);

  const tabs = open ?? [];
  const active = tabs.find((file) => file.path === activePath);

  // Which tab is asking to be closed while it still has unsaved work, and the
  // button the question is anchored to.
  const [closing, setClosing] = useState<string | null>(null);
  const [closeAnchor, setCloseAnchor] = useState<HTMLButtonElement | null>(null);

  /**
   * Close a tab, but never throw away an edit on a single click.
   *
   * The dirty dot exists to say "this is not on disk yet"; closing the tab
   * disposes the model, so the undo history goes with it and there is no way
   * back. A popover rather than a modal — this is one tab, not the window
   * (§84) — and portalled, because the tab strip scrolls and would clip it.
   */
  const requestClose = (path: string, dirty: boolean, anchor: HTMLButtonElement) => {
    if (!dirty) {
      closeFile(projectId, path);
      return;
    }
    setCloseAnchor(anchor);
    setClosing(path);
  };

  const saveActive = useCallback(() => {
    if (activePath) void save(projectId, activePath);
  }, [projectId, activePath, save]);

  // Ctrl+S also works when focus is in the tree or on a tab, not only inside
  // the editor — the shortcut belongs to the surface, not to the text box.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        saveActive();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [saveActive]);

  return (
    <div className="files">
      <aside className="files__tree">
        <FileTree
          projectId={projectId}
          selected={activePath}
          onOpen={(path) => void openFile(projectId, path)}
        />
      </aside>

      <section className="files__editor">
        {tabs.length === 0 ? (
          <div className="files__empty">
            <p className="files__empty-title">{t("files.empty.title")}</p>
            <p className="files__empty-body">{t("files.empty.body")}</p>
          </div>
        ) : (
          <>
            <div className="files__tabs" role="tablist">
              {tabs.map((file) => (
                <div
                  key={file.path}
                  className="files__tab"
                  data-active={file.path === activePath || undefined}
                >
                  <button
                    type="button"
                    role="tab"
                    aria-selected={file.path === activePath}
                    className="files__tab-label"
                    onClick={() => setActive(projectId, file.path)}
                    title={file.path}
                  >
                    {basename(file.path)}
                    {/* A dot, not an asterisk in the name: the filename must
                        stay readable, and the marker is state, not text. */}
                    {isDirty(file) && <span className="files__tab-dirty" aria-hidden="true" />}
                  </button>
                  <button
                    type="button"
                    className="files__tab-close"
                    onClick={(event) =>
                      requestClose(file.path, isDirty(file), event.currentTarget)
                    }
                    aria-label={t("files.close")}
                  >
                    <X size={11} strokeWidth={2.2} aria-hidden="true" />
                  </button>
                </div>
              ))}

              {active && (
                <div className="files__actions">
                  <span className="files__path selectable">{active.path}</span>
                  <button
                    type="button"
                    className="files__action"
                    onClick={() => void reload(projectId, active.path)}
                    title={t("files.reload")}
                    aria-label={t("files.reload")}
                  >
                    <RotateCcw size={12} strokeWidth={2} aria-hidden="true" />
                  </button>
                  <button
                    type="button"
                    className="files__action"
                    data-primary
                    // Nothing to save is not an error, it is simply nothing to
                    // do — so the control says so by being unavailable.
                    disabled={!isDirty(active) || active.saving}
                    onClick={saveActive}
                    title={t("files.save")}
                    aria-label={t("files.save")}
                  >
                    <Save size={12} strokeWidth={2} aria-hidden="true" />
                  </button>
                </div>
              )}

              <Popover
                anchor={closeAnchor}
                open={closing !== null}
                onClose={() => setClosing(null)}
              >
                <p className="popover__heading">{t("files.unsaved.title")}</p>
                <button
                  type="button"
                  role="menuitem"
                  className="popover__item"
                  onClick={() => {
                    const path = closing;
                    setClosing(null);
                    if (!path) return;
                    // Close only once the write has actually landed.
                    // `save` reports failure by setting an error rather than
                    // rejecting, so the store is what has to be asked — a
                    // read-only file would otherwise close and take the edit.
                    void save(projectId, path).then(() => {
                      const saved = useFiles
                        .getState()
                        .open[projectId]?.find((f) => f.path === path);
                      if (saved && !isDirty(saved) && !saved.error) {
                        closeFile(projectId, path);
                      }
                    });
                  }}
                >
                  {t("files.unsaved.saveAndClose")}
                </button>
                <button
                  type="button"
                  role="menuitem"
                  className="popover__item"
                  data-danger
                  onClick={() => {
                    const path = closing;
                    setClosing(null);
                    if (path) closeFile(projectId, path);
                  }}
                >
                  {t("files.unsaved.discard")}
                </button>
              </Popover>
            </div>

            {/* A refused save is not an error — it is the product protecting
                work that is not ours. It reads as a warning and it says what
                the two ways out are. */}
            {active?.stale && <p className="files__warning">{t("files.stale")}</p>}
            {active?.error && <p className="files__error">{active.error}</p>}

            <div className="files__surface">
              {active?.unreadable ? (
                <div className="files__empty">
                  <p className="files__empty-title">
                    {active.unreadable === "binary"
                      ? t("files.binary.title")
                      : t("files.tooLarge.title")}
                  </p>
                  <p className="files__empty-body">
                    {active.unreadable === "binary"
                      ? t("files.binary.body")
                      : t("files.tooLarge.body", { size: formatBytes(active.size) })}
                  </p>
                </div>
              ) : (
                active && (
                  <Suspense
                    fallback={<p className="files__loading">{t("files.editorLoading")}</p>}
                  >
                    <CodeEditor
                      // Scoped by project: two projects can both contain
                      // `src/main.rs`, and they must not share one buffer.
                      scope={projectId}
                      path={active.path}
                      value={active.text}
                      theme={resolved}
                      onChange={(text) => edit(projectId, active.path, text)}
                      onSave={saveActive}
                    />
                  </Suspense>
                )
              )}
            </div>
          </>
        )}
      </section>
    </div>
  );
}

function basename(path: string): string {
  return path.split("/").pop() ?? path;
}

/** Byte counts in the unit a person would say out loud. */
function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
