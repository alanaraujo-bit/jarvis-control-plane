/**
 * The Markdown an agent writes, read into data (M29).
 *
 * A small subset — what agents actually produce — rendered later as React
 * elements, never as HTML. Agent output can carry markup copied from a web
 * page, and this webview has IPC: nothing here ever becomes `innerHTML`, and
 * only `http(s)` addresses ever become links.
 */

export type Inline =
  | { type: "text"; text: string }
  | { type: "code"; text: string }
  | { type: "strong"; children: Inline[] }
  | { type: "em"; children: Inline[] }
  | { type: "link"; href: string; children: Inline[] };

export type Block =
  | { type: "heading"; level: 1 | 2 | 3; inline: Inline[] }
  | { type: "paragraph"; inline: Inline[] }
  | { type: "list"; ordered: boolean; start: number; items: Block[][] }
  | { type: "quote"; blocks: Block[] }
  | { type: "code"; lang: string; text: string }
  | { type: "table"; head: Inline[][]; rows: Inline[][][] }
  | { type: "rule" };

const FENCE = /^\s{0,3}(`{3,}|~{3,})\s*([^`\s]*)/;
const HEADING = /^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
const RULE = /^\s{0,3}([-*_])(\s*\1){2,}\s*$/;
const QUOTE = /^\s{0,3}>\s?(.*)$/;
const ITEM = /^(\s*)([-*+]|\d{1,9}[.)])\s+(.*)$/;
const TABLE_RULE = /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/;
const SAFE_LINK = /^https?:\/\//i;

export function parseMarkdown(text: string): Block[] {
  return parseLines(text.replace(/\r\n?/g, "\n").split("\n"));
}

function parseLines(lines: string[]): Block[] {
  const blocks: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) {
      i += 1;
      continue;
    }

    const fence = FENCE.exec(line);
    if (fence) {
      const marker = fence[1];
      const body: string[] = [];
      i += 1;
      // An unterminated fence is still code: a reply streaming in has opened
      // its block and not closed it yet.
      while (i < lines.length && !lines[i].trim().startsWith(marker)) {
        body.push(lines[i]);
        i += 1;
      }
      i += 1;
      blocks.push({ type: "code", lang: fence[2] ?? "", text: body.join("\n") });
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) {
      const level = Math.min(3, heading[1].length) as 1 | 2 | 3;
      blocks.push({ type: "heading", level, inline: parseInline(heading[2]) });
      i += 1;
      continue;
    }

    if (RULE.test(line)) {
      blocks.push({ type: "rule" });
      i += 1;
      continue;
    }

    if (QUOTE.test(line)) {
      const body: string[] = [];
      while (i < lines.length && QUOTE.test(lines[i])) {
        body.push(QUOTE.exec(lines[i])![1]);
        i += 1;
      }
      blocks.push({ type: "quote", blocks: parseLines(body) });
      continue;
    }

    if (line.includes("|") && i + 1 < lines.length && TABLE_RULE.test(lines[i + 1])) {
      const head = cells(line);
      const rows: Inline[][][] = [];
      i += 2;
      while (i < lines.length && lines[i].includes("|") && lines[i].trim()) {
        rows.push(cells(lines[i]));
        i += 1;
      }
      blocks.push({ type: "table", head, rows });
      continue;
    }

    const item = ITEM.exec(line);
    if (item) {
      const indent = item[1].length;
      const ordered = /\d/.test(item[2]);
      const start = ordered ? Number.parseInt(item[2], 10) : 1;
      const items: Block[][] = [];
      while (i < lines.length) {
        const current = ITEM.exec(lines[i]);
        if (!current || current[1].length !== indent || /\d/.test(current[2]) !== ordered) break;
        const body = [current[3]];
        i += 1;
        // Everything indented past the marker belongs to this item, nested
        // lists included; one blank line inside an item does not end it.
        while (i < lines.length) {
          const next = lines[i];
          if (!next.trim()) {
            const following = lines[i + 1];
            if (following !== undefined && leading(following) > indent) {
              body.push("");
              i += 1;
              continue;
            }
            break;
          }
          if (leading(next) <= indent) break;
          body.push(next.slice(Math.min(leading(next), indent + 2)));
          i += 1;
        }
        items.push(parseLines(body));
      }
      blocks.push({ type: "list", ordered, start, items });
      continue;
    }

    // The first line is taken unconditionally, so a line no branch above
    // claimed can never stall the loop.
    const paragraph: string[] = [line.trim()];
    i += 1;
    while (i < lines.length && lines[i].trim() && !startsBlock(lines, i)) {
      paragraph.push(lines[i].trim());
      i += 1;
    }
    blocks.push({ type: "paragraph", inline: parseInline(paragraph.join("\n")) });
  }
  return blocks;
}

