import { LOCALES, LOCALE_NAMES } from "@jarvis/i18n";
import { useI18n, useT } from "../../app/i18n";
import { useTheme, type ThemePreference } from "../../app/theme";
import { EnvironmentPanel } from "../environment/EnvironmentPanel";
import { GuardrailPanel } from "../guardrails/GuardrailPanel";
import { AutonomyPanel } from "./AutonomyPanel";
import { CompanionPanel } from "./CompanionPanel";
import { NotificationsPanel } from "./NotificationsPanel";
import { NumberSetting } from "./NumberSetting";
import { PREF, usePreferences } from "./usePreferences";
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
  const { prefs, set } = usePreferences();

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

        {/* Terminal before Agents: it is the surface people spend the day in,
            and these are the settings most likely to be changed on a first
            visit. Frequency of use decides the order (§64). */}
        <section className="settings__section">
          <h2 className="settings__section-title">{t("settings.terminal")}</h2>
          <NumberSetting
            preference={PREF.fontSize}
            label="settings.fontSize"
            value={prefs.terminalFontSize}
            onChange={(value) => void set(PREF.fontSize, value)}
          />
          <NumberSetting
            preference={PREF.scrollback}
            label="settings.scrollback"
            unit="settings.scrollbackUnit"
            help="settings.scrollbackHelp"
            value={prefs.terminalScrollback}
            onChange={(value) => void set(PREF.scrollback, value)}
          />
        </section>

        {/* Autonomy before guardrails, on purpose: how much an agent does on
            its own is the broader question, and what it may never do is the
            fence around that answer. Reading them the other way round asks
            someone to bound a behaviour they have not chosen yet. */}
        <section className="settings__section">
          <h2 className="settings__section-title">{t("settings.agents")}</h2>
          <AutonomyPanel />
          {/* The turn limit sits under Autonomy rather than beside the
              terminal settings: it only means anything for an Unattended run,
              which is the choice directly above it. */}
          <NumberSetting
            preference={PREF.turnBudget}
            label="settings.turnBudget"
            unit="settings.turnBudgetUnit"
            help="settings.turnBudgetHelp"
            value={prefs.autopilotTurnBudget}
            onChange={(value) => void set(PREF.turnBudget, value)}
          />
          <GuardrailPanel />
        </section>

        {/* Notifications sit after Agents because what they report on is
            agents stopping, and before the companion because they are the
            same question answered on this machine rather than on a phone. */}
        <section className="settings__section">
          <h2 className="settings__section-title">{t("settings.notifications")}</h2>
          <NotificationsPanel />
        </section>

        {/* The companion sits after Agents and before Environment: it is
            about this desktop rather than about what agents do, and it is the
            one section that sends anything anywhere. */}
        <section className="settings__section">
          <h2 className="settings__section-title">{t("settings.companion")}</h2>
          <CompanionPanel />
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
