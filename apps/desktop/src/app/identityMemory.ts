import { invoke, isTauri } from "./platform";

/**
 * Store one preference against whoever is signed in (M20 §5).
 *
 * It lives in `app/` rather than in the identity surface on purpose. The three
 * callers — the theme store, the locale provider and `usePreferences` — are all
 * older than accounts and none of them should have to import a surface to save
 * a value; a module that did would also close a cycle (`app/theme` →
 * `surfaces/identity` → `app/theme`).
 *
 * Fire-and-forget, and a no-op when nobody is signed in — the core decides
 * that, so no caller has to ask first. A failure here is never allowed to be
 * the reason a preference does not apply: the value is already stored where the
 * product actually reads it, and this is the copy that follows the person to
 * the next machine.
 */
export function remember(key: string, value: unknown): void {
  if (!isTauri()) return;
  void invoke("identity_remember", { key, value }).catch(() => {
    // Deliberately silent. See above.
  });
}
