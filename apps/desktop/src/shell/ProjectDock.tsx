import { GitBranch, X } from "lucide-react";
import { useT } from "../app/i18n";
import type { Project } from "../surfaces/projects/useProjects";
import "./ProjectDock.css";

interface ProjectDockProps {
  projects: Project[];
  activeProjectId: string | null;
  onActivate: (project: Project) => void;
  onClose: (projectId: string) => void;
}

export function ProjectDock({ projects, activeProjectId, onActivate, onClose }: ProjectDockProps) {
  const t = useT();
  if (projects.length === 0) return null;
  return (
    <nav className="project-dock" aria-label={t("workspace.openProjects")}>
      {projects.map((project) => (
        <div
          key={project.id}
          className="project-dock__item"
          data-active={project.id === activeProjectId || undefined}
        >
          <button
            type="button"
            className="project-dock__activate"
            onClick={() => onActivate(project)}
            title={project.path}
          >
            <span className="project-dock__name">{project.name}</span>
            {project.gitBranch && (
              <span className="project-dock__branch">
                <GitBranch size={10} aria-hidden="true" />
                {project.gitBranch}
              </span>
            )}
          </button>
          <button
            type="button"
            className="project-dock__close"
            onClick={() => onClose(project.id)}
            aria-label={t("workspace.closeProject", { name: project.name })}
            title={t("workspace.closeProjectHint")}
          >
            <X size={11} aria-hidden="true" />
          </button>
        </div>
      ))}
    </nav>
  );
}
