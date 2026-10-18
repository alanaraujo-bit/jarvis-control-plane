import { useCallback, useEffect, useState } from "react";
import { X } from "lucide-react";
import { useT } from "../../app/i18n";
import { social, type Post } from "./api";
import { Composer } from "./Composer";
import { Empty, useErrorText } from "./parts";
import { PostCard } from "./PostCard";

type Scope = "following" | "discover";

/** A list of posts that can page and be edited in place. */
export function usePostList(path: (before: string | null) => string | null) {
  const errorText = useErrorText();
  const [posts, setPosts] = useState<Post[] | null>(null);
  const [more, setMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(async (append: boolean, before: string | null) => {
    const url = path(before);
    if (!url) return;
    try {
      const { posts: page } = await social<{ posts: Post[] }>("GET", url);
      setPosts((current) => (append && current ? [...current, ...page] : page));
      setMore(page.length >= 30);
      setError(null);
    } catch (cause) {
      setError(errorText(cause));
      setPosts((current) => current ?? []);
    }
  }, [path, errorText]);
  return {
    posts,
    more,
    error,
    reload: () => load(false, null),
    loadMore: () => posts && posts.length > 0 && load(true, posts[posts.length - 1].createdAt),
    update: (post: Post) => setPosts((current) => current?.map((p) => (p.id === post.id ? post : p)) ?? null),
    remove: (id: string) => setPosts((current) => current?.filter((p) => p.id !== id) ?? null),
    prepend: (post: Post) => setPosts((current) => [post, ...(current ?? [])]),
  };
}

export function PostList({ list, empty }: { list: ReturnType<typeof usePostList>; empty: string }) {
  const t = useT();
  if (list.posts === null) return <div className="cm-skeleton" aria-busy="true" />;
  return (
    <>
      {list.error && <p className="cm-error" role="alert">{list.error}</p>}
      {list.posts.length === 0 && !list.error ? <Empty>{empty}</Empty> : null}
      <div className="cm-list">
        {list.posts.map((post) => (
          <PostCard key={post.id} post={post} onChange={list.update} onRemove={list.remove} />
        ))}
      </div>
      {list.more && (
        <button type="button" className="cm-button cm-button--ghost cm-more" onClick={() => void list.loadMore()}>
          {t("community.feed.more")}
        </button>
      )}
    </>
  );
}

/** Following and Discover, with the composer on top (M28). */
export function Feed() {
  const t = useT();
  const [scope, setScope] = useState<Scope>("discover");
  const [tag, setTag] = useState<string | null>(null);
  const path = useCallback(
    (before: string | null) => {
      const params = new URLSearchParams({ scope });
      if (tag) params.set("tag", tag);
      if (before) params.set("before", before);
      return `feed?${params}`;
    },
    [scope, tag],
  );
  const list = usePostList(path);
  const { reload } = list;

  useEffect(() => {
    void reload();
  }, [path]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const onTag = (event: Event) => {
      setTag((event as CustomEvent<string>).detail);
      setScope("discover");
    };
    window.addEventListener("community:tag", onTag);
    return () => window.removeEventListener("community:tag", onTag);
  }, []);

  return (
    <div className="cm-feed">
      <Composer onPosted={list.prepend} />
      <div className="cm-tabs" role="tablist">
        {(["following", "discover"] as const).map((value) => (
          <button key={value} type="button" role="tab" aria-selected={scope === value} data-active={scope === value || undefined} onClick={() => setScope(value)}>
            {value === "following" ? t("community.feed.following") : t("community.feed.discover")}
          </button>
        ))}
        {tag && (
          <span className="cm-tag-filter">
            {t("community.feed.tag", { tag })}
            <button type="button" className="cm-icon-button" aria-label={t("community.feed.clearTag")} onClick={() => setTag(null)}>
              <X size={12} aria-hidden="true" />
            </button>
          </span>
        )}
      </div>
      <PostList list={list} empty={scope === "following" ? t("community.feed.emptyFollowing") : t("community.feed.emptyDiscover")} />
    </div>
  );
}
