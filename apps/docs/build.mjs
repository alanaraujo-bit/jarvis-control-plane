#!/usr/bin/env node
/**
 * J.A.R.V.I.S. — documentation site builder.
 *
 * No dependencies, no framework, no watcher (D-B). It reads `nav.json`, reads
 * one HTML partial per page per locale, and writes a static site that opens
 * from `file://` as happily as it serves from a CDN.
 *
 *   node build.mjs            build into dist/
 *   node build.mjs --check    build, then fail loudly on anything broken
 *
 * The build is a verification pass as much as a build. It refuses to finish
 * when the two locale trees disagree, when a figure points at an image that is
 * not on disk, or when a link points at a page that does not exist — the three
 * ways a bilingual, illustrated, cross-linked site rots silently.
 */

import { readFileSync, writeFileSync, mkdirSync, rmSync, cpSync, existsSync, readdirSync, statSync } from "node:fs";
import { join, dirname, relative } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(fileURLToPath(import.meta.url));
const REPO = join(ROOT, "..", "..");
const DIST = join(ROOT, "dist");

const nav = JSON.parse(readFileSync(join(ROOT, "nav.json"), "utf8"));
const LOCALES = nav.locales;
const DEFAULT_LOCALE = nav.defaultLocale;

/** Problems collected as the build runs, reported together at the end. */
const problems = [];
const warn = (msg) => problems.push(msg);

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

const esc = (s) =>
  String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

/** Slugify a heading's own text into an anchor id. Accents folded, not dropped. */
function slugify(text) {
  return text
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/<[^>]+>/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 60);
}

/** Strip tags for the search index and for `<title>`. */
const plain = (html) =>
  html
    .replace(/<script[\s\S]*?<\/script>/g, " ")
    .replace(/<style[\s\S]*?<\/style>/g, " ")
    .replace(/<[^>]+>/g, " ")
    .replace(/&nbsp;/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/\s+/g, " ")
    .trim();

// ---------------------------------------------------------------------------
// The page list, flattened once from nav.json
// ---------------------------------------------------------------------------

const pages = [];
for (const section of nav.sections) {
  for (const page of section.pages) {
    pages.push({ ...page, section });
  }
}
const SLUGS = new Set(pages.map((p) => p.slug));

// Parity is structural, not aspirational (D-D): the same page set must exist in
// both trees, or the build stops before anyone can publish half a site.
for (const locale of LOCALES) {
  for (const page of pages) {
    const file = join(ROOT, "content", locale, `${page.slug}.html`);
    if (!existsSync(file)) warn(`missing partial: content/${locale}/${page.slug}.html`);
    if (!page.title[locale]) warn(`nav.json: ${page.slug} has no ${locale} title`);
  }
  const dir = join(ROOT, "content", locale);
  if (existsSync(dir)) {
    for (const name of readdirSync(dir)) {
      const slug = name.replace(/\.html$/, "");
      if (name.endsWith(".html") && !SLUGS.has(slug)) {
        warn(`orphan partial (not in nav.json): content/${locale}/${name}`);
      }
    }
  }
}

// ---------------------------------------------------------------------------
// Reading a partial: pull out headings, check figures, check links
// ---------------------------------------------------------------------------

/**
 * Headings become both the on-this-page rail and search results of their own,
 * so an id is assigned here rather than being asked of every author.
 */
