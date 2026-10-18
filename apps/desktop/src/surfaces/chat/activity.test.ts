import { strict as assert } from "node:assert";
import { test } from "node:test";

import { describeTool, shortPath } from "./activity.ts";

test("file tools name the file by its last two segments", () => {
  assert.deepEqual(describeTool("Read", "C:\\a\\b\\src\\app.ts"), { verb: "read", target: "src/app.ts" });
  assert.deepEqual(describeTool("Edit", "/home/x/login.ts"), { verb: "edit", target: "x/login.ts" });
  assert.deepEqual(describeTool("Write", "hello.txt"), { verb: "create", target: "hello.txt" });
});

test("commands keep their text, from either provider", () => {
  assert.deepEqual(describeTool("Bash", "pnpm test"), { verb: "run", target: "pnpm test" });
  assert.deepEqual(describeTool("Shell", "Set-Content a.txt"), { verb: "run", target: "Set-Content a.txt" });
});

test("searches, the web, subagents and todos", () => {
  assert.equal(describeTool("Grep", "TODO").verb, "search");
  assert.equal(describeTool("WebSearch", "tauri channels").verb, "web");
  assert.equal(describeTool("Task", "explore").verb, "task");
  assert.equal(describeTool("TodoWrite", "").verb, "todo");
});

test("a tool nobody named is still said, not hidden", () => {
  assert.deepEqual(describeTool("mcp__github__create_issue", "Fix it"), { verb: "other", target: "Fix it" });
});

test("short paths", () => {
  assert.equal(shortPath("a"), "a");
  assert.equal(shortPath("C:/x/y/z.ts"), "y/z.ts");
});
