import { useEffect, useRef } from "react";

/**
 * Re-read when a project area actually comes into view.
 *
 * ## The bug this exists for
 *
 * Files, Review, Worktrees and the Brain are **mounted once and then hidden
 * with CSS**, so that returning to one keeps its open file, its scroll position
 * and its selected diff. That is deliberate and worth keeping.
 *
 * The consequence is not obvious and was wrong in three surfaces at once: a
 * `useEffect` keyed on the project id fires when the component mounts and never
 * again. Review carried the comment *"Re-read on every visit. An agent may have
 * been working the whole time the user was on another surface, and a stale diff
 * is worse than a slow one"* directly above an effect that did not do that.
 *
 * Nothing errored. The surface simply showed what had been true the first time
 * it was opened. Found by running a real agent, going back to the Brain, and
 * seeing "nothing has happened in this project yet" over a project where
 * something had just happened.
 *
 * `active` flipping from false to true is the signal a mounted-and-hidden
 * component has instead of a mount, so that is what this watches. The first
 * activation is included — an area opened for the first time still needs its
 * initial read.
 */
export function useVisitRefresh(active: boolean, refresh: () => void) {
  // Kept in a ref so a caller passing an inline arrow does not re-trigger on
  // every render — which would turn "read when shown" into "read constantly".
  const latest = useRef(refresh);
  latest.current = refresh;

  const wasActive = useRef(false);

  useEffect(() => {
    if (active && !wasActive.current) latest.current();
    wasActive.current = active;
  }, [active]);
}
