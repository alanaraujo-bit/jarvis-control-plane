import { useEffect, useState } from "react";
import { DEFAULT_CATEGORY, type CategoryId } from "./categories";

/**
 * Which section of Settings is on screen.
 *
 * Kept outside the component for two reasons, and only two:
 *
 * - **Somewhere else asks for a section.** The command palette's "Rescan"
 *   navigates to Settings and then triggers a scan; without this it landed on
 *   Appearance while the thing it had just done was three sections away. Every
 *   palette entry for a section goes through the same door.
 * - **The choice survives leaving.** Coming back to Settings from a project
 *   returns to the section you were last in, which is what a person expects of
 *   a place they have already been.
 *
 * It is deliberately *not* persisted to disk. Where you were last session is
 * not a preference — it is a memory, and one that stops being true the moment
 * the reason you were there is gone.
 *
 * A module store rather than `zustand` because the whole state is one string,
 * which is the same call `usePreferences` made next door.
 */
let current: CategoryId = DEFAULT_CATEGORY;
const listeners = new Set<(id: CategoryId) => void>();

export function openSettingsCategory(id: CategoryId) {
  if (current === id) return;
  current = id;
  for (const listener of listeners) listener(id);
}

export function useSettingsCategory(): [CategoryId, (id: CategoryId) => void] {
  const [id, setId] = useState<CategoryId>(current);

  useEffect(() => {
    listeners.add(setId);
    // The store may have moved between render and subscribe — a palette entry
    // navigates and sets the category in the same tick.
    if (current !== id) setId(current);
    return () => {
      listeners.delete(setId);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return [id, openSettingsCategory];
}
