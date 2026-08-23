import type { MonacoApi } from "./monaco.ts";

/**
 * Editor palettes.
 *
 * Authored per theme rather than derived, exactly as the terminal's are (§8),
 * and for the same reason: light is not an inversion of dark.
 *
 * Monaco reads plain hex strings out of a JavaScript object, so a CSS custom
 * property cannot be used here — the values below are the token values,
 * repeated. That duplication is the price of the boundary; a drifting copy
 * would be visible immediately, which is what makes it survivable.
 *
 * ## Why so little colour
 *
 * Quiet Intelligence (§6) says colour reports state and never decorates. A
 * seven-hue syntax theme decorates. Five muted tints carry the categories that
 * genuinely change how a line is read — comment, string, keyword, number, type
 * — and everything else, function names included, stays the ordinary text
 * colour.
 *
 * **Amber is absent on purpose.** It is the colour of agent work everywhere
 * else in the product, and spending it on numeric literals would spend the one
 * signal the interface is built around. It appears only as the cursor and the
 * selection, where it still means "this is where the work is".
 */

const DARK_TOKENS = [
  { token: "comment", foreground: "6B6B74", fontStyle: "italic" },
  { token: "string", foreground: "8FBF9F" },
  { token: "keyword", foreground: "A99BC9" },
  { token: "number", foreground: "7FBFBF" },
  { token: "regexp", foreground: "8FBF9F" },
  { token: "type", foreground: "8FAECC" },
  { token: "type.identifier", foreground: "8FAECC" },
  { token: "operator", foreground: "9E9EA6" },
  { token: "delimiter", foreground: "9E9EA6" },
  { token: "tag", foreground: "A99BC9" },
  { token: "attribute.name", foreground: "8FAECC" },
  { token: "attribute.value", foreground: "8FBF9F" },
  { token: "invalid", foreground: "E28B7C" },
];

const LIGHT_TOKENS = [
  { token: "comment", foreground: "86867E", fontStyle: "italic" },
  { token: "string", foreground: "2C7A4B" },
  { token: "keyword", foreground: "5F4A87" },
  { token: "number", foreground: "17696B" },
  { token: "regexp", foreground: "2C7A4B" },
  { token: "type", foreground: "2F5F8F" },
  { token: "type.identifier", foreground: "2F5F8F" },
  { token: "operator", foreground: "5C5C55" },
  { token: "delimiter", foreground: "5C5C55" },
  { token: "tag", foreground: "5F4A87" },
  { token: "attribute.name", foreground: "2F5F8F" },
  { token: "attribute.value", foreground: "2C7A4B" },
  { token: "invalid", foreground: "B23F2C" },
];

const DARK_COLORS: Record<string, string> = {
  "editor.background": "#0C0C0D",
  "editor.foreground": "#D6D6DA",
  "editorLineNumber.foreground": "#3F3F46",
  "editorLineNumber.activeForeground": "#9E9EA6",
  "editor.lineHighlightBackground": "#FFFFFF07",
  "editor.lineHighlightBorder": "#00000000",
  "editorCursor.foreground": "#D9A55C",
  "editor.selectionBackground": "#D9A55C42",
  "editor.inactiveSelectionBackground": "#D9A55C22",
  "editor.selectionHighlightBackground": "#FFFFFF14",
  "editor.wordHighlightBackground": "#FFFFFF12",
  "editor.wordHighlightStrongBackground": "#FFFFFF18",
  "editor.findMatchBackground": "#D9A55C55",
  "editor.findMatchHighlightBackground": "#D9A55C2A",
  "editorIndentGuide.background1": "#FFFFFF0D",
  "editorIndentGuide.activeBackground1": "#FFFFFF1F",
  "editorWhitespace.foreground": "#FFFFFF14",
  "editorGutter.background": "#0C0C0D",
  "editorWidget.background": "#1A1A1C",
  "editorWidget.border": "#FFFFFF19",
  "editorSuggestWidget.background": "#1A1A1C",
  "editorSuggestWidget.selectedBackground": "#FFFFFF11",
  "editorHoverWidget.background": "#1A1A1C",
  "editorHoverWidget.border": "#FFFFFF19",
  "editorBracketMatch.background": "#00000000",
  "editorBracketMatch.border": "#FFFFFF2B",
  // A backstop for bracket pair colourisation, which the model option turns
  // off (see CodeEditor). Painting all six depths as ordinary punctuation means
  // that if that switch ever moves again, the worst case is brackets in the
  // right colour rather than a gold that reads as agent work.
  "editorBracketHighlight.foreground1": "#9E9EA6",
  "editorBracketHighlight.foreground2": "#9E9EA6",
  "editorBracketHighlight.foreground3": "#9E9EA6",
  "editorBracketHighlight.foreground4": "#9E9EA6",
  "editorBracketHighlight.foreground5": "#9E9EA6",
  "editorBracketHighlight.foreground6": "#9E9EA6",
  "editorBracketHighlight.unexpectedBracket.foreground": "#E28B7C",
  "editorOverviewRuler.border": "#00000000",
  "scrollbarSlider.background": "#FFFFFF24",
  "scrollbarSlider.hoverBackground": "#FFFFFF3D",
  "scrollbarSlider.activeBackground": "#FFFFFF4F",
  "minimap.background": "#0C0C0D",
  focusBorder: "#00000000",
};

