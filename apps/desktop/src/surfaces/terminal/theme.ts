import type { ITheme } from "@xterm/xterm";

/**
 * Terminal palettes.
 *
 * Authored per theme rather than derived, for the same reason the interface is
 * (§8). The ANSI ramp is deliberately muted: agent CLIs colour heavily, and a
 * saturated palette turns a busy session into noise. Hues line up with the
 * product's state colours so red still means failure and amber still means work
 * in progress, in the terminal as everywhere else.
 */

const DARK: ITheme = {
  background: "#0C0C0D",
  foreground: "#D6D6DA",
  cursor: "#D9A55C",
  cursorAccent: "#0C0C0D",
  selectionBackground: "rgba(217, 165, 92, 0.26)",
  selectionForeground: undefined,

  black: "#2A2A2E",
  red: "#E28B7C",
  green: "#7FC098",
  yellow: "#D9A55C",
  blue: "#7FA6CC",
  magenta: "#C093C0",
  cyan: "#7FC0C0",
  white: "#C4C4CA",

  brightBlack: "#5C5C64",
  brightRed: "#F0A79A",
  brightGreen: "#9AD4B0",
  brightYellow: "#E9BE7E",
  brightBlue: "#9CBFE0",
  brightMagenta: "#D4AAD4",
  brightCyan: "#9AD4D4",
  brightWhite: "#EDEDEF",
};

const LIGHT: ITheme = {
  background: "#FFFFFF",
  foreground: "#26261F",
  cursor: "#9A6A1E",
  cursorAccent: "#FFFFFF",
  selectionBackground: "rgba(154, 106, 30, 0.20)",
  selectionForeground: undefined,

  black: "#3A3A34",
  red: "#B23F2C",
  green: "#2C7A4B",
  yellow: "#8A5E12",
  blue: "#2F5F8F",
  magenta: "#7A3F7A",
  cyan: "#17696B",
  white: "#8E8E86",

  // On white, "bright" must stay legible, so these are lifted in saturation
  // rather than in lightness — the usual inversion would wash them out.
  brightBlack: "#6B6B63",
  brightRed: "#8F3021",
  brightGreen: "#1F6039",
  brightYellow: "#6E4A0C",
  brightBlue: "#244C73",
  brightMagenta: "#61315F",
  brightCyan: "#0F5254",
  brightWhite: "#26261F",
};

export function terminalTheme(resolved: "dark" | "light"): ITheme {
  return resolved === "light" ? LIGHT : DARK;
}

/**
 * How a scrollback search match is painted.
 *
 * **Not amber, on purpose.** Amber is the colour of agent work in this product
 * (§6) — the brand accent and the "working" state are deliberately the same
 * hue so colour always reports state. A find highlight is not state, so it
 * borrows the waiting blue instead: it reads clearly against both palettes and
 * never gets confused with a session that is doing something.
 *
 * These are literal hex rather than tokens because xterm paints decorations
 * into its own canvas and requires `#RRGGBB` — `var(--…)` does not resolve
 * there, the same constraint the `fontFamily` above already documents.
 */
const DARK_SEARCH: SearchDecorations = {
  matchBackground: "#25384A",
  matchBorder: "#3D5A73",
  matchOverviewRuler: "#7FA6CC",
  activeMatchBackground: "#3E5C78",
  activeMatchBorder: "#9CBFE0",
  activeMatchColorOverviewRuler: "#9CBFE0",
};

const LIGHT_SEARCH: SearchDecorations = {
  matchBackground: "#DCE7F2",
  matchBorder: "#A9C3DC",
  matchOverviewRuler: "#2F5F8F",
  activeMatchBackground: "#B7CFE6",
  activeMatchBorder: "#2F5F8F",
  activeMatchColorOverviewRuler: "#244C73",
};

export interface SearchDecorations {
  matchBackground: string;
  matchBorder: string;
  matchOverviewRuler: string;
  activeMatchBackground: string;
  activeMatchBorder: string;
  activeMatchColorOverviewRuler: string;
}

export function terminalSearchDecorations(resolved: "dark" | "light"): SearchDecorations {
  return resolved === "light" ? LIGHT_SEARCH : DARK_SEARCH;
}
