import { useEffect, useState } from "react";
import { Bell, Gauge, LogIn, LogOut, Palette, SquareTerminal } from "lucide-react";
import type { MessageKey } from "@jarvis/i18n";
import { useT } from "../../app/i18n";
import { invoke } from "../../app/platform";
import { initials } from "./Auth";
import { MIN_PASSWORD, useIdentity, type IdentityReport } from "./useIdentity";
import "./AccountPanel.css";

const CARRIED: { icon: typeof Palette; label: MessageKey }[] = [
  { icon: Palette, label: "identity.carries.appearance" },
  { icon: SquareTerminal, label: "identity.carries.terminal" },
  { icon: Gauge, label: "identity.carries.autonomy" },
  { icon: Bell, label: "identity.carries.notifications" },
];

/**
 * The Account section of Settings (§64, M20).
 *
 * Signed out, it is one paragraph and one button — and the paragraph says the
 * product works as it is, because that is true and because a settings screen is
 * exactly where somebody goes to find out whether they have missed something.
 *
 * Signed in, it is the four things anybody ever wants here: who you are, what
 * this account carries, the password, and the way out. Deleting is last,
 * separated, and asks for the password — a destructive control that needs one
 * click is one somebody eventually hits by accident.
 */
export function AccountPanel() {
  const t = useT();
  const report = useIdentity((state) => state.report);
  const openAuth = useIdentity((state) => state.openAuth);
  const signOut = useIdentity((state) => state.signOut);
  const put = useIdentity((state) => state.put);

  const account = report?.account ?? null;

  const [name, setName] = useState(account?.displayName ?? "");
  const [email, setEmail] = useState(account?.email ?? "");
  const [savedAt, setSavedAt] = useState(0);
  const [profileError, setProfileError] = useState<string | null>(null);

  const [currentPassword, setCurrentPassword] = useState("");
  const [nextPassword, setNextPassword] = useState("");
  const [passwordDone, setPasswordDone] = useState(false);
  const [passwordError, setPasswordError] = useState<string | null>(null);

  const [confirm, setConfirm] = useState("");
  const [deleteError, setDeleteError] = useState<string | null>(null);

  // The fields follow the account, which changes under this panel whenever
  // somebody signs in or out from the auth screen without leaving Settings.
  useEffect(() => {
    setName(account?.displayName ?? "");
    setEmail(account?.email ?? "");
    setProfileError(null);
  }, [account?.id, account?.displayName, account?.email]);

  // "Saved" is a moment, not a state — it says the write happened and then
  // gets out of the way.
  useEffect(() => {
    if (!savedAt) return;
    const timer = window.setTimeout(() => setSavedAt(0), 2400);
    return () => window.clearTimeout(timer);
  }, [savedAt]);

  if (report === null) return null;

  if (!account) {
    return (
      <div className="account">
        <header className="account__head">
          <h2 className="account__title">{t("identity.settings.signedOut.title")}</h2>
          <p className="account__blurb">{t("identity.settings.signedOut.body")}</p>
        </header>
        <button type="button" className="account__primary" onClick={openAuth}>
          <LogIn size={14} strokeWidth={1.9} aria-hidden="true" />
          {t("identity.settings.open")}
        </button>
      </div>
    );
  }

  const saveProfile = async () => {
    setProfileError(null);
    try {
      put(
        await invoke<IdentityReport>("identity_update_profile", {
          displayName: name,
          email,
        }),
      );
      setSavedAt(Date.now());
    } catch (cause) {
      // The core answers with a message key, so the reason is said in the
      // person's own language rather than as a raw error string.
      setProfileError(String(cause));
    }
  };

  const changePassword = async () => {
    setPasswordError(null);
    setPasswordDone(false);
    try {
      await invoke("identity_change_password", { currentPassword, nextPassword });
      setCurrentPassword("");
      setNextPassword("");
      setPasswordDone(true);
    } catch (cause) {
      setPasswordError(String(cause));
    }
  };

  const remove = async () => {
    setDeleteError(null);
    try {
      put(await invoke<IdentityReport>("identity_delete", { password: confirm }));
      setConfirm("");
    } catch (cause) {
      setDeleteError(String(cause));
    }
  };

  const since = new Date(account.createdAt).toLocaleDateString();

  return (
    <div className="account">
      <header className="account__identity">
        <span className="account__avatar" aria-hidden="true">
          {initials(account.displayName)}
        </span>
        <div>
          <p className="account__name">{account.displayName}</p>
          <p className="account__email">{account.email}</p>
          <p className="account__since">{t("identity.settings.since", { when: since })}</p>
        </div>
        <button type="button" className="account__ghost" onClick={() => void signOut()}>
          <LogOut size={13} strokeWidth={1.9} aria-hidden="true" />
          {t("identity.action.signOut")}
        </button>
      </header>

      <section className="account__section">
        <h3 className="account__section-title">{t("identity.settings.carried")}</h3>
        <ul className="account__carried">
          {CARRIED.map((item) => (
            <li key={item.label}>
              <item.icon size={13} strokeWidth={1.7} aria-hidden="true" />
              {t(item.label)}
            </li>
          ))}
        </ul>
        <p className="account__note">{t("identity.carries.local")}</p>
      </section>

      <section className="account__section">
        <h3 className="account__section-title">{t("identity.settings.profile")}</h3>
        <div className="account__row">
          <label className="account__field">
            <span>{t("identity.field.name")}</span>
            <input
              className="account__input"
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
          </label>
          <label className="account__field">
            <span>{t("identity.field.email")}</span>
            <input
              className="account__input"
              type="email"
              spellCheck={false}
              value={email}
              onChange={(event) => setEmail(event.target.value)}
            />
          </label>
        </div>
        <div className="account__actions">
          <button
            type="button"
            className="account__primary"
            disabled={name === account.displayName && email === account.email}
            onClick={() => void saveProfile()}
          >
            {t("common.save")}
          </button>
          {savedAt > 0 && <span className="account__ok">{t("identity.settings.saved")}</span>}
          {profileError && <span className="account__bad">{t(errorKey(profileError), { min: MIN_PASSWORD })}</span>}
        </div>
      </section>

      {account.hasPassword && (
        <section className="account__section">
          <h3 className="account__section-title">{t("identity.settings.password")}</h3>
          <div className="account__row">
            <label className="account__field">
              <span>{t("identity.settings.password.current")}</span>
              <input
                className="account__input"
                type="password"
                autoComplete="current-password"
                value={currentPassword}
                onChange={(event) => setCurrentPassword(event.target.value)}
              />
            </label>
            <label className="account__field">
              <span>{t("identity.settings.password.next")}</span>
              <input
                className="account__input"
                type="password"
                autoComplete="new-password"
                value={nextPassword}
                onChange={(event) => setNextPassword(event.target.value)}
              />
            </label>
          </div>
          <div className="account__actions">
            <button
              type="button"
              className="account__secondary"
              disabled={!currentPassword || nextPassword.length < MIN_PASSWORD}
              onClick={() => void changePassword()}
            >
              {t("identity.settings.password.change")}
            </button>
            {passwordDone && (
              <span className="account__ok">{t("identity.settings.password.changed")}</span>
            )}
            {passwordError && <span className="account__bad">{t(errorKey(passwordError), { min: MIN_PASSWORD })}</span>}
          </div>
        </section>
      )}

      <section className="account__section account__section--danger">
        <h3 className="account__section-title">{t("identity.settings.danger")}</h3>
        <p className="account__note">{t("identity.settings.danger.body")}</p>
        <div className="account__actions">
          {account.hasPassword && (
            <input
              className="account__input account__input--inline"
              type="password"
              placeholder={t("identity.settings.danger.confirm")}
              value={confirm}
              onChange={(event) => setConfirm(event.target.value)}
            />
          )}
          <button
            type="button"
            className="account__danger"
            disabled={account.hasPassword && confirm.length === 0}
            onClick={() => void remove()}
          >
            {t("identity.settings.danger.action")}
          </button>
          {deleteError && <span className="account__bad">{t(errorKey(deleteError), { min: MIN_PASSWORD })}</span>}
        </div>
      </section>
    </div>
  );
}

/**
 * The core answers with a message key, not a sentence.
 *
 * That is the same contract the guardrail refusal and the evidence summaries
 * use (§65) — the core states *what* happened and the surface says it in the
 * person's language. Anything unrecognised falls back rather than rendering a
 * Rust string at somebody.
 */
const ERRORS: Record<string, MessageKey> = {
  "identity.nameRequired": "identity.error.nameRequired",
  "identity.invalidEmail": "identity.error.invalidEmail",
  "identity.emailTaken": "identity.error.emailTaken",
  "identity.wrongPassword": "identity.error.currentWrong",
  "identity.passwordTooShort": "identity.error.passwordTooShort",
  "identity.noPassword": "identity.error.noPassword",
};

function errorKey(raw: string): MessageKey {
  return ERRORS[raw.trim()] ?? "identity.error.generic";
}
