import { useEffect, useState } from "react";
import { useT } from "../../app/i18n";
import { social, useCommunity, type Post } from "./api";
import { Avatar, useErrorText } from "./parts";

const tagsOf = (text: string) => text.split(/[\s,]+/).map((tag) => tag.replace(/^#/, "").toLowerCase()).filter(Boolean);

/**
 * What somebody is building, or a prompt that works for them. A prompt handed
 * over from the notebook arrives here and waits for Publish: nothing leaves
 * the machine until that button is pressed.
 */
export function Composer({ onPosted, replyTo, compact = false }: { onPosted: (post: Post) => void; replyTo?: string; compact?: boolean }) {
  const t = useT();
  const errorText = useErrorText();
  const me = useCommunity((s) => s.me);
  const draft = useCommunity((s) => s.draft);
  const setDraft = useCommunity((s) => s.setDraft);
  // A prompt handed over from the notebook seeds the composer once, on mount.
  const [seed] = useState(() => (replyTo ? null : draft));
  const [kind, setKind] = useState<"post" | "prompt">(seed?.kind ?? "post");
  const [body, setBody] = useState(seed?.body ?? "");
  const [title, setTitle] = useState(seed?.title ?? "");
  const [prompt, setPrompt] = useState(seed?.prompt ?? "");
  const [tags, setTags] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fromNotebook, setFromNotebook] = useState(seed !== null);

  useEffect(() => {
    if (seed) setDraft(null);
  }, [seed, setDraft]);

  const reset = () => {
    setBody("");
    setTitle("");
    setPrompt("");
    setTags("");
    setKind("post");
    setFromNotebook(false);
    setError(null);
  };
  const publish = async () => {
    setBusy(true);
    setError(null);
    try {
      const { post } = await social<{ post: Post }>("POST", "posts", {
        kind: replyTo ? "post" : kind,
        body,
        title,
        prompt,
        tags: replyTo ? [] : tagsOf(tags),
        replyTo: replyTo ?? null,
      });
      reset();
      onPosted(post);
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setBusy(false);
    }
  };

  const ready = kind === "prompt" && !replyTo ? title.trim() && prompt.trim() : body.trim();
  return (
    <section className="cm-composer" data-compact={compact || undefined}>
      {!compact && <Avatar name={me?.profile?.displayName ?? "?"} />}
      <div className="cm-composer__main">
        {!replyTo && (
          <div className="cm-segmented cm-composer__kind" role="radiogroup">
            {(["post", "prompt"] as const).map((value) => (
              <button key={value} type="button" role="radio" aria-checked={kind === value} data-active={kind === value || undefined} onClick={() => setKind(value)}>
                {value === "post" ? t("community.compose.kindPost") : t("community.compose.kindPrompt")}
              </button>
            ))}
          </div>
        )}
        {fromNotebook && <p className="cm-note">{t("community.compose.fromNotebook")}</p>}
        {kind === "prompt" && !replyTo && (
          <>
            <input className="cm-input" value={title} maxLength={120} placeholder={t("community.compose.title")} aria-label={t("community.compose.title")} onChange={(e) => setTitle(e.target.value)} />
            <textarea className="cm-input cm-input--mono" rows={6} value={prompt} placeholder={t("community.compose.prompt")} aria-label={t("community.compose.prompt")} onChange={(e) => setPrompt(e.target.value)} />
          </>
        )}
        <textarea
          className="cm-input"
          rows={compact ? 2 : 3}
          maxLength={2000}
          value={body}
          placeholder={replyTo ? t("community.thread.replyPlaceholder") : kind === "prompt" ? t("community.compose.note") : t("community.compose.placeholder")}
          aria-label={replyTo ? t("community.thread.replyPlaceholder") : t("community.compose.placeholder")}
          onChange={(e) => setBody(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && ready && !busy) void publish();
          }}
        />
        <div className="cm-composer__foot">
          {!replyTo && (
            <input className="cm-input cm-composer__tags" value={tags} placeholder={t("community.compose.tags")} aria-label={t("community.compose.tags")} onChange={(e) => setTags(e.target.value)} />
          )}
          <span className="cm-muted cm-composer__public">{t("community.compose.public")}</span>
          {fromNotebook && (
            <button type="button" className="cm-button cm-button--ghost" onClick={reset}>
              {t("community.compose.discard")}
            </button>
          )}
          <button type="button" className="cm-button cm-button--primary" disabled={!ready || busy} onClick={() => void publish()}>
            {busy ? t("community.compose.publishing") : replyTo ? t("community.post.reply") : t("community.compose.publish")}
          </button>
        </div>
        {error && <p className="cm-error" role="alert">{error}</p>}
      </div>
    </section>
  );
}
