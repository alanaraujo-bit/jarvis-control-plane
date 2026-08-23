import { useState } from "react";
import {
  Bot,
  Check,
  Pencil,
  Pin,
  PinOff,
  Plus,
  Sparkles,
  Trash2,
  User,
  X,
} from "lucide-react";
import { useT } from "../../app/i18n";
import { useVisitRefresh } from "../../app/useVisitRefresh";
import type { MessageKey } from "@jarvis/i18n";
import { KINDS, useBrain, type Fact, type Kind, type Knowledge, type Note } from "./useBrain";
import "./BrainView.css";

interface BrainViewProps {
  projectId: string;
  /** True while this area is the one on screen. See `useVisitRefresh`. */
  active: boolean;
}

const TABS = ["knowledge", "notes", "timeline"] as const;
type Tab = (typeof TABS)[number];

/**
 * Project Brain (§36–§39) and Notes (§40).
 *
 * Three tabs, and the split between them is the point rather than a layout
 * convenience:
 *
 * - **Knowledge** is what people and agents stated. It is what gets briefed.
 * - **Notes** are working memory, and are never sent anywhere.
 * - **History** is what the record shows — computed, never written by anyone.
 *
 * Keeping stated and derived on separate tabs makes §28's rule structural
 * instead of a caption somebody has to read. A panel that mixed "deploys go
 * through staging" with "src/app.ts changed nine times" would invite the reader
 * to trust both the same amount.
 */
export function BrainView({ projectId, active }: BrainViewProps) {
  const t = useT();
  const [tab, setTab] = useState<Tab>("knowledge");
  const report = useBrain((s) => s.report[projectId]);
  const error = useBrain((s) => s.error[projectId]);
  const refresh = useBrain((s) => s.refresh);

  // The derived half of this surface is a projection of the log, so it goes
  // stale the moment an agent does anything. Re-read when it is looked at.
  useVisitRefresh(active, () => void refresh(projectId));

  return (
    <div className="brain">
      <header className="brain__header">
        <div>
          <h2 className="brain__title">{t("brain.title")}</h2>
          <p className="brain__subtitle">{t("brain.subtitle")}</p>
        </div>
        <nav className="brain__tabs" aria-label={t("brain.title")}>
          {TABS.map((id) => (
            <button
              key={id}
              type="button"
              className="brain__tab"
              data-active={tab === id || undefined}
              aria-current={tab === id ? "page" : undefined}
              onClick={() => setTab(id)}
            >
              {t(`brain.tab.${id}` as MessageKey)}
              {id === "notes" && (report?.notes.length ?? 0) > 0 && (
                <span className="brain__tab-count">{report?.notes.length}</span>
              )}
            </button>
          ))}
        </nav>
      </header>

      {error && <p className="brain__error">{error}</p>}

      {tab === "knowledge" && <KnowledgeTab projectId={projectId} />}
      {tab === "notes" && <NotesTab projectId={projectId} />}
      {tab === "timeline" && <HistoryTab projectId={projectId} />}
    </div>
  );
}

/** What people and agents have stated, grouped by kind. */
function KnowledgeTab({ projectId }: { projectId: string }) {
  const t = useT();
  const report = useBrain((s) => s.report[projectId]);
  const knowledge = report?.knowledge ?? [];

  return (
    <div className="brain__body">
      <BriefPanel projectId={projectId} />

      {/* One way in, rather than a form under every heading. Four identical
          input boxes made the surface look like a form to fill in, and their
          headings stood over nothing. */}
      <AddKnowledge projectId={projectId} />

      {knowledge.length === 0 && (
        <div className="brain__empty">
          <p className="brain__empty-title">{t("brain.empty.title")}</p>
          <p className="brain__empty-body">{t("brain.empty.body")}</p>
        </div>
      )}

      {/* A section with nothing in it does not appear (§18). The kind picker
          above is how a new one comes into being. */}
      {KINDS.map((kind) => {
        const entries = knowledge.filter((k) => k.kind === kind);
        if (entries.length === 0) return null;
        return (
          <section key={kind} className="brain__section">
            <h3 className="brain__section-title">{t(`brain.kind.${kind}` as MessageKey)}</h3>
            <ul className="brain__entries">
              {entries.map((entry) => (
                <KnowledgeRow key={entry.id} projectId={projectId} entry={entry} />
              ))}
            </ul>
          </section>
        );
      })}
    </div>
  );
}

