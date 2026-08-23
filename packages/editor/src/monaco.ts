/**
 * The one place Monaco is imported.
 *
 * Nothing outside this package may import `monaco-editor` (D4). Everything the
 * product needs from an editor goes through `index.ts`, so a later LSP client —
 * or a different editor entirely — changes this package and no surface.
 *
 * ## What is deliberately left out
 *
 * Monaco ships language *services* for TypeScript, CSS, HTML and JSON, each in
 * its own web worker. They are not loaded here, for two reasons:
 *
 * 1. **They would be wrong.** Monaco's TypeScript worker knows nothing about
 *    the project's `tsconfig.json` or its `node_modules`, so it reports missing
 *    imports and phantom errors on perfectly good code. A diagnostic that is
 *    confidently incorrect is worse than no diagnostic (§28 is the same
 *    principle applied to numbers). Real intelligence arrives with an LSP
 *    client, which does know those things.
 * 2. **They are most of the weight.** The TypeScript worker alone is 7 MB —
 *    larger than the entire installed product. Measured, not assumed; the
 *    figures are in D17.
 *
 * What remains is the editor core plus tokenisation grammars: syntax
 * highlighting, multiple cursors, find and replace, folding, minimap. That is
 * what §42 asks for today.
 */

import type * as Monaco from "monaco-editor/esm/vs/editor/editor.api.js";

export type MonacoApi = typeof Monaco;
export type { Monaco };

let pending: Promise<MonacoApi> | null = null;

/**
 * Load Monaco, once.
 *
 * Dynamically imported so the ~3.7 MB editor chunk is not part of the initial
 * bundle. The application window has to paint before anything is visible (§11)
 * and most sessions never open a file, so making every launch pay for the
 * editor would be a straight regression. Concurrent callers share one promise:
 * opening three files at once must not start three loads.
 */
export function isMonacoLoaded(): boolean {
  return pending !== null;
}

export function loadMonaco(): Promise<MonacoApi> {
  if (!pending) {
    pending = (async () => {
      const monaco = await import("monaco-editor/esm/vs/editor/editor.api.js");
      const { default: EditorWorker } = await import(
        "monaco-editor/esm/vs/editor/editor.worker.js?worker"
      );

      // Monaco asks the host for its workers. Vite compiles the import above
      // into a real same-origin module, which the application's CSP allows;
      // Monaco's own default constructs one from a `blob:` URL, which it does
      // not, and the editor would silently lose word-based completion and its
      // background diff.
      (self as unknown as { MonacoEnvironment?: unknown }).MonacoEnvironment = {
        getWorker: () => new EditorWorker(),
      };

      await import("./languages.ts").then((m) => m.registerLanguages());
      return monaco;
    })();
  }
  return pending;
}
