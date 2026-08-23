/**
 * The editor boundary (D4).
 *
 * Surfaces import from here and never from `monaco-editor`. Everything Monaco
 * knows about — its API shape, its themes, its models, its workers — stops at
 * this package, so an LSP client, or a different editor entirely, can land
 * without touching a single surface (§42).
 */

export { CodeEditor, disposeModel, type CodeEditorProps } from "./CodeEditor.tsx";
export { languageForPath } from "./languages.ts";
export { THEME_DARK, THEME_LIGHT, themeName } from "./theme.ts";
