import { LOCALES, LOCALE_NAMES } from "@jarvis/i18n";
import { useI18n, useT } from "../../app/i18n";
import { useTheme, type ThemePreference } from "../../app/theme";
import { EnvironmentPanel } from "../environment/EnvironmentPanel";
import { Updates } from "./Updates";
import "./Settings.css";

const THEME_OPTIONS = [
  { value: "dark", label: "settings.theme.dark" },
  { value: "light", label: "settings.theme.light" },
  { value: "system", label: "settings.theme.system" },
] as const;

/**
 * Settings (§64).
 *
 * Grouped into flat sections rather than a nested menu tree. Only sections
 * backed by working functionality appear — the remaining groups arrive with
 * the features they configure.
 */
export function Settings() {
  const t = useT();
  const { locale, setLocale } = useI18n();
  const { preference, setPreference } = useTheme();

  return (
    <div className="settings">
      <div className="settings__inner">
        <h1 className="settings__heading">{t("nav.settings")}</h1>

        <section className="settings__section">
          <h2 className="settings__section-title">{t("settings.appearance")}</h2>

          <div className="settings__field">
            <div className="settings__label">
              <span className="settings__label-text">{t("settings.theme")}</span>
            </div>
            <div className="settings__segmented" role="radiogroup" aria-label={t("settings.theme")}>
              {THEME_OPTIONS.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  role="radio"
                  aria-checked={preference === option.value}
                  data-active={preference === option.value || undefined}
                  className="settings__segment"
                  onClick={() => setPreference(option.value as ThemePreference)}
                >
                  {t(option.label)}
                </button>
              ))}
            </div>
          </div>

          <div className="settings__field">
            <div className="settings__label">
              <span className="settings__label-text">{t("settings.language")}</span>
            </div>
            <div className="settings__segmented" role="radiogroup" aria-label={t("settings.language")}>
              {LOCALES.map((value) => (
                <button
                  key={value}
                  type="button"
                  role="radio"
                  aria-checked={locale === value}
                  data-active={locale === value || undefined}
                  className="settings__segment"
                  onClick={() => setLocale(value)}
                >
                  {LOCALE_NAMES[value]}
                </button>
              ))}
            </div>
          </div>
        </section>

        <section className="settings__section">
          <EnvironmentPanel />
        </section>

        <section className="settings__section">
          <Updates />
        </section>
      </div>
    </div>
  );
}
