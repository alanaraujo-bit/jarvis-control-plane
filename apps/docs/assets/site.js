/* ==========================================================================
   J.A.R.V.I.S. — documentation site behaviour
   --------------------------------------------------------------------------
   No framework and no build step, the same choice the product's own PWA made.
   Four things: the theme, the mobile sidebar, the on-this-page rail, and a
   search overlay that opens on Ctrl+K.

   The search deliberately ranks the way the product's command palette ranks —
   the subsequence scorer in `shell/CommandPalette.tsx`, ported unchanged. It
   is the same interaction in the same product, so typing "mc" should find
   Mission Control in both places.
   ========================================================================== */
(function () {
  "use strict";

  var doc = document;
  var root = doc.documentElement;
  var ui = window.DOCS_UI || {};

  // ---- Theme -------------------------------------------------------------
  var themeBtn = doc.querySelector("[data-theme-toggle]");
  if (themeBtn) {
    themeBtn.addEventListener("click", function () {
      var next = root.dataset.theme === "light" ? "dark" : "light";
      root.dataset.theme = next;
      try {
        localStorage.setItem("jarvis-docs-theme", next);
      } catch (e) {
        /* Private windows and blocked site data both throw here. The toggle
           still works for this page; only the memory of it is lost. */
      }
    });
  }

  // ---- The language switch remembers where you were -----------------------
  // Reading in Portuguese is a preference, not a per-page decision.
  Array.prototype.forEach.call(doc.querySelectorAll(".langswitch"), function (link) {
    link.addEventListener("click", function () {
      try {
        localStorage.setItem("jarvis-docs-locale", link.getAttribute("hreflang"));
      } catch (e) {}
    });
  });

  // ---- Mobile sidebar -----------------------------------------------------
  var sidebar = doc.querySelector("[data-sidebar]");
  var scrim = doc.querySelector("[data-scrim]");
  var menuBtn = doc.querySelector("[data-menu]");
  function closeSidebar() {
    if (!sidebar) return;
    delete sidebar.dataset.open;
    if (scrim && !paletteOpen()) scrim.hidden = true;
  }
  if (menuBtn && sidebar) {
    menuBtn.addEventListener("click", function () {
      if (sidebar.dataset.open) closeSidebar();
      else {
        sidebar.dataset.open = "1";
        if (scrim) scrim.hidden = false;
      }
    });
    sidebar.addEventListener("click", function (e) {
      if (e.target.tagName === "A") closeSidebar();
    });
  }

  // The active sidebar entry is scrolled into view on load. With seven
  // sections the current page is often below the fold, and a navigation rail
  // that does not show where you are is decoration.
  var active = doc.querySelector(".sidebar a.is-active");
  if (active && sidebar) {
    var top = active.offsetTop - sidebar.clientHeight / 2;
    if (top > 0) sidebar.scrollTop = top;
  }

  // ---- On-this-page: highlight the heading you are actually reading -------
  var tocLinks = Array.prototype.slice.call(doc.querySelectorAll(".toc a"));
  if (tocLinks.length && "IntersectionObserver" in window) {
    var byId = {};
    tocLinks.forEach(function (a) {
      byId[a.getAttribute("href").slice(1)] = a;
    });
    var visible = {};
    var observer = new IntersectionObserver(
      function (entries) {
        entries.forEach(function (entry) {
          visible[entry.target.id] = entry.isIntersecting;
        });
        var current = null;
        Object.keys(byId).forEach(function (id) {
          if (visible[id] && !current) current = id;
        });
        tocLinks.forEach(function (a) {
          a.classList.toggle("is-current", a.getAttribute("href") === "#" + current);
        });
      },
      // A band across the upper third: a heading counts as "where you are"
      // once it has passed under the sticky bar, not when it first appears.
      { rootMargin: "-" + (54 + 10) + "px 0px -68% 0px", threshold: 0 },
    );
    Object.keys(byId).forEach(function (id) {
      var el = doc.getElementById(id);
      if (el) observer.observe(el);
    });
  }

  // ---- Every table gets a scroll container --------------------------------
  // A capability matrix is wider than a reading column, and the page body must
  // never scroll sideways.
  Array.prototype.forEach.call(doc.querySelectorAll(".prose table"), function (table) {
    if (table.parentElement.classList.contains("tablewrap")) return;
    var wrap = doc.createElement("div");
    wrap.className = "tablewrap";
    table.parentNode.insertBefore(wrap, table);
    wrap.appendChild(table);
  });

  // ---- Search -------------------------------------------------------------
  var palette = doc.querySelector("[data-palette]");
  var input = doc.querySelector("[data-search-input]");
  var results = doc.querySelector("[data-results]");
  var index = window.DOCS_INDEX || [];
  var selected = 0;
  var current = [];

  function paletteOpen() {
    return palette && !palette.hidden;
  }

  /**
   * Ported from `score()` in the product's command palette (§50). Contiguous
   * runs and word-start matches score higher, and no match returns null so
   * filtering and ranking stay one pass.
   */
  function score(haystack, title, needle) {
    if (!needle) return 0;
    var hay = 0;
    var total = 0;
    var streak = 0;
    for (var i = 0; i < needle.length; i++) {
      var ch = needle[i];
      if (ch === " ") continue;
      var found = haystack.indexOf(ch, hay);
      if (found === -1) return null;
      var atWordStart = found === 0 || haystack[found - 1] === " ";
      streak = found === hay ? streak + 1 : 0;
      total += 1 + streak * 2 + (atWordStart ? 3 : 0);
      hay = found + 1;
    }
    return total - title.length * 0.01;
  }

  var fold = function (s) {
    return s.toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "");
  };

  // Folded once at load rather than per keystroke. Portuguese makes this
  // load-bearing rather than an optimisation: "sessão" must be found by
  // "sessao", and by "SESSÃO".
  index.forEach(function (row) {
    row._t = fold(row.t);
    row._d = fold(row.d || "");
    row._b = fold(row.b || "");
    row._g = fold(row.g || "");
  });

  function excerpt(row, needle) {
    var body = row.b || row.d || "";
    if (!needle) return row.d || "";
    var at = row._b.indexOf(needle);
    if (at === -1) return row.d || body.slice(0, 120);
    var from = Math.max(0, at - 42);
    var text = body.slice(from, from + 160);
    return (from > 0 ? "…" : "") + text + "…";
  }

  function escapeHtml(s) {
    return String(s).replace(/[&<>"]/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c];
    });
  }

  function search(query) {
    var needle = fold(query.trim());
    if (!needle) {
      // With nothing typed, the useful default is the page list itself —
      // never an empty box that makes the reader guess what is searchable.
      return index
        .filter(function (r) {
          return !r.h;
        })
        .slice(0, 40);
    }
    var out = [];
    for (var i = 0; i < index.length; i++) {
      var row = index[i];
      // Title, section and summary first; the body only if none of them match.
      // A body hit on a 2,600-character page is a weaker answer than a title
      // hit, and scoring them together buries the strong one.
      var head = row._t + " " + row._g + " " + row._d;
      var s = score(head, row.t, needle);
      var where = "head";
      if (s === null && row._b) {
        s = score(row._b, row.t, needle);
        where = "body";
        if (s !== null) s = s * 0.35;
      }
      if (s === null) continue;
      if (row._t.indexOf(needle) === 0) s += 24; // an exact prefix is what you meant
      else if (row._t.indexOf(needle) !== -1) s += 12;
      if (row.h) s -= 3; // a heading sits just under its own page
      out.push({ row: row, rank: s, where: where, needle: needle });
    }
    out.sort(function (a, b) {
      return b.rank - a.rank;
    });
    return out.slice(0, 40).map(function (e) {
      e.row._where = e.where;
      e.row._needle = e.needle;
      return e.row;
    });
  }

  function render(rows) {
    current = rows;
    selected = 0;
    if (!rows.length) {
      results.innerHTML = '<div class="palette__empty">' + escapeHtml(ui.noResults || "No results") + "</div>";
      return;
    }
    var html = "";
    var lastGroup = null;
    rows.forEach(function (row, i) {
      var group = row.h ? row.g : row.g;
      if (group !== lastGroup) {
        html += '<div class="palette__group">' + escapeHtml(group) + "</div>";
        lastGroup = group;
      }
      var href = row.s + ".html" + (row.h ? "#" + row.h : "");
      var sub = row._where === "body" ? excerpt(row, row._needle) : row.d || "";
      html +=
        '<a class="hit" href="' +
        href +
        '" role="option" aria-selected="' +
        (i === 0) +
        '"><strong>' +
        escapeHtml(row.t) +
        "</strong>" +
        (sub ? "<small>" + escapeHtml(sub) + "</small>" : "") +
        "</a>";
    });
    results.innerHTML = html;
  }

  function move(delta) {
    var items = results.querySelectorAll(".hit");
    if (!items.length) return;
    items[selected] && items[selected].setAttribute("aria-selected", "false");
    selected = (selected + delta + items.length) % items.length;
    items[selected].setAttribute("aria-selected", "true");
    items[selected].scrollIntoView({ block: "nearest" });
  }

  function openSearch() {
    if (!palette) return;
    closeSidebar();
    palette.hidden = false;
    if (scrim) scrim.hidden = false;
    input.value = "";
    render(search(""));
    // After paint: the element is not focusable until it is in the document.
    requestAnimationFrame(function () {
      input.focus();
    });
  }

  function closeSearch() {
    if (!palette) return;
    palette.hidden = true;
    if (scrim && !(sidebar && sidebar.dataset.open)) scrim.hidden = true;
  }

  if (palette) {
    input.addEventListener("input", function () {
      render(search(input.value));
    });

    palette.addEventListener("keydown", function (e) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        move(1);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        move(-1);
      } else if (e.key === "Enter") {
        var item = results.querySelectorAll(".hit")[selected];
        if (item) {
          e.preventDefault();
          location.href = item.getAttribute("href");
        }
      } else if (e.key === "Escape") {
        e.preventDefault();
        closeSearch();
      }
    });

    Array.prototype.forEach.call(doc.querySelectorAll("[data-open-search]"), function (b) {
      b.addEventListener("click", openSearch);
    });
    Array.prototype.forEach.call(doc.querySelectorAll("[data-close-search]"), function (b) {
      b.addEventListener("click", closeSearch);
    });
    if (scrim) {
      scrim.addEventListener("click", function () {
        closeSearch();
        closeSidebar();
      });
    }

    // Capture phase, for the same reason `App.tsx` resolves Ctrl+K there: a
    // shortcut the interface advertises must be resolved before any widget
    // gets an opinion about it.
    window.addEventListener(
      "keydown",
      function (e) {
        if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
          e.preventDefault();
          e.stopPropagation();
          paletteOpen() ? closeSearch() : openSearch();
        } else if (e.key === "/" && !paletteOpen() && !/^(INPUT|TEXTAREA)$/.test(doc.activeElement.tagName)) {
          e.preventDefault();
          openSearch();
        } else if (e.key === "Escape") {
          closeSearch();
          closeSidebar();
        }
      },
      { capture: true },
    );
  }
})();
