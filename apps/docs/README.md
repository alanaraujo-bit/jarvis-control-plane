# The documentation site

The public documentation for J.A.R.V.I.S. — 47 pages, English and Portuguese,
illustrated with photographs of the real build.

```bash
node build.mjs          # build into dist/
node build.mjs --check  # build, and exit non-zero on any problem
node serve.mjs          # http://localhost:4180
node test-search.mjs    # exercise the shipped search against the built index
node audit.mjs          # the checks a green build does not catch
```

No dependencies. No framework. No build step beyond one Node script — the same
choice the product's own phone web app made, and for the same reason: nothing
here is hard enough to justify a toolchain that can break.

## How it is put together

```
nav.json              the shared spine — sections, slugs, titles, summaries
content/en/*.html     one HTML partial per page
content/pt-BR/*.html  the same page set, in Portuguese
assets/               site.css, site.js, theme.js, the mark, the fonts
public/img/           figures, cropped and scaled for a reading column
build.mjs             assembles dist/
dist/                 the output (gitignored)
```

A partial is body HTML only. Titles, summaries and ordering live in
`nav.json`, which is shared between locales — so the two trees cannot drift in
structure even if their prose does.

Internal links are written `href="page:slug"` or `href="page:slug#anchor"`.
The build rewrites them and fails on a target that does not exist, which is
why no page ever hardcodes a locale.

## The build is a verification pass

`build.mjs --check` exits non-zero when:

- a partial is missing in one locale but not the other;
- a partial exists that `nav.json` does not know about;
- a page has no title in one of the locales;
- a figure points at an image that is not on disk;
- an `<img>` has no `alt`;
- a `page:` link points at a page that does not exist.

`audit.mjs` covers the tier below that — the things that survive a green build
and are still wrong for a reader:

- a link to an anchor that does not exist on the page it points at;
- two locales whose page lengths or heading counts have diverged;
- Portuguese pages that still read like English;
- figures without captions, or with alt text too short to describe anything;
- images on disk that no page uses, or that are too heavy for a figure.

`test-search.mjs` runs the **shipped** `assets/site.js` in a DOM stub and lifts
its actual ranking function out of the source. An earlier version of that test
reimplemented the ranking and passed against its own copy while the real one
changed underneath — which is the failure mode a test like this exists to have.

## Design

The palette, the easing and the restraint are the product's own
(`apps/desktop/src/design/tokens.css`), and the fonts are literally the files
the application ships. Documentation that looks like a different product
describes a different product.

One thing is deliberately not inherited: the type scale. The app is a dense
desktop tool and sets its body at 13.5px; this is a reading surface, so the
same hierarchy is re-cut at reading sizes.

The search overlay ports the command palette's own subsequence scorer, so
typing `mc` finds Mission Control in both places — with one deliberate
difference, documented in `site.js`: body matches require every word to
actually appear, because subsequence matching over 2,600 characters of prose
matches almost anything.

## Figures

Photographs of the real 0.5.0 build, taken with `tools/shoot.ps1` (drives the
app to a surface and photographs it in one call) and cropped with
`tools/crop.ps1` **before** scaling — a 1442×902 window in a 750px column puts
13.5px interface text at about 7px, which is present and unreadable.

Email addresses on the Accounts surface are replaced with `you@example.com` by
`tools/redact.ps1`, and that figure's caption says so. Nothing else in any
figure is altered.

Concepts get diagrams instead: inline SVG drawn from the same tokens as the
page, so they theme with it. A photograph of Mission Control cannot show *why*
its ordering is what it is.

## Publishing

The output is plain files. It opens from `file://`, serves from any static
host, and carries no inline script at all — so its content security policy can
be `script-src 'self'` rather than needing `unsafe-inline`.

`vercel.json` is set up for a Vercel deploy from this directory:

```bash
cd apps/docs
vercel            # preview
vercel --prod     # production
```

**Pick a project name that cannot be confused with the existing `jarvis` and
`jarvis-guardian` projects on that account** — they are unrelated to this
product. Something like `jarvis-docs`.
