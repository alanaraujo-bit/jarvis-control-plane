import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowLeft,
  ArrowRight,
  Bell,
  Eye,
  EyeOff,
  Gauge,
  Palette,
  SquareTerminal,
} from "lucide-react";
import type { MessageKey } from "@jarvis/i18n";
import { useT } from "../../app/i18n";
import { Logo } from "../../design/Logo";
import { relative } from "../history/format";
import { GoogleMark } from "./GoogleMark";
import { domainSuggestion, STRENGTH_LABEL, strengthOf } from "./password";
import { MIN_PASSWORD, useIdentity, type KnownAccount } from "./useIdentity";
import "./Auth.css";

type Mode = "signIn" | "signUp";

/** The preferences an account carries, said in the order somebody meets them. */
const CARRIES: { icon: typeof Palette; label: MessageKey }[] = [
  { icon: Palette, label: "identity.carries.appearance" },
  { icon: SquareTerminal, label: "identity.carries.terminal" },
  { icon: Gauge, label: "identity.carries.autonomy" },
  { icon: Bell, label: "identity.carries.notifications" },
];

/**
 * Sign in, or make an account (M20).
 *
 * ## This screen is not a wall
 *
 * Read `identity`'s module note before changing anything here. The product is
 * local-first and half of it runs with nobody present, so an account is
 * additive: **Continue without an account** is a real destination, drawn with
 * enough weight to be found, and choosing it costs nothing anywhere else in the
 * product. A login screen that has to be got past would be a different product.
 *
 * ## Why it looks like this
 *
 * Two columns. The left says *what an account is for* — the four preferences it
 * carries — because "why would I sign in to a local tool" is the only question
 * anybody actually has here, and a screen that answers it is worth more than one
 * that decorates. The right is the form, and nothing else.
 *
 * The colour discipline (§6) is the constraint that makes the screen: amber
 * means agent work, so it cannot be sprayed around a login. The ambient light
 * behind the stage is **neutral**, at an alpha low enough that sampling it finds
 * the base colour; the only amber on screen is the brand mark and the one
 * primary action. What carries the eye instead is motion and depth.
 *
 * Every animation is authored under `prefers-reduced-motion: no-preference`, so
 * the reduced-motion pass is the screen with nothing moving rather than the
 * screen with its layout subtly different.
 */
