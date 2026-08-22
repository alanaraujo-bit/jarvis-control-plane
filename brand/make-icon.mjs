/**
 * J.A.R.V.I.S. mark generator.
 *
 * Renders the monogram with signed-distance fields and 4x supersampling so the
 * geometry stays crisp at every size the installer and taskbar ask for, then
 * encodes a PNG directly (zlib is in the stdlib; no image dependency needed).
 *
 * The mark: a geometric J built from a stem and a half-annulus hook. It reads
 * as a plumb line — a precision instrument, not a sci-fi glyph (§5).
 */
import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";

const S = 1024;
const SS = 4; // supersampling factor

// ---- SDF primitives (all in unsupersampled coordinates) -------------------
const sdRoundRect = (px, py, cx, cy, hw, hh, r) => {
  const qx = Math.abs(px - cx) - (hw - r);
  const qy = Math.abs(py - cy) - (hh - r);
  return Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0) - r;
};

/** Half-annulus opening upward: the bottom hook of the J. */
const sdHookBottom = (px, py, cx, cy, rOuter, rInner) => {
  const d = Math.hypot(px - cx, py - cy);
  const ring = Math.max(d - rOuter, rInner - d);
  if (py >= cy) return ring;
  // Above the centre line the hook is capped flat; distance to the two arm ends.
  const armMid = (rOuter + rInner) / 2;
  const armHalf = (rOuter - rInner) / 2;
  const left = sdRoundRect(px, py, cx - armMid, cy, armHalf, 1, 0);
  const right = sdRoundRect(px, py, cx + armMid, cy, armHalf, 1, 0);
  return Math.min(left, right);
};

const mix = (a, b, t) => a + (b - a) * t;
const clamp01 = (v) => (v < 0 ? 0 : v > 1 ? 1 : v);

// ---- Palette --------------------------------------------------------------
const AMBER_TOP = [0xe0, 0xac, 0x63];
const AMBER_BOT = [0xc6, 0x8f, 0x42];
const INK = [0x16, 0x13, 0x0e];

function render() {
  const px = Buffer.alloc(S * S * 4);
  const n = SS * SS;

  for (let y = 0; y < S; y++) {
    for (let x = 0; x < S; x++) {
      let fieldA = 0; // rounded-square coverage
      let glyphA = 0; // monogram coverage

      for (let sy = 0; sy < SS; sy++) {
        for (let sx = 0; sx < SS; sx++) {
          const fx = x + (sx + 0.5) / SS;
          const fy = y + (sy + 0.5) / SS;

          // App-icon container: a squircle-ish rounded square.
          const dField = sdRoundRect(fx, fy, 512, 512, 448, 448, 216);
          fieldA += clamp01(0.5 - dField);

          // Monogram: stem + hook, nudged so the optical centre lands centred.
          const gx = fx + 4;
          const gy = fy + 5;
          // Stem: rounded cap on top, flat bottom landing on the hook axis so
          // the two shapes fuse into one contour with no seam.
          const stemCap = sdRoundRect(gx, gy, 616, 443, 50, 159, 50);
          const stemFoot = sdRoundRect(gx, gy, 616, 520, 50, 82, 0);
          const stem = Math.min(stemCap, stemFoot);
          const hook = sdHookBottom(gx, gy, 516, 600, 150, 50);
          const d = Math.min(stem, hook);
          glyphA += clamp01(0.5 - d);
        }
      }
      fieldA /= n;
      glyphA /= n;

      const t = y / S;
      let r = mix(AMBER_TOP[0], AMBER_BOT[0], t);
      let g = mix(AMBER_TOP[1], AMBER_BOT[1], t);
      let b = mix(AMBER_TOP[2], AMBER_BOT[2], t);

      // Knock the monogram out of the amber field in near-black ink.
      r = mix(r, INK[0], glyphA);
      g = mix(g, INK[1], glyphA);
      b = mix(b, INK[2], glyphA);

      const o = (y * S + x) * 4;
      px[o] = r; px[o + 1] = g; px[o + 2] = b;
      px[o + 3] = Math.round(fieldA * 255);
    }
  }
  return px;
}

// ---- Minimal PNG encoder --------------------------------------------------
const crcTable = (() => {
  const t = new Int32Array(256);
  for (let i = 0; i < 256; i++) {
    let c = i;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[i] = c;
  }
  return t;
})();
const crc32 = (buf) => {
  let c = -1;
  for (const byte of buf) c = crcTable[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
};
const chunk = (type, data) => {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
};

function encodePng(rgba, size) {
  // Filter type 0 (None) per scanline.
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y++) {
    raw[y * (size * 4 + 1)] = 0;
    rgba.copy(raw, y * (size * 4 + 1) + 1, y * size * 4, (y + 1) * size * 4);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8;  // bit depth
  ihdr[9] = 6;  // colour type RGBA
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

const out = process.argv[2] || "brand/icon.png";
writeFileSync(out, encodePng(render(), S));
console.log(`wrote ${out} (${S}x${S})`);
