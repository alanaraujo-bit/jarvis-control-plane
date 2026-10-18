# Design and accessibility

## Direction

"Quiet Intelligence" (§6): high power, low visual noise. Near-monochrome
neutrals carry the interface; colour is reserved for meaning. **Amber is the
colour of agent work** — brand accent and "working" state are the same hue, so
colour always reports state and never decorates. Desktop-dense: 13.5px body.

## Existing system (source of truth)

- Tokens: [`apps/desktop/src/design/tokens.css`](../apps/desktop/src/design/tokens.css)
  — typography, spacing, colour, motion. Dark and light are **authored
  separately** (§8); light is not an inversion.
- Base styles: `apps/desktop/src/design/base.css`. Shared primitives:
  `Popover`, `StatusDot`, `Logo` in the same folder.
- Fonts: Inter Variable (UI), JetBrains Mono Variable (code/terminal).
- Icons: Lucide. Terminal palettes are per theme (xterm).

Do not copy token values into documents — link the file.

## Interaction

Keyboard-first (command palette, shortcuts captured before WebView2 handles
them — HANDOFF §5 item 39). Visible focus, labelled controls, reduced-motion
respected (wired since M0). Asynchronous work shows progress and result; an
estimate never renders like an official figure (§28).

## Review

Every UI change is reviewed in the running app, in dark and light and in en and
pt-BR (HANDOFF §3). Judge colour by sampling pixels, not by eye (HANDOFF §5
items 42 and 64). Use the `web-design-guidelines` skill for accessibility and
interaction review.

## Maintenance

Visual changes update `tokens.css` or the component; update this file only when
a rule changes.
