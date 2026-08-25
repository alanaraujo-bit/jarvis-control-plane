#!/usr/bin/env node
/**
 * Serve `dist/` locally.
 *
 * The site is built to open from `file://` as well, so this is a convenience
 * rather than a requirement — but a real origin is the honest way to check
 * anything that depends on one, and it is how the site will actually be read.
 *
 *   node serve.mjs [port]
 */

import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { join, extname, normalize } from "node:path";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "dist");
const PORT = Number(process.argv[2]) || 4180;

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".woff2": "font/woff2",
};

createServer(async (req, res) => {
  try {
    const url = new URL(req.url, "http://localhost");
    // normalize() collapses `..`, and the prefix check catches whatever is
    // left. Serving a build directory is not a security boundary, but a static
    // server that walks out of its root is a bad habit to leave lying around.
    let path = normalize(join(ROOT, decodeURIComponent(url.pathname)));
    if (!path.startsWith(ROOT)) {
      res.writeHead(403).end("forbidden");
      return;
    }

    let info = await stat(path).catch(() => null);
    if (info?.isDirectory()) {
      path = join(path, "index.html");
      info = await stat(path).catch(() => null);
    }
    if (!info) {
      res.writeHead(404, { "content-type": "text/plain; charset=utf-8" }).end("not found");
      return;
    }

    const body = await readFile(path);
    res.writeHead(200, {
      "content-type": TYPES[extname(path)] ?? "application/octet-stream",
      "cache-control": "no-cache",
    }).end(body);
  } catch (cause) {
    res.writeHead(500, { "content-type": "text/plain; charset=utf-8" }).end(String(cause));
  }
}).listen(PORT, () => {
  console.log(`documentation on http://localhost:${PORT}/`);
});
