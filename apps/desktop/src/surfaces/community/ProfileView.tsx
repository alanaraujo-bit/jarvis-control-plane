import { useCallback, useEffect, useState } from "react";
import { Github, Globe, Link2, MapPin, MoreHorizontal } from "lucide-react";
import { useI18n, useT } from "../../app/i18n";
import { Popover } from "../../design/Popover";
import { social, useCommunity, WEB_ORIGIN, type Person, type ProfileView as View } from "./api";
import { usePostList, PostList } from "./Feed";
import { Avatar, Empty, formatCount, openExternal, RichText, useErrorText } from "./parts";
import { StatsPanel } from "./Stats";

type Tab = "posts" | "prompts" | "replies" | "followers" | "following";

function PeopleList({ handle, which }: { handle: string; which: "followers" | "following" }) {
  const t = useT();
  const go = useCommunity((s) => s.go);
  const [people, setPeople] = useState<Person[] | null>(null);
  useEffect(() => {
    void social<{ people: Person[] }>("GET", `profiles/${encodeURIComponent(handle)}/${which}`).then((r) => setPeople(r.people)).catch(() => setPeople([]));
  }, [handle, which]);
  if (!people) return <div className="cm-skeleton" aria-busy="true" />;
  if (people.length === 0) return <Empty>{t("community.profile.empty")}</Empty>;
  return (
    <ul className="cm-people">
      {people.map((person) => (
        <li key={person.handle}>
          <button type="button" className="cm-person" onClick={() => go({ kind: "profile", handle: person.handle })}>
            <Avatar name={person.displayName} size={32} />
            <span className="cm-person__text">
              <b>{person.displayName}</b>
              <span className="cm-muted">@{person.handle}{person.building ? ` · ${person.building}` : ""}</span>
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}

/** Someone's public profile: who they are, how they build, what they posted. */
export function ProfileView({ handle }: { handle: string }) {
  const t = useT();
  const { locale } = useI18n();
  const errorText = useErrorText();
  const go = useCommunity((s) => s.go);
  const loadMe = useCommunity((s) => s.load);
  const [view, setView] = useState<View | null>(null);
  const [missing, setMissing] = useState(false);
  const [tab, setTab] = useState<Tab>("posts");
  const [said, setSaid] = useState<string | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const [menuAnchor, setMenuAnchor] = useState<HTMLButtonElement | null>(null);

  const load = useCallback(async () => {
    try {
      setView(await social<View>("GET", `profiles/${encodeURIComponent(handle)}`));
      setMissing(false);
    } catch {
      setMissing(true);
    }
  }, [handle]);
  useEffect(() => {
    let alive = true;
    social<View>("GET", `profiles/${encodeURIComponent(handle)}`)
      .then((next) => alive && setView(next))
      .catch(() => alive && setMissing(true));
    return () => {
      alive = false;
    };
  }, [handle]);

  const path = useCallback(
    (before: string | null) =>
      tab === "followers" || tab === "following" ? null : `profiles/${encodeURIComponent(handle)}/posts?tab=${tab}${before ? `&before=${encodeURIComponent(before)}` : ""}`,
    [handle, tab],
  );
  const list = usePostList(path);
  const { reload } = list;
  useEffect(() => {
    void reload();
  }, [path]); // eslint-disable-line react-hooks/exhaustive-deps

  if (missing) return <Empty>{t("community.profile.notFound")}</Empty>;
  if (!view) return <div className="cm-skeleton cm-skeleton--tall" aria-busy="true" />;
  const { profile, counts, relation, stats, presence } = view;

  const flash = (text: string) => {
    setSaid(text);
    window.setTimeout(() => setSaid(null), 2200);
  };
  const act = async (method: "POST" | "DELETE", what: "follows" | "blocks") => {
    setMenuOpen(false);
    try {
      await social(method, `${what}/${encodeURIComponent(profile.handle)}`);
      await load();
      void loadMe();
    } catch (cause) {
      flash(errorText(cause));
    }
  };
  const link = `${WEB_ORIGIN}/u/${profile.handle}`;

  return (
    <div className="cm-profile">
      <section className="cm-card cm-profile__head">
        <div className="cm-profile__top">
          <Avatar name={profile.displayName} size={64} working={presence?.working} />
          <div className="cm-profile__names">
            <h2 className="cm-profile__name">{profile.displayName}</h2>
            <span className="cm-muted">@{profile.handle}{relation.followsYou && <span className="cm-badge cm-profile__follows">{t("community.profile.followsYou")}</span>}</span>
            {presence?.working && <span className="cm-working">{t("community.profile.working", { count: presence.activeSessions })}</span>}
          </div>
          <div className="cm-profile__actions">
            {relation.self ? (
              <button type="button" className="cm-button" onClick={() => go({ kind: "edit" })}>{t("community.profile.edit")}</button>
            ) : relation.blocked ? (
              <button type="button" className="cm-button" onClick={() => void act("DELETE", "blocks")}>{t("community.profile.unblock")}</button>
            ) : (
              <button type="button" className={relation.following ? "cm-button cm-button--following" : "cm-button cm-button--primary"} onClick={() => void act(relation.following ? "DELETE" : "POST", "follows")}>
                <span className="cm-follow__idle">{relation.following ? t("community.profile.following") : t("community.profile.follow")}</span>
                {relation.following && <span className="cm-follow__hover">{t("community.profile.unfollow")}</span>}
              </button>
            )}
            <button ref={setMenuAnchor} type="button" className="cm-icon-button" aria-label={t("community.post.more")} aria-expanded={menuOpen} onClick={() => setMenuOpen((v) => !v)}>
              <MoreHorizontal size={15} aria-hidden="true" />
            </button>
            <Popover anchor={menuAnchor} open={menuOpen} onClose={() => setMenuOpen(false)} align="end">
              <button type="button" className="popover__item" onClick={() => { setMenuOpen(false); void navigator.clipboard.writeText(link).catch(() => undefined); flash(t("community.post.linkCopied")); }}>
                <Link2 size={13} aria-hidden="true" /> {t("community.profile.openWeb")}
              </button>
              {!relation.self && !relation.blocked && (
                <button type="button" className="popover__item cm-danger" onClick={() => { if (window.confirm(t("community.profile.blockConfirm", { handle: profile.handle }))) void act("POST", "blocks"); else setMenuOpen(false); }}>
                  {t("community.profile.block", { handle: profile.handle })}
                </button>
              )}
            </Popover>
          </div>
        </div>
        {relation.blocked && <p className="cm-note">{t("community.profile.blocked")}</p>}
        {profile.building && <p className="cm-profile__building">{profile.building}</p>}
        {profile.bio && <p className="cm-profile__bio"><RichText text={profile.bio} /></p>}
        <div className="cm-profile__meta">
          {profile.location && <span><MapPin size={12} aria-hidden="true" />{profile.location}</span>}
          {profile.website && (
            <button type="button" className="cm-link" onClick={() => openExternal(profile.website)}>
              <Globe size={12} aria-hidden="true" />{profile.website.replace(/^https?:\/\//, "")}
            </button>
          )}
          {profile.github && (
            <button type="button" className="cm-link" onClick={() => openExternal(`https://github.com/${profile.github}`)}>
              <Github size={12} aria-hidden="true" />{profile.github}
            </button>
          )}
          <span>{t("community.profile.joined", { date: new Date(profile.createdAt).toLocaleDateString(locale, { month: "long", year: "numeric" }) })}</span>
        </div>
        {profile.stack.length > 0 && (
          <div className="cm-tags">{profile.stack.map((item) => <span key={item} className="cm-chip">{item}</span>)}</div>
        )}
        <div className="cm-profile__counts">
          <span><b>{formatCount(counts.posts, locale)}</b> {t("community.profile.posts")}</span>
          <button type="button" className="cm-link" onClick={() => setTab("followers")}><b>{formatCount(counts.followers, locale)}</b> {t("community.profile.followers")}</button>
          <button type="button" className="cm-link" onClick={() => setTab("following")}><b>{formatCount(counts.following, locale)}</b> {t("community.profile.followingCount")}</button>
          {said && <span className="cm-said" role="status">{said}</span>}
        </div>
      </section>

      {stats ? (
        <StatsPanel stats={stats} self={relation.self} />
      ) : relation.self ? (
        <p className="cm-note">{profile.statsVisibility === "private" ? t("community.stats.private") : t("community.stats.waiting")}</p>
      ) : null}

      <div className="cm-tabs" role="tablist">
        {(["posts", "prompts", "replies", "followers", "following"] as const).map((value) => (
          <button key={value} type="button" role="tab" aria-selected={tab === value} data-active={tab === value || undefined} onClick={() => setTab(value)}>
            {t(`community.profile.tab${value[0].toUpperCase()}${value.slice(1)}` as Parameters<typeof t>[0])}
          </button>
        ))}
      </div>
      {tab === "followers" || tab === "following" ? (
        <PeopleList handle={profile.handle} which={tab} />
      ) : (
        <PostList list={list} empty={t("community.profile.empty")} />
      )}
    </div>
  );
}