/**
 * One stated thing, with who said it.
 *
 * The source is shown on every row rather than only on agent-written ones. A
 * marker that appears only sometimes reads as a warning; showing both makes it
 * an attribution, which is what it is (§28).
 */
function KnowledgeRow({ projectId, entry }: { projectId: string; entry: Knowledge }) {
  const t = useT();
  const [editing, setEditing] = useState(false);
  const [body, setBody] = useState(entry.body);
  const [kind, setKind] = useState<Kind>(entry.kind);
  const update = useBrain((s) => s.updateKnowledge);
  const archive = useBrain((s) => s.archiveKnowledge);

  if (editing) {
    return (
      <li className="brain__entry brain__entry--editing">
        <textarea
          className="brain__input"
          value={body}
          rows={3}
          autoFocus
          onChange={(e) => setBody(e.target.value)}
        />
        <div className="brain__entry-editRow">
          <KindPicker value={kind} onChange={setKind} />
          <button
            type="button"
            className="brain__button brain__button--primary"
            disabled={!body.trim()}
            onClick={() => {
              void update(projectId, entry.id, kind, body);
              setEditing(false);
            }}
          >
            {t("brain.save")}
          </button>
          <button
            type="button"
            className="brain__button"
            onClick={() => {
              setBody(entry.body);
              setKind(entry.kind);
              setEditing(false);
            }}
          >
            {t("brain.cancel")}
          </button>
        </div>
      </li>
    );
  }

  return (
    <li className="brain__entry">
      <p className="brain__entry-body selectable">{entry.body}</p>
      <div className="brain__entry-meta">
        <span className="brain__source" data-source={entry.source}>
          {entry.source === "agent" ? (
            <Bot size={11} strokeWidth={2} aria-hidden="true" />
          ) : (
            <User size={11} strokeWidth={2} aria-hidden="true" />
          )}
          {t(`brain.source.${entry.source}` as MessageKey)}
        </span>
        <span className="brain__entry-actions">
          <button
            type="button"
            className="brain__icon"
            onClick={() => setEditing(true)}
            aria-label={t("brain.edit")}
            title={t("brain.edit")}
          >
            <Pencil size={12} strokeWidth={2} aria-hidden="true" />
          </button>
          <button
            type="button"
            className="brain__icon"
            onClick={() => void archive(projectId, entry.id)}
            aria-label={t("brain.archiveTitle")}
            title={t("brain.archiveTitle")}
          >
            <Trash2 size={12} strokeWidth={2} aria-hidden="true" />
          </button>
        </span>
      </div>
    </li>
  );
}

function KindPicker({ value, onChange }: { value: Kind; onChange: (kind: Kind) => void }) {
  const t = useT();
  return (
    <select
      className="brain__kindPicker"
      value={value}
      onChange={(e) => onChange(e.target.value as Kind)}
      aria-label={t("brain.tab.knowledge")}
    >
      {KINDS.map((kind) => (
        <option key={kind} value={kind}>
          {t(`brain.kind.${kind}` as MessageKey)}
        </option>
      ))}
    </select>
  );
}

/**
 * The one way to record something.
 *
 * The kind is chosen here rather than implied by which box was typed into. Four
 * add forms, one per heading, made the page read as a form to be filled in
 * rather than a document to be written — and left four headings standing over
 * nothing on a project that had recorded one thing.
 *
 * The placeholder follows the kind, because "something that stays true about
 * this project" repeated four times tells nobody what belongs where.
 */
function AddKnowledge({ projectId }: { projectId: string }) {
  const t = useT();
  const [body, setBody] = useState("");
  const [kind, setKind] = useState<Kind>("what");
  const add = useBrain((s) => s.addKnowledge);

  return (
    <form
      className="brain__add"
      onSubmit={(e) => {
        e.preventDefault();
        if (!body.trim()) return;
        void add(projectId, kind, body);
        setBody("");
      }}
    >
      <KindPicker value={kind} onChange={setKind} />
      <input
        className="brain__addInput"
        value={body}
        placeholder={t(`brain.placeholder.${kind}` as MessageKey)}
        onChange={(e) => setBody(e.target.value)}
      />
      <button type="submit" className="brain__button" disabled={!body.trim()}>
        <Plus size={12} strokeWidth={2.2} aria-hidden="true" />
        {t("brain.add")}
      </button>
    </form>
  );
}