export function Auth() {
  const t = useT();
  const report = useIdentity((state) => state.report);
  const signIn = useIdentity((state) => state.signIn);
  const signUp = useIdentity((state) => state.signUp);
  const googleSignIn = useIdentity((state) => state.googleSignIn);
  const skip = useIdentity((state) => state.skip);
  const closeAuth = useIdentity((state) => state.closeAuth);

  const known = report?.known ?? [];
  // Somebody who has an account here is far more likely to be coming back than
  // making a second one, so the mode this opens in is a fact about the machine
  // rather than a default.
  const [mode, setMode] = useState<Mode>(known.length > 0 ? "signIn" : "signUp");
  const [name, setName] = useState("");
  const [email, setEmail] = useState(known[0]?.email ?? "");
  const [password, setPassword] = useState("");
  const [reveal, setReveal] = useState(false);
  const [capsLock, setCapsLock] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ key: MessageKey; values?: Record<string, string | number> } | null>(null);
  const [googleWhy, setGoogleWhy] = useState(false);
  const [lockedFor, setLockedFor] = useState(0);

  const passwordRef = useRef<HTMLInputElement | null>(null);
  const emailRef = useRef<HTMLInputElement | null>(null);
  const nameRef = useRef<HTMLInputElement | null>(null);

  // The screen can be reached deliberately from Settings after the one-time
  // offer is over. Then it needs a way out that is not signing in.
  const dismissible = report?.prompted === true;

  useEffect(() => {
    // Land the cursor where the person's next keystroke belongs: on the
    // password when the address is already filled in from a known account.
    const target = mode === "signUp" ? nameRef.current : email ? passwordRef.current : emailRef.current;
    target?.focus();
    // Only on a mode change — refocusing on every keystroke would fight the
    // person typing.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode]);

  useEffect(() => {
    if (!dismissible) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeAuth();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [dismissible, closeAuth]);

  /** The lockout counts down on screen rather than saying "try again later". */
  useEffect(() => {
    if (lockedFor <= 0) return;
    const timer = window.setInterval(() => {
      setLockedFor((seconds) => {
        const next = seconds - 1;
        if (next <= 0) {
          setError(null);
          return 0;
        }
        setError({ key: "identity.error.lockedOut", values: { seconds: next } });
        return next;
      });
    }, 1000);
    return () => window.clearInterval(timer);
  }, [lockedFor]);

  const suggestion = useMemo(() => domainSuggestion(email.trim()), [email]);
  const strength = strengthOf(password);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (busy || lockedFor > 0) return;
    setBusy(true);
    setError(null);
    try {
      if (mode === "signUp") {
        const outcome = await signUp(name, email, password);
        switch (outcome.status) {
          case "ok":
            return;
          case "nameRequired":
            setError({ key: "identity.error.nameRequired" });
            nameRef.current?.focus();
            return;
          case "invalidEmail":
            setError({ key: "identity.error.invalidEmail" });
            emailRef.current?.focus();
            return;
          case "emailTaken":
            setError({ key: "identity.error.emailTaken" });
            emailRef.current?.focus();
            return;
          case "passwordTooShort":
            setError({
              key: "identity.error.passwordTooShort",
              values: { min: outcome.minimum },
            });
            passwordRef.current?.focus();
            return;
        }
      }

      const outcome = await signIn(email, password);
      switch (outcome.status) {
        case "ok":
          return;
        case "unknownEmail":
          setError({ key: "identity.error.unknownEmail" });
          emailRef.current?.focus();
          return;
        case "wrongPassword":
          setError({
            key: "identity.error.wrongPassword",
            values: { count: outcome.attemptsLeft },
          });
          setPassword("");
          passwordRef.current?.focus();
          return;
        case "lockedOut": {
          const seconds = Math.max(1, Math.ceil(outcome.retryInMs / 1000));
          setLockedFor(seconds);
          setError({ key: "identity.error.lockedOut", values: { seconds } });
          setPassword("");
          return;
        }
        case "noPassword":
          setError({ key: "identity.error.noPassword" });
          return;
      }
    } catch {
      setError({ key: "identity.error.generic" });
    } finally {
      setBusy(false);
    }
  };

  const pickKnown = (account: KnownAccount) => {
    setEmail(account.email);
    setMode("signIn");
    setError(null);
    // A microtask, so the focus lands after the mode change has re-rendered.
    window.setTimeout(() => passwordRef.current?.focus(), 0);
  };

  const now = Date.now();

  const enterWithGoogle = async () => {
    if (!report?.googleAvailable || busy) return;
    setBusy(true);
    setError(null);
    setGoogleWhy(false);
    try {
      await googleSignIn();
    } catch {
      setError({ key: "identity.google.failed" });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="auth">
      {/* Ambient light. Three neutral washes on their own slow paths, plus a
          fine grid that gives the drift something to move against — without it
          the motion is invisible, which is the failure mode of every "subtle"
          background. `aria-hidden` because it says nothing. */}
      <div className="auth__ambient" aria-hidden="true">
        <span className="auth__wash auth__wash--a" />
        <span className="auth__wash auth__wash--b" />
        <span className="auth__wash auth__wash--c" />
        <span className="auth__grid" />
      </div>

      {dismissible && (
        <button type="button" className="auth__back" onClick={closeAuth}>
          <ArrowLeft size={14} strokeWidth={1.9} aria-hidden="true" />
          {t("common.cancel")}
        </button>
      )}

      <div className="auth__layout">
        {/* ---- The stage: what an account is actually for ------------------ */}
        <section className="auth__stage">
          <header className="auth__brand">
            <Logo size={30} boxed />
            <div>
              <p className="auth__wordmark">{t("app.name")}</p>
              <p className="auth__tagline">{t("app.tagline")}</p>
            </div>
          </header>

          <h2 className="auth__carries-title">{t("identity.carries.title")}</h2>
          <ul className="auth__carries">
            {CARRIES.map((item, index) => (
              <li
                key={item.label}
                className="auth__carry"
                // The stagger is a delay per item rather than a keyframe per
                // item: four rules and one variable instead of four animations.
                style={{ "--index": index } as React.CSSProperties}
              >
                <span className="auth__carry-icon">
                  <item.icon size={14} strokeWidth={1.7} aria-hidden="true" />
                </span>
                {t(item.label)}
              </li>
            ))}
          </ul>

          <p className="auth__local">{t("identity.carries.local")}</p>
        </section>

        {/* ---- The card: the form, and nothing else ------------------------ */}
        <section className="auth__card">
          {/* Keyed on the mode so the heading genuinely re-enters rather than
              having its text swapped underneath a static element. */}
          <header className="auth__head" key={mode}>
            <h1 className="auth__title">{t(`identity.${mode}.title`)}</h1>
            <p className="auth__subtitle">{t(`identity.${mode}.subtitle`)}</p>
          </header>

          <div className="auth__google-slot">
            <button
              type="button"
              className="auth__google"
              aria-disabled={!report?.googleAvailable}
              disabled={busy}
              onClick={() => report?.googleAvailable ? void enterWithGoogle() : setGoogleWhy((open) => !open)}
            >
              <GoogleMark />
              {busy ? t("identity.action.working") : t("identity.action.google")}
              {!report?.googleAvailable && <span className="auth__soon">{t("identity.google.soon")}</span>}
            </button>
            <div className="auth__why" data-open={googleWhy || undefined}>
              <div className="auth__why-inner">
                <p className="auth__why-body">{t("identity.google.unavailable")}</p>
              </div>
            </div>
          </div>

          <div className="auth__or">
            <span>{t("identity.or")}</span>
          </div>

          {known.length > 0 && mode === "signIn" && (
            <div className="auth__known">
              <p className="auth__known-title">{t("identity.known.title")}</p>
              <div className="auth__known-list">
                {known.map((account) => (
                  <button
                    key={account.id}
                    type="button"
                    className="auth__known-item"
                    data-active={account.email === email.trim().toLowerCase() || undefined}
                    onClick={() => pickKnown(account)}
                  >
                    <span className="auth__avatar" aria-hidden="true">
                      {initials(account.displayName)}
                    </span>
                    <span className="auth__known-text">
                      <span className="auth__known-name">{account.displayName}</span>
                      <span className="auth__known-email">
                        {account.lastSignedInAt
                          ? t("identity.known.lastSeen", {
                              when: relative(account.lastSignedInAt, now, t),
                            })
                          : t("identity.known.never")}
                      </span>
                    </span>
                  </button>
                ))}
              </div>
            </div>
          )}

          <form className="auth__form" onSubmit={(event) => void submit(event)} noValidate>
            {/* The name field opens and closes rather than appearing: a card
                that jumps by one field height on every mode switch is the
                single most jarring thing this screen could do. `grid-template-
                rows: 0fr → 1fr` animates a height nobody had to measure. */}
            <div className="auth__collapse" data-open={mode === "signUp" || undefined}>
              <div className="auth__collapse-inner">
                <Field label="identity.field.name">
                  <input
                    ref={nameRef}
                    className="auth__input"
                    type="text"
                    value={name}
                    autoComplete="name"
                    // Never submit a field that is not on screen.
                    disabled={mode !== "signUp"}
                    placeholder={t("identity.field.namePlaceholder")}
                    onChange={(event) => setName(event.target.value)}
                  />
                </Field>
              </div>
            </div>

            <Field label="identity.field.email">
              <input
                ref={emailRef}
                className="auth__input"
                type="email"
                inputMode="email"
                value={email}
                autoComplete="email"
                spellCheck={false}
                placeholder={t("identity.field.emailPlaceholder")}
                onChange={(event) => {
                  setEmail(event.target.value);
                  setError(null);
                }}
              />
              {suggestion && (
                <button
                  type="button"
                  className="auth__suggest"
                  onClick={() => setEmail(suggestion)}
                >
                  {t("identity.email.didYouMean", { suggestion })}
                </button>
              )}
            </Field>

            <Field label="identity.field.password">
              <div className="auth__password">
                <input
                  ref={passwordRef}
                  className="auth__input"
                  type={reveal ? "text" : "password"}
                  value={password}
                  autoComplete={mode === "signUp" ? "new-password" : "current-password"}
                  placeholder={t("identity.field.passwordPlaceholder", { min: MIN_PASSWORD })}
                  onChange={(event) => {
                    setPassword(event.target.value);
                    setError(null);
                  }}
                  // Both events: `keydown` catches the state while typing, and
                  // `keyup` catches the moment Caps Lock itself is released.
                  onKeyDown={(event) => setCapsLock(event.getModifierState("CapsLock"))}
                  onKeyUp={(event) => setCapsLock(event.getModifierState("CapsLock"))}
                  onBlur={() => setCapsLock(false)}
                />
                <button
                  type="button"
                  className="auth__reveal"
                  onClick={() => setReveal((shown) => !shown)}
                  aria-label={t(reveal ? "identity.password.hide" : "identity.password.show")}
                  title={t(reveal ? "identity.password.hide" : "identity.password.show")}
                  // Never in the tab order between the password and the submit
                  // button: somebody typing a password and pressing Tab-Enter
                  // must reach the action, not the eye.
                  tabIndex={-1}
                >
                  {reveal ? (
                    <EyeOff size={14} strokeWidth={1.8} aria-hidden="true" />
                  ) : (
                    <Eye size={14} strokeWidth={1.8} aria-hidden="true" />
                  )}
                </button>
              </div>

              {/* Advice, never a gate — see `strengthOf`. Only while making an
                  account: telling somebody their existing password is weak at
                  the moment they are trying to get in is a lecture, not help. */}
              <div className="auth__meter" data-open={(mode === "signUp" && password.length > 0) || undefined}>
                <div className="auth__meter-inner">
                  <div className="auth__meter-row">
                    <div className="auth__bars" data-score={strength}>
                      {[1, 2, 3, 4].map((step) => (
                        <span key={step} data-lit={strength >= step || undefined} />
                      ))}
                    </div>
                    <span className="auth__meter-label">
                      {strength !== 0 && t(STRENGTH_LABEL[strength])}
                    </span>
                  </div>
                </div>
              </div>

              {/* CapsLock is why a password "that is definitely right" is
                  rejected, and the browser is the only thing that knows. */}
              <div className="auth__caps" data-open={capsLock || undefined}>
                <div className="auth__caps-inner">
                  <p className="auth__caps-body">{t("identity.password.capsLock")}</p>
                </div>
              </div>
            </Field>

            {/* Three elements rather than two, and the third is the reason the
                closed state actually takes no space: the outer is the grid
                that animates, the middle is the clipped box that owns the
                spacing, and only the innermost carries the colour. Spacing as
                a *margin* on the middle one escaped the clip and left 50px of
                empty card below the password field with nothing in it — see
                the note in Auth.css. */}
            <div className="auth__error" data-open={error !== null || undefined} role="alert">
              <div className="auth__error-inner">
                <p className="auth__error-body">{error && t(error.key, error.values)}</p>
              </div>
            </div>

            <button
              type="submit"
              className="auth__submit"
              disabled={busy || lockedFor > 0}
              data-busy={busy || undefined}
            >
              <span>{busy ? t("identity.action.working") : t(`identity.action.${mode}`)}</span>
              <ArrowRight size={14} strokeWidth={2} aria-hidden="true" />
            </button>
          </form>

          <p className="auth__switch">
            {t(mode === "signIn" ? "identity.toggle.noAccount" : "identity.toggle.hasAccount")}{" "}
            <button
              type="button"
              className="auth__link"
              onClick={() => {
                setMode(mode === "signIn" ? "signUp" : "signIn");
                setError(null);
              }}
            >
              {t(mode === "signIn" ? "identity.action.signUp" : "identity.action.signIn")}
            </button>
          </p>

          {!dismissible && (
            <div className="auth__skip">
              <button type="button" className="auth__skip-action" onClick={() => void skip()}>
                {t("identity.action.skip")}
              </button>
              <p className="auth__skip-hint">{t("identity.skip.hint")}</p>
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: MessageKey; children: React.ReactNode }) {
  const t = useT();
  return (
    <label className="auth__field">
      <span className="auth__label">{t(label)}</span>
      {children}
    </label>
  );
}

/**
 * One or two letters, from the name somebody actually typed.
 *
 * Not a generated identicon: a monogram is legible at 26px and says whose
 * account it is, which is the only job it has here.
 */
export function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return "?";
  if (words.length === 1) return words[0].slice(0, 2).toUpperCase();
  return (words[0][0] + words[words.length - 1][0]).toUpperCase();
}
