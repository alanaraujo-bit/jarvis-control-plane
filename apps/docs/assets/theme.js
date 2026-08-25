/**
 * Applied before first paint. Loaded synchronously from <head> rather than
 * inlined, so the site needs no 'unsafe-inline' in its content security
 * policy — the whole page is static, and a static page has no reason to allow
 * inline script.
 *
 * A documentation site that flashes the wrong theme is the first thing anyone
 * notices about it, which is why this is not deferred.
 *
 * A "theme" query parameter wins over everything, so a link can carry the
 * theme it was meant to be read in — and so this site's own screenshots can be
 * taken without a preference from one run leaking into the next.
 */
(function () {
  var root = document.documentElement;
  try {
    var q = new URLSearchParams(location.search).get("theme");
    if (q === "light" || q === "dark") {
      root.dataset.theme = q;
      return;
    }
  } catch (e) {
    /* URLSearchParams is everywhere this site runs, but a throw here must not
       cost the reader a theme. */
  }
  try {
    var stored = localStorage.getItem("jarvis-docs-theme");
    if (stored === "light" || stored === "dark") {
      root.dataset.theme = stored;
      return;
    }
  } catch (e) {
    /* Private windows and blocked site data both throw on access. */
  }
  root.dataset.theme = matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
})();
