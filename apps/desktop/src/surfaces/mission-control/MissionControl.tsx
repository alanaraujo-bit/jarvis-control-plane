import { FolderPlus } from "lucide-react";
import { useT } from "../../app/i18n";
import { Logo } from "../../design/Logo";
import "./MissionControl.css";

/**
 * Mission Control (§18) — the home surface.
 *
 * It answers, in priority order: what needs me, what is working, what is
 * blocked, what finished, which projects are active. Sections render only when
 * they have something to say; an empty section is removed rather than shown
 * empty, so the surface stays calm as activity rises and falls.
 *
 * With no projects yet the whole surface collapses to a single honest empty
 * state. There is deliberately no placeholder chart or fake counter here.
 */
export function MissionControl({ onOpenProject }: { onOpenProject: () => void }) {
  const t = useT();

  // Mission and project stores land in M5/M6; until they exist this surface
  // tells the truth rather than rendering invented rows.
  const hasAnything = false;

  if (!hasAnything) {
    return (
      <div className="mc">
        <div className="mc__empty">
          <Logo size={30} className="mc__empty-mark" />
          <h1 className="mc__empty-title">{t("missionControl.empty.title")}</h1>
          <p className="mc__empty-body">{t("missionControl.empty.body")}</p>
          <button type="button" className="mc__empty-action" onClick={onOpenProject}>
            <FolderPlus size={14} strokeWidth={1.9} aria-hidden="true" />
            {t("missionControl.empty.action")}
          </button>
        </div>
      </div>
    );
  }

  return <div className="mc" />;
}