/**
 * What an agent is actually told.
 *
 * Shown as a preview the person can open, for the same reason the guardrail
 * shows the command it is about to run verbatim (§35): a briefing nobody can
 * inspect is a briefing nobody can trust. The size is always visible, because
 * this text is paid for on every turn of every session and a cost that only
 * appears on a bill is a surprise.
 */
function BriefPanel({ projectId }: { projectId: string }) {
  const t = useT();
  const report = useBrain((s) => s.report[projectId]);
  const brief = useBrain((s) => s.brief[projectId]);
  const loadBrief = useBrain((s) => s.loadBrief);
  const [open, setOpen] = useState(false);

  const size = report?.briefSize ?? 0;

  return (
    <section className="brain__brief">
      <header className="brain__brief-head">
        <Sparkles size={13} strokeWidth={1.9} aria-hidden="true" />
        <h3 className="brain__brief-title">{t("brain.brief.title")}</h3>
        {size > 0 ? (
          <span className="brain__brief-size">{t("brain.brief.size", { count: size })}</span>
        ) : (
          <span className="brain__brief-size">{t("brain.brief.none")}</span>
        )}
        {size > 0 && (
          <button
            type="button"
            className="brain__button"
            onClick={() => {
              if (!open && brief === undefined) void loadBrief(projectId);
              setOpen((v) => !v);
            }}
          >
            {open ? t("brain.brief.hide") : t("brain.brief.show")}
          </button>
        )}
      </header>
      {open && brief && <pre className="brain__brief-text selectable">{brief}</pre>}
    </section>
  );
}

/** Working memory. Never briefed, and the panel says so. */
function NotesTab({ projectId }: { projectId: string }) {
  const t = useT();
  const report = useBrain((s) => s.report[projectId]);
  const add = useBrain((s) => s.addNote);
  const [body, setBody] = useState("");
  const notes = report?.notes ?? [];

  return (
    <div className="brain__body">
      <form
        className="brain__add brain__add--note"
        onSubmit={(e) => {
          e.preventDefault();
          if (!body.trim()) return;
          void add(projectId, body);
          setBody("");
        }}
      >
        <textarea
          className="brain__input"
          value={body}
          rows={2}
          placeholder={t("brain.notes.placeholder")}
          onChange={(e) => setBody(e.target.value)}
          onKeyDown={(e) => {
            // The convention every capture box uses. Enter alone stays a
            // newline: a note has more than one line in it often enough.
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              if (body.trim()) {
                void add(projectId, body);
                setBody("");
              }
            }
          }}
        />
        <button type="submit" className="brain__button" disabled={!body.trim()}>
          <Plus size={12} strokeWidth={2.2} aria-hidden="true" />
          {t("brain.notes.add")}
        </button>
      </form>

      {notes.length === 0 ? (
        <div className="brain__empty">
          <p className="brain__empty-title">{t("brain.notes.empty.title")}</p>
          <p className="brain__empty-body">{t("brain.notes.empty.body")}</p>
        </div>
      ) : (
        <ul className="brain__entries">
          {notes.map((note) => (
            <NoteRow key={note.id} projectId={projectId} note={note} />
          ))}
        </ul>
      )}
    </div>
  );
}

function NoteRow({ projectId, note }: { projectId: string; note: Note }) {
  const t = useT();
  const [promoting, setPromoting] = useState(false);
  const [kind, setKind] = useState<Kind>("gotcha");
  const pin = useBrain((s) => s.setNotePinned);
  const remove = useBrain((s) => s.deleteNote);
  const promote = useBrain((s) => s.promoteNote);

  return (
    <li className="brain__entry" data-pinned={note.pinned || undefined}>
      <p className="brain__entry-body selectable">{note.body}</p>
      <div className="brain__entry-meta">
        {promoting ? (
          <span className="brain__entry-editRow">
            <KindPicker value={kind} onChange={setKind} />
            <button
              type="button"
              className="brain__button brain__button--primary"
              onClick={() => {
                void promote(projectId, note.id, kind);
                setPromoting(false);
              }}
            >
              <Check size={12} strokeWidth={2.2} aria-hidden="true" />
              {t("brain.notes.promote")}
            </button>
            <button type="button" className="brain__icon" onClick={() => setPromoting(false)}>
              <X size={12} strokeWidth={2} aria-hidden="true" />
            </button>
          </span>
        ) : (
          <span className="brain__entry-actions">
            {/* The path a thing takes when it turns out to be durable. */}
            <button
              type="button"
              className="brain__button"
              onClick={() => setPromoting(true)}
              title={t("brain.notes.promoteAs")}
            >
              {t("brain.notes.promote")}
            </button>
            <button
              type="button"
              className="brain__icon"
              onClick={() => void pin(projectId, note.id, !note.pinned)}
              aria-label={t(note.pinned ? "brain.notes.unpin" : "brain.notes.pin")}
              title={t(note.pinned ? "brain.notes.unpin" : "brain.notes.pin")}
              data-on={note.pinned || undefined}
            >
              {note.pinned ? (
                <PinOff size={12} strokeWidth={2} aria-hidden="true" />
              ) : (
                <Pin size={12} strokeWidth={2} aria-hidden="true" />
              )}
            </button>
            <button
              type="button"
              className="brain__icon"
              onClick={() => void remove(projectId, note.id)}
              aria-label={t("brain.notes.delete")}
              title={t("brain.notes.delete")}
            >
              <Trash2 size={12} strokeWidth={2} aria-hidden="true" />
            </button>
          </span>
        )}
      </div>
    </li>
  );
}

