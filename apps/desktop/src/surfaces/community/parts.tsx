import { Fragment, type ReactNode } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { MessageKey } from "@jarvis/i18n";
import { useT } from "../../app/i18n";
import { isTauri } from "../../app/platform";
import { initials, type Visibility } from "./api";

/** A neutral monogram. Colour in this product reports state, so an avatar
 *  carries none; "working now" is the only colour a person gets. */
export function Avatar({ name, size = 36, working = false }: { name: string; size?: number; working?: boolean }) {
  return (
    <span className="cm-avatar" style={{ width: size, height: size, fontSize: size * 0.36 }} data-working={working || undefined} aria-hidden="true">
      {initials(name)}
    </span>
  );
}

const ERRORS: Record<string, MessageKey> = {
  "social.handleInvalid": "community.error.handleInvalid",
  "social.handleTaken": "community.error.handleTaken",
  "social.nameRequired": "community.error.nameRequired",
  "social.nameTooLong": "community.error.tooLong",
  "social.bioTooLong": "community.error.tooLong",
  "social.locationTooLong": "community.error.tooLong",
  "social.buildingTooLong": "community.error.tooLong",
  "social.bodyTooLong": "community.error.tooLong",
  "social.titleTooLong": "community.error.tooLong",
  "social.promptTooLong": "community.error.tooLong",
  "social.websiteInvalid": "community.error.websiteInvalid",
  "social.githubInvalid": "community.error.githubInvalid",
  "social.stackInvalid": "community.error.stackInvalid",
  "social.bodyRequired": "community.error.bodyRequired",
  "social.titleRequired": "community.error.titleRequired",
  "social.promptRequired": "community.error.promptRequired",
  "social.tagInvalid": "community.error.tags",
  "social.tooManyTags": "community.error.tags",
  "social.slowDown": "community.error.slowDown",
  "social.unreachable": "community.error.unreachable",
};

/** The core answers with a stable code; this says it in the person's language. */
export function useErrorText() {
  const t = useT();
  return (cause: unknown) => t(ERRORS[String(cause)] ?? "community.error.generic");
}

export function openExternal(url: string) {
  void (isTauri() ? openUrl(url) : window.open(url, "_blank", "noopener"));
}

const URL_PATTERN = /(https?:\/\/[^\s<>()]+[^\s<>().,;:!?'"])/g;

/** Plain text with its links made clickable. Never HTML: what people write is
 *  rendered as text, line breaks and all. */
export function RichText({ text }: { text: string }) {
  const parts = text.split(URL_PATTERN);
  return (
    <>
      {parts.map((part, index) =>
        index % 2 === 1 ? (
          <a key={index} href={part} onClick={(event) => { event.preventDefault(); openExternal(part); }}>
            {part}
          </a>
        ) : (
          <Fragment key={index}>{part}</Fragment>
        ),
      )}
    </>
  );
}

const LEVELS: Visibility[] = ["public", "followers", "private"];

export function VisibilityPicker({ label, hint, value, onChange }: { label: string; hint: string; value: Visibility; onChange: (value: Visibility) => void }) {
  const t = useT();
  return (
    <div className="cm-visibility">
      <div>
        <span className="cm-visibility__label">{label}</span>
        <p className="cm-hint">{hint}</p>
      </div>
      <div className="cm-segmented" role="radiogroup" aria-label={label}>
        {LEVELS.map((level) => (
          <button key={level} type="button" role="radio" aria-checked={value === level} data-active={value === level || undefined} onClick={() => onChange(level)}>
            {t(`community.visibility.${level}` as MessageKey)}
          </button>
        ))}
      </div>
    </div>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  return <p className="cm-empty">{children}</p>;
}

export function formatCount(value: number, locale: string): string {
  return new Intl.NumberFormat(locale, { notation: value >= 10_000 ? "compact" : "standard", maximumFractionDigits: 1 }).format(value);
}
