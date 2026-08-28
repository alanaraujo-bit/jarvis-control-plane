import { LOCALES, LOCALE_NAMES } from "@jarvis/i18n";
import { useI18n, useT } from "../../app/i18n";
import { useTheme, type ThemePreference } from "../../app/theme";
import { EnvironmentPanel } from "../environment/EnvironmentPanel";
import { AccountPanel } from "../identity/AccountPanel";
import { GuardrailPanel } from "../guardrails/GuardrailPanel";
import { AutonomyPanel } from "./AutonomyPanel";
import { CATEGORIES, CATEGORY, type CategoryId } from "./categories";
import { CompanionPanel } from "./CompanionPanel";
import { NotificationsPanel, Switch } from "./NotificationsPanel";
import { NumberSetting } from "./NumberSetting";
import { useSettingsCategory } from "./settingsNav";
import { PREF, usePreferences } from "./usePreferences";
import { Updates } from "./Updates";
import "./Settings.css";

const THEME_OPTIONS = [
  { value: "dark", label: "settings.theme.dark" },
  { value: "light", label: "settings.theme.light" },
  { value: "system", label: "settings.theme.system" },
] as const;

/**
 * Settings (§64), as a place with rooms.
 *
 * It used to be one column holding every group at once. That put the theme
 * picker, the guardrail table, the phone pairing code and the updater on the
 * same page, and the result was a screen nobody could aim at: to change the
 * terminal's type size you scrolled past four unrelated decisions, and to know
 * whether a setting existed at all you had to read every one of them.
 *
 * Now one section is on screen at a time and the left column is the map. The
 * gain is not tidiness — it is that the map is readable *without opening
 * anything*. Eight names say what this product can be configured to do; the old
 * screen only said that after you had scrolled the whole of it.
 *
 * ## One heading per section, never two
 *
 * Half the old clutter was double-titling: the page said "Phone" and the panel
 * underneath said "Mobile companion". So a section whose panel already titles
 * and explains itself — Autonomy, Guardrails, the phone, Environment, Updates —
 * gets **no** heading from this component; the panel's own header is the
 * heading. Only the three sections built out of loose fields (Appearance,
 * Terminal, Notifications) get one from here, in the shape the panels use.
 *
 * That also leaves `AutonomyPanel` and `GuardrailPanel` untouched — both are
 * rendered inside a project as well (§19), where their own headers are the only
 * headings there are, so a prop that suppressed them would have to be threaded
 * through two call sites to solve a problem only one of them has.
 */
export function Settings() {
  const t = useT();
  const [category, goTo] = useSettingsCategory();

  return (
    <div className="settings">
      <div className="settings__nav">
        <h1 className="settings__heading">{t("nav.settings")}</h1>

        {/* Navigation, rendered as navigation: a `<nav>` of buttons carrying
            `aria-current`, the same shape as the rail and the areas inside a
            project. It must not be spelled as a segmented control — those pick
            a value, and this changes what you are looking at. */}
        <nav className="settings__list" aria-label={t("settings.nav.label")}>
          {CATEGORIES.map((id) => {
            const { icon: Icon, label } = CATEGORY[id];
            const active = category === id;
            return (
              <button
                key={id}
                type="button"
                className="settings__link"
                data-active={active || undefined}
                aria-current={active ? "page" : undefined}
                // In a narrow window the map is a strip across the top, and a
                // section arrived at from the command palette was landing off
                // the right-hand edge of it — the page showed Environment while
                // the strip showed no selection at all. Found at 620px, not in
                // the stylesheet. `nearest` so the common case, where the item
                // is already visible, moves nothing.
                ref={
                  active
                    ? (node) => {
                        // Braces on purpose: React 19 reads a callback ref's
                        // return value as a cleanup function, so this must not
                        // be an expression body.
                        node?.scrollIntoView({ block: "nearest", inline: "nearest" });
                      }
                    : undefined
                }
                onClick={() => goTo(id)}
              >
                <Icon size={15} strokeWidth={1.75} aria-hidden="true" />
                <span className="settings__link-text">{t(label)}</span>
              </button>
            );
          })}
        </nav>
      </div>

      {/* Keyed on the section so a switch starts the pane at the top, rather
          than landing halfway down a short panel because the previous one was
          long and the scroll position survived. */}
      <div className="settings__pane" key={category}>
        <div className="settings__pane-inner">
          <Section id={category} />
        </div>
      </div>
    </div>
  );
}

/**
 * The heading for a section built out of loose fields rather than a panel.
 *
 * Icon, title, one line — the shape `AutonomyPanel` and `GuardrailPanel`
 * already draw, and the icon is the *same* one the left column shows for this
 * section. Seen side by side, three sections headed without an icon and five
 * with one read as two different screens; that they matched mattered more than
 * whether the icon adds information, which it does not.
 */
function Head({ id }: { id: CategoryId }) {
  const t = useT();
  const { icon: Icon, label, blurb } = CATEGORY[id];
  if (!blurb) return null;
  return (
    <header className="settings__head">
      <h2 className="settings__title">
        <Icon size={15} strokeWidth={1.75} aria-hidden="true" />
        {t(label)}
      </h2>
      <p className="settings__blurb">{t(blurb)}</p>
    </header>
  );
}

function Section({ id }: { id: CategoryId }) {
  const t = useT();
  const { locale, setLocale } = useI18n();
  const { preference, setPreference } = useTheme();
  const { prefs, set, setPerformanceHud } = usePreferences();

  switch (id) {
    case "account":
      return (
        <>
          <Head id="account" />
          <AccountPanel />
        </>
      );

    case "appearance":
      return (
        <>
          <Head id="appearance" />

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
            <div
              className="settings__segmented"
              role="radiogroup"
              aria-label={t("settings.language")}
            >
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
        </>
      );

    case "terminal":
      return (
        <>
          <Head id="terminal" />
          <div className="settings__field">
            <Switch
              label={t("settings.performanceHud")}
              help={t("settings.performanceHudHelp")}
              checked={prefs.performanceHudEnabled}
              onChange={(value) => void setPerformanceHud(value)}
            />
          </div>
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
        </>
      );

    case "autonomy":
      return (
        <>
          <AutonomyPanel />
          {/* The turn limit stays with Autonomy rather than moving to the
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
        </>
      );

    // Guardrails left Autonomy's page and became a section of their own. They
    // had been the tail of an "Agents" group that also held two other things,
    // and the table is one row per operation — a list the core is free to grow
    // without this file being touched (§26). A section that can get longer on
    // its own should not be sharing a page with anything.
    case "guardrails":
      return <GuardrailPanel />;

    case "notifications":
      return (
        <>
          <Head id="notifications" />
          <NotificationsPanel />
        </>
      );

    case "companion":
      return <CompanionPanel />;

    case "environment":
      return <EnvironmentPanel />;

    case "updates":
      return <Updates />;
  }
}
