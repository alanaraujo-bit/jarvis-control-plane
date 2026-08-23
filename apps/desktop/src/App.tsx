import { useCallback, useEffect, useMemo, useState } from "react";
import { Languages, Moon, RefreshCw, Sun, SunMoon } from "lucide-react";
import { LOCALES, LOCALE_NAMES } from "@jarvis/i18n";
import { Rail, RAIL_ITEMS, type SurfaceId } from "./shell/Rail";
import { TitleBar } from "./shell/TitleBar";
import { StatusBar } from "./shell/StatusBar";
import { CommandPalette, type Command } from "./shell/CommandPalette";
import { Activity } from "./surfaces/activity/Activity";
import { Analytics } from "./surfaces/analytics/Analytics";
import { MissionControl } from "./surfaces/mission-control/MissionControl";
import { Missions } from "./surfaces/missions/Missions";
import { Projects } from "./surfaces/projects/Projects";
import { ProjectWorkspace } from "./surfaces/project/ProjectWorkspace";
import type { Project } from "./surfaces/projects/useProjects";
import { Settings } from "./surfaces/settings/Settings";
import { useEnvironmentStore } from "./surfaces/environment/useEnvironment";
import { useProjects } from "./surfaces/projects/useProjects";
import { useTerminals } from "./surfaces/terminal/useTerminals";
import { useI18n, useT } from "./app/i18n";
import { useTheme } from "./app/theme";
import "./App.css";

/**
 * Surfaces that actually exist.
 *
 * Navigation is derived from this list, so the product never advertises a
 * destination it cannot deliver (§81). Milestones add entries here as they
 * land; nothing has to be restructured when they do.
 */
const IMPLEMENTED: SurfaceId[] = [
  "mission-control",
  "projects",
  "missions",
  "activity",
  "analytics",
  "settings",
];

