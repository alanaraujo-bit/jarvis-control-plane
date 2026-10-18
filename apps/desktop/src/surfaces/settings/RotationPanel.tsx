import { useEffect } from "react";
import type { MessageKey } from "@jarvis/i18n";
import { useT } from "../../app/i18n";
import {
  useAccounts,
  type RotationMode,
  type RotationPolicy,
} from "../accounts/useAccounts";
import "./NumberSetting.css";
import "./RotationPanel.css";

const MODES: RotationMode[] = ["off", "ask", "auto"];
const DEFAULT_PERCENT = 90;

/**
 * Account rotation (M26): who decides, and what makes it act.
 *
 * Two choices and one number. The trigger is disabled rather than hidden
 * while rotation is off, for the same reason the notification switches are:
 * a control that vanishes leaves somebody wondering whether they imagined it.
 */
export function RotationPanel() {
  const t = useT();
  const report = useAccounts((state) => state.report);
  const load = useAccounts((state) => state.load);
  const setRotation = useAccounts((state) => state.setRotation);

  useEffect(() => {
    if (!report) void load("cached");
  }, [report, load]);

  if (!report) return null;
  const policy = report.rotation;
  const off = policy.mode === "off";
  const percent = policy.trigger.kind === "nearLimit" ? policy.trigger.percent : DEFAULT_PERCENT;
  const update = (next: Partial<RotationPolicy>) => void setRotation({ ...policy, ...next });

  return (
    <div className="rotation-settings">
      <div className="settings__field">
        <div className="settings__label">
          <span className="settings__label-text">{t("rotation.mode")}</span>
        </div>
        <div className="settings__segmented" role="radiogroup" aria-label={t("rotation.mode")}>
          {MODES.map((mode) => (
            <button
              key={mode}
              type="button"
              role="radio"
              aria-checked={policy.mode === mode}
              data-active={policy.mode === mode || undefined}
              className="settings__segment"
              onClick={() => update({ mode })}
            >
              {t(`rotation.mode.${mode}` as MessageKey)}
            </button>
          ))}
        </div>
      </div>

      <div className="settings__field" data-disabled={off || undefined}>
        <div className="settings__label">
          <span className="settings__label-text">{t("rotation.trigger")}</span>
        </div>
        <div className="settings__segmented" role="radiogroup" aria-label={t("rotation.trigger")}>
          <button
            type="button"
            role="radio"
            disabled={off}
            aria-checked={policy.trigger.kind === "nearLimit"}
            data-active={policy.trigger.kind === "nearLimit" || undefined}
            className="settings__segment"
            onClick={() => update({ trigger: { kind: "nearLimit", percent } })}
          >
            {t("rotation.trigger.nearLimit")}
          </button>
          <button
            type="button"
            role="radio"
            disabled={off}
            aria-checked={policy.trigger.kind === "exhausted"}
            data-active={policy.trigger.kind === "exhausted" || undefined}
            className="settings__segment"
            onClick={() => update({ trigger: { kind: "exhausted" } })}
          >
            {t("rotation.trigger.exhausted")}
          </button>
        </div>
      </div>

      {policy.trigger.kind === "nearLimit" && (
        <div className="numset" data-disabled={off || undefined}>
          <div className="numset__row">
            <span className="numset__label">{t("rotation.trigger.at")}</span>
            <div className="numset__control">
              <input
                className="numset__slider"
                type="range"
                min={50}
                max={98}
                step={1}
                value={percent}
                disabled={off}
                aria-label={t("rotation.trigger.percent", { percent })}
                onChange={(event) =>
                  update({ trigger: { kind: "nearLimit", percent: Number(event.target.value) } })
                }
              />
              <span className="numset__value">{t("rotation.trigger.percent", { percent })}</span>
            </div>
          </div>
        </div>
      )}

      <p className="rotation-settings__help">
        {policy.trigger.kind === "nearLimit"
          ? t("rotation.trigger.nearLimitHelp")
          : t("rotation.trigger.exhaustedHelp")}
      </p>
      <p className="rotation-settings__help">{t("rotation.order")}</p>
    </div>
  );
}
