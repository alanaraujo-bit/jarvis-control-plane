/**
 * The root document picks a language and gets out of the way.
 *
 * External rather than inline for the same reason theme.js is: the site's
 * content security policy is `script-src 'self'`, and an inline script here
 * would be blocked in production while working perfectly from `file://` —
 * leaving every visitor to the bare domain on a blank page.
 */
(function () {
  // A language you chose beats the one your browser reports. The switch in the
  // top bar records the choice; this is the half that reads it, and without it
  // that write would be a preference nothing ever honours.
  try {
    var chosen = localStorage.getItem("jarvis-docs-locale");
    if (chosen === "en" || chosen === "pt-BR") {
      location.replace(chosen + "/overview.html");
      return;
    }
  } catch (e) {
    /* Private windows and blocked site data both throw on access. */
  }

  var langs = navigator.languages || [navigator.language || "en"];
  var pt = Array.prototype.some.call(langs, function (l) {
    return String(l).toLowerCase().indexOf("pt") === 0;
  });
  location.replace((pt ? "pt-BR" : "en") + "/overview.html");
})();
