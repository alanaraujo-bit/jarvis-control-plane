import { memo, useEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronRight, TriangleAlert, X } from "lucide-react";
import {
  EMPTY_CHAT,
  answerChat,
  attachChat,
  chatErrorKey,
  foldChat,
  interruptChat,
  sendChat,
  setChatEffort,
  setChatModel,
  type ChatAnswer,
  type ChatEvent,
  type ChatSnapshot,
} from "../../app/chat";
import { useT } from "../../app/i18n";
import { invoke, isTauri } from "../../app/platform";
import { insertIntoComposer } from "../terminal/live";
import { describeTool } from "./activity";
import { Composer } from "./Composer";
import { Markdown } from "./MarkdownView";
import { RequestCard } from "./RequestCard";
import { buildThread, reconcile, sentNow, type ChatItem, type Draft, type Entry, type Sent, type Step } from "./thread";
import "./ChatView.css";

/** How often the log is read while the chat is live. */
const POLL_MS = 500;

/**
 * A chat session (M29): what the person said, what the agent answered, the work
 * in between in one line, and the person's turn at the foot.
 *
 * The conversation is the session's own log (`session_conversation`), as for
 * Conversation View; the live half — streaming text, requests, whether the
 * agent is working — comes over the chat's channel.
 */
export function ChatView({
  sessionId,
  live,
  projectName,
  projectPath,
  active,
  onContinue,
}: {
  sessionId: string;
  /** False for a chat read from History: no process, no box to write in. */
  live: boolean;
  projectName: string;
  projectPath: string;
  /** On screen and holding the keyboard. */
  active: boolean;
  /** Present where a finished chat can be continued (D41). */
  onContinue?: () => void;
}) {
  const t = useT();
  const [snapshot, setSnapshot] = useState<ChatSnapshot>(live ? EMPTY_CHAT : { ...EMPTY_CHAT, ended: true });
  const [items, setItems] = useState<ChatItem[]>([]);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [sent, setSent] = useState<Sent[]>([]);
  const [error, setError] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);

  // The live half.
  useEffect(() => {
    if (!live || !isTauri()) return;
    let detach: (() => void) | undefined;
    let cancelled = false;
    const apply = (event: ChatEvent) => {
      setSnapshot((current) => foldChat(current, event));
      if (event.type === "draft") {
        setDraft((current) =>
          current && current.doneAt === null
            ? { ...current, text: event.text }
            : { text: event.text, startedAt: Date.now(), doneAt: null },
        );
      } else if (event.type === "draftDone") {
        setDraft((current) => (current ? { ...current, doneAt: Date.now() } : current));
      } else if (event.type === "exited") {
        // Nothing more will stream or arrive: a half-written reply and a
        // message that never reached the agent must not wait forever.
        setDraft(null);
        setSent([]);
      }
    };
    // The stream can overtake the snapshot it follows; what arrives first is
    // held and applied on top of the snapshot, in order.
    let held: ChatEvent[] | null = [];
    void attachChat(sessionId, (event) => {
      if (held) held.push(event);
      else apply(event);
    })
      .then((attached) => {
        if (cancelled) {
          attached.detach();
          return;
        }
        detach = attached.detach;
        setSnapshot(attached.snapshot);
        if (attached.snapshot.draft) {
          setDraft({ text: attached.snapshot.draft, startedAt: Date.now(), doneAt: null });
        }
        for (const event of held ?? []) apply(event);
        held = null;
      })
      .catch(() => setSnapshot((current) => ({ ...current, ended: true })));
    return () => {
      cancelled = true;
      detach?.();
    };
  }, [sessionId, live]);

  // The record: the session's log, read the same way live or long after.
  const liveNow = live && !snapshot.ended;
  useEffect(() => {
    if (!isTauri()) return;
    let cancelled = false;
    const read = async () => {
      try {
        const next = await invoke<ChatItem[]>("session_conversation", { sessionId });
        if (cancelled) return;
        setItems((current) => (current.length === next.length && sameTail(current, next) ? current : next));
        // Let go of the draft and the optimistic sends once the log has them.
        const thread = buildThread(next);
        const now = Date.now();
        setDraft((current) => reconcile(thread, current, [], now).draft);
        setSent((current) => {
          const left = reconcile(thread, null, current, now).sent;
          return left.length === current.length ? current : left;
        });
      } catch {
        // The next read tries again; a chat never blanks on one failure.
      }
    };
    void read();
    const timer = liveNow ? window.setInterval(() => void read(), POLL_MS) : undefined;
    return () => {
      cancelled = true;
      if (timer) window.clearInterval(timer);
    };
  }, [sessionId, liveNow]);

  const entries = useMemo(() => buildThread(items), [items]);

  // Follow new output only while the reader is at the bottom.
  useEffect(() => {
    const el = scrollRef.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [entries, draft, sent, snapshot.pending.length, snapshot.busy]);

  const act = (work: Promise<void>) => {
    setError(null);
    work.catch((cause) => setError(t(chatErrorKey(cause))));
  };

  const send = (text: string) => {
    pinned.current = true;
    const message = sentNow(text);
    setSent((current) => [...current, message]);
    setError(null);
    sendChat(sessionId, text).catch((cause) => {
      // Not sent: the bubble goes, and the words go back where they were
      // typed instead of being lost.
      setSent((current) => current.filter((entry) => entry !== message));
      insertIntoComposer(sessionId, text);
      setError(t(chatErrorKey(cause)));
    });
  };
  const answer = (id: string, value: ChatAnswer) => act(answerChat(sessionId, id, value));

  const empty = entries.length === 0 && sent.length === 0 && !draft;
  const request = snapshot.pending[0];

  return (
    <div className="chat">
      <div
        className="chat__scroll"
        ref={scrollRef}
        onScroll={() => {
          const el = scrollRef.current;
          if (el) pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
        }}
      >
        <div className="chat__column">
          {empty ? (
            <div className="chat__empty">
              <p className="chat__empty-title">{t("chat.empty.title", { project: projectName })}</p>
              <p className="chat__empty-body">{t("chat.empty.body")}</p>
            </div>
          ) : (
            entries.map((entry, index) => <EntryView key={`${entry.type}-${index}`} entry={entry} />)
          )}
          {sent.map((message, index) => (
            <div key={`sent-${index}`} className="chat__user" data-pending>
              <div className="chat__bubble selectable">{message.text}</div>
            </div>
          ))}
          {draft && (
            <div className="chat__agent" data-streaming={draft.doneAt === null || undefined}>
              <Markdown text={draft.text} />
            </div>
          )}
          {liveNow && (snapshot.busy || !snapshot.ready) && !request && (
            <div className="chat__working" role="status">
              <span className="chat__working-dot" aria-hidden="true" />
              {!snapshot.ready ? t("chat.starting") : draft ? t("chat.working") : t("chat.thinking")}
            </div>
          )}
          {liveNow && !snapshot.busy && snapshot.quickReplies.length > 0 && (
            <div className="chat__replies">
              {snapshot.quickReplies.map((option) => (
                <button key={option} type="button" className="chat__reply" onClick={() => send(option)}>
                  {option}
                </button>
              ))}
            </div>
          )}
        </div>
      </div>

      {live && (
        <div className="chat__dock">
          <div className="chat__column">
            {error && (
              <p className="chat__error" role="alert">
                {error}
              </p>
            )}
            {request && !snapshot.ended && (
              <RequestCard
                key={request.id}
                request={request}
                projectPath={projectPath}
                onAnswer={(value) => answer(request.id, value)}
              />
            )}
            {snapshot.ended ? (
              <div className="chat__ended">
                <span>{t("chat.ended")}</span>
                {onContinue && (
                  <button type="button" className="chat__continue" onClick={onContinue}>
                    {t("chat.continue")}
                  </button>
                )}
              </div>
            ) : (
              <Composer
                sessionId={sessionId}
                snapshot={snapshot}
                autoFocus={active}
                onSend={send}
                onStop={() => act(interruptChat(sessionId))}
                onModel={(id) => act(setChatModel(sessionId, id))}
                onEffort={(effort) => act(setChatEffort(sessionId, effort))}
              />
            )}
          </div>
        </div>
      )}
    </div>
  );
}

