/**
 * The J.A.R.V.I.S. monogram.
 *
 * The same geometry as the application icon: a stem with a rounded cap and a
 * half-annulus hook, fused into a single contour. Drawn as one path so it stays
 * crisp at rail size (18px) and in the installer header alike.
 */
const GLYPH_PATH =
  "M566 334 A50 50 0 0 1 666 334 L666 600 A150 150 0 0 1 366 600 L466 600 A50 50 0 0 0 566 600 L566 334 Z";

interface LogoProps {
  size?: number;
  /** Draw the amber container behind the glyph, as on the app icon. */
  boxed?: boolean;
  className?: string;
}

export function Logo({ size = 18, boxed = false, className }: LogoProps) {
  if (boxed) {
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 1024 1024"
        className={className}
        aria-hidden="true"
        focusable="false"
      >
        <rect x="64" y="64" width="896" height="896" rx="216" fill="var(--accent)" />
        <path d={GLYPH_PATH} fill="var(--text-on-accent)" />
      </svg>
    );
  }

  return (
    <svg
      width={(size * 300) / 466}
      height={size}
      viewBox="366 284 300 466"
      className={className}
      aria-hidden="true"
      focusable="false"
    >
      <path d={GLYPH_PATH} fill="currentColor" />
    </svg>
  );
}
