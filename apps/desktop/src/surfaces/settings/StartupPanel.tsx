import { useCallback, useEffect, useState } from "react";
import { useT } from "../../app/i18n";
import { invoke, isTauri } from "../../app/platform";
import { Switch } from "./NotificationsPanel";

/**
 * Start with the machine (§93).
 *
 * Mirrors `settings::LaunchPreferences`, and the shape carries the point: the
 * first value is read from the operating system on every call, not from this
 * product's database. A person can turn the startup entry off in Task
 * Manager's Startup tab, and a switch that kept showing "on" afterwards would
 * be asserting something Windows had already overruled.
 */
interface LaunchPreferences {
  startsWithSystem: boolean;
  startMinimized: boolean;
  /** False where no startup entry can be registered; the panel then says so. */
  supported: boolean;
}

const INITIAL: LaunchPreferences = {
  startsWithSystem: false,
  startMinimized: true,
  supported: true,
};

export function StartupPanel() {
  const t = useT();
  const [launch, setLaunch] = useState<LaunchPreferences>(INITIAL);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const read = useCallback(async () => {
    if (!isTauri()) return;
    try {
      setLaunch(await invoke<LaunchPreferences>("settings_launch"));
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  // Re-read whenever this panel is opened rather than caching for the session:
  // the answer lives outside this product and can change while it is running.
  useEffect(() => {
    void read();
  }, [read]);

  const update = useCallback(
    async (patch: Partial<Omit<LaunchPreferences, "supported">>) => {
      if (!isTauri()) return;
      setBusy(true);
      try {
        setLaunch(await invoke<LaunchPreferences>("settings_set_launch", patch));
        setError(null);
      } catch (cause) {
        // Registering a startup entry writes outside the app's own data and a
        // managed machine can refuse. Saying so beats a switch that slides
        // back with no explanation.
        setError(String(cause));
        await read();
      } finally {
        setBusy(false);
      }
    },
    [read],
  );

  return (
    // The same container the notification switches use: a switch carries its
    // own label and help, so the two-column `settings__field` grid — built for
    // a label beside a control — squeezes it into a 1fr column and wraps every
    // sentence to five words.
    <div className="notify-settings">
      <Switch
        label={t("settings.startup.withSystem")}
        help={t("settings.startup.withSystemHelp")}
        checked={launch.startsWithSystem}
        disabled={busy || !launch.supported}
        onChange={(value) => void update({ startsWithSystem: value })}
      />

      {/* Only meaningful once something starts it automatically: a person who
          opened the app by hand is looking at it. */}
      {launch.startsWithSystem && (
        <Switch
          label={t("settings.startup.minimized")}
          help={t("settings.startup.minimizedHelp")}
          checked={launch.startMinimized}
          disabled={busy}
          onChange={(value) => void update({ startMinimized: value })}
        />
      )}

      {!launch.supported && (
        <p className="settings__blurb">{t("settings.startup.unsupported")}</p>
      )}
      {error && (
        <p className="settings__blurb settings__blurb--error">
          {t("settings.startup.failed")} {error}
        </p>
      )}
    </div>
  );
}
