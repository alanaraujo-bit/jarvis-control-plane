#!/usr/bin/env node
/**
 * Audit the built site for the failures that survive a green build.
 *
 * `build.mjs` already refuses to finish on a missing partial, a missing image,
 * an `<img>` without alt text, or a link to a page that does not exist. This
 * looks for the next tier down: things that are structurally fine and still
 * wrong for a reader.
 *
 *   node audit.mjs
 */

import { readFileSync, readdirSync, existsSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(fileURLToPath(import.meta.url));
const DIST = join(ROOT, "dist");
const nav = JSON.parse(readFileSync(join(ROOT, "nav.json"), "utf8"));

let problems = 0;
const flag = (msg) => {
  console.log(`  ! ${msg}`);
  problems++;
};

const plain = (html) =>
  html
    .replace(/<[^>]+>/g, " ")
    .replace(/&[a-z]+;/g, " ")
    .replace(/\s+/g, " ")
    .trim();

const pages = nav.sections.flatMap((s) => s.pages.map((p) => ({ ...p, section: s })));

// ---------------------------------------------------------------------------
// 1. Every anchor a page links to must exist on the page it points at.
//    A link to `guardrails.html#the-nine` that silently lands at the top is
//    worse than no link: the reader believes they arrived.
// ---------------------------------------------------------------------------
console.log("anchors");
const anchorsByPage = {};
for (const locale of nav.locales) {
  for (const page of pages) {
    const file = join(DIST, locale, `${page.slug}.html`);
    if (!existsSync(file)) continue;
    const html = readFileSync(file, "utf8");
    anchorsByPage[`${locale}/${page.slug}`] = new Set(
      [...html.matchAll(/<h[23] id="([^"]+)"/g)].map((m) => m[1]),
    );
  }
}
for (const locale of nav.locales) {
  for (const page of pages) {
    const file = join(DIST, locale, `${page.slug}.html`);
    if (!existsSync(file)) continue;
    const html = readFileSync(file, "utf8");
    for (const m of html.matchAll(/href="([a-z0-9-]+)\.html#([^"]+)"/g)) {
      const key = `${locale}/${m[1]}`;
      if (anchorsByPage[key] && !anchorsByPage[key].has(m[2])) {
        flag(`${locale}/${page.slug}: link to ${m[1]}#${m[2]} — that anchor does not exist`);
      }
    }
  }
}

// ---------------------------------------------------------------------------
// 2. The two locales must stay structurally comparable. A page that is a third
//    the length of its counterpart is a page somebody half-translated.
// ---------------------------------------------------------------------------
console.log("locale parity");
for (const page of pages) {
  const lens = {};
  for (const locale of nav.locales) {
    const file = join(DIST, locale, `${page.slug}.html`);
    if (!existsSync(file)) continue;
    const body = readFileSync(file, "utf8");
    const article = /<article class="prose">([\s\S]*?)<\/article>/.exec(body);
    lens[locale] = article ? plain(article[1]).length : 0;
  }
  const [a, b] = nav.locales.map((l) => lens[l] ?? 0);
  if (a === 0 || b === 0) {
    flag(`${page.slug}: one locale is empty (${a} / ${b})`);
  } else {
    const ratio = Math.min(a, b) / Math.max(a, b);
    if (ratio < 0.65) flag(`${page.slug}: locales differ a lot — ${a} vs ${b} chars (${ratio.toFixed(2)})`);
  }

  // Headings should correspond one-to-one, or the on-this-page rails diverge.
  const counts = nav.locales.map((locale) => {
    const file = join(DIST, locale, `${page.slug}.html`);
    if (!existsSync(file)) return 0;
    return [...readFileSync(file, "utf8").matchAll(/<h[23] id="/g)].length;
  });
  if (counts[0] !== counts[1]) {
    flag(`${page.slug}: heading counts differ — ${counts[0]} vs ${counts[1]}`);
  }
}

// ---------------------------------------------------------------------------
// 3. Untranslated leftovers. Not a dictionary — a short list of words that
//    only appear in Portuguese prose if a sentence was never translated.
// ---------------------------------------------------------------------------
console.log("untranslated leftovers");
const TELLS = [
  " the ", " and ", " with ", " which ", " because ", " rather than ",
  " something ", " nothing ", " already ", " instead ",
];
for (const page of pages) {
  const file = join(DIST, "pt-BR", `${page.slug}.html`);
  if (!existsSync(file)) continue;
  const article = /<article class="prose">([\s\S]*?)<\/article>/.exec(readFileSync(file, "utf8"));
  if (!article) continue;
  // Quoted agent output and code are deliberately left in English.
  const prose = article[1]
    .replace(/<pre[\s\S]*?<\/pre>/g, " ")
    .replace(/<code[\s\S]*?<\/code>/g, " ")
    .replace(/<em>[\s\S]*?<\/em>/g, " ");
  const text = " " + plain(prose).toLowerCase() + " ";
  const hits = TELLS.filter((w) => text.includes(w));
  if (hits.length >= 2) flag(`pt-BR/${page.slug}: possibly untranslated — ${hits.join(", ")}`);
}

// ---------------------------------------------------------------------------
// 4. Figures need captions, and captions should say something.
// ---------------------------------------------------------------------------
console.log("figures");
for (const locale of nav.locales) {
  for (const page of pages) {
    const file = join(DIST, locale, `${page.slug}.html`);
    if (!existsSync(file)) continue;
    const html = readFileSync(file, "utf8");
    for (const fig of html.matchAll(/<figure class="fig">([\s\S]*?)<\/figure>/g)) {
      if (!/<figcaption>/.test(fig[1])) flag(`${locale}/${page.slug}: a figure has no caption`);
      const alt = /alt="([^"]*)"/.exec(fig[1]);
      if (alt && alt[1].trim().length < 20) {
        flag(`${locale}/${page.slug}: alt text is too short to describe anything — "${alt[1]}"`);
      }
    }
  }
}

// ---------------------------------------------------------------------------
// 5. Every image on disk should be used, and be a reasonable size.
// ---------------------------------------------------------------------------
console.log("images");
const imgDir = join(DIST, "img");
if (existsSync(imgDir)) {
  const used = new Set();
  for (const locale of nav.locales) {
    for (const page of pages) {
      const file = join(DIST, locale, `${page.slug}.html`);
      if (!existsSync(file)) continue;
      for (const m of readFileSync(file, "utf8").matchAll(/src="\.\.\/img\/([^"]+)"/g)) used.add(m[1]);
    }
  }
  for (const name of readdirSync(imgDir)) {
    if (!used.has(name)) flag(`img/${name} is not used by any page`);
    const kb = statSync(join(imgDir, name)).size / 1024;
    if (kb > 900) flag(`img/${name} is ${Math.round(kb)} KB — heavy for a documentation figure`);
  }
  console.log(`  ${used.size} figure(s) in use`);
}

// ---------------------------------------------------------------------------
// 6. Size and shape of the output.
// ---------------------------------------------------------------------------
console.log("output");
const total = (dir) =>
  readdirSync(dir, { withFileTypes: true }).reduce(
    (n, e) => n + (e.isDirectory() ? total(join(dir, e.name)) : statSync(join(dir, e.name)).size),
    0,
  );
console.log(`  dist is ${(total(DIST) / 1024 / 1024).toFixed(1)} MB`);

console.log(problems === 0 ? "\naudit clean" : `\n${problems} thing(s) to look at`);
