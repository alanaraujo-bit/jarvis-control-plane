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

## Status

See the checklist at the bottom of this file, rewritten as work lands.
