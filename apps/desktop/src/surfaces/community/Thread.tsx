import { useEffect, useState } from "react";
import { ArrowLeft } from "lucide-react";
import { useT } from "../../app/i18n";
import { social, useCommunity, type Post } from "./api";
import { Composer } from "./Composer";
import { Empty } from "./parts";
import { PostCard } from "./PostCard";

/** A post, its replies, and a box to add one. */
export function Thread({ id }: { id: string }) {
  const t = useT();
  const go = useCommunity((s) => s.go);
  const [post, setPost] = useState<Post | null>(null);
  const [replies, setReplies] = useState<Post[]>([]);
  const [gone, setGone] = useState(false);
  // A reply opens with what it answers, so a notification lands in context.
  const [parent, setParent] = useState<Post | null>(null);

  useEffect(() => {
    let alive = true;
    social<{ post: Post; replies: Post[] }>("GET", `posts/${id}`)
      .then((answer) => {
        if (!alive) return;
        setPost(answer.post);
        setReplies(answer.replies);
        if (answer.post.replyTo) {
          void social<{ post: Post }>("GET", `posts/${answer.post.replyTo}`)
            .then((up) => alive && setParent(up.post))
            .catch(() => undefined);
        }
      })
      .catch(() => alive && setGone(true));
    return () => {
      alive = false;
    };
  }, [id]);

  return (
    <div className="cm-thread">
      <button type="button" className="cm-link cm-back" onClick={() => go({ kind: "feed" })}>
        <ArrowLeft size={13} aria-hidden="true" /> {t("community.thread.back")}
      </button>
      {gone ? (
        <Empty>{t("community.thread.gone")}</Empty>
      ) : !post ? (
        <div className="cm-skeleton" aria-busy="true" />
      ) : (
        <>
          {parent && (
            <div className="cm-thread__parent">
              <PostCard post={parent} onChange={setParent} onRemove={() => setParent(null)} />
            </div>
          )}
          <div className="cm-card cm-thread__root">
            <PostCard post={post} linked={false} onChange={setPost} onRemove={() => go({ kind: "feed" })} />
          </div>
          <Composer compact replyTo={post.id} onPosted={(reply) => { setReplies((current) => [...current, reply]); setPost({ ...post, replyCount: post.replyCount + 1 }); }} />
          {replies.length === 0 ? (
            <Empty>{t("community.thread.noReplies")}</Empty>
          ) : (
            <div className="cm-list">
              {replies.map((reply) => (
                <PostCard
                  key={reply.id}
                  post={reply}
                  onChange={(next) => setReplies((current) => current.map((r) => (r.id === next.id ? next : r)))}
                  onRemove={(gone) => setReplies((current) => current.filter((r) => r.id !== gone))}
                />
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}
