import { useState } from "react";
import { Bookmark, BookmarkCheck, Copy, Heart, Link2, MessageCircle, MoreHorizontal } from "lucide-react";
import { useI18n, useT } from "../../app/i18n";
import { invoke } from "../../app/platform";
import { Popover } from "../../design/Popover";
import { useNotebook } from "../notebook/useNotebook";
import { ago, social, useCommunity, WEB_ORIGIN, type Post } from "./api";
import { Avatar, formatCount, RichText, useErrorText } from "./parts";

interface Saved {
  title: string;
  prompt: string;
  author: string;
  post: Post;
}

/**
 * One post or prompt. A prompt can be copied or saved straight into the
 * person's notebook; saving also tells its author (M28).
 */
export function PostCard({ post, onChange, onRemove, linked = true }: { post: Post; onChange: (post: Post) => void; onRemove: (id: string) => void; linked?: boolean }) {
  const t = useT();
  const { locale } = useI18n();
  const errorText = useErrorText();
  const go = useCommunity((s) => s.go);
  const me = useCommunity((s) => s.me);
  const loadNotebook = useNotebook((s) => s.load);
  const [expanded, setExpanded] = useState(false);
  const [said, setSaid] = useState<string | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const [menuAnchor, setMenuAnchor] = useState<HTMLButtonElement | null>(null);
  const mine = me?.profile?.handle === post.author.handle;
  const long = post.prompt.split("\n").length > 10 || post.prompt.length > 700;

  const flash = (text: string) => {
    setSaid(text);
    window.setTimeout(() => setSaid(null), 2200);
  };
  const run = async (task: () => Promise<void>) => {
    try {
      await task();
    } catch (cause) {
      flash(errorText(cause));
    }
  };
  const like = () => run(async () => {
    const { post: next } = await social<{ post: Post }>(post.likedByMe ? "DELETE" : "POST", `posts/${post.id}/like`);
    onChange(next);
  });
  const save = () => run(async () => {
    const saved = await social<Saved>("POST", `posts/${post.id}/save`);
    // The prompt lands in the notebook on this machine, attributed.
    const created = await invoke<{ id: string }>("notebook_note_create", { notebookId: null });
    await invoke("notebook_note_update", {
      id: created.id,
      title: saved.title,
      body: `${saved.prompt}\n\n— @${saved.author}, ${WEB_ORIGIN}/p/${post.id}`,
    });
    void loadNotebook();
    onChange(saved.post);
    flash(t("community.post.savedToNotebook"));
  });
  const copyText = (text: string, message: string) => {
    void navigator.clipboard.writeText(text).catch(() => undefined);
    flash(message);
  };
  const remove = () => run(async () => {
    setMenuOpen(false);
    if (!window.confirm(t("community.post.deleteConfirm"))) return;
    await social("DELETE", `posts/${post.id}`);
    onRemove(post.id);
  });
  const report = () => run(async () => {
    setMenuOpen(false);
    await social("POST", `posts/${post.id}/report`, { reason: "" });
    flash(t("community.post.reported"));
  });

  return (
    <article className="cm-post">
      <button type="button" className="cm-post__avatar" onClick={() => go({ kind: "profile", handle: post.author.handle })} aria-label={post.author.displayName}>
        <Avatar name={post.author.displayName} />
      </button>
      <div className="cm-post__main">
        <header className="cm-post__head">
          <button type="button" className="cm-link cm-post__name" onClick={() => go({ kind: "profile", handle: post.author.handle })}>
            {post.author.displayName}
          </button>
          <span className="cm-muted">@{post.author.handle}</span>
          <span className="cm-muted">·</span>
          {linked ? (
            <button type="button" className="cm-link cm-muted" onClick={() => go({ kind: "thread", id: post.id })} title={new Date(post.createdAt).toLocaleString(locale)}>
              {ago(post.createdAt, locale)}
            </button>
          ) : (
            <time className="cm-muted" dateTime={post.createdAt}>{new Date(post.createdAt).toLocaleString(locale)}</time>
          )}
        </header>
        {post.parent && <p className="cm-muted cm-post__reply-to">{t("community.post.replyingTo", { handle: post.parent.handle })}</p>}
        {post.body && <p className="cm-post__body"><RichText text={post.body} /></p>}
        {post.kind === "prompt" && (
          <div className="cm-prompt">
            <div className="cm-prompt__head">
              <strong>{post.title}</strong>
              <span className="cm-muted">{t("community.post.savedCount", { count: post.saveCount })}</span>
            </div>
            <pre className="cm-prompt__text" data-clamped={(long && !expanded) || undefined}>{post.prompt}</pre>
            {long && (
              <button type="button" className="cm-link cm-prompt__more" onClick={() => setExpanded((v) => !v)}>
                {expanded ? t("community.post.showLess") : t("community.post.showMore")}
              </button>
            )}
            <div className="cm-prompt__actions">
              {!mine && <button type="button" className="cm-button" disabled={post.savedByMe} onClick={() => void save()}>
                {post.savedByMe ? <BookmarkCheck size={13} aria-hidden="true" /> : <Bookmark size={13} aria-hidden="true" />}
                {post.savedByMe ? t("community.post.savedToNotebook") : t("community.post.saveToNotebook")}
              </button>}
              <button type="button" className="cm-button cm-button--ghost" onClick={() => copyText(post.prompt, t("community.post.copied"))}>
                <Copy size={13} aria-hidden="true" />
                {t("community.post.copy")}
              </button>
            </div>
          </div>
        )}
        {post.tags.length > 0 && (
          <div className="cm-tags">
            {post.tags.map((tag) => (
              <button key={tag} type="button" className="cm-tag" onClick={() => { useCommunity.setState({ view: { kind: "feed" } }); window.dispatchEvent(new CustomEvent("community:tag", { detail: tag })); }}>
                #{tag}
              </button>
            ))}
          </div>
        )}
        <footer className="cm-post__actions">
          <button type="button" className="cm-action" data-on={post.likedByMe || undefined} aria-pressed={post.likedByMe} aria-label={post.likedByMe ? t("community.post.unlike") : t("community.post.like")} onClick={() => void like()}>
            <Heart size={14} aria-hidden="true" fill={post.likedByMe ? "currentColor" : "none"} />
            {post.likeCount > 0 && formatCount(post.likeCount, locale)}
          </button>
          <button type="button" className="cm-action" aria-label={t("community.post.reply")} onClick={() => go({ kind: "thread", id: post.id })}>
            <MessageCircle size={14} aria-hidden="true" />
            {post.replyCount > 0 && formatCount(post.replyCount, locale)}
          </button>
          <button type="button" className="cm-action" aria-label={t("community.post.copyLink")} onClick={() => copyText(`${WEB_ORIGIN}/p/${post.id}`, t("community.post.linkCopied"))}>
            <Link2 size={14} aria-hidden="true" />
          </button>
          <button ref={setMenuAnchor} type="button" className="cm-action" aria-label={t("community.post.more")} aria-expanded={menuOpen} onClick={() => setMenuOpen((v) => !v)}>
            <MoreHorizontal size={14} aria-hidden="true" />
          </button>
          {said && <span className="cm-said" role="status">{said}</span>}
        </footer>
        <Popover anchor={menuAnchor} open={menuOpen} onClose={() => setMenuOpen(false)} align="start">
          {mine ? (
            <button type="button" className="popover__item cm-danger" onClick={() => void remove()}>{t("community.post.delete")}</button>
          ) : (
            <button type="button" className="popover__item" onClick={() => void report()}>{t("community.post.report")}</button>
          )}
        </Popover>
      </div>
    </article>
  );
}