/** Cheap change check for a log that only ever grows. */
function sameTail(a: ChatItem[], b: ChatItem[]): boolean {
  const last = a.at(-1);
  const next = b.at(-1);
  return last === next || (last !== undefined && next !== undefined && JSON.stringify(last) === JSON.stringify(next));
}

const EntryView = memo(function EntryView({ entry }: { entry: Entry }) {
  switch (entry.type) {
    case "user":
      return (
        <div className="chat__user">
          <div className="chat__bubble selectable">{entry.text}</div>
        </div>
      );
    case "assistant":
      return (
        <div className="chat__agent">
          <Markdown text={entry.text} />
        </div>
      );
    case "activity":
      return <Activity steps={entry.steps} />;
    case "error":
      return (
        <div className="chat__failure">
          <TriangleAlert size={13} strokeWidth={2} aria-hidden="true" />
          <span className="selectable">{entry.message}</span>
        </div>
      );
  }
});

function Activity({ steps }: { steps: Step[] }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const work = steps.filter((step) => !step.thinking);
  const count = work.length || steps.length;
  return (
    <div className="chat-activity">
      <button
        type="button"
        className="chat-activity__toggle"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
      >
        <ChevronRight size={12} strokeWidth={2} className="chat-activity__chevron" data-open={open || undefined} aria-hidden="true" />
        {work.length === 0 ? t("chat.activity.think") : t("chat.activity.steps", { count })}
      </button>
      {open && (
        <ul className="chat-activity__steps">
          {steps.map((step, index) => (
            <li key={index} className="chat-activity__step">
              <StepText step={step} />
              {step.ok === true && <Check size={11} strokeWidth={2.4} className="chat-activity__ok" aria-hidden="true" />}
              {step.ok === false && <X size={11} strokeWidth={2.4} className="chat-activity__fail" aria-hidden="true" />}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function StepText({ step }: { step: Step }) {
  const t = useT();
  if (step.thinking) return <span>{t("chat.activity.think")}</span>;
  const { verb, target } = describeTool(step.name, step.summary);
  if (verb === "todo") return <span>{t("chat.activity.todo")}</span>;
  if (verb === "other") return <span>{t("chat.activity.other", { name: step.name })}</span>;
  // The target is the agent's own text (a path, a command), so it is set
  // apart and never translated.
  const [before, after] = t(`chat.activity.${verb}` as "chat.activity.read", { target: "\u0000" }).split("\u0000");
  return (
    <span>
      {before}
      <code className="chat-activity__target selectable">{target}</code>
      {after}
    </span>
  );
}
