/**
 * What an agent did, in the words a person reads (M29).
 *
 * Tool names are the provider's own — `Read`, `Bash`, Codex's `Shell` — and a
 * person who never opened a terminal should not have to know them. This only
 * picks the verb and the thing acted on; the surface words it in the person's
 * language.
 */

export type Verb = "read" | "edit" | "create" | "run" | "search" | "web" | "task" | "todo" | "other";

const VERBS: Record<string, Verb> = {
  Read: "read",
  Edit: "edit",
  MultiEdit: "edit",
  NotebookEdit: "edit",
  Write: "create",
  Bash: "run",
  PowerShell: "run",
  Shell: "run",
  Glob: "search",
  Grep: "search",
  WebFetch: "web",
  WebSearch: "web",
  Task: "task",
  Agent: "task",
  TodoWrite: "todo",
};

const FILE_VERBS = new Set<Verb>(["read", "edit", "create"]);

export function describeTool(name: string, summary: string): { verb: Verb; target: string } {
  const verb = VERBS[name] ?? "other";
  const target = FILE_VERBS.has(verb) ? shortPath(summary) : summary;
  return { verb, target };
}

/** The last two segments of a path, `/`-separated: enough to recognise it. */
export function shortPath(path: string): string {
  const parts = path.split(/[\\/]+/).filter(Boolean);
  return parts.length <= 2 ? parts.join("/") : parts.slice(-2).join("/");
}
