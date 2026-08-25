# M17 — Settings, reorganised (§64, D45)

Working record for the settings reorganisation, so the work can be picked up
mid-flight. Decisions that outlive this milestone live in `DECISIONS.md` (D45).

## The complaint

Settings was one column with every group stacked in it: Appearance, Terminal,
Agents (autonomy + turn limit + guardrails), Notifications, Phone, Environment,
Updates. Everything was visible at once, which reads as clutter rather than as
completeness — to change one thing you scrolled past four unrelated decisions,
and to know whether a setting existed you had to read all of them.

Asked for: everything divided, each in its own place, and clicking something
opens its options.

## The shape

Two columns. A map on the left that does not scroll away, one section on the
right. Eight sections, ordered by how often somebody comes to them:

| Section | What it holds | Heading comes from |
| --- | --- | --- |
| Appearance | theme, language | `Settings.tsx` |
| Terminal | type size, scrollback | `Settings.tsx` |
| Autonomy | autonomy default, turn limit | `AutonomyPanel` |
| Guardrails | one row per operation | `GuardrailPanel` |
| Notifications | three switches, test button | `Settings.tsx` |
| Phone | companion pairing | `CompanionPanel` |
| Environment | tool scan | `EnvironmentPanel` |
| Updates | version, check, install | `Updates` |

**Guardrails became its own section.** It had been the tail of an "Agents"
group that also held autonomy and the turn limit, and it is a table the core is
free to grow (§26). A section that can get longer on its own should not share a
page.

**One heading per section, never two.** The old screen said "Phone" and the
panel under it said "Mobile companion". A section whose panel titles and
explains itself gets no heading from `Settings.tsx`; only the three built out of
loose fields do. That also leaves `AutonomyPanel` and `GuardrailPanel`
untouched — both render inside a project too (§19), where their own headers are
the only headings there are.

**No accordions inside a section.** The left column already answers "click and
more options open". Nesting a second level of hiding under it would mean two
things to expand before seeing a setting, which reads as more buried, not less.

## Files

- `surfaces/settings/categories.ts` — the section list, as data. Each section's
  label is the key the thing already had (`autonomy.title`, `guardrail.title`,
  `env.title`, …) rather than a nav-only duplicate of it.
- `surfaces/settings/settingsNav.ts` — which section is open; a module store so
  the command palette can aim at one and so the choice survives leaving.
- `surfaces/settings/Settings.tsx` — the two-column shell and the section bodies.
- `surfaces/settings/Settings.css` — rewritten; the pane owns the scroll and the
  reading measure, the map is fixed at 220px and collapses to a top strip below
  720px.
- `environment/EnvironmentPanel.{tsx,css}`, `settings/Updates.{tsx,css}` — an
  icon on the heading, so all eight sections are headed the same way.
- `App.tsx` — every section is a command palette entry, and `env.rescan` now
  lands on Environment rather than on whatever section was last open.
- `packages/i18n/src/{en,pt-BR}.ts` — `settings.nav.label` and five `*.blurb`
  lines; `settings.agents` deleted, its last reader gone with the rewrite.

## What only showed up on screen

Four things were wrong in the browser and looked right in the stylesheet. All
four were found by opening the real Tauri build and looking.

1. **The section floated away from its map.** `margin: 0 auto` on a 780px column
   inside an 1150px pane left a 215px gutter, and the section read as belonging
   to nothing. Left-aligned, measure raised to 860px.
2. **The selected row vanished in the light theme.** `--bg-raised` is an opaque
   surface colour and light's surface is nearly the page. Now `--bg-active`
   (translucent) plus a 2px amber edge — the rail's own idiom.
3. **In the narrow strip the selected chip sat off-screen.** The page showed
   Environment while the strip showed no selection at all. Fixed with
   `scrollIntoView` on the active item; the amber edge moves to the bottom in
   that layout, where it reads as an underline rather than a margin marker.
4. **Every panel asked the *viewport* how wide it was.** True when Settings was
   one full-width column; false the moment a 220px map appeared beside it. The
   pane is about 344px narrower than the window, so at a 746px window the
   autonomy control ran off the right edge with a horizontal scrollbar under
   it. Corrected with `.settings__pane`-scoped breakpoints, which leaves the
   same panels alone inside a project where their own numbers are right.

## Known limitation

`key={category}` unmounts the previous section, so starting an update download
and then switching sections loses the **progress display** — the download itself
continues, but `Updates` remounts at `idle` with a "Check for updates" button.
Accepted rather than fixed: keeping all eight mounted (the idiom
`ProjectWorkspace` uses for its areas) would re-run `loadPolicies` and the
environment scan on every visit to Settings, which is the worse trade for a case
measured in seconds a year.

## State

- [x] i18n keys, both catalogues
- [x] categories + nav store
- [x] two-column shell, section bodies
- [x] stylesheet, including the narrow-window strip
- [x] command palette entries + rescan deep link
- [x] `pnpm typecheck` clean, `pnpm -r test` clean
- [x] seen in the real Tauri app: all eight sections, dark and light, pt-BR and
      en, at 1440 / 1086 / 746 / 606 px — shots `m17-*`
- [x] `DECISIONS.md` D45, `HANDOFF.md` row rewritten
