import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { ArrowUp, Square } from "lucide-react";
import type { ChatSnapshot } from "../../app/chat";
import { useT } from "../../app/i18n";
import { registerComposer, unregisterComposer } from "../terminal/live";
import { useVoice } from "../voice/useVoice";
import { ModelPicker } from "./ModelPicker";

/** The tallest the box grows before it scrolls, as a share of the window. */
const MAX_HEIGHT_SHARE = 0.4;

/**
 * Where the person writes (M29). Enter sends, Shift+Enter breaks the line.
 * Sending while the agent works is allowed — Claude queues the message and
 * Codex steers the running turn — so the button only becomes Stop when there
 * is nothing to send.
 */
export function Composer({
  sessionId,
  snapshot,
  autoFocus,
  onSend,
  onStop,
  onModel,
  onEffort,
}: {
  sessionId: string;
  snapshot: ChatSnapshot;
  autoFocus: boolean;
  onSend: (text: string) => void;
  onStop: () => void;
  onModel: (id: string) => void;
  onEffort: (effort: string) => void;
}) {
  const t = useT();
  const [text, setText] = useState("");
  const box = useRef<HTMLTextAreaElement>(null);

  // Grow with the text, up to a share of the window.
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, window.innerHeight * MAX_HEIGHT_SHARE)}px`;
  }, [text]);

  useEffect(() => {
    if (autoFocus) box.current?.focus();
  }, [autoFocus]);

  // Text from elsewhere — the Notebook, dictation — lands at the cursor,
  // never sent (§54).
  const insert = useCallback((incoming: string) => {
    const el = box.current;
    setText((current) => {
      const start = el?.selectionStart ?? current.length;
      const end = el?.selectionEnd ?? current.length;
      return current.slice(0, start) + incoming + current.slice(end);
    });
    el?.focus();
  }, []);

  useEffect(() => {
    registerComposer(sessionId, insert);
    return () => unregisterComposer(sessionId);
  }, [sessionId, insert]);

  const transcript = useVoice((s) => (s.lastTranscriptSession === sessionId ? s.lastTranscript : null));
  const clearTranscript = useVoice((s) => s.clearTranscript);
  useEffect(() => {
    if (!transcript) return;
    insert(transcript);
    clearTranscript();
  }, [transcript, insert, clearTranscript]);

  const send = () => {
    const message = text.trim();
    if (!message || snapshot.ended) return;
    onSend(message);
    setText("");
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    // An IME composing a character is not somebody pressing Send.
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      send();
    }
  };

  const stopping = snapshot.busy && !text.trim();

  return (
    <div className="chat-composer">
      <textarea
        ref={box}
        className="chat-composer__input"
        value={text}
        rows={1}
        placeholder={t("chat.placeholder")}
        aria-label={t("chat.placeholder")}
        onChange={(event) => setText(event.target.value)}
        onKeyDown={onKeyDown}
      />
      <div className="chat-composer__bar">
        <ModelPicker
          models={snapshot.models}
          model={snapshot.model}
          effort={snapshot.effort}
          disabled={!snapshot.ready}
          onModel={onModel}
          onEffort={onEffort}
        />
        <button
          type="button"
          className="chat-composer__send"
          data-stop={stopping || undefined}
          onClick={stopping ? onStop : send}
          disabled={!stopping && !text.trim()}
          aria-label={stopping ? t("chat.stop") : t("chat.send")}
          title={stopping ? t("chat.stop") : t("chat.send")}
        >
          {stopping ? (
            <Square size={11} strokeWidth={0} fill="currentColor" aria-hidden="true" />
          ) : (
            <ArrowUp size={15} strokeWidth={2.2} aria-hidden="true" />
          )}
        </button>
      </div>
    </div>
  );
}
