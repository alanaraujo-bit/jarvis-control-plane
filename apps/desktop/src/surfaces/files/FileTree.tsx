import { useEffect } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useT } from "../../app/i18n";
import { entryKey, useFiles } from "./useFiles";

interface FileTreeProps {
  projectId: string;
  /** Path of the file currently in the editor, so the tree can mark it. */
  selected?: string;
  onOpen: (path: string) => void;
}

/**
 * The project's file tree (§41).
 *
 * Lazy by construction: a directory's contents are fetched when it is opened
 * and not before. A repository with a `node_modules` folder in it is the normal
 * case, not the exception, and eagerly walking one would make opening a project
 * feel broken.
 */
export function FileTree({ projectId, selected, onOpen }: FileTreeProps) {
  const loadDir = useFiles((state) => state.loadDir);

  useEffect(() => {
    void loadDir(projectId, "");
  }, [projectId, loadDir]);

  return (
    <div className="filetree" role="tree" aria-label="Files">
      <Level projectId={projectId} path="" depth={0} selected={selected} onOpen={onOpen} />
    </div>
  );
}

interface LevelProps {
  projectId: string;
  path: string;
  depth: number;
  selected?: string;
  onOpen: (path: string) => void;
}

function Level({ projectId, path, depth, selected, onOpen }: LevelProps) {
  const t = useT();
  const id = entryKey(projectId, path);
  const entries = useFiles((state) => state.entries[id]);
  const loading = useFiles((state) => state.loading[id]);
  const error = useFiles((state) => state.error[id]);
  const expanded = useFiles((state) => state.expanded);
  const toggleDir = useFiles((state) => state.toggleDir);

  if (error) {
    return (
      <p className="filetree__error" style={{ paddingLeft: indent(depth) }}>
        {error}
      </p>
    );
  }

  if (!entries) {
    return loading ? (
      <p className="filetree__note" style={{ paddingLeft: indent(depth) }}>
        {t("files.loading")}
      </p>
    ) : null;
  }

  if (entries.length === 0) {
    return (
      <p className="filetree__note" style={{ paddingLeft: indent(depth) }}>
        {t("files.emptyFolder")}
      </p>
    );
  }

  return (
    <>
      {entries.map((entry) => {
        const open = Boolean(expanded[entryKey(projectId, entry.path)]);
        return (
          <div key={entry.path}>
            <button
              type="button"
              role="treeitem"
              aria-expanded={entry.isDir ? open : undefined}
              aria-selected={entry.path === selected}
              className="filetree__row"
              data-dir={entry.isDir || undefined}
              data-selected={entry.path === selected || undefined}
              // Git-ignored entries are shown but recede. Hiding them would
              // make the tree disagree with the folder the user can see in
              // Explorer, which is a worse kind of surprise.
              data-ignored={entry.ignored || undefined}
              style={{ paddingLeft: indent(depth) }}
              onClick={() => (entry.isDir ? toggleDir(projectId, entry.path) : onOpen(entry.path))}
              title={entry.path}
            >
              <span className="filetree__twisty" aria-hidden="true">
                {entry.isDir ? (
                  open ? (
                    <ChevronDown size={12} strokeWidth={2} />
                  ) : (
                    <ChevronRight size={12} strokeWidth={2} />
                  )
                ) : null}
              </span>
              <span className="filetree__name">{entry.name}</span>
            </button>

            {entry.isDir && open && (
              <Level
                projectId={projectId}
                path={entry.path}
                depth={depth + 1}
                selected={selected}
                onOpen={onOpen}
              />
            )}
          </div>
        );
      })}
    </>
  );
}

/**
 * Indentation for a nesting level.
 *
 * A plain multiple rather than a nested container with padding: nesting the
 * containers makes every row at depth 8 carry eight wrapper elements, and the
 * tree is the one surface where thousands of rows are plausible.
 */
function indent(depth: number): number {
  return 6 + depth * 13;
}
