import { useState } from "react";
import { FolderOpen } from "lucide-react";
import { useT } from "../../app/i18n";
import { EnvironmentPanel } from "../environment/EnvironmentPanel";
import { useProjects, type Project } from "../projects/useProjects";
import { useOnboarding } from "./useOnboarding";
import "./Onboarding.css";

interface OnboardingProps {
  /**
   * Takes the `Project` object directly, not an id to look up. `openFolder`
   * already hands one back — looking it up again by id through a
   * `useCallback` closed over the *previous* render's project list is a race
   * against that list catching up, and the new project only sometimes wins
   * it. Found by opening a first project from this exact screen and landing
   * back on Mission Control instead.
   */
  onOpenProject: (project: Project) => void;
}

/**
 * The welcome screen (§13), shown exactly once per install.
 *
 * One calm screen, not a multi-step wizard — Quiet Intelligence (§6) applies
 * to a first run as much as to anything else the product shows. It reuses
 * the environment scan (§14) as-is rather than a second summary of the same
 * facts, and reuses `openFolder` rather than a bespoke folder picker.
 *
 * There is no "back" and no page indicator, because there is only one page.
 */
export function Onboarding({ onOpenProject }: OnboardingProps) {
  const t = useT();
  const openFolder = useProjects((state) => state.openFolder);
  const complete = useOnboarding((state) => state.complete);
  const [opening, setOpening] = useState(false);

  const handleOpenFolder = async () => {
    setOpening(true);
    const project = await openFolder();
    setOpening(false);
    // Cancelling the picker leaves the screen exactly as it was — nothing to
    // undo, nothing to explain.
    if (!project) return;
    await complete();
    onOpenProject(project);
  };

  return (
    <div className="onboarding">
      <div className="onboarding__inner">
        <p className="onboarding__mark">{t("app.name")}</p>
        <p className="onboarding__tagline">{t("app.tagline")}</p>
        <p className="onboarding__intro">{t("onboarding.intro")}</p>

        <div className="onboarding__environment">
          <EnvironmentPanel />
        </div>

        <div className="onboarding__actions">
          <button
            type="button"
            className="onboarding__primary"
            onClick={() => void handleOpenFolder()}
            disabled={opening}
          >
            <FolderOpen size={14} strokeWidth={1.9} aria-hidden="true" />
            {t("projects.openFolder")}
          </button>
          <button
            type="button"
            className="onboarding__skip"
            onClick={() => void complete()}
          >
            {t("onboarding.continue")}
          </button>
        </div>
      </div>
    </div>
  );
}
