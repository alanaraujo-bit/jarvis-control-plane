/**
 * How old a snapshot is, in words (§28).
 *
 * Its own module so it can be tested. This is the first line on the companion
 * screen and the one that decides whether the rest of it can be trusted — a
 * phone showing an hour-old reading as if it were current is worse than one
 * that says plainly it has not heard anything, because the whole value of the
 * screen is being able to believe it at a glance.
 *
 * `now` is a parameter rather than a call to `Date.now()` inside, which is the
 * only reason the boundaries below can be tested at all.
 */
export function freshness(snapshot, now = Date.now()) {
  if (!snapshot) return { text: "Sem contato com o desktop", stale: true };

  const age = Math.max(0, now - new Date(snapshot.freshness.capturedAt).getTime());
  const seconds = Math.round(age / 1000);
  const stale = seconds > snapshot.freshness.staleAfterSeconds;

  if (stale) {
    const minutes = Math.round(seconds / 60);
    return { text: `Sem contato há ${minutes} min — pode estar desatualizado`, stale: true };
  }
  // Under a minute and a half reads as "now": a phone polls every fifteen
  // seconds, and "atualizado há 0 min" is noise rather than information.
  if (seconds < 90) return { text: "Atualizado agora", stale: false };
  return { text: `Atualizado há ${Math.round(seconds / 60)} min`, stale: false };
}
