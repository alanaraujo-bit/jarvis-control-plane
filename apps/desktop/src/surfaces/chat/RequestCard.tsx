import { useState, type KeyboardEvent } from "react";
import { FileEdit, FilePlus, Globe, MessageCircleQuestion, Terminal, Wrench } from "lucide-react";
import type { ChatAction, ChatAnswer, ChatRequest } from "../../app/chat";
import { useT } from "../../app/i18n";

const ICONS: Record<ChatAction, typeof Terminal> = {
  command: Terminal,
  edit: FileEdit,
  create: FilePlus,
  web: Globe,
  question: MessageCircleQuestion,
  other: Wrench,
};

/** A path inside the project, shown from the project's folder. */
function fromProject(detail: string, projectPath: string): string {
  const norm = (text: string) => text.replace(/\\/g, "/");
  const root = norm(projectPath).replace(/\/+$/, "");
  const path = norm(detail);
  return path.toLowerCase().startsWith(`${root.toLowerCase()}/`) ? path.slice(root.length + 1) : detail;
}

/**
 * The agent is waiting on the person (M29): a permission, or a question.
 *
 * Coloured as "waiting for you" (`--state-waiting`), never amber — amber is
 * the agent working, and this is the agent stopped. It never takes the focus
 * on its own: a card that grabbed it while somebody was typing would turn
 * their next Enter into a "yes" they never meant.
 */
export function RequestCard({
  request,
  projectPath,
  onAnswer,
}: {
  request: ChatRequest;
  projectPath: string;
  onAnswer: (answer: ChatAnswer) => void;
}) {
  return request.kind === "question" ? (
    <QuestionCard request={request} onAnswer={onAnswer} />
  ) : (
    <PermissionCard request={request} projectPath={projectPath} onAnswer={onAnswer} />
  );
}

function PermissionCard({
  request,
  projectPath,
  onAnswer,
}: {
  request: ChatRequest;
  projectPath: string;
  onAnswer: (answer: ChatAnswer) => void;
}) {
  const t = useT();
  const Icon = ICONS[request.action];
  const title =
    request.action === "other"
      ? t("chat.request.other", { tool: request.tool })
      : t(`chat.request.${request.action}` as "chat.request.command");
  const isPath = request.action === "edit" || request.action === "create";
  const detail = isPath
    ? request.detail
        .split("\n")
        .map((line) => fromProject(line, projectPath))
        .join("\n")
    : request.detail;

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onAnswer({ type: "deny" });
    }
  };

  return (
    <section className="chat-card" role="group" aria-label={title} onKeyDown={onKeyDown}>
      <header className="chat-card__head">
        <Icon size={14} strokeWidth={1.9} aria-hidden="true" />
        <span className="chat-card__title">{title}</span>
      </header>
      {detail && <pre className="chat-card__detail selectable">{detail}</pre>}
      <div className="chat-card__actions">
        <button type="button" className="chat-card__button" data-primary onClick={() => onAnswer({ type: "allow", always: false })}>
          {t("chat.request.allow")}
        </button>
        {request.canAlways && (
          <button type="button" className="chat-card__button" onClick={() => onAnswer({ type: "allow", always: true })}>
            {t("chat.request.always")}
          </button>
        )}
        <button type="button" className="chat-card__button" data-quiet onClick={() => onAnswer({ type: "deny" })}>
          {t("chat.request.deny")}
        </button>
      </div>
    </section>
  );
}

function QuestionCard({ request, onAnswer }: { request: ChatRequest; onAnswer: (answer: ChatAnswer) => void }) {
  const t = useT();
  const [chosen, setChosen] = useState<Record<string, string[]>>({});
  const [other, setOther] = useState<Record<string, string>>({});

  const answers = Object.fromEntries(
    request.questions.map((question) => {
      const typed = (other[question.id] ?? "").trim();
      return [question.id, [...(chosen[question.id] ?? []), ...(typed ? [typed] : [])]];
    }),
  );
  const complete = request.questions.every((question) => answers[question.id].length > 0);

  const toggle = (id: string, label: string, multi: boolean) =>
    setChosen((current) => {
      const now = current[id] ?? [];
      if (!multi) return { ...current, [id]: now[0] === label ? [] : [label] };
      return { ...current, [id]: now.includes(label) ? now.filter((l) => l !== label) : [...now, label] };
    });

  return (
    <section className="chat-card" role="group" aria-label={t("chat.question.title")}>
      <header className="chat-card__head">
        <MessageCircleQuestion size={14} strokeWidth={1.9} aria-hidden="true" />
        <span className="chat-card__title">{t("chat.question.title")}</span>
      </header>
      {request.questions.map((question) => (
        <div key={question.id} className="chat-card__question">
          {question.header && <span className="chat-card__chip">{question.header}</span>}
          <p className="chat-card__ask selectable">{question.question}</p>
          <div className="chat-card__options" role={question.multiSelect ? "group" : "radiogroup"}>
            {question.options.map((option) => {
              const on = (chosen[question.id] ?? []).includes(option.label);
              return (
                <button
                  key={option.label}
                  type="button"
                  role={question.multiSelect ? "checkbox" : "radio"}
                  aria-checked={on}
                  data-on={on || undefined}
                  className="chat-card__option"
                  onClick={() => toggle(question.id, option.label, question.multiSelect)}
                  title={option.description || undefined}
                >
                  <span className="chat-card__option-label">{option.label}</span>
                  {option.description && <span className="chat-card__option-help">{option.description}</span>}
                </button>
              );
            })}
          </div>
          <input
            className="chat-card__other"
            value={other[question.id] ?? ""}
            onChange={(event) => setOther((current) => ({ ...current, [question.id]: event.target.value }))}
            placeholder={t("chat.question.other")}
            aria-label={t("chat.question.other")}
          />
        </div>
      ))}
      <div className="chat-card__actions">
        <button
          type="button"
          className="chat-card__button"
          data-primary
          disabled={!complete}
          onClick={() => onAnswer({ type: "answers", answers })}
        >
          {t("chat.question.answer")}
        </button>
        <button type="button" className="chat-card__button" data-quiet onClick={() => onAnswer({ type: "deny" })}>
          {t("chat.question.skip")}
        </button>
      </div>
    </section>
  );
}
