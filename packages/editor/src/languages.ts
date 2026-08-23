/**
 * Which grammars are compiled in, and how a filename maps to one.
 *
 * Monaco's `esm/vs/basic-languages/monaco.contribution` entry point pulls in
 * every grammar it ships — ABAP, FreeMarker, PowerQuery, Solidity and eighty
 * others — so each is imported by name instead. The list is what a developer
 * working on software actually opens; adding one is a line, and the cost of a
 * grammar is a few kilobytes.
 *
 * A file whose extension is not here still opens. It is shown as plain text,
 * which is honest, rather than highlighted with the wrong grammar.
 */

export async function registerLanguages(): Promise<void> {
  await Promise.all([
    import("monaco-editor/esm/vs/basic-languages/typescript/typescript.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/javascript/javascript.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/rust/rust.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/css/css.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/scss/scss.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/html/html.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/markdown/markdown.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/python/python.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/shell/shell.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/powershell/powershell.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/yaml/yaml.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/sql/sql.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/go/go.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/java/java.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/cpp/cpp.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/csharp/csharp.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/php/php.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/ruby/ruby.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/xml/xml.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/dockerfile/dockerfile.contribution.js"),
    import("monaco-editor/esm/vs/basic-languages/ini/ini.contribution.js"),
  ]);
}

/** Extensions, lowercase and without the dot, to Monaco language ids. */
const BY_EXTENSION: Record<string, string> = {
  ts: "typescript",
  tsx: "typescript",
  mts: "typescript",
  cts: "typescript",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  rs: "rust",
  css: "css",
  scss: "scss",
  html: "html",
  htm: "html",
  md: "markdown",
  markdown: "markdown",
  py: "python",
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  ps1: "powershell",
  psm1: "powershell",
  yaml: "yaml",
  yml: "yaml",
  sql: "sql",
  go: "go",
  java: "java",
  c: "cpp",
  h: "cpp",
  cc: "cpp",
  cpp: "cpp",
  hpp: "cpp",
  cs: "csharp",
  php: "php",
  rb: "ruby",
  xml: "xml",
  svg: "xml",
  ini: "ini",
  toml: "ini",
  // JSON is tokenised by the editor core, so it needs no grammar import — but
  // it does need the mapping, or `package.json` opens as plain text.
  json: "json",
  jsonc: "json",
};

/** Filenames that carry no extension but are unambiguous. */
const BY_NAME: Record<string, string> = {
  dockerfile: "dockerfile",
  makefile: "shell",
  ".gitignore": "shell",
  ".env": "shell",
  ".npmrc": "ini",
  ".editorconfig": "ini",
};

/**
 * Best guess at a file's language, from its path alone.
 *
 * By name first: `Dockerfile` has no extension, and `.gitignore` is *all*
 * extension — splitting on the last dot would call its language `gitignore`.
 */
export function languageForPath(path: string): string {
  const name = path.split(/[\\/]/).pop() ?? path;
  const byName = BY_NAME[name.toLowerCase()];
  if (byName) return byName;

  const dot = name.lastIndexOf(".");
  if (dot <= 0) return "plaintext";
  return BY_EXTENSION[name.slice(dot + 1).toLowerCase()] ?? "plaintext";
}
