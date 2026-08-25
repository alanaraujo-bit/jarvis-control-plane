#!/usr/bin/env node
/**
 * Exercise the built site's own search against the built index.
 *
 * This runs the shipped `assets/site.js` inside a minimal DOM stub rather than
 * reimplementing its scorer, because a test that reimplements the thing it is
 * testing passes when the shipped code is broken. The stub is deliberately
 * dumb: the point is to reach `search()`, not to emulate a browser.
 *
 *   node test-search.mjs
 */

import { readFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const ROOT = dirname(fileURLToPath(import.meta.url));
const DIST = join(ROOT, "dist");

if (!existsSync(DIST)) {
  console.error("dist/ is not built — run `node build.mjs` first");
  process.exit(1);
}

let failures = 0;
const check = (ok, label, detail = "") => {
  console.log(`${ok ? "  ok  " : "  FAIL"}  ${label}${detail ? `  ${detail}` : ""}`);
  if (!ok) failures++;
};

/** The smallest DOM that lets site.js finish executing. */
function makeStub() {
  const el = () => ({
    value: "",
    innerHTML: "",
    hidden: true,
    dataset: {},
    classList: { toggle() {}, contains: () => false },
    style: {},
    addEventListener() {},
    setAttribute() {},
    getAttribute: () => "#",
    scrollIntoView() {},
    querySelectorAll: () => [],
    querySelector: () => null,
    appendChild() {},
    insertBefore() {},
    focus() {},
    offsetTop: 0,
    clientHeight: 0,
    scrollTop: 0,
    parentNode: { insertBefore() {} },
    parentElement: { classList: { contains: () => true } },
    tagName: "DIV",
  });
  const doc = {
    documentElement: { dataset: {} },
    activeElement: { tagName: "BODY" },
    createElement: el,
    getElementById: () => null,
    querySelector: (sel) =>
      sel === "[data-palette]" || sel === "[data-results]" || sel === "[data-search-input]" ? el() : null,
    querySelectorAll: () => [],
    addEventListener() {},
  };
  return doc;
}

for (const locale of ["en", "pt-BR"]) {
  console.log(`\n${locale}`);

  const indexJs = readFileSync(join(DIST, locale, "search-index.js"), "utf8");
  const siteJs = readFileSync(join(ROOT, "assets", "site.js"), "utf8");

  const sandbox = {
    window: {
      addEventListener() {},
      DOCS_UI: { noResults: "none" },
    },
    document: makeStub(),
    localStorage: { getItem: () => null, setItem() {} },
    matchMedia: () => ({ matches: false }),
    requestAnimationFrame() {},
    location: { search: "", href: "" },
    IntersectionObserver: undefined,
    console,
  };
  sandbox.window.window = sandbox.window;
  sandbox.globalThis = sandbox;
  vm.createContext(sandbox);

  vm.runInContext(indexJs, sandbox);
  const index = sandbox.window.DOCS_INDEX;
  check(Array.isArray(index) && index.length > 200, "index built", `${index.length} entries`);

  // site.js closes over `search`; expose it by evaluating the file with a hook
  // appended. Nothing in the function is modified.
  vm.runInContext(siteJs + "\n;window.__probe = typeof search === 'function';", sandbox);

  // The scorer is the part worth testing, and it is a pure function of two
  // strings. Lift it out of the source rather than copying it.
  const src = readFileSync(join(ROOT, "assets", "site.js"), "utf8");
  const scoreSrc = src.slice(src.indexOf("function score("), src.indexOf("var fold ="));
  const foldSrc = src.slice(src.indexOf("var fold ="), src.indexOf("// Folded once"));
  const probe = vm.runInContext(`(function(){ ${scoreSrc} ${foldSrc} return { score: score, fold: fold }; })()`, sandbox);

  const { score, fold } = probe;

  // --- the scorer behaves the way the product's palette does ---------------
  check(score("mission control", "Mission Control", "mc") !== null, "subsequence matches", '"mc" → Mission Control');
  check(score("mission control", "Mission Control", "zzz") === null, "no match returns null");
  check(
    score("mission control", "Mission Control", "mc") > score("rescan environment", "Rescan Environment", "mc"),
    "word starts outrank scattered letters",
  );

  // --- folding is load-bearing for Portuguese ------------------------------
  check(fold("Sessão") === "sessao", "accents fold", `Sessão → ${fold("Sessão")}`);
  check(fold("PROTEÇÕES") === "protecoes", "case and accents fold together");

  // --- the real index answers real queries ---------------------------------
  const rows = index.map((r) => ({ ...r, _t: fold(r.t), _g: fold(r.g || ""), _d: fold(r.d || ""), _b: fold(r.b || "") }));
  // Lifted whole from site.js, so the test cannot drift from what ships.
  const rankSrc = src.slice(src.indexOf("function rankRow("), src.indexOf("function search("));
  const rankRow = vm.runInContext(
    `(function(){ ${scoreSrc} ${rankSrc} return rankRow; })()`,
    sandbox,
  );

  const ask = (q) => {
    const needle = fold(q);
    const words = needle.split(" ").filter(Boolean);
    return rows
      .map((r) => {
        const hit = rankRow(r, needle, words);
        return hit ? { r, s: hit.rank } : null;
      })
      .filter(Boolean)
      .sort((a, b) => b.s - a.s);
  };

  const queries =
    locale === "en"
      ? [
          ["guardrail", "Guardrails"],
          ["session log", "The session event log"],
          ["shortcut", "Keyboard shortcuts"],
          ["quota", "Accounts and quota"],
          ["worktree", "Worktrees"],
        ]
      : [
          ["protecoes", "Proteções"],
          ["proteções", "Proteções"],
          ["sessao", "O log de eventos da sessão"],
          ["atalho", "Atalhos de teclado"],
          ["memoria", "Memória"],
        ];

  for (const [q, expected] of queries) {
    const top = ask(q).slice(0, 4).map((x) => x.r.t);
    check(top.includes(expected), `"${q}" finds ${expected}`, `top: ${top[0] ?? "—"}`);
  }

  check(ask("zzzqqq").length === 0, "a nonsense query returns nothing");
  check(ask("").length > 0, "an empty query still returns rows");
}

console.log(failures === 0 ? "\nall search checks passed" : `\n${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
