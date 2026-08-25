import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Check,
  Copy,
  CornerDownLeft,
  FolderPlus,
  Files,
  Pin,
  PinOff,
  Plus,
  Search,
  SquarePen,
  Trash2,
  X,
} from "lucide-react";
import type { MessageKey } from "@jarvis/i18n";
import { useI18n, useT } from "../../app/i18n";
import { hasLiveTerminal, pasteIntoSession } from "../terminal/live";
import { noteName, useNotebook, type NotebookNote } from "./useNotebook";
import "./Notebook.css";

/**
 * The Notebook (M19).
 *
 * Alan kept his prompts in WhatsApp messages to himself. This is where they
 * live instead — his own library of ideas and prompts, one shortcut away from
 * anywhere, and openable *over* a working agent rather than instead of it.
 *
 * ## Why an overlay rather than a rail destination
 *
 * The request was explicit: reach it without leaving the terminal. A rail
 * destination unmounts the terminal you were watching; a side panel reflows it,
 * which resizes the PTY and sends a resize storm through a running agent — the
 * same constraint that shaped split panes (§20). An overlay changes nothing
 * behind it, and closing it returns you to a screen that never moved.
 *
 * ## Three panes, and why the middle one exists
 *
 * Folders, notes, editor. The middle column is not decoration: a prompt library
 * is *found* rather than browsed, so the note list is always visible and always
 * filtered by the same search field. Everything is already in memory (see
 * `notebook::report`), so filtering costs nothing and never shows a spinner.
 *
 * ## The one thing a notes app cannot do
 *
 * **Send to agent.** The note goes into the prompt of the terminal on screen,
 * through `pasteIntoSession` — xterm's own paste path, which knows whether that
 * particular terminal accepts bracketed paste and therefore whether a
 * multi-paragraph prompt survives with its structure. It is never submitted:
 * that key belongs to the person, exactly as §54 decided for dictation.
 *
 * When there is no terminal on screen the button is **absent with a sentence**
 * rather than present and dead — the rule this product follows everywhere a
 * capability is missing (§81).
 */
interface NotebookProps {
  open: boolean;
  onClose: () => void;
  /** The session the Send button would type into, when one is on screen. */
  target: { sessionId: string; agent: string } | null;
}

