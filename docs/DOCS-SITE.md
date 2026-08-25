# The documentation site (`apps/docs`)

Alan asked, on 2026-08-25, for the public documentation of the product: dense,
elegant, searchable, bilingual, illustrated with real screenshots — the
standard he named was Apple's, Cursor's and Vercel's. He then left, so every
decision below was taken alone and each one is recorded with its reason.

## Decisions

**D-A. A static site in the repository, not an Artifact.** The deliverable has
to be launchable publicly. An Artifact is capped at 16 MB, the screenshot
library alone is 27 MB, and its CSP forbids external hosts. `apps/docs/` builds
to plain files that deploy anywhere and also open from `file://`.

**D-B. Zero dependencies, one `build.mjs`.** The repository's own culture
already made this choice twice: the M11 PWA ships with "no framework and no
build step", and i18n is a hand-written catalogue rather than a library. Being
alone with nobody to unblock an install fight is the second reason.

**D-C. Content is authored as HTML partials, not Markdown.** Documentation at
the standard named is mostly custom components — figures with captions,
capability matrices, callouts, step lists, keycaps. A Markdown subset would
fight the build on exactly the parts that carry the quality.

**D-D. Bilingual is structural and decided before page one.** `content/en/` and
`content/pt-BR/` hold the same page set, `nav.json` is shared, and `build.mjs`
fails the build when the two trees diverge. English first and "translate
later" ships a half-translated site.

**D-E. Portuguese uses the product's own catalogue, never a fresh
translation.** `packages/i18n/src/pt-BR.ts` already decided that Mission
Control is *Central de Missões* and the Brain is *Memória*. Documentation that
invents its own nouns is documentation about a different product.

**D-F. Search is a build-time index inlined into the page.** `fetch()` of a
local JSON file fails under `file://`, and the site has to open from disk.
Ranking mirrors the command palette's own subsequence match (§50).

**D-G. Figures are photographs of the running product, cropped hard.** A short
capture session against the real 0.5.0 build produced every surface that could
be reached without staging (`tools/shoot.ps1`, written for this); the rest are
curated from the 483 QA screenshots already in `shots/`. Everything is cropped
to the part being written about before it is scaled — a 1442×902 window in a
750px column puts 13.5px interface text at about 7px, which is present and
unreadable, and that is worse than absent (`tools/crop.ps1`).

**D-G1. Concepts get diagrams, surfaces get screenshots.** A photograph of
Mission Control cannot show *why* its ordering is what it is. Inline SVG, drawn
from the same tokens as the page, carries the mechanism; the screenshot carries
the evidence that the mechanism shipped.

**D-G2. Personal data is removed from figures, and the caption says so.** The
Accounts surface prints the email address each configuration directory is
signed in as. Publishing those addresses is a privacy exposure with no
documentary value, so they are replaced with `you@example.com`
(`tools/redact.ps1`) and that figure's caption states the replacement. Nothing
else in any figure is altered.

**D-G3. One figure set serves both languages.** Capturing every surface twice
doubles a cost this schedule cannot carry, and the reference documentation this
is measured against ships one screenshot set with translated captions.

**D-G4. A demo project, not the author's own work.** `Aurora` — a small billing
service with a real Git history, real uncommitted changes and a populated
Brain — was created so the figures show a product being used rather than a
product sitting empty. It is a real folder and a real repository; nothing in
any figure is a mock-up.

**D-H. Every sentence about behaviour is traceable to a file that was read.**
ROADMAP and the source are authoritative; the README is not — it still lists
Preview, onboarding and voice as unbuilt while M8, M9 and M12 shipped them.

## Verification loop

Headless Edge renders the built site and the screenshot is looked at:

```
msedge --headless=new --disable-gpu --hide-scrollbars \
  --window-size=1440,2400 --screenshot=<abs>.png "file:///<abs>/index.html"
```

Proven working on this machine on 2026-08-25.

---

## What was built

**47 pages, 94 partials, both languages, no dependencies.**

| Section | Pages |
|---|---|
| Start here | Overview, Install, Your first ten minutes |
| Concepts | Session log, Providers, Missions, Autonomy, Guardrails, Confidence, Memory, Accounts, Local-first |
| Surfaces | 17 — every screen the product has |
| Guides | 6 task-shaped walkthroughs |
| Reference | Shortcuts, Provider matrix, Data locations, Settings, Glossary |
| Architecture | How it is built, Inside the guardrail, Inside the autopilot, Distribution |
| Project state | What is built, What is not built, Known blockers |

**20 figures**, all photographs of the real 0.5.0 build. **8 diagrams**, inline
SVG drawn from the page's own tokens so they theme with it.

### Tooling added along the way

| Tool | Does |
|---|---|
| `apps/docs/build.mjs` | Builds, and fails on locale divergence, a missing image, a missing alt, or a dead internal link |
| `apps/docs/audit.mjs` | The tier below that — dead anchors, diverged locales, untranslated Portuguese, uncaptioned figures, unused images |
| `apps/docs/test-search.mjs` | Runs the shipped `site.js` in a DOM stub and lifts its real ranking function |
| `apps/docs/serve.mjs` | A local static server |
| `apps/docs/shoot-page.ps1` | Renders a built page headlessly, at any width, in either theme |
| `tools/shoot.ps1` | Drives the app to a surface and photographs it in one call |
| `tools/crop.ps1` | Crops before scaling |
| `tools/redact.ps1` | Replaces personal data in a figure |

### Bugs this pass found in its own work

1. **The theme could not be previewed at all.** A `\b` inside a template
   literal is consumed before it reaches the browser, so the built page
   carried a real backspace character in the regex and the match never fired.
2. **`$args` is an automatic PowerShell variable.** Assigning to it silently
   does not do what it looks like, and the screenshot URL lost its query
   string — which is what made the first bug look like it might be elsewhere.
3. **The search returned results for `zzzqqq`.** Subsequence matching is right
   for the short titles the product's palette ranks and wrong over 2,600
   characters of prose, where almost any string of letters matches.
4. **The first search test reimplemented the ranking** and passed against its
   own copy while the real one changed underneath. The ranking is now one
   function, lifted whole into the test.
5. **The pt-BR summary for Guardrails said eight operations.** `classify.rs`
   has nine — `git.discard-changes` was added after the roadmap's count was
   written. The roadmap is still wrong; the docs are not.

### One mistake made against real data, and repaired

The screenshot automation clicked *New note* and then sent Enter, which
re-fired the still-focused button **48 times** and left 48 empty notes in the
real notebook. Repaired through the product's own delete affordance — direct
deletion from the database was refused, correctly — and verified back to one
note, the real one, untouched.

The lesson is in `tools/shoot.ps1` now: never send Enter straight after
clicking a button.

## Left for Alan

- **Deploying it.** `cd apps/docs && vercel` — set up and not run, because
  publishing is outward-facing and the account carries two unrelated projects
  called `jarvis` and `jarvis-guardian`. Pick a name that cannot be confused
  with those.
- **The demo data.** `Projetos/Aurora` is a real repository created for the
  figures, and `Projetos/Aurora-agent-reconcile-fix` is a real worktree of it
  created for the Worktrees figure. Both are safe to delete — remove the
  worktree from inside the app so its project row is archived properly.

- **B-DOC1 in `BLOCKERS.md`** — an unanswered AnyDesk remote-access request
  that appeared during this session. Read that one first.
