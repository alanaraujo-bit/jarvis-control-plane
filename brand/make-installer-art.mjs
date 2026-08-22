/**
 * Installer artwork.
 *
 * NSIS wants uncompressed BMPs at fixed sizes, so these are rendered with the
 * same signed-distance geometry as the application icon rather than being
 * traced by hand — the mark stays identical everywhere it appears.
 *
 * The experience of the product starts before the app opens (§12), so the
 * installer carries real identity rather than a default grey banner.
 */
import { writeFileSync } from "node:fs";

// ---- Geometry (shared with make-icon.mjs) ---------------------------------
const sdRoundRect = (px, py, cx, cy, hw, hh, r) => {
  const qx = Math.abs(px - cx) - (hw - r);
  const qy = Math.abs(py - cy) - (hh - r);
  return Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0) - r;
};

const sdHookBottom = (px, py, cx, cy, rOuter, rInner) => {
  const d = Math.hypot(px - cx, py - cy);
  const ring = Math.max(d - rOuter, rInner - d);
  if (py >= cy) return ring;
  const armMid = (rOuter + rInner) / 2;
  const armHalf = (rOuter - rInner) / 2;
  return Math.min(
    sdRoundRect(px, py, cx - armMid, cy, armHalf, 1, 0),
    sdRoundRect(px, py, cx + armMid, cy, armHalf, 1, 0),
  );
};

/** The monogram, in its own 1024-unit space, as a distance field. */
const sdGlyph = (x, y) => {
  const gx = x + 4;
  const gy = y + 5;
  const stem = Math.min(
    sdRoundRect(gx, gy, 616, 443, 50, 159, 50),
    sdRoundRect(gx, gy, 616, 520, 50, 82, 0),
  );
  return Math.min(stem, sdHookBottom(gx, gy, 516, 600, 150, 50));
};

const clamp01 = (v) => (v < 0 ? 0 : v > 1 ? 1 : v);
const mix = (a, b, t) => a + (b - a) * t;

// ---- Palette: the product's dark surface, with the amber mark --------------
const BG_TOP = [0x16, 0x16, 0x18];
const BG_BOTTOM = [0x0b, 0x0b, 0x0c];
const AMBER = [0xd9, 0xa5, 0x5c];

/**
 * 24-bit BMP encoder. Rows are bottom-up and padded to a 4-byte boundary.
 */
function encodeBmp(width, height, rgbAt) {
  const rowSize = Math.ceil((width * 3) / 4) * 4;
  const pixelBytes = rowSize * height;
  const buffer = Buffer.alloc(54 + pixelBytes);

  buffer.write("BM", 0, "ascii");
  buffer.writeUInt32LE(54 + pixelBytes, 2);
  buffer.writeUInt32LE(54, 10); // pixel data offset
  buffer.writeUInt32LE(40, 14); // DIB header size
  buffer.writeInt32LE(width, 18);
  buffer.writeInt32LE(height, 22);
  buffer.writeUInt16LE(1, 26); // planes
  buffer.writeUInt16LE(24, 28); // bits per pixel
  buffer.writeUInt32LE(pixelBytes, 34);
  buffer.writeInt32LE(2835, 38); // ~72 DPI
  buffer.writeInt32LE(2835, 42);

  for (let y = 0; y < height; y++) {
    // BMP stores the bottom row first.
    const row = 54 + (height - 1 - y) * rowSize;
    for (let x = 0; x < width; x++) {
      const [r, g, b] = rgbAt(x, y);
      const o = row + x * 3;
      buffer[o] = b;
      buffer[o + 1] = g;
      buffer[o + 2] = r;
    }
  }
  return buffer;
}

/** Supersampled coverage of the monogram at a given placement. */
function glyphCoverage(x, y, { cx, cy, size }) {
  const SS = 3;
  let total = 0;
  for (let sy = 0; sy < SS; sy++) {
    for (let sx = 0; sx < SS; sx++) {
      const fx = x + (sx + 0.5) / SS;
      const fy = y + (sy + 0.5) / SS;
      // Map the pixel back into the glyph's 1024-unit space.
      const gx = ((fx - cx) / size) * 466 + 512;
      const gy = ((fy - cy) / size) * 466 + 512;
      total += clamp01(0.5 - sdGlyph(gx, gy) * (size / 466));
    }
  }
  return total / (SS * SS);
}

function render(width, height, placement) {
  return encodeBmp(width, height, (x, y) => {
    const t = y / height;
    let r = mix(BG_TOP[0], BG_BOTTOM[0], t);
    let g = mix(BG_TOP[1], BG_BOTTOM[1], t);
    let b = mix(BG_TOP[2], BG_BOTTOM[2], t);

    const a = glyphCoverage(x, y, placement);
    r = mix(r, AMBER[0], a);
    g = mix(g, AMBER[1], a);
    b = mix(b, AMBER[2], a);
    return [Math.round(r), Math.round(g), Math.round(b)];
  });
}

// NSIS header banner: the mark sits at the right, where the wizard leaves space.
writeFileSync(
  "apps/desktop/src-tauri/installer/header.bmp",
  render(150, 57, { cx: 118, cy: 28, size: 34 }),
);

// NSIS welcome/finish sidebar: the mark sits high, above the wizard text.
writeFileSync(
  "apps/desktop/src-tauri/installer/sidebar.bmp",
  render(164, 314, { cx: 82, cy: 96, size: 84 }),
);

console.log("wrote installer/header.bmp (150x57) and installer/sidebar.bmp (164x314)");
