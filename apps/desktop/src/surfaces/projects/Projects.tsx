import { useEffect } from "react";
import { FolderOpen, GitBranch, TriangleAlert } from "lucide-react";
import { useT } from "../../app/i18n";
import { useProjects, type Project } from "./useProjects";
import "./Projects.css";

interface ProjectsProps {
  onOpen: (project: Project) => void;
}

/**
 * Projects (§16).
 *
 * A dense list of real folders on this machine — not a gallery of cards. Each
 * row carries the facts that decide which project you want: its name, where it
 * lives, and what branch it is on.
 */
export function Projects({ onOpen }: ProjectsProps) {
  const t = useT();
  const { projects, loading, error, refresh, openFolder } = useProjects();

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const pick = async () => {
    const project = await openFolder();
    if (project) onOpen(project);
  };

  return (
    <div className="projects">
      <div className="projects__inner">
        <header className="projects__header">
          <h1 className="projects__title">{t("nav.projects")}</h1>
          <button type="button" className="projects__primary" onClick={() => void pick()}>
            <FolderOpen size={14} strokeWidth={1.9} aria-hidden="true" />
            {t("projects.openFolder")}
          </button>
        </header>

        {error && <p className="projects__error">{error}</p>}

        {!loading && projects.length === 0 ? (
          <div className="projects__empty">
            <p className="projects__empty-title">{t("projects.empty.title")}</p>
            <p className="projects__empty-body">{t("projects.empty.body")}</p>
          </div>
        ) : (
          <ul className="projects__list">
            {projects.map((project) => (
              <li key={project.id}>
                <button
                  type="button"
                  className="projects__row"
                  onClick={() => onOpen(project)}
                  data-missing={!project.exists || undefined}
                >
                  <span className="projects__name">{project.name}</span>

                  <span className="projects__meta">
                    {project.isGit && project.gitBranch && (
                      <span className="projects__branch">
                        <GitBranch size={11} strokeWidth={2} aria-hidden="true" />
                        {project.gitBranch}
                      </span>
                    )}
                    {!project.exists && (
                      <span className="projects__missing">
                        <TriangleAlert size={11} strokeWidth={2} aria-hidden="true" />
                        {t("projects.missing")}
                      </span>
                    )}
                  </span>

                  <span className="projects__path" title={project.path}>
                    {project.path}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
