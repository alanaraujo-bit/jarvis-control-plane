import { useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useT } from "../../app/i18n";
import "./LiveCaption.css";

interface LiveCaptionProps {
  anchor: HTMLElement | null;
  open: boolean;
  committed: string;
  tail: string;
}

/**
 * The floating caption shown while recording (§54 streaming, D31) —
 * "text appearing while speaking", the thing this whole feature was built
 * for. It only ever previews: nothing here is what gets typed into the
 * terminal, so nothing here needs to be exactly right, only responsive.
 *
 * Committed words are rendered one `<span>` per word, keyed by index. Since
 * `committed` only ever grows (see `stream::AgreementState`), React mounts a
 * fresh span only for each newly-agreed word and leaves every earlier one
 * alone — which is what makes the entrance animation play exactly once, on
 * the word that just settled, and never replay on words already on screen.
 * No manual "what's new" tracking needed; the DOM keys do it.
 *
 * Deliberately not built on the shared `Popover` — that closes on any
 * outside click, which is wrong here: a person keeps working (clicking
 * around the terminal, reading a file) while still dictating, and the
 * caption must stay put until the recording itself stops.
 */
export function LiveCaption({ anchor, open, committed, tail }: LiveCaptionProps) {
  const t = useT();
  const ref = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<{ top: number; left: number } | null>(null);

  useLayoutEffect(() => {
    if (!open || !anchor) {
      setPosition(null);
      return;
    }
    const place = () => {
      const rect = anchor.getBoundingClientRect();
      const width = ref.current?.offsetWidth ?? 280;
      const left = Math.max(8, Math.min(rect.right - width, window.innerWidth - width - 8));
      setPosition({ top: rect.bottom + 8, left });
    };
    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => {
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
    };
  }, [open, anchor, committed, tail]);

  if (!open) return null;

  const words = committed.length > 0 ? committed.split(" ") : [];
  const isListening = words.length === 0 && tail.length === 0;

  return createPortal(
    <div
      ref={ref}
      className="live-caption"
      role="status"
      aria-live="polite"
      style={{
        top: position?.top ?? -9999,
        left: position?.left ?? -9999,
        visibility: position ? "visible" : "hidden",
      }}
    >
      {isListening ? (
        <span className="live-caption__listening">
          <span className="live-caption__dot" />
          <span className="live-caption__dot" />
          <span className="live-caption__dot" />
          <span className="live-caption__listening-label">{t("voice.listening")}</span>
        </span>
      ) : (
        <p className="live-caption__text">
          {words.map((word, i) => (
            <span className="live-caption__word" key={i}>
              {word}{" "}
            </span>
          ))}
          {tail.length > 0 && <span className="live-caption__tail">{tail}</span>}
        </p>
      )}
    </div>,
    document.body,
  );
}