function processBody(raw, { locale, slug }) {
  const headings = [];
  const seen = new Set();

  let body = raw.replace(/<h([23])(\s[^>]*)?>([\s\S]*?)<\/h\1>/g, (full, level, attrs = "", inner) => {
    const text = plain(inner);
    const explicit = /\sid="([^"]+)"/.exec(attrs || "");
    let id = explicit ? explicit[1] : slugify(text);
    if (!id) id = `s${headings.length + 1}`;
    let unique = id;
    let n = 2;
    while (seen.has(unique)) unique = `${id}-${n++}`;
    seen.add(unique);
    headings.push({ id: unique, text, level: Number(level) });
    const rest = (attrs || "").replace(/\sid="[^"]+"/, "");
    return `<h${level} id="${unique}"${rest}><a class="anchor" href="#${unique}" aria-label="${text}">#</a>${inner}</h${level}>`;
  });

  // Every figure must point at an image that is actually on disk. With 483
  // screenshots being curated down to a few dozen, a broken path is not a
  // hypothetical failure — it is the expected one.
  for (const m of body.matchAll(/<img[^>]+src="([^"]+)"/g)) {
    const src = m[1];
    if (src.startsWith("http") || src.startsWith("data:")) continue;
    const onDisk = join(ROOT, "public", src.replace(/^\.\.\//, "").replace(/^\//, ""));
    if (!existsSync(onDisk)) warn(`${locale}/${slug}: image not on disk — ${src}`);
  }
  for (const m of body.matchAll(/<img(?![^>]*\salt=)[^>]*>/g)) {
    warn(`${locale}/${slug}: <img> without alt — ${m[0].slice(0, 70)}`);
  }

  // Internal links are written as `page:slug` or `page:slug#anchor`, so a page
  // never hardcodes a locale and the checker can see every one of them.
  body = body.replace(/href="page:([a-z0-9-]+)(#[^"]*)?"/g, (full, target, hash = "") => {
    if (!SLUGS.has(target)) warn(`${locale}/${slug}: link to unknown page — ${target}`);
    return `href="${target}.html${hash}"`;
  });

  return { body, headings };
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

const UI = {
  en: {
    search: "Search the documentation",
    searchShort: "Search",
    onThisPage: "On this page",
    prev: "Previous",
    next: "Next",
    theme: "Switch theme",
    lang: "Language",
    close: "Close",
    noResults: "Nothing matched.",
    searchHint: "Type to search. ↑↓ to move, ↵ to open, Esc to close.",
    menu: "Menu",
    edited: "Documentation for J.A.R.V.I.S.",
    skip: "Skip to content",
    home: "Documentation",
  },
  "pt-BR": {
    search: "Pesquisar na documentação",
    searchShort: "Pesquisar",
    onThisPage: "Nesta página",
    prev: "Anterior",
    next: "Próxima",
    theme: "Trocar o tema",
    lang: "Idioma",
    close: "Fechar",
    noResults: "Nada corresponde.",
    searchHint: "Digite para pesquisar. ↑↓ para mover, ↵ para abrir, Esc para fechar.",
    menu: "Menu",
    edited: "Documentação do J.A.R.V.I.S.",
    skip: "Ir para o conteúdo",
    home: "Documentação",
  },
};

function renderSidebar(locale, current) {
  let out = "";
  for (const section of nav.sections) {
    out += `<div class="nav-section"><div class="nav-section__title">${esc(section.title[locale])}</div><ul>`;
    for (const page of section.pages) {
      const active = page.slug === current ? ' class="is-active" aria-current="page"' : "";
      out += `<li><a href="${page.slug}.html"${active}>${esc(page.title[locale])}</a></li>`;
    }
    out += `</ul></div>`;
  }
  return out;
}

function renderToc(headings, locale) {
  if (headings.length < 2) return "";
  const items = headings
    .map((h) => `<li data-level="${h.level}"><a href="#${h.id}">${esc(h.text)}</a></li>`)
    .join("");
  return `<nav class="toc" aria-label="${esc(UI[locale].onThisPage)}">
      <div class="toc__title">${esc(UI[locale].onThisPage)}</div>
      <ul>${items}</ul>
    </nav>`;
}