/** What the record shows, and what happened. Both computed, neither stated. */
function HistoryTab({ projectId }: { projectId: string }) {
  const t = useT();
  const report = useBrain((s) => s.report[projectId]);
  const facts = report?.facts ?? [];
  const timeline = report?.timeline ?? [];

  return (
    <div className="brain__body">
      <section className="brain__section">
        <h3 className="brain__section-title">{t("brain.facts.title")}</h3>
        {/* Said once, at the top, rather than repeated on every line. The
            whole panel is derived — that is why it is its own tab. */}
        <p className="brain__derivedNote">{t("brain.facts.derived")}</p>
        {facts.length === 0 ? (
          <p className="brain__note">{t("brain.facts.empty")}</p>
        ) : (
          <ul className="brain__facts">
            {facts.map((fact, i) => (
              <li key={`${fact.code}-${i}`} className="brain__fact">
                {renderFact(t, fact)}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="brain__section">
        <h3 className="brain__section-title">{t("brain.tab.timeline")}</h3>
        {timeline.length === 0 ? (
          <p className="brain__note">{t("brain.timeline.empty")}</p>
        ) : (
          <ul className="brain__timeline">
            {timeline.map((moment, i) => (
              <li
                key={`${moment.tsMs}-${i}`}
                className="brain__moment"
                data-severity={moment.severity}
              >
                <time className="brain__moment-when">{formatWhen(moment.tsMs)}</time>
                <span className="brain__moment-kind">
                  {labelFor(t, `activity.kind.${moment.kind}`, moment.kind)}
                </span>
                <span className="brain__moment-title">{moment.title}</span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}

/**
 * Render a derived fact from its code and arguments.
 *
 * The core never assembles prose (§65), so the sentence is here and the numbers
 * arrive separately. Arguments are positional — `{0}`, `{1}` — because word
 * order differs between the two languages and a fixed sentence would not
 * survive translation.
 */
function renderFact(t: ReturnType<typeof useT>, fact: Fact): string {
  const values: Record<string, string | number> = {};
  fact.args.forEach((arg, i) => {
    values[String(i)] = arg;
  });
  // `count` is what selects the singular form. Without it the catalogue cannot
  // agree with the number and pt-BR reads "1 sessões", which is the tell that a
  // translation was done on top of an English sentence rather than with it.
  values.count = fact.count;
  return t(fact.code as MessageKey, values);
}

/**
 * A localised label, falling back to the raw id.
 *
 * An unknown activity kind shows its identifier rather than an empty row. Ugly
 * on purpose: a raw key on screen is how D13 was found, and a blank would have
 * hidden it.
 */
function labelFor(t: ReturnType<typeof useT>, key: string, fallback: string): string {
  const label = t(key as MessageKey);
  return label === key ? fallback : label;
}

function formatWhen(tsMs: number): string {
  const date = new Date(tsMs);
  const today = new Date();
  const sameDay =
    date.getFullYear() === today.getFullYear() &&
    date.getMonth() === today.getMonth() &&
    date.getDate() === today.getDate();

  // Today shows a time, anything older shows a date: "14:32" answers "when
  // today", and "12 Aug" answers "how long ago" without the noise of both.
  return sameDay
    ? date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
    : date.toLocaleDateString(undefined, { day: "numeric", month: "short" });
}
