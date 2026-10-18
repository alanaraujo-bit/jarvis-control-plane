import { useEffect, useState } from "react";
import { Check, X } from "lucide-react";
import { useT } from "../../app/i18n";
import { invoke } from "../../app/platform";
import { social, useCommunity, type Me, type Profile, type Visibility } from "./api";
import { useErrorText, VisibilityPicker } from "./parts";

/**
 * Create or edit a community profile. Creating is the moment to choose what
 * the profile shows: both visibilities are on screen, with what each one
 * shares, before anything is published.
 */
export function ProfileForm({ existing }: { existing: Profile | null }) {
  const t = useT();
  const errorText = useErrorText();
  const me = useCommunity((s) => s.me);
  const setMe = useCommunity((s) => s.setMe);
  const go = useCommunity((s) => s.go);
  const [handle, setHandle] = useState(existing?.handle ?? "");
  const [displayName, setDisplayName] = useState(existing?.displayName ?? me?.suggestedName ?? "");
  const [building, setBuilding] = useState(existing?.building ?? "");
  const [bio, setBio] = useState(existing?.bio ?? "");
  const [location, setLocation] = useState(existing?.location ?? "");
  const [website, setWebsite] = useState(existing?.website ?? "");
  const [github, setGithub] = useState(existing?.github ?? "");
  const [stack, setStack] = useState((existing?.stack ?? []).join(", "));
  const [stats, setStats] = useState<Visibility>(existing?.statsVisibility ?? "public");
  const [presence, setPresence] = useState<Visibility>(existing?.presenceVisibility ?? "followers");
  // The answer for one handle; shown only while that handle is still typed.
  const [checked, setChecked] = useState<{ handle: string; available: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const wanted = handle.trim().toLowerCase();
  const checkable = /^[a-z0-9_]{3,24}$/.test(wanted) && wanted !== existing?.handle;
  const available = checkable && checked?.handle === wanted ? checked.available : null;
  useEffect(() => {
    if (!checkable) return;
    const timer = window.setTimeout(() => {
      void social<{ available: boolean }>("GET", `handles/${wanted}`).then((r) => setChecked({ handle: wanted, available: r.available })).catch(() => undefined);
    }, 300);
    return () => window.clearTimeout(timer);
  }, [wanted, checkable]);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const next = await social<Me>("PUT", "profile", {
        handle: handle.trim().toLowerCase(),
        displayName,
        building,
        bio,
        location,
        website,
        github,
        stack: stack.split(",").map((s) => s.trim()).filter(Boolean),
        statsVisibility: stats,
        presenceVisibility: presence,
      });
      setMe(next);
      // Shown at once rather than at the next 30-minute tick.
      if (stats !== "private") void invoke("social_publish_stats").catch(() => undefined);
      if (existing) {
        setSaved(true);
        window.setTimeout(() => setSaved(false), 2200);
      } else {
        go({ kind: "profile", handle: next.profile?.handle ?? handle });
      }
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setBusy(false);
    }
  };
  const remove = async () => {
    if (!window.confirm(t("community.edit.deleteConfirm"))) return;
    try {
      await social("DELETE", "profile");
      setMe({ profile: null, counts: null, unread: 0, suggestedName: displayName });
      go({ kind: "feed" });
    } catch (cause) {
      setError(errorText(cause));
    }
  };

  const field = (label: string, value: string, set: (v: string) => void, hint?: string, max?: number) => (
    <label className="cm-field">
      <span>{label}</span>
      <input className="cm-input" value={value} maxLength={max} onChange={(e) => set(e.target.value)} />
      {hint && <small className="cm-hint">{hint}</small>}
    </label>
  );

  return (
    <form className="cm-card cm-form" onSubmit={(e) => { e.preventDefault(); void submit(); }}>
      <header>
        <h2 className="cm-form__title">{existing ? t("community.edit.title") : t("community.create.title")}</h2>
        {!existing && <p className="cm-hint">{t("community.create.body")}</p>}
      </header>
      <div className="cm-form__grid">
        <label className="cm-field">
          <span>{t("community.field.handle")}</span>
          <span className="cm-handle">
            <b>@</b>
            <input className="cm-input" value={handle} maxLength={24} autoComplete="off" spellCheck={false} onChange={(e) => setHandle(e.target.value.toLowerCase())} />
            {available === true && <span className="cm-handle__ok"><Check size={12} aria-hidden="true" />{t("community.field.handleFree")}</span>}
            {available === false && <span className="cm-handle__taken"><X size={12} aria-hidden="true" />{t("community.field.handleTaken")}</span>}
          </span>
          <small className="cm-hint">{t("community.field.handleHint")}</small>
        </label>
        {field(t("community.field.name"), displayName, setDisplayName, undefined, 60)}
        <div className="cm-form__wide">{field(t("community.field.building"), building, setBuilding, t("community.field.buildingHint"), 140)}</div>
        <label className="cm-field cm-form__wide">
          <span>{t("community.field.bio")}</span>
          <textarea className="cm-input" rows={3} maxLength={280} value={bio} onChange={(e) => setBio(e.target.value)} />
        </label>
        {field(t("community.field.location"), location, setLocation, undefined, 60)}
        {field(t("community.field.website"), website, setWebsite, undefined, 200)}
        {field(t("community.field.github"), github, setGithub, undefined, 39)}
        {field(t("community.field.stack"), stack, setStack, t("community.field.stackHint"))}
      </div>
      <section className="cm-form__privacy">
        <h3 className="cm-section-title">{t("community.privacy.title")}</h3>
        <VisibilityPicker label={t("community.privacy.stats")} hint={t("community.privacy.statsHint")} value={stats} onChange={setStats} />
        <VisibilityPicker label={t("community.privacy.presence")} hint={t("community.privacy.presenceHint")} value={presence} onChange={setPresence} />
      </section>
      {error && <p className="cm-error" role="alert">{error}</p>}
      <footer className="cm-form__foot">
        {existing && <button type="button" className="cm-button cm-button--ghost" onClick={() => go({ kind: "profile", handle: existing.handle })}>{t("community.edit.cancel")}</button>}
        {saved && <span className="cm-said" role="status">{t("community.edit.saved")}</span>}
        <button type="submit" className="cm-button cm-button--primary" disabled={busy || available === false}>
          {existing ? t("community.edit.save") : t("community.create.action")}
        </button>
      </footer>
      {existing && (
        <section className="cm-form__danger">
          <h3 className="cm-section-title">{t("community.edit.delete")}</h3>
          <p className="cm-hint">{t("community.edit.deleteBody")}</p>
          <button type="button" className="cm-button cm-danger" onClick={() => void remove()}>{t("community.edit.delete")}</button>
        </section>
      )}
    </form>
  );
}
