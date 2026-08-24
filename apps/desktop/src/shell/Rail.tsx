import {
  Activity,
  ChartNoAxesColumn,
  FolderGit2,
  Settings,
  Radar,
  Target,
  UsersRound,
  type LucideIcon,
} from "lucide-react";
import { useT } from "../app/i18n";
import type { MessageKey } from "@jarvis/i18n";
import "./Rail.css";

export type SurfaceId =
  | "mission-control"
  | "projects"
  | "missions"
  | "activity"
  | "analytics"
  | "accounts"
  | "settings";

export interface RailItem {
  id: SurfaceId;
  icon: LucideIcon;
  label: MessageKey;
  /** Settings sits apart, at the foot of the rail. */
  footer?: boolean;
}

/**
 * Global navigation (§87).
 *
 * Global destinations only. Project-scoped tools — terminal,
 * files, editor, diff, preview, notes — do not live here; they appear inside a
 * project, where they have context (§85).
 */
export const RAIL_ITEMS: RailItem[] = [
  { id: "mission-control", icon: Radar, label: "nav.missionControl" },
  { id: "projects", icon: FolderGit2, label: "nav.projects" },
  { id: "missions", icon: Target, label: "nav.missions" },
  { id: "activity", icon: Activity, label: "nav.activity" },
  { id: "analytics", icon: ChartNoAxesColumn, label: "nav.analytics" },
  { id: "accounts", icon: UsersRound, label: "nav.accounts" },
  { id: "settings", icon: Settings, label: "nav.settings", footer: true },
];

interface RailProps {
  active: SurfaceId;
  /** Only these destinations are rendered — see `IMPLEMENTED` in App. */
  available: SurfaceId[];
  onNavigate: (id: SurfaceId) => void;
}

export function Rail({ active, available, onNavigate }: RailProps) {
  const t = useT();
  const items = RAIL_ITEMS.filter((item) => available.includes(item.id));

  const renderItem = ({ id, icon: Icon, label }: RailItem) => {
    const isActive = active === id;
    return (
      <button
        key={id}
        type="button"
        className="rail__item"
        data-active={isActive || undefined}
        onClick={() => onNavigate(id)}
        aria-current={isActive ? "page" : undefined}
      >
        <Icon size={18} strokeWidth={1.75} aria-hidden="true" />
        <span className="sr-only">{t(label)}</span>
        <span className="rail__tip" role="tooltip">
          {t(label)}
        </span>
      </button>
    );
  };

  return (
    <nav className="rail" aria-label={t("nav.missionControl")}>
      <div className="rail__group">{items.filter((i) => !i.footer).map(renderItem)}</div>
      <div className="rail__group">{items.filter((i) => i.footer).map(renderItem)}</div>
    </nav>
  );
}