export function Notebook({ open, onClose, target }: NotebookProps) {
  const t = useT();
  const { locale } = useI18n();
  const report = useNotebook((s) => s.report);
  const load = useNotebook((s) => s.load);
  const flush = useNotebook((s) => s.flush);
  const selectedId = useNotebook((s) => s.selectedId);
  const select = useNotebook((s) => s.select);
  const createNote = useNotebook((s) => s.createNote);
  const createNotebook = useNotebook((s) => s.createNotebook);

  const [query, setQuery] = useState("");
  /** `undefined` is every note; `null` is the unfiled ones. */
  const [folder, setFolder] = useState<string | null | undefined>(undefined);
  const [adding, setAdding] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) return;
    void load();
    requestAnimationFrame(() => searchRef.current?.focus());
  }, [open, load]);

  /**
   * Close through here, never by calling `onClose` directly.
   *
   * Every exit has to write the pending draft first, and there are three of
   * them — Escape, the button, the backdrop. A notes app that loses the last
   * sentence somebody typed before pressing Escape is worse than one with a
   * Save button.
   */
  const close = useCallback(() => {
    void flush().finally(onClose);
  }, [flush, onClose]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      // A folder being named owns Escape first.
      //
      // This listener is capture-phase and stops propagation — which it has to
      // be, so Escape works while the terminal behind holds focus. But capture
      // runs before React's delegated handlers, so the cancel written on the
      // folder inputs was unreachable: pressing Escape while naming a folder
      // closed the whole notebook instead. `TerminalView` guards its own
      // Escape against `findOpen` for exactly this reason.
      if ((event.target as HTMLElement | null)?.classList.contains("nb__folder-input")) return;
      event.preventDefault();
      event.stopPropagation();
      close();
    };
    window.addEventListener("keydown", onKeyDown, { capture: true });
    return () => window.removeEventListener("keydown", onKeyDown, { capture: true });
  }, [open, close]);

  const notes = report?.notes ?? [];
  const folders = report?.notebooks ?? [];

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return notes.filter((note) => {
      if (folder !== undefined && note.notebookId !== folder) return false;
      if (!needle) return true;
      // Title and body together: a prompt is usually found by a phrase inside
      // it rather than by what it was called.
      return (
        note.title.toLowerCase().includes(needle) || note.body.toLowerCase().includes(needle)
      );
    });
  }, [notes, folder, query]);

  const selected = notes.find((note) => note.id === selectedId) ?? null;

  const countIn = (id: string | null) =>
    notes.filter((note) => note.notebookId === id).length;

  if (!open) return null;

  return (
    <div className="nb__scrim" onMouseDown={close} role="presentation">
      <div
        className="nb"
        role="dialog"
        aria-modal="true"
        aria-label={t("notebook.title")}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="nb__head">
          <div className="nb__field">
            <Search size={14} strokeWidth={2} aria-hidden="true" />
            <input
              ref={searchRef}
              className="nb__search"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t("notebook.search")}
              spellCheck={false}
              autoComplete="off"
            />
          </div>
          <button
            type="button"
            className="nb__icon-button"
            onClick={close}
            aria-label={t("notebook.close")}
            title={t("notebook.close")}
          >
            <X size={14} strokeWidth={2} aria-hidden="true" />
          </button>
        </header>

        <div className="nb__body">
          <nav className="nb__folders" aria-label={t("notebook.notebooks")}>
            <FolderRow
              label={t("notebook.all")}
              count={notes.length}
              active={folder === undefined}
              onClick={() => setFolder(undefined)}
            />
            <FolderRow
              label={t("notebook.unfiled")}
              count={countIn(null)}
              active={folder === null}
              onClick={() => setFolder(null)}
            />

            {folders.length > 0 && <div className="nb__folders-rule" />}

            {folders.map((book) => (
              <FolderRow
                key={book.id}
                label={book.name}
                count={countIn(book.id)}
                active={folder === book.id}
                onClick={() => setFolder(book.id)}
                notebookId={book.id}
              />
            ))}

            {adding ? (
              <NewFolder
                onCancel={() => setAdding(false)}
                onCreate={async (name) => {
                  await createNotebook(name);
                  setAdding(false);
                }}
              />
            ) : (
              <button type="button" className="nb__add-folder" onClick={() => setAdding(true)}>
                <FolderPlus size={13} strokeWidth={1.9} aria-hidden="true" />
                {t("notebook.newNotebook")}
              </button>
            )}
          </nav>

          <div className="nb__list">
            <div className="nb__list-head">
              <span className="nb__list-count">
                {t("notebook.count", { count: visible.length })}
              </span>
              <button
                type="button"
                className="nb__new"
                // A new note lands in the folder being looked at. Creating one
                // while filtered to "Prompts" and having it appear in Unfiled
                // is the kind of small betrayal that stops a tool being used.
                onClick={() => void createNote(folder === undefined ? null : folder)}
              >
                <Plus size={13} strokeWidth={2.2} aria-hidden="true" />
                {t("notebook.newNote")}
              </button>
            </div>

            {visible.length === 0 ? (
              <div className="nb__empty">
                {query.trim() ? (
                  <p className="nb__empty-body">
                    {t("notebook.searchEmpty", { query: query.trim() })}
                  </p>
                ) : (
                  <>
                    <p className="nb__empty-title">{t("notebook.empty.title")}</p>
                    <p className="nb__empty-body">{t("notebook.empty.body")}</p>
                  </>
                )}
              </div>
            ) : (
              <ul className="nb__notes">
                {visible.map((note) => (
                  <li key={note.id}>
                    <button
                      type="button"
                      className="nb__note"
                      data-active={note.id === selectedId || undefined}
                      onClick={() => void select(note.id)}
                    >
                      <span className="nb__note-line">
                        {note.pinned && (
                          <Pin size={10} strokeWidth={2.2} aria-hidden="true" />
                        )}
                        <span className="nb__note-name">
                          {noteName(note, t("notebook.untitled"))}
                        </span>
                      </span>
                      <span className="nb__note-preview">{preview(note)}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>

          {selected ? (
            <Editor
              key={selected.id}
              note={selected}
              target={target}
              locale={locale}
              onSent={close}
            />
          ) : (
            <div className="nb__editor nb__editor--empty">
              <p className="nb__empty-body">{t("notebook.noneSelected")}</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

/**
 * What the footer says after an action with no visible result of its own.
 *
 * A successful send is absent from this map on purpose: it closes the overlay,
 * and the words landing in the prompt are a better confirmation than a tick on
 * a panel that is disappearing.
 */
const SAID = {
  copied: "notebook.copied",
  noTerminal: "notebook.send.failed",
  wouldSubmit: "notebook.send.wouldSubmit",
} as const satisfies Record<string, MessageKey>;

/** The second line of a row: the body minus whatever the name already showed. */
function preview(note: NotebookNote): string {
  const lines = note.body.split("\n").filter((line) => line.trim().length > 0);
  const rest = note.title.trim() ? lines : lines.slice(1);
  return rest.join(" ").slice(0, 120);
}

function FolderRow({
  label,
  count,
  active,
  onClick,
  notebookId,
}: {
  label: string;
  count: number;
  active: boolean;
  onClick: () => void;
  /** Present only for a real folder — the two built-in rows cannot be edited. */
  notebookId?: string;
}) {
  const t = useT();
  const rename = useNotebook((s) => s.renameNotebook);
  const remove = useNotebook((s) => s.deleteNotebook);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(label);

  if (editing && notebookId) {
    return (
      <form
        className="nb__folder-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (draft.trim()) void rename(notebookId, draft);
          setEditing(false);
        }}
      >
        <input
          autoFocus
          className="nb__folder-input"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={() => setEditing(false)}
          onKeyDown={(event) => {
            if (event.key === "Escape") setEditing(false);
          }}
        />
      </form>
    );
  }

  return (
    <div className="nb__folder-row" data-active={active || undefined}>
      <button type="button" className="nb__folder" onClick={onClick}>
        <span className="nb__folder-name">{label}</span>
        <span className="nb__folder-count">{count}</span>
      </button>

      {notebookId && (
        <span className="nb__folder-actions">
          <button
            type="button"
            className="nb__icon-button nb__icon-button--tiny"
            onClick={() => {
              setDraft(label);
              setEditing(true);
            }}
            aria-label={t("notebook.rename")}
            title={t("notebook.rename")}
          >
            <SquarePen size={11} strokeWidth={2} aria-hidden="true" />
          </button>
          <button
            type="button"
            className="nb__icon-button nb__icon-button--tiny"
            onClick={() => {
              // Says where the notes go rather than asking whether to destroy
              // them — deleting a folder never deletes what is in it.
              const message =
                count === 0
                  ? t("notebook.deleteNotebook.confirmEmpty", { name: label })
                  : t("notebook.deleteNotebook.confirm", { name: label, count });
              if (window.confirm(message)) void remove(notebookId);
            }}
            aria-label={t("notebook.deleteNotebook")}
            title={t("notebook.deleteNotebook")}
          >
            <Trash2 size={11} strokeWidth={2} aria-hidden="true" />
          </button>
        </span>
      )}
    </div>
  );
}

function NewFolder({
  onCreate,
  onCancel,
}: {
  onCreate: (name: string) => Promise<void>;
  onCancel: () => void;
}) {
  const t = useT();
  const [name, setName] = useState("");
  // Enter submits and blur also commits, and a folder is a real row in a real
  // table — so the two paths are latched against each other rather than left to
  // race. Observed to create one folder in practice; a guard costs nothing and
  // the failure it prevents is a duplicate somebody has to notice and clean up.
  const done = useRef(false);
  const commit = (value: string) => {
    if (done.current) return;
    done.current = true;
    if (value.trim()) void onCreate(value);
    else onCancel();
  };

  return (
    <form
      className="nb__folder-form"
      onSubmit={(event) => {
        event.preventDefault();
        commit(name);
      }}
    >
      <input
        autoFocus
        className="nb__folder-input"
        value={name}
        placeholder={t("notebook.newNotebook.placeholder")}
        onChange={(event) => setName(event.target.value)}
        onBlur={() => commit(name)}
        onKeyDown={(event) => {
          if (event.key === "Escape") onCancel();
        }}
      />
    </form>
  );
}

function Editor({
  note,
  target,
  locale,
  onSent,
}: {
  note: NotebookNote;
  target: { sessionId: string; agent: string } | null;
  locale: string;
  /**
   * Closes the overlay after a successful send.
   *
   * Not a nicety.  focuses the terminal so the words land
   * where they belong, and leaving the overlay up would put keyboard focus
   * behind its own scrim — every keystroke afterwards going invisibly into the
   * PTY, and Escape reaching this component instead of the terminal. Sending is
   * also the moment you want to look at the agent, so closing is what somebody
   * would have done next anyway.
   */
  onSent: () => void;
}) {
  const t = useT();
  const draftTitle = useNotebook((s) => s.draftTitle);
  const draftBody = useNotebook((s) => s.draftBody);
  const edit = useNotebook((s) => s.edit);
  const pin = useNotebook((s) => s.pinNote);
  const move = useNotebook((s) => s.moveNote);
  const duplicate = useNotebook((s) => s.duplicateNote);
  const remove = useNotebook((s) => s.deleteNote);
  const folders = useNotebook((s) => s.report?.notebooks ?? []);

  /** A short-lived confirmation, for the two actions with no visible result. */
  const [said, setSaid] = useState<"copied" | "noTerminal" | "wouldSubmit" | null>(null);
  useEffect(() => {
    if (!said) return;
    const id = window.setTimeout(() => setSaid(null), 2600);
    return () => window.clearTimeout(id);
  }, [said]);

  const body = draftBody;
  // The Send button is offered only when it can actually do something, and
  // `hasLiveTerminal` is asked at render rather than trusted from a prop: a
  // terminal can be closed while this overlay is open.
  const canSend = target !== null && hasLiveTerminal(target.sessionId) && body.trim().length > 0;

  const send = () => {
    if (!target) return;
    const outcome = pasteIntoSession(target.sessionId, body);
    if (outcome === "sent") {
      // The words appearing in the prompt are the confirmation, and they are a
      // more honest one than a tick in a panel that is about to disappear.
      onSent();
      return;
    }
    setSaid(outcome);
  };

  return (
    <div className="nb__editor">
      <div className="nb__editor-head">
        <input
          className="nb__title"
          value={draftTitle}
          placeholder={t("notebook.titlePlaceholder")}
          onChange={(event) => edit(event.target.value, draftBody)}
          spellCheck={false}
        />

        <div className="nb__actions">
          <button
            type="button"
            className="nb__icon-button"
            onClick={() => void pin(note.id, !note.pinned)}
            aria-label={note.pinned ? t("notebook.unpin") : t("notebook.pin")}
            title={note.pinned ? t("notebook.unpin") : t("notebook.pin")}
            data-on={note.pinned || undefined}
          >
            {note.pinned ? (
              <PinOff size={13} strokeWidth={1.9} aria-hidden="true" />
            ) : (
              <Pin size={13} strokeWidth={1.9} aria-hidden="true" />
            )}
          </button>

          <button
            type="button"
            className="nb__icon-button"
            onClick={() => {
              void navigator.clipboard.writeText(body).catch(() => {});
              setSaid("copied");
            }}
            aria-label={t("notebook.copy")}
            title={t("notebook.copy")}
          >
            <Copy size={13} strokeWidth={1.9} aria-hidden="true" />
          </button>

          <button
            type="button"
            className="nb__icon-button"
            onClick={() => void duplicate(note.id)}
            aria-label={t("notebook.duplicate")}
            title={t("notebook.duplicate")}
          >
            <Files size={13} strokeWidth={1.9} aria-hidden="true" />
          </button>

          <select
            className="nb__move"
            value={note.notebookId ?? ""}
            aria-label={t("notebook.moveTo")}
            title={t("notebook.moveTo")}
            onChange={(event) => void move(note.id, event.target.value || null)}
          >
            <option value="">{t("notebook.unfiled")}</option>
            {folders.map((book) => (
              <option key={book.id} value={book.id}>
                {book.name}
              </option>
            ))}
          </select>

          <button
            type="button"
            className="nb__icon-button nb__icon-button--danger"
            onClick={() => {
              const name = noteName(note, t("notebook.untitled"));
              if (window.confirm(t("notebook.deleteNote.confirm", { name }))) {
                void remove(note.id);
              }
            }}
            aria-label={t("notebook.deleteNote")}
            title={t("notebook.deleteNote")}
          >
            <Trash2 size={13} strokeWidth={1.9} aria-hidden="true" />
          </button>
        </div>
      </div>

      <textarea
        className="nb__body-input"
        value={draftBody}
        placeholder={t("notebook.bodyPlaceholder")}
        onChange={(event) => edit(draftTitle, event.target.value)}
        spellCheck={false}
      />

      <footer className="nb__foot">
        <span className="nb__edited">
          {t("notebook.edited", { when: when(note.updatedAt, locale) })}
        </span>

        {said && (
          <span className="nb__said" data-tone={said === "copied" ? undefined : "warn"}>
            {said === "copied" && <Check size={12} strokeWidth={2.4} aria-hidden="true" />}
            {t(SAID[said])}
          </span>
        )}

        {canSend ? (
          <button type="button" className="nb__send" onClick={send}>
            <CornerDownLeft size={13} strokeWidth={2} aria-hidden="true" />
            {t("notebook.send", { agent: target!.agent })}
          </button>
        ) : (
          // Absent with a reason rather than present and dead (§81).
          target === null && <span className="nb__no-session">{t("notebook.send.noSession")}</span>
        )}
      </footer>
    </div>
  );
}

function when(tsMs: number, locale: string): string {
  const date = new Date(tsMs);
  const sameDay = date.toDateString() === new Date().toDateString();
  return sameDay
    ? date.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" })
    : date.toLocaleString(locale, {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      });
}
