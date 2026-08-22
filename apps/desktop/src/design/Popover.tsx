import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import "./Popover.css";

interface PopoverProps {
  /** The element the popover is positioned against. */
  anchor: HTMLElement | null;
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  /** Horizontal edge of the anchor to align to. */
  align?: "start" | "end";
}

/**
 * A popover rendered into a portal.
 *
 * Portalling is not stylistic here. An absolutely positioned popover is clipped
 * by any ancestor that scrolls, and the terminal tab strip is exactly that — it
 * scrolls horizontally when there are many tabs. Rendered inline, the menu was
 * simply invisible. Escaping to the document root is the only reliable fix.
 *
 * Position is measured from the anchor and re-measured on scroll and resize, so
 * the popover never drifts away from the control that opened it.
 */
export function Popover({ anchor, open, onClose, children, align = "start" }: PopoverProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<{ top: number; left: number } | null>(null);

  // Measure before paint so the popover never appears at the wrong place first.
  useLayoutEffect(() => {
    if (!open || !anchor) {
      setPosition(null);
      return;
    }

    const place = () => {
      const rect = anchor.getBoundingClientRect();
      const width = ref.current?.offsetWidth ?? 0;
      const left = align === "end" ? rect.right - width : rect.left;
      // Keep it inside the window, with a small margin.
      const clamped = Math.max(8, Math.min(left, window.innerWidth - width - 8));
      setPosition({ top: rect.bottom + 6, left: clamped });
    };

    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => {
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
    };
  }, [open, anchor, align]);

  useEffect(() => {
    if (!open) return;

    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as Node;
      // A click on the anchor is the toggle; let the anchor handle it rather
      // than closing here and immediately reopening.
      if (ref.current?.contains(target) || anchor?.contains(target)) return;
      onClose();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };

    window.addEventListener("mousedown", onPointerDown);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("mousedown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [open, anchor, onClose]);

  if (!open) return null;

  return createPortal(
    <div
      ref={ref}
      className="popover"
      role="menu"
      style={{
        top: position?.top ?? -9999,
        left: position?.left ?? -9999,
        // Hidden until measured, so it cannot flash in the wrong spot.
        visibility: position ? "visible" : "hidden",
      }}
    >
      {children}
    </div>,
    document.body,
  );
}
