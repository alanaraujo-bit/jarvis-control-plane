import { useEffect, useState } from "react";
import { Bell, Compass, Network, UserRound } from "lucide-react";
import { useI18n, useT } from "../../app/i18n";
import { useIdentity } from "../identity/useIdentity";
import { Activity } from "./Activity";
import { social, useCommunity, type Person, type View } from "./api";
import { Feed } from "./Feed";
import { Avatar, Empty, formatCount } from "./parts";
import { People, PersonRow } from "./People";
import { ProfileForm } from "./ProfileForm";
import { ProfileView } from "./ProfileView";
import { Thread } from "./Thread";
import "./Community.css";
import "./CommunityPosts.css";

/** Your card and people to follow, beside the feed. */
function Side() {
  const t = useT();
  const { locale } = useI18n();
  const me = useCommunity((s) => s.me);
  const go = useCommunity((s) => s.go);
  const [people, setPeople] = useState<Person[] | null>(null);
  useEffect(() => {
    void social<{ people: Person[] }>("GET", "suggestions").then((r) => setPeople(r.people.slice(0, 5))).catch(() => setPeople([]));
  }, []);
  const profile = me?.profile;
  if (!profile || !me.counts) return null;
  return (
    <aside className="cm-side">
      <button type="button" className="cm-card cm-me" onClick={() => go({ kind: "profile", handle: profile.handle })}>
        <Avatar name={profile.displayName} size={40} />
        <span className="cm-me__text">
          <b>{profile.displayName}</b>
          <span className="cm-muted">@{profile.handle}</span>
        </span>
        <span className="cm-me__counts">
          <span><b>{formatCount(me.counts.followers, locale)}</b> {t("community.profile.followers")}</span>
          <span><b>{formatCount(me.counts.following, locale)}</b> {t("community.profile.followingCount")}</span>
        </span>
      </button>
      {people && people.length > 0 && (
        <section className="cm-card">
          <h3 className="cm-section-title">{t("community.people.suggested")}</h3>
          <ul className="cm-people">{people.map((p) => <li key={p.handle}><PersonRow person={p} compact /></li>)}</ul>
        </section>
      )}
    </aside>
  );
}

const NAV: { kind: View["kind"]; icon: typeof Network; label: Parameters<ReturnType<typeof useT>>[0] }[] = [
  { kind: "feed", icon: Network, label: "community.tab.feed" },
  { kind: "people", icon: Compass, label: "community.tab.people" },
  { kind: "profile", icon: UserRound, label: "community.tab.profile" },
  { kind: "activity", icon: Bell, label: "community.tab.activity" },
];

/**
 * The J.A.R.V.I.S. community (M28): people who build with agents, what they
 * build, and the prompts that work for them. It needs a J.A.R.V.I.S. account;
 * nothing else in the product does.
 */
export function Community() {
  const t = useT();
  const status = useCommunity((s) => s.status);
  const me = useCommunity((s) => s.me);
  const view = useCommunity((s) => s.view);
  const go = useCommunity((s) => s.go);
  const load = useCommunity((s) => s.load);
  const signedIn = useIdentity((s) => s.report?.account ?? null);
  const openAuth = useIdentity((s) => s.openAuth);

  useEffect(() => {
    void load();
    const timer = window.setInterval(() => void load(), 60_000);
    return () => window.clearInterval(timer);
  }, [load, signedIn?.id]);

  if (status === "loading") return <main className="cm"><div className="cm-skeleton cm-skeleton--tall" aria-busy="true" /></main>;
  if (status === "signedOut" || !signedIn) {
    return (
      <main className="cm cm--center">
        <section className="cm-card cm-welcome">
          <Network size={22} aria-hidden="true" />
          <h1>{t("community.signedOut.title")}</h1>
          <p className="cm-hint">{t("community.signedOut.body")}</p>
          <button type="button" className="cm-button cm-button--primary" onClick={openAuth}>{t("community.signedOut.action")}</button>
        </section>
      </main>
    );
  }
  if (status === "error" && !me) {
    return <main className="cm cm--center"><Empty>{t("community.error.unreachable")}</Empty></main>;
  }
  if (!me?.profile) {
    return (
      <main className="cm cm--center">
        <div className="cm-create">
          <ProfileForm existing={null} />
        </div>
      </main>
    );
  }

  const active = view.kind === "thread" || view.kind === "edit" ? (view.kind === "edit" ? "profile" : "feed") : view.kind === "profile" && view.handle !== me.profile.handle ? "people" : view.kind;
  return (
    <main className="cm">
      <div className="cm__layout">
        <nav className="cm-nav" aria-label={t("community.title")}>
          <h1 className="cm-nav__title">{t("community.title")}</h1>
          {NAV.map((item) => (
            <button
              key={item.kind}
              type="button"
              className="cm-nav__item"
              data-active={active === item.kind || undefined}
              aria-current={active === item.kind ? "page" : undefined}
              onClick={() => go(item.kind === "profile" ? { kind: "profile", handle: me.profile!.handle } : ({ kind: item.kind } as View))}
            >
              <item.icon size={15} strokeWidth={1.8} aria-hidden="true" />
              <span>{t(item.label)}</span>
              {item.kind === "activity" && me.unread > 0 && <span className="cm-nav__badge">{me.unread}</span>}
            </button>
          ))}
        </nav>
        <div className="cm-main">
          {view.kind === "feed" && <Feed />}
          {view.kind === "people" && <People />}
          {view.kind === "activity" && <Activity />}
          {view.kind === "profile" && <ProfileView key={view.handle} handle={view.handle} />}
          {view.kind === "thread" && <Thread key={view.id} id={view.id} />}
          {view.kind === "edit" && <ProfileForm existing={me.profile} />}
        </div>
        <Side />
      </div>
    </main>
  );
}
