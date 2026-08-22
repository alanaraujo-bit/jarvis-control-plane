import { useCallback, useEffect, useMemo, useState } from "react";
import { Languages, Moon, RefreshCw, Sun, SunMoon } from "lucide-react";
import { LOCALES, LOCALE_NAMES } from "@jarvis/i18n";
import { Rail, RAIL_ITEMS, type SurfaceId } from "./shell/Rail";
import { TitleBar } from "./shell/TitleBar";
import { StatusBar } from "./shell/StatusBar";
import { CommandPalette, type Command } from "./shell/CommandPalette";
import { MissionControl } from "./surfaces/mission-control/MissionControl";
import { Settings } from "./surfaces/settings/Settings";
import { useEnvironmentStore } from "./surfaces/environment/useEnvironment";
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
const IMPLEMENTED: SurfaceId[] = ["mission-control", "settings"];

export function App() {
  const t = useT();
  const { locale, setLocale } = useI18n();
  const { preference, setPreference } = useTheme();
  const rescanEnvironment = useEnvironmentStore((state) => state.scan);

  const [surface, setSurface] = useState<SurfaceId>("mission-control");
  const [paletteOpen, setPaletteOpen] = useState(false);

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

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        togglePalette();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [togglePalette]);

  const commands = useMemo<Command[]>(() => {
    const navigation: Command[] = RAIL_ITEMS.filter((item) =>
      IMPLEMENTED.includes(item.id),
    ).map((item) => ({
      id: `go.${item.id}`,
      title: t(item.label),
      group: "Go to",
      icon: item.icon,
      run: () => setSurface(item.id),
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
          setSurface("settings");
          void rescanEnvironment(true);
        },
      },
    ];

    return [...navigation, ...appearance, ...actions];
  }, [t, locale, preference, setLocale, setPreference, rescanEnvironment]);

  return (
    <div className="app">
      <TitleBar onOpenPalette={togglePalette} />

      <div className="app__body">
        <Rail active={surface} available={IMPLEMENTED} onNavigate={setSurface} />

        <main className="app__surface" key={surface}>
          {surface === "settings" ? (
            <Settings />
          ) : (
            <MissionControl onOpenProject={() => setSurface("mission-control")} />
          )}
        </main>
      </div>

      <StatusBar />

      <CommandPalette open={paletteOpen} commands={commands} onClose={() => setPaletteOpen(false)} />
    </div>
  );
}
