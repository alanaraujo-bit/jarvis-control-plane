/** M27: everything a person sets up, carried as last-write-wins documents.
 *  The kinds and setting keys are closed lists, mirrored in the desktop's
 *  `identity/documents.rs`; this side is the trust boundary, so it checks
 *  shape and size and refuses the whole request on anything unexpected. */

export const DOCUMENT_KINDS = new Set([
  "setting", "project", "projectNote", "projectKnowledge", "guardrailPolicy", "providerAccount",
]);

/** Settings carried as documents. M23's nine preferences keep their own
 *  route (`PUT /v1/sync/settings`); these are the ones it never carried. */
export const SETTING_KEYS = new Set([
  "accounts.rotation", "autonomy.global", "startup.minimized", "localai.runtime", "workspace.snapshot.v1",
]);

/** The provider-account registry travels; a login never does. Any other field
 *  (a config directory, a token) is refused, so a bug on the desktop cannot
 *  upload a credential. */
const ACCOUNT_FIELDS = new Set(["provider", "label", "email", "position", "paused"]);

export const MAX_DOCUMENTS = 20_000;
const MAX_ID = 200;
const MAX_BODY = 200_000;

export interface SyncDocument {
  kind: string;
  id: string;
  body: unknown;
  touchedAt: number;
  deletedAt: number | null;
}

const clock = (value: unknown) => (typeof value === "number" && Number.isSafeInteger(value) ? value : null);

export function parseDocuments(input: unknown): SyncDocument[] | null {
  if (!Array.isArray(input) || input.length > MAX_DOCUMENTS) return null;
  const out: SyncDocument[] = [];
  for (const raw of input) {
    if (!raw || typeof raw !== "object") return null;
    const { kind, id, body, touchedAt, deletedAt } = raw as Record<string, unknown>;
    if (typeof kind !== "string" || !DOCUMENT_KINDS.has(kind)) return null;
    if (typeof id !== "string" || id.length === 0 || id.length > MAX_ID) return null;
    const touched = clock(touchedAt);
    if (touched === null) return null;
    if (deletedAt !== null && deletedAt !== undefined && clock(deletedAt) === null) return null;
    if (kind === "setting" && !SETTING_KEYS.has(id)) return null;
    if (kind === "providerAccount") {
      if (!body || typeof body !== "object" || Object.keys(body).some((key) => !ACCOUNT_FIELDS.has(key))) return null;
    }
    const encoded = JSON.stringify(body ?? null);
    if (encoded.length > MAX_BODY) return null;
    out.push({ kind, id, body: body ?? null, touchedAt: touched, deletedAt: clock(deletedAt) });
  }
  return out;
}