function renderPage({ locale, page, body, headings, index, version }) {
  const ui = UI[locale];
  const prev = index > 0 ? pages[index - 1] : null;
  const next = index < pages.length - 1 ? pages[index + 1] : null;
  const other = LOCALES.filter((l) => l !== locale);

  const pager = `<nav class="pager">
    ${prev ? `<a class="pager__item" href="${prev.slug}.html" rel="prev"><span>${esc(ui.prev)}</span><strong>${esc(prev.title[locale])}</strong></a>` : `<span></span>`}
    ${next ? `<a class="pager__item pager__item--next" href="${next.slug}.html" rel="next"><span>${esc(ui.next)}</span><strong>${esc(next.title[locale])}</strong></a>` : `<span></span>`}
  </nav>`;

  const langLinks = other
    .map((l) => `<a class="langswitch" href="../${l}/${page.slug}.html" hreflang="${l}">${l === "pt-BR" ? "Português" : "English"}</a>`)
    .join("");

  const alternates = LOCALES.map(
    (l) => `<link rel="alternate" hreflang="${l}" href="../${l}/${page.slug}.html">`,
  ).join("\n  ");

  return `<!doctype html>
<html lang="${locale}" data-page="${page.slug}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${esc(page.title[locale])} — J.A.R.V.I.S.</title>
<meta name="description" content="${esc(page.summary[locale] || "")}">
<meta property="og:title" content="${esc(page.title[locale])} — J.A.R.V.I.S.">
<meta property="og:description" content="${esc(page.summary[locale] || "")}">
<meta property="og:type" content="article">
${alternates}
<link rel="icon" href="../assets/mark.svg" type="image/svg+xml">
<link rel="stylesheet" href="../assets/site.css">
<script>
  /* Applied before first paint: a documentation site that flashes the wrong
     theme is the first thing anyone notices about it. */
  try {
    var t = localStorage.getItem("jarvis-docs-theme");
    if (t === "light" || t === "dark") document.documentElement.dataset.theme = t;
    else document.documentElement.dataset.theme =
      matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  } catch (e) { document.documentElement.dataset.theme = "dark"; }
</script>
</head>
<body>
<a class="skip" href="#content">${esc(ui.skip)}</a>
<header class="topbar">
  <button class="topbar__menu" type="button" aria-label="${esc(ui.menu)}" data-menu>
    <svg viewBox="0 0 16 16" width="17" height="17" aria-hidden="true"><path d="M2 4h12M2 8h12M2 12h12" stroke="currentColor" stroke-width="1.5" fill="none" stroke-linecap="round"/></svg>
  </button>
  <a class="brand" href="index.html">
    <img src="../assets/mark.svg" alt="" width="20" height="20">
    <span class="brand__name">J.A.R.V.I.S.</span>
    <span class="brand__docs">${esc(ui.home)}</span>
  </a>
  <button class="searchbtn" type="button" data-open-search>
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><circle cx="7" cy="7" r="4.5" stroke="currentColor" stroke-width="1.4" fill="none"/><path d="M10.5 10.5 14 14" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>
    <span>${esc(ui.searchShort)}</span>
    <kbd>Ctrl</kbd><kbd>K</kbd>
  </button>
  <div class="topbar__right">
    ${langLinks}
    <button class="iconbtn" type="button" data-theme-toggle aria-label="${esc(ui.theme)}" title="${esc(ui.theme)}">
      <svg class="i-sun" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><circle cx="8" cy="8" r="3.2" fill="currentColor"/><g stroke="currentColor" stroke-width="1.3" stroke-linecap="round"><path d="M8 1v1.8M8 13.2V15M15 8h-1.8M2.8 8H1M12.9 3.1l-1.3 1.3M4.4 11.6l-1.3 1.3M12.9 12.9l-1.3-1.3M4.4 4.4 3.1 3.1"/></g></svg>
      <svg class="i-moon" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><path d="M13.4 9.8A5.8 5.8 0 0 1 6.2 2.6 5.8 5.8 0 1 0 13.4 9.8Z" fill="currentColor"/></svg>
    </button>
    <span class="version">v${esc(version)}</span>
  </div>
</header>

<div class="layout">
  <aside class="sidebar" data-sidebar>
    <nav aria-label="${esc(ui.home)}">${renderSidebar(locale, page.slug)}</nav>
  </aside>
  <main id="content" class="main">
    <article class="prose">
      <div class="page-eyebrow">${esc(page.section.title[locale])}</div>
      <h1>${esc(page.title[locale])}</h1>
      ${page.summary[locale] ? `<p class="lead">${page.summary[locale]}</p>` : ""}
      ${body}
    </article>
    ${pager}
  </main>
  ${renderToc(headings, locale)}
</div>

<div class="scrim" data-scrim hidden></div>

<div class="palette" data-palette hidden role="dialog" aria-modal="true" aria-label="${esc(ui.search)}">
  <div class="palette__box">
    <div class="palette__input">
      <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true"><circle cx="7" cy="7" r="4.5" stroke="currentColor" stroke-width="1.4" fill="none"/><path d="M10.5 10.5 14 14" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>
      <input type="text" data-search-input placeholder="${esc(ui.search)}" autocomplete="off" spellcheck="false" aria-label="${esc(ui.search)}">
      <button class="palette__esc" type="button" data-close-search aria-label="${esc(ui.close)}">Esc</button>
    </div>
    <div class="palette__results" data-results></div>
    <div class="palette__hint">${esc(ui.searchHint)}</div>
  </div>
</div>

<script>window.DOCS_LOCALE=${JSON.stringify(locale)};window.DOCS_UI=${JSON.stringify(ui)};</script>
<script src="./search-index.js"></script>
<script src="../assets/site.js"></script>
</body>
</html>`;
}

