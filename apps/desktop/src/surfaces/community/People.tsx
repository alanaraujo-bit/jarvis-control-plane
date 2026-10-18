import { useEffect, useState } from "react";
import { Search } from "lucide-react";
import { useI18n, useT } from "../../app/i18n";
import { social, useCommunity, type Person, type Post } from "./api";
import { Avatar, Empty, formatCount } from "./parts";
import { PostCard } from "./PostCard";

export function PersonRow({ person, compact = false }: { person: Person; compact?: boolean }) {
  const t = useT();
  const { locale } = useI18n();
  const go = useCommunity((s) => s.go);
  return (
    <button type="button" className="cm-person" data-compact={compact || undefined} onClick={() => go({ kind: "profile", handle: person.handle })}>
      <Avatar name={person.displayName} size={compact ? 28 : 34} />
      <span className="cm-person__text">
        <b>{person.displayName}</b>
        <span className="cm-muted">
          @{person.handle}
          {person.followers !== undefined && !compact ? ` · ${t("community.people.followers", { count: formatCount(person.followers, locale) })}` : ""}
        </span>
        {person.building && !compact && <span className="cm-person__building">{person.building}</span>}
      </span>
    </button>
  );
}

/** People who build now, and search across people, stacks and posts. */
export function People() {
  const t = useT();
  const [query, setQuery] = useState("");
  const [suggested, setSuggested] = useState<Person[] | null>(null);
  const [found, setFound] = useState<{ people: Person[]; posts: Post[] } | null>(null);

  useEffect(() => {
    void social<{ people: Person[] }>("GET", "suggestions").then((r) => setSuggested(r.people)).catch(() => setSuggested([]));
  }, []);
  useEffect(() => {
    const q = query.trim();
    if (!q) return;
    const timer = window.setTimeout(() => {
      void social<{ people: Person[]; posts: Post[] }>("GET", `search?q=${encodeURIComponent(q)}`).then(setFound).catch(() => setFound({ people: [], posts: [] }));
    }, 250);
    return () => window.clearTimeout(timer);
  }, [query]);

  return (
    <div className="cm-people-page">
      <label className="cm-search">
        <Search size={14} aria-hidden="true" />
        <input value={query} placeholder={t("community.people.search")} aria-label={t("community.people.search")} onChange={(e) => setQuery(e.target.value)} autoFocus />
      </label>
      {found && query.trim() ? (
        <>
          <h3 className="cm-section-title">{t("community.people.results")}</h3>
          {found.people.length === 0 ? <Empty>{t("community.people.none")}</Empty> : (
            <ul className="cm-people">{found.people.map((p) => <li key={p.handle}><PersonRow person={p} /></li>)}</ul>
          )}
          {found.posts.length > 0 && (
            <>
              <h3 className="cm-section-title">{t("community.people.postResults")}</h3>
              <div className="cm-list">
                {found.posts.map((post) => (
                  <PostCard key={post.id} post={post}
                    onChange={(next) => setFound((f) => f && { ...f, posts: f.posts.map((p) => (p.id === next.id ? next : p)) })}
                    onRemove={(id) => setFound((f) => f && { ...f, posts: f.posts.filter((p) => p.id !== id) })} />
                ))}
              </div>
            </>
          )}
        </>
      ) : (
        <>
          <h3 className="cm-section-title">{t("community.people.suggested")}</h3>
          {suggested === null ? <div className="cm-skeleton" aria-busy="true" /> : suggested.length === 0 ? <Empty>{t("community.people.none")}</Empty> : (
            <ul className="cm-people">{suggested.map((p) => <li key={p.handle}><PersonRow person={p} /></li>)}</ul>
          )}
        </>
      )}
    </div>
  );
}
