import { strict as assert } from "node:assert";
import { test } from "node:test";

import { parseInline, parseMarkdown, type Inline } from "./markdown.ts";

const text = (inline: Inline[]): string =>
  inline
    .map((node) => (node.type === "text" || node.type === "code" ? node.text : text(node.children)))
    .join("");

test("headings, paragraphs and rules", () => {
  const blocks = parseMarkdown("# Title\n\nFirst line\nsecond line\n\n---\n#### deep");
  assert.equal(blocks[0].type, "heading");
  assert.deepEqual(blocks[1], { type: "paragraph", inline: [{ type: "text", text: "First line\nsecond line" }] });
  assert.equal(blocks[2].type, "rule");
  assert.deepEqual(blocks[3], { type: "heading", level: 3, inline: [{ type: "text", text: "deep" }] });
});

test("a fenced block keeps its text and language exactly", () => {
  const [code] = parseMarkdown("```ts\nconst a = 1;\n  indented();\n```\nafter");
  assert.deepEqual(code, { type: "code", lang: "ts", text: "const a = 1;\n  indented();" });
});

test("a fence still streaming in is already a code block", () => {
  const blocks = parseMarkdown("Here:\n```bash\npnpm test");
  assert.deepEqual(blocks[1], { type: "code", lang: "bash", text: "pnpm test" });
});

test("lists, ordered and nested", () => {
  const [list] = parseMarkdown("1. one\n2. two\n   - inner\n3. three");
  assert.equal(list.type, "list");
  if (list.type !== "list") return;
  assert.equal(list.ordered, true);
  assert.equal(list.items.length, 3);
  const second = list.items[1];
  assert.equal(second[0].type, "paragraph");
  assert.equal(second[1].type, "list", "the indented bullet nests under its item");
});

test("a list right after a paragraph line still starts a list", () => {
  const blocks = parseMarkdown("I changed:\n- a.ts\n- b.ts");
  assert.equal(blocks[0].type, "paragraph");
  assert.equal(blocks[1].type, "list");
});

test("tables", () => {
  const [table] = parseMarkdown("| File | Lines |\n|---|---:|\n| `a.ts` | 3 |\n| b.ts | 4 |");
  assert.equal(table.type, "table");
  if (table.type !== "table") return;
  assert.equal(text(table.head[0]), "File");
  assert.equal(table.rows.length, 2);
  assert.deepEqual(table.rows[0][0], [{ type: "code", text: "a.ts" }]);
});

test("quotes hold blocks", () => {
  const [quote] = parseMarkdown("> **Note**\n> keep it");
  assert.equal(quote.type, "quote");
});

test("inline code, strong, emphasis", () => {
  assert.deepEqual(parseInline("run `pnpm test` **now** *please*"), [
    { type: "text", text: "run " },
    { type: "code", text: "pnpm test" },
    { type: "text", text: " " },
    { type: "strong", children: [{ type: "text", text: "now" }] },
    { type: "text", text: " " },
    { type: "em", children: [{ type: "text", text: "please" }] },
  ]);
});

test("snake_case is not emphasis", () => {
  assert.deepEqual(parseInline("call read_file_now here"), [{ type: "text", text: "call read_file_now here" }]);
});

test("only http(s) addresses become links", () => {
  assert.deepEqual(parseInline("[docs](https://example.com/a)"), [
    { type: "link", href: "https://example.com/a", children: [{ type: "text", text: "docs" }] },
  ]);
  const unsafe = parseInline("[x](javascript:alert(1))");
  assert.ok(unsafe.every((node) => node.type === "text"), JSON.stringify(unsafe));
  assert.ok(text(unsafe).includes("javascript:"), "kept readable, never clickable");
  const file = parseInline("[app](file:///C:/a.ts)");
  assert.ok(file.every((node) => node.type === "text"));
});

test("a bare address becomes a link without its trailing punctuation", () => {
  const nodes = parseInline("see https://example.com/x.");
  assert.deepEqual(nodes[1], { type: "link", href: "https://example.com/x", children: [{ type: "text", text: "https://example.com/x" }] });
  assert.deepEqual(nodes[2], { type: "text", text: "." });
});

test("raw HTML stays text", () => {
  const blocks = parseMarkdown('<script>alert(1)</script>\n<img src=x onerror="y">');
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].type, "paragraph");
  if (blocks[0].type !== "paragraph") return;
  assert.ok(blocks[0].inline.every((node) => node.type === "text"));
  assert.ok(text(blocks[0].inline).includes("<script>"));
});

test("nothing unusual makes it loop", () => {
  for (const sample of ["|", "| a |", "#", "-", "* ", "```", "> ", "1.", "__", "**x", "[a](", "`"]) {
    parseMarkdown(sample);
  }
});
