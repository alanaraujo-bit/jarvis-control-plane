import type { MessageKey } from "@jarvis/i18n";
import { useT } from "../../app/i18n";
import { BOUNDS } from "./usePreferences";
import "./NumberSetting.css";

interface NumberSettingProps {
  /** Which preference this control owns. */
  preference: keyof typeof BOUNDS;
  label: MessageKey;
  /** How the value reads as a sentence — "20000 lines", "24 turns". */
  unit?: MessageKey;
  help?: MessageKey;
  value: number;
  onChange: (value: number | null) => void;
}

/**
 * One bounded number, as a slider with its value beside it (§64).
 *
 * A slider rather than a text field because every one of these is a bounded
 * range with a sensible default, and a field invites typing a number the core
 * will refuse — a control that can express an invalid answer is a control that
 * has to explain itself afterwards. The bounds are the shape of the thing, so
 * the shape should be visible.
 *
 * **Reset appears only when the value is not the default.** A permanent reset
 * button beside an untouched setting is a control that does nothing, and
 * "Default" beside the number is what tells you there is nothing to reset.
 */
export function NumberSetting({
  preference,
  label,
  unit,
  help,
  value,
  onChange,
}: NumberSettingProps) {
  const t = useT();
  const bounds = BOUNDS[preference];
  const isDefault = value === bounds.default;
  // Locale-aware, so a scrollback of 20000 reads as "20.000" in pt-BR and
  // "20,000" in English rather than as a bare run of digits.
  const shown = value.toLocaleString();

  return (
    <div className="numset">
      <div className="numset__row">
        <span className="numset__label">{t(label)}</span>

        <div className="numset__control">
          <input
            className="numset__slider"
            type="range"
            min={bounds.min}
            max={bounds.max}
            step={bounds.step}
            value={value}
            aria-label={t(label)}
            // `valueText`, not the bare number: a screen reader should say
            // "20,000 lines", which is the sentence the eye gets too.
            aria-valuetext={unit ? t(unit, { 0: shown }) : shown}
            onChange={(event) => onChange(Number(event.target.value))}
          />

          <span className="numset__value">{unit ? t(unit, { 0: shown }) : shown}</span>

          {isDefault ? (
            <span className="numset__default">{t("settings.default")}</span>
          ) : (
            <button
              type="button"
              className="numset__reset"
              // `null` restores the default rather than writing today's
              // default as a chosen value — the core treats those as
              // different things and so should this.
              onClick={() => onChange(null)}
            >
              {t("settings.reset")}
            </button>
          )}
        </div>
      </div>

      {help && <p className="numset__help">{t(help)}</p>}
    </div>
  );
}