const LIGHT_COLORS: Record<string, string> = {
  "editor.background": "#FFFFFF",
  "editor.foreground": "#26261F",
  "editorLineNumber.foreground": "#B8B8B0",
  "editorLineNumber.activeForeground": "#5C5C55",
  "editor.lineHighlightBackground": "#10100C07",
  "editor.lineHighlightBorder": "#00000000",
  "editorCursor.foreground": "#9A6A1E",
  "editor.selectionBackground": "#9A6A1E33",
  "editor.inactiveSelectionBackground": "#9A6A1E1A",
  "editor.selectionHighlightBackground": "#10100C12",
  "editor.wordHighlightBackground": "#10100C10",
  "editor.wordHighlightStrongBackground": "#10100C16",
  "editor.findMatchBackground": "#9A6A1E4D",
  "editor.findMatchHighlightBackground": "#9A6A1E26",
  "editorIndentGuide.background1": "#10100C12",
  "editorIndentGuide.activeBackground1": "#10100C26",
  "editorWhitespace.foreground": "#10100C1A",
  "editorGutter.background": "#FFFFFF",
  "editorWidget.background": "#FFFFFF",
  "editorWidget.border": "#10100C1F",
  "editorSuggestWidget.background": "#FFFFFF",
  "editorSuggestWidget.selectedBackground": "#10100C0F",
  "editorHoverWidget.background": "#FFFFFF",
  "editorHoverWidget.border": "#10100C1F",
  "editorBracketMatch.background": "#00000000",
  "editorBracketMatch.border": "#10100C33",
  "editorBracketHighlight.foreground1": "#5C5C55",
  "editorBracketHighlight.foreground2": "#5C5C55",
  "editorBracketHighlight.foreground3": "#5C5C55",
  "editorBracketHighlight.foreground4": "#5C5C55",
  "editorBracketHighlight.foreground5": "#5C5C55",
  "editorBracketHighlight.foreground6": "#5C5C55",
  "editorBracketHighlight.unexpectedBracket.foreground": "#B23F2C",
  "editorOverviewRuler.border": "#00000000",
  "scrollbarSlider.background": "#10100C2E",
  "scrollbarSlider.hoverBackground": "#10100C4D",
  "scrollbarSlider.activeBackground": "#10100C5C",
  "minimap.background": "#FFFFFF",
  focusBorder: "#00000000",
};

export const THEME_DARK = "jarvis-dark";
export const THEME_LIGHT = "jarvis-light";

/** Register both themes. Idempotent — Monaco replaces a theme of the same name. */
export function defineThemes(monaco: MonacoApi): void {
  monaco.editor.defineTheme(THEME_DARK, {
    base: "vs-dark",
    inherit: true,
    rules: DARK_TOKENS,
    colors: DARK_COLORS,
  });
  monaco.editor.defineTheme(THEME_LIGHT, {
    base: "vs",
    inherit: true,
    rules: LIGHT_TOKENS,
    colors: LIGHT_COLORS,
  });
}

export function themeName(resolved: "dark" | "light"): string {
  return resolved === "light" ? THEME_LIGHT : THEME_DARK;
}