function leading(line: string): number {
  return line.length - line.trimStart().length;
}

function startsBlock(lines: string[], i: number): boolean {
  const line = lines[i];
  return (
    FENCE.test(line) ||
    HEADING.test(line) ||
    RULE.test(line) ||
    QUOTE.test(line) ||
    ITEM.test(line) ||
    (line.includes("|") && i + 1 < lines.length && TABLE_RULE.test(lines[i + 1]))
  );
}

function cells(line: string): Inline[][] {
  let row = line.trim();
  if (row.startsWith("|")) row = row.slice(1);
  if (row.endsWith("|") && !row.endsWith("\\|")) row = row.slice(0, -1);
  return row.split(/(?<!\\)\|/).map((cell) => parseInline(cell.trim().replace(/\\\|/g, "|")));
}

const BARE_URL = /https?:\/\/[^\s<>()[\]]*[^\s<>()[\].,;:!?'"]/y;

/** Inline Markdown: code, strong, emphasis, links and bare addresses. */
export function parseInline(text: string): Inline[] {
  const out: Inline[] = [];
  let plain = "";
  const flush = () => {
    if (plain) out.push({ type: "text", text: plain });
    plain = "";
  };

  let i = 0;
  while (i < text.length) {
    const ch = text[i];

    if (ch === "\\" && i + 1 < text.length && /[\\`*_[\]()#|>-]/.test(text[i + 1])) {
      plain += text[i + 1];
      i += 2;
      continue;
    }

    if (ch === "`") {
      const run = /^`+/.exec(text.slice(i))![0];
      const close = text.indexOf(run, i + run.length);
      if (close !== -1) {
        flush();
        out.push({ type: "code", text: text.slice(i + run.length, close).replace(/^ (.*) $/s, "$1") });
        i = close + run.length;
        continue;
      }
      plain += run;
      i += run.length;
      continue;
    }

    if ((ch === "*" || ch === "_") && text[i + 1] === ch) {
      const marker = ch + ch;
      const close = text.indexOf(marker, i + 2);
      if (close > i + 2 && wordBoundary(text, i, close + 2, ch)) {
        flush();
        out.push({ type: "strong", children: parseInline(text.slice(i + 2, close)) });
        i = close + 2;
        continue;
      }
    }

    if ((ch === "*" || ch === "_") && text[i + 1] !== ch && text[i + 1] !== " " && text[i + 1] !== undefined) {
      const close = findSingle(text, i + 1, ch);
      if (close > i + 1 && wordBoundary(text, i, close + 1, ch)) {
        flush();
        out.push({ type: "em", children: parseInline(text.slice(i + 1, close)) });
        i = close + 1;
        continue;
      }
    }

    if (ch === "[") {
      const link = /^\[([^\]]*)\]\(([^)\s]*)\)/.exec(text.slice(i));
      if (link) {
        if (SAFE_LINK.test(link[2])) {
          flush();
          out.push({ type: "link", href: link[2], children: parseInline(link[1]) });
        } else {
          // Not an address this app opens: the whole thing stays readable text.
          plain += link[0];
        }
        i += link[0].length;
        continue;
      }
    }

    if ((ch === "h" || ch === "H") && /^https?:\/\//i.test(text.slice(i, i + 8))) {
      BARE_URL.lastIndex = i;
      const url = BARE_URL.exec(text);
      if (url) {
        flush();
        out.push({ type: "link", href: url[0], children: [{ type: "text", text: url[0] }] });
        i += url[0].length;
        continue;
      }
    }

    plain += ch;
    i += 1;
  }
  flush();
  return out;
}

/** `snake_case_names` are not emphasis: `_` only counts at word edges. */
function wordBoundary(text: string, open: number, end: number, ch: string): boolean {
  if (ch !== "_") return true;
  const before = text[open - 1];
  const after = text[end];
  return !(before && /\w/.test(before)) && !(after && /\w/.test(after));
}

function findSingle(text: string, from: number, ch: string): number {
  for (let j = from; j < text.length; j += 1) {
    if (text[j] === ch && text[j + 1] !== ch && text[j - 1] !== ch && text[j - 1] !== " ") return j;
  }
  return -1;
}
