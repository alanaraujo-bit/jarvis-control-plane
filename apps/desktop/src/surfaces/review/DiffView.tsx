import { useT } from "../../app/i18n";
import type { FileDiff, Hunk } from "./useReview";

interface DiffViewProps {
  diff: FileDiff;
}

/**
 * A unified diff, rendered from structured hunks (§43).
 *
 * The hunks come from Git via the Rust core — nothing here decides what
 * changed, only how to show it. Line numbers are rendered from the parsed
 * positions rather than counted in the browser, because the counting is
 * exactly the part that goes wrong around blank context lines and
 * single-line hunks.
 */
export function DiffView({ diff }: DiffViewProps) {
  const t = useT();

  if (diff.binary) {
    return <p className="diff__note">{t("review.binary")}</p>;
  }

  // Checked before the empty-hunks case below, which would otherwise claim
  // that no line changed in a file that is entirely new.
  if (diff.tooLarge) {
    return <p className="diff__note">{t("review.tooLarge")}</p>;
  }

  if (diff.hunks.length === 0) {
    return <p className="diff__note">{t("review.noTextChange")}</p>;
  }

  return (
    <div className="diff">
      {diff.hunks.map((hunk, index) => (
        <HunkBlock key={`${hunk.oldStart}-${hunk.newStart}-${index}`} hunk={hunk} />
      ))}
      {/* The count is the number of lines actually rendered, not a constant
          repeated into the translation — a duplicated cap goes stale silently
          and then the sentence is simply wrong. */}
      {diff.truncated && (
        <p className="diff__note">
          {t("review.truncated", {
            count: diff.hunks.reduce((total, hunk) => total + hunk.lines.length, 0),
          })}
        </p>
      )}
    </div>
  );
}

function HunkBlock({ hunk }: { hunk: Hunk }) {
  return (
    <div className="diff__hunk">
      <div className="diff__hunk-header">
        <span className="diff__hunk-range">
          @@ −{hunk.oldStart} +{hunk.newStart} @@
        </span>
        {/* Git works out the enclosing function when it can. Free context. */}
        {hunk.heading && <span className="diff__hunk-heading">{hunk.heading}</span>}
      </div>

      {hunk.lines.map((line, index) => (
        <div key={index} className="diff__line" data-kind={line.kind}>
          <span className="diff__gutter">{line.oldLine ?? ""}</span>
          <span className="diff__gutter">{line.newLine ?? ""}</span>
          <span className="diff__sign" aria-hidden="true">
            {line.kind === "added" ? "+" : line.kind === "removed" ? "−" : " "}
          </span>
          {/* Rendered as-is: a diff must show exactly the bytes on the line,
              tabs and trailing spaces included, or it is not evidence. */}
          <span className="diff__text">{line.text || " "}</span>
        </div>
      ))}
    </div>
  );
}
