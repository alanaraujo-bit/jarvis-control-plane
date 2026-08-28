import { useState } from "react";
import { useT } from "../../app/i18n";
import { isTauri } from "../../app/platform";
import { agentName, useNotifications } from "../../app/notifications";
import { describe } from "../../shell/notify/describe";
import { chime, systemToast } from "../../shell/notify/present";
import { SWITCH, usePreferences } from "./usePreferences";
import "./NotificationsPanel.css";

/**
 * Notification settings (§49, §64).
 *
 * Three switches, nested: the second and third are about *how* you are told and
 * mean nothing when the answer to the first is "don't". They are disabled
 * rather than hidden, because a control that vanishes leaves somebody
 * wondering whether they imagined it.
 *
 * ## Why there is a test button
 *
 * A desktop notification is the one part of this product whose success depends
 * on things outside it — the OS notification setting, Focus Assist, whether the
 * app has a Start Menu shortcut to be identified by. When it silently does not
 * appear, there is nothing in the interface to look at. This makes the
 * invisible thing testable in one click, which is the difference between "it
 * doesn't work" and knowing which half is broken.
 *
 * It deliberately fires the **real** path — the same call, the same words, the
 * same sound — rather than a mock. A test that takes a different route tests
 * the route it takes.
 */
export function NotificationsPanel() {
  const t = useT();
  const { prefs, setSwitch } = usePreferences();
  const setEnabled = useNotifications((state) => state.setEnabled);
  const [sent, setSent] = useState(false);

  const flip = (key: string, value: boolean) => {
    if (key === SWITCH.notifications) setEnabled(value);
    void setSwitch(key, value);
  };

  const sendTest = () => {
    // Composed through `describe`, from a notification shaped exactly like a
    // real one, so what a person sees here is what they will see later.
    const { title, body } = describe(
      {
        id: -1,
        tsMs: Date.now(),
        kind: "needsApproval",
        reason: "providerPrompt",
        confidence: "observed",
        projectId: null,
        projectName: null,
        sessionId: null,
        missionId: null,
        missionTitle: null,
        provider: "claude-code",
        preview: t("settings.notifications.testPreview"),
        detailCode: null,
        seenAt: null,
        actedAt: null,
      },
      t,
    );
    void systemToast({ title, body: body ?? agentName("claude-code") });
    if (prefs.notificationsSound) chime("waiting");
    setSent(true);
    window.setTimeout(() => setSent(false), 2400);
  };

  return (
    <div className="notify-settings">
      <Switch
        label={t("settings.notifications.enabled")}
        help={t("settings.notifications.enabledHelp")}
        checked={prefs.notificationsEnabled}
        onChange={(value) => flip(SWITCH.notifications, value)}
      />
      <Switch
        label={t("settings.notifications.system")}
        help={t("settings.notifications.systemHelp")}
        checked={prefs.notificationsSystem}
        disabled={!prefs.notificationsEnabled}
        onChange={(value) => flip(SWITCH.system, value)}
      />
      <Switch
        label={t("settings.notifications.sound")}
        checked={prefs.notificationsSound}
        disabled={!prefs.notificationsEnabled}
        onChange={(value) => flip(SWITCH.sound, value)}
      />

      {isTauri() && (
        <button
          type="button"
          className="notify-settings__test"
          onClick={sendTest}
          disabled={!prefs.notificationsEnabled || !prefs.notificationsSystem}
        >
          {sent ? t("settings.notifications.testSent") : t("settings.notifications.test")}
        </button>
      )}
    </div>
  );
}

export function Switch({
  label,
  help,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  help?: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <label className="notify-switch" data-disabled={disabled || undefined}>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={label}
        disabled={disabled}
        className="notify-switch__control"
        data-on={checked || undefined}
        onClick={() => onChange(!checked)}
      >
        <span className="notify-switch__knob" />
      </button>
      <span className="notify-switch__text">
        <span className="notify-switch__label">{label}</span>
        {help && <span className="notify-switch__help">{help}</span>}
      </span>
    </label>
  );
}
