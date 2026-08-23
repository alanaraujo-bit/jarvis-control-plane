import { useEffect, useRef, useState } from "react";
import { loadMonaco, type Monaco, type MonacoApi } from "./monaco.ts";
import { languageForPath } from "./languages.ts";
import { defineThemes, themeName } from "./theme.ts";

/**
 * The literal monospace stack.
 *
 * **Not** `var(--font-mono)`. Monaco measures character width by rendering into
 * a canvas from this exact string, where a CSS custom property does not resolve
 * — it falls back to a proportional font and every line renders with visibly
 * uneven spacing while the CSS looks perfectly correct. This is D8, which cost
 * real time in the terminal; the same trap, a second surface.
 */
const MONO_STACK = '"JetBrains Mono Variable", "Cascadia Mono", "Consolas", ui-monospace, monospace';

export interface CodeEditorProps {
  /**
   * Namespace the document belongs to — the project id.
   *
   * Not cosmetic. Monaco keys models by URI, so two projects that both contain
   * `src/main.rs` would share one model, one buffer and one undo stack. The
   * second project to open the file would silently overwrite the first's
   * contents, and an undo could then put one project's code into the other's
   * file. The scope makes the key unique.
   */
  scope: string;
  /** The document's path within that scope, used for the language too. */
  path: string;
  value: string;
  theme: "dark" | "light";
  readOnly?: boolean;
  onChange?: (value: string) => void;
  /** Ctrl/Cmd+S inside the editor. */
  onSave?: () => void;
}

/**
 * The URI a document is stored under.
 *
 * One place, because the editor and `disposeModel` must agree exactly: a
 * mismatch would leak a model per file for the life of the session.
 */
function modelUri(monaco: MonacoApi, scope: string, path: string) {
  return monaco.Uri.parse(`jarvis://${encodeURIComponent(scope)}/${path}`);
}

/**
 * A text editor over one file.
 *
 * Monaco is loaded on first use, not at startup (see `monaco.ts`), so this
 * renders nothing until it arrives. The parent decides what to show meanwhile;
 * a spinner belongs to the surface, not to the boundary.
 */
export function CodeEditor({ scope, path, value, theme, readOnly, onChange, onSave }: CodeEditorProps) {
  const host = useRef<HTMLDivElement | null>(null);
  const editor = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const [monaco, setMonaco] = useState<MonacoApi | null>(null);

  // Callbacks reach Monaco through refs. Its commands and listeners are
  // registered once, and a handler captured at that moment would keep calling
  // the first render's `onSave` forever — saving whichever file was open when
  // the editor was created.
  const onChangeRef = useRef(onChange);
  const onSaveRef = useRef(onSave);
  onChangeRef.current = onChange;
  onSaveRef.current = onSave;

  useEffect(() => {
    let cancelled = false;
    void loadMonaco().then((api) => {
      if (cancelled) return;
      defineThemes(api);
      setMonaco(api);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  // Create the editor once Monaco is here and the host element exists.
  useEffect(() => {
    if (!monaco || !host.current || editor.current) return;

    const instance = monaco.editor.create(host.current, {
      theme: themeName(theme),
      automaticLayout: true,
      fontFamily: MONO_STACK,
      fontSize: 13,
      lineHeight: 20,
      // Ligatures are off: in a product where an agent's output and the user's
      // code sit side by side, `!=` rendering as `≠` in one and not the other
      // is a difference the eye has to resolve for no benefit.
      fontLigatures: false,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      renderLineHighlight: "line",
      cursorBlinking: "smooth",
      smoothScrolling: true,
      padding: { top: 10, bottom: 24 },
      // The overview ruler and its border draw a hard vertical line against a
      // near-monochrome surface; the scrollbar already says where you are.
      overviewRulerBorder: false,
      overviewRulerLanes: 0,
      scrollbar: { verticalScrollbarSize: 10, horizontalScrollbarSize: 10, useShadows: false },
      guides: {
        indentation: true,
        bracketPairs: false,
        highlightActiveIndentation: true,
      },
      renderWhitespace: "selection",
      tabSize: 2,
      wordWrap: "off",
      contextmenu: false,
      // Suggestions here are word-based, not language-aware (see `monaco.ts`).
      // Popping them up unprompted would look like intelligence the product
      // does not have yet (§81), so they are opened deliberately or not at all.
      quickSuggestions: false,
      suggestOnTriggerCharacters: false,
    });

    instance.onDidChangeModelContent(() => {
      onChangeRef.current?.(instance.getValue());
    });

    instance.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
      onSaveRef.current?.();
    });

    editor.current = instance;

    return () => {
      instance.dispose();
      editor.current = null;
    };
    // `theme` is applied by its own effect; re-creating the editor to change a
    // colour would throw away the user's cursor, selection and undo history.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [monaco]);

  // Swap the model when the open file changes.
  useEffect(() => {
    if (!monaco || !editor.current) return;

    // One model per path, reused. Monaco keeps undo history on the model, so a
    // file that is closed and reopened — or simply switched away from and back
    // — still remembers what was undone.
    const uri = modelUri(monaco, scope, path);
    const existing = monaco.editor.getModel(uri);
    const model = existing ?? monaco.editor.createModel(value, languageForPath(path), uri);

    if (!existing) {
      // Turn off bracket pair colourisation.
      //
      // It paints nested brackets in saturated gold, pink and blue: decoration,
      // since the colour encodes nesting depth and nobody reads that, and its
      // gold is close enough to the product's amber to read as a state signal
      // (§6). Caught in the first screenshot of the real editor.
      //
      // It has to be switched off **on the model**, not in `editor.create`
      // options. `IEditorOptions.bracketPairColorization` exists, type-checks,
      // and is inert in the standalone build — nothing in Monaco's ESM tree
      // reads it; it is wired through VS Code's configuration service, which is
      // not here. Verified by grepping the shipped module for the option, and
      // then by looking at the editor again with it set and the brackets still
      // gold. The model option is the one that works, and note the name loses
      // the "Pair": `bracketColorizationOptions`.
      model.updateOptions({
        bracketColorizationOptions: {
          enabled: false,
          independentColorPoolPerBracketType: false,
        },
      });
    }

    // A model that already existed may be stale: the file can have changed on
    // disk, or an agent may have rewritten it. Only overwrite when it actually
    // differs, because setValue resets the cursor and clears undo.
    if (model.getValue() !== value) {
      model.setValue(value);
    }

    editor.current.setModel(model);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [monaco, scope, path]);

  // Content replaced from outside — a reload, or a save elsewhere.
  useEffect(() => {
    const model = editor.current?.getModel();
    if (model && model.getValue() !== value) {
      model.setValue(value);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value]);

  useEffect(() => {
    if (monaco) monaco.editor.setTheme(themeName(theme));
  }, [monaco, theme]);

  useEffect(() => {
    editor.current?.updateOptions({ readOnly: readOnly ?? false });
  }, [readOnly]);

  return <div ref={host} className="jarvis-editor" style={{ width: "100%", height: "100%" }} />;
}

/**
 * Forget a file's editor state.
 *
 * Closing a tab should not leave its model — and its undo history — alive for
 * the rest of the session. Called by the surface that owns the tabs, because
 * only it knows when a file is really closed rather than merely hidden.
 */
export async function disposeModel(scope: string, path: string): Promise<void> {
  const monaco = await loadMonaco();
  monaco.editor.getModel(modelUri(monaco, scope, path))?.dispose();
}