// ---------------------------------------------------------------------------
// Build
// ---------------------------------------------------------------------------

const version = JSON.parse(readFileSync(join(REPO, "package.json"), "utf8")).version;

rmSync(DIST, { recursive: true, force: true });
mkdirSync(DIST, { recursive: true });

cpSync(join(ROOT, "assets"), join(DIST, "assets"), { recursive: true });
if (existsSync(join(ROOT, "public"))) cpSync(join(ROOT, "public"), DIST, { recursive: true });

let built = 0;
for (const locale of LOCALES) {
  mkdirSync(join(DIST, locale), { recursive: true });
  const index = [];

  pages.forEach((page, i) => {
    const file = join(ROOT, "content", locale, `${page.slug}.html`);
    if (!existsSync(file)) return;
    const raw = readFileSync(file, "utf8");
    const { body, headings } = processBody(raw, { locale, slug: page.slug });

    writeFileSync(
      join(DIST, locale, `${page.slug}.html`),
      renderPage({ locale, page, body, headings, index: i, version }),
    );
    built++;

    // One index entry per page, plus one per heading. A heading is what people
    // actually search for — "guardrail policy", not "Autonomy and guardrails".
    const text = plain(body);
    index.push({
      s: page.slug,
      t: page.title[locale],
      d: page.summary[locale] || "",
      g: page.section.title[locale],
      b: text.slice(0, 2600),
    });
    for (const h of headings) {
      index.push({ s: page.slug, h: h.id, t: h.text, d: "", g: page.title[locale], b: "" });
    }
  });

  // Inlined as a script, never fetched (D-F): `fetch()` of a local file is
  // blocked under `file://`, and this site has to open from disk.
  writeFileSync(
    join(DIST, locale, "search-index.js"),
    `window.DOCS_INDEX=${JSON.stringify(index)};`,
  );
}

// The root document picks a language and gets out of the way.
writeFileSync(
  join(DIST, "index.html"),
  `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>J.A.R.V.I.S. — Documentation</title>
<link rel="icon" href="./assets/mark.svg" type="image/svg+xml">
<meta name="viewport" content="width=device-width,initial-scale=1">
<style>html{background:#0C0C0D}</style>
<script>
  var pt = (navigator.languages || [navigator.language || "en"]).some(function (l) {
    return String(l).toLowerCase().indexOf("pt") === 0;
  });
  location.replace((pt ? "pt-BR" : "${DEFAULT_LOCALE}") + "/overview.html");
</script>
</head>
<body><noscript><a href="./${DEFAULT_LOCALE}/overview.html">Documentation</a> ·
<a href="./pt-BR/overview.html">Documentação</a></noscript></body></html>`,
);

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

console.log(`built ${built} pages · ${pages.length} in nav · ${LOCALES.join(", ")} · v${version}`);
if (problems.length) {
  console.log(`\n${problems.length} problem(s):`);
  for (const p of problems) console.log(`  - ${p}`);
  if (process.argv.includes("--check")) process.exit(1);
} else {
  console.log("no problems");
}