export function App() {
  const t = useT();
  const { locale, setLocale } = useI18n();
  const { preference, setPreference } = useTheme();
  const rescanEnvironment = useEnvironmentStore((state) => state.scan);
  const projects = useProjects((state) => state.projects);
  const openTerminal = useTerminals((state) => state.openTerminal);

  const [surface, setSurface] = useState<SurfaceId>("mission-control");
  const [paletteOpen, setPaletteOpen] = useState(false);
  // An open project takes over the surface area. The rail stays put, so the
  // user never loses their bearings when they go deeper (§85).
  const [openProject, setOpenProject] = useState<Project | null>(null);
  // Set when arriving at Missions from somewhere that already knows which one.
  const [focusMission, setFocusMission] = useState<string | undefined>();

  /**
   * Open a project workspace by id.
   *
   * Missions know their project as an id; the workspace needs the project
   * itself, so this is the join between the two (§86).
   */
  const openProjectById = useCallback(
    (projectId: string) => {
      const project = projects.find((p) => p.id === projectId);
      if (project) setOpenProject(project);
      return project;
    },
    [projects],
  );

  const goTo = useCallback((id: SurfaceId) => {
    setOpenProject(null);
    setFocusMission(undefined);
    setSurface(id);
  }, []);

  // Reveal the window only once the first frame has painted, so launching
  // never shows an empty white rectangle (§11).
  useEffect(() => {
    let cancelled = false;
    const reveal = async () => {
      if (!("__TAURI_INTERNALS__" in window)) return;
      const { invoke } = await import("@tauri-apps/api/core");
      if (!cancelled) await invoke("window_ready");
    };
    const frame = requestAnimationFrame(() => void reveal());
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
    };
  }, []);

  const togglePalette = useCallback(() => setPaletteOpen((open) => !open), []);

  /**
   * Ctrl/Cmd+K opens the command palette (§50), from anywhere.
   *
   * Registered in the **capture** phase, and it stops the event there. A
   * bubble-phase listener is not enough: Monaco treats Ctrl+K as a chord prefix
   * and calls `stopPropagation`, so the event never reaches `window` and the
   * palette silently stopped opening whenever the editor had focus — while the
   * titlebar went on advertising the shortcut. Found by pressing it in the real
   * app and watching "claro" get typed into a source file.
   *
   * A shortcut the whole product advertises has to be resolved before any
   * widget gets an opinion about it.
   */
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        event.stopPropagation();
        togglePalette();
      }
    };
    window.addEventListener("keydown", onKeyDown, { capture: true });
    return () => window.removeEventListener("keydown", onKeyDown, { capture: true });
  }, [togglePalette]);

  const commands = useMemo<Command[]>(() => {
    const navigation: Command[] = RAIL_ITEMS.filter((item) =>
      IMPLEMENTED.includes(item.id),
    ).map((item) => ({
      id: `go.${item.id}`,
      title: t(item.label),
      group: "Go to",
      icon: item.icon,
      run: () => goTo(item.id),
    }));

    const appearance: Command[] = [
      {
        id: "theme.dark",
        title: t("settings.theme.dark"),
        group: t("settings.appearance"),
        icon: Moon,
        keywords: "theme dark tema escuro",
        hint: preference === "dark" ? "•" : undefined,
        run: () => setPreference("dark"),
      },
      {
        id: "theme.light",
        title: t("settings.theme.light"),
        group: t("settings.appearance"),
        icon: Sun,
        keywords: "theme light tema claro",
        hint: preference === "light" ? "•" : undefined,
        run: () => setPreference("light"),
      },
      {
        id: "theme.system",
        title: t("settings.theme.system"),
        group: t("settings.appearance"),
        icon: SunMoon,
        keywords: "theme system tema sistema",
        hint: preference === "system" ? "•" : undefined,
        run: () => setPreference("system"),
      },
      ...LOCALES.map((value) => ({
        id: `locale.${value}`,
        title: LOCALE_NAMES[value],
        group: t("settings.language"),
        icon: Languages,
        keywords: `language idioma locale ${value}`,
        hint: locale === value ? "•" : undefined,
        run: () => setLocale(value),
      })),
    ];

    const actions: Command[] = [
      {
        id: "env.rescan",
        title: t("env.rescan"),
        group: t("env.title"),
        icon: RefreshCw,
        keywords: "environment scan doctor ambiente verificar",
        run: () => {
          goTo("settings");
          void rescanEnvironment(true);
        },
      },
    ];

    return [...navigation, ...appearance, ...actions];
  }, [t, locale, preference, setLocale, setPreference, rescanEnvironment, goTo]);

  return (
    <div className="app">
      <TitleBar onOpenPalette={togglePalette} />

      <div className="app__body">
        <Rail active={surface} available={IMPLEMENTED} onNavigate={goTo} />

        <main className="app__surface" key={openProject ? openProject.id : surface}>
          {openProject ? (
            <ProjectWorkspace project={openProject} onBack={() => setOpenProject(null)} />
          ) : surface === "settings" ? (
            <Settings />
          ) : surface === "projects" ? (
            <Projects onOpen={setOpenProject} />
          ) : surface === "activity" ? (
            <Activity />
          ) : surface === "analytics" ? (
            <Analytics />
          ) : surface === "missions" ? (
            <Missions
              initialMissionId={focusMission}
              // Starting an agent from a mission tags the session with it, so
              // the terminal, the conversation and the evidence all belong to
              // the same thread of work (§86).
              onLaunchAgent={(projectId, missionId) => {
                if (openProjectById(projectId)) {
                  void openTerminal(projectId, "claude-code", { cols: 120, rows: 30 }, missionId);
                }
              }}
              onOpenSession={(projectId) => {
                openProjectById(projectId);
              }}
            />
          ) : (
            <MissionControl
              onOpenProject={() => goTo("projects")}
              onOpenMission={(mission) => {
                setOpenProject(null);
                setFocusMission(mission.id);
                setSurface("missions");
              }}
            />
          )}
        </main>
      </div>

      <StatusBar />

      <CommandPalette open={paletteOpen} commands={commands} onClose={() => setPaletteOpen(false)} />
    </div>
  );
}
