# M27 — Everything a person sets up follows their account

**State: in progress (2026-10-18).**

## Why

Alan: uninstall, reinstall, sign in, and nothing comes back. He wants
everything he personalises saved to his account: prompts, texts, settings,
accounts.

## What was actually wrong (measured 2026-10-18)

1. **The server is gone.** `https://social-api-production-edb6.up.railway.app/health`
   answers `Application not found`. The Railway project that held the
   `social-api` service and its Postgres (`jarvis-social`) no longer exists in
   the account (other projects there are scheduled for deletion on 2026-10-19).
   With no server, every cloud sign-in fails, a password sign-in quietly falls
   back to a local-only account, and nothing is saved anywhere. The pushes are
   fire-and-forget, so nothing on screen ever said so.
2. **Most of what a person sets up never went up, even when the server was
   alive.** M23 carries nine preferences and the notebook. Not carried: the
   rotation policy, global autonomy, start minimised, the local-model runtime,
   the workspace layout, the project list, project notes and knowledge,
   guardrail policies, and the provider-account registry.
3. **Core-owned preferences never reach the account copy.** `identity_settings`
   is only written through `identity_remember`, which the screens call. A value
   the core writes itself (rotation policy, autonomy) was never mirrored.

## Decisions

- **Same stack, new home.** The existing `apps/social-api` (Node + `pg`) is
  redeployed to a new Railway project named `jarvis-account-sync`, with its own
  Postgres. The name cannot be confused with `jarvis-guardian`, which must
  not be touched (BLOCKERS B3). Railway was asked for by Alan.
- **One generic, last-write-wins document sync** carries everything new,
  instead of one endpoint per table:
  `identity_documents(user_id, kind, id, body JSONB, touched_at, deleted_at)`,
  with `PUT /v1/sync/documents` and the documents returned in every state
  answer. The kinds are a closed list on both sides:

  | kind | id | body | applied on another machine as |
  |---|---|---|---|
  | `setting` | the key | the JSON value | the machine `settings` row |
  | `project` | the project id | name, path, autonomy, archived | a registered project, **only if the folder exists here** |
  | `projectNote` | note id | project path, body, pinned, times | a note on the project with that path |
  | `projectKnowledge` | entry id | project path, kind, body, source, archived, times | a knowledge entry on that project |
  | `guardrailPolicy` | policy id | project path (or global), operation, decision | a policy on that project (or global) |
  | `providerAccount` | account id | provider, label, email, position, paused | a **signed-out** account row with a fresh, empty config dir |

  Carried setting keys (new, on top of M23's nine): `accounts.rotation`,
  `autonomy.global`, `startup.minimized`, `localai.runtime`,
  `workspace.snapshot.v1`.
- **Provider credentials still never leave the machine** (AGENTS.md
  non-negotiable; `identity/cloud.rs` boundary). The account registry travels
  (which Claude/Codex accounts exist, their names and order), the logins do
  not. After a reinstall the accounts reappear, signed out, each with "Sign in".
  A copied login is not carried at all, since it is a copy of this machine's.
- **Stays local, as before:** sessions, transcripts, usage and analytics,
  missions, `onboarding.seen`, relay pairing, `identity.*`, `sync.*`,
  `social.token`. Missions reference sessions and evidence that stay here.
- **A local ledger gives timestamps without touching every table.**
  `sync_documents(kind, id, hash, touched_at, deleted_at)` (migration 22). On
  each push the current documents are hashed; a changed hash gets
  `touched_at = now`, a document that disappeared gets a tombstone. Applying a
  remote document happens only when its `touched_at` is newer than the
  ledger's, and the ledger then takes the remote's hash and time.
- **Deletions that would destroy something are not applied remotely:** a
  `providerAccount` tombstone never deletes a local account (its directory
  holds credentials), and a `project` tombstone archives the local project
  rather than unregistering it. Note, knowledge and policy tombstones delete.
- **Pushes:** after every pull and sign-in, and from a background loop every
  60 s while signed in (only when a hash changed). No command needs wiring.
- **Sync is no longer silent to the person.** Every cloud call records its
  outcome (`sync.status`: last success time, or the error and when). Settings →
  Account shows "Saved to your account · 16:05" or what is wrong, with "Sync
  now". A machine signed in locally but with no cloud session says so and
  offers to sign in again.
- **An account the server lost is re-created on the next sign-in.** When this
  machine's own hash accepts the password and the server says it does not know
  the account, the desktop signs up with the same credentials and pushes
  everything it has. That is the path that brings Alan's existing local
  account back onto the new server.

## Not in scope

- Syncing provider logins (non-negotiable).
- Sessions, transcripts, analytics, missions.
- Live multi-machine collaboration; last write wins per document is enough.
- Google sign-in needs the OAuth client id and secret, which lived only in the
  deleted Railway project, and the callback URL registered in Google Cloud must
  point at the new domain. Until Alan provides those, the Google button
  answers "unavailable" honestly; password accounts work.

## Acceptance

1. `GET /health` on the new domain answers `{ok:true}`.
2. Against the deployed server, a scratch account: sign up, push documents of
   every kind, pull, and get them back; a stale `touched_at` does not
   overwrite a newer one; a tombstone wins over an older body.
3. Desktop unit tests: the ledger marks changed and deleted documents, applies
   only newer remote documents, restores a project only when its folder
   exists, restores notes onto the project by path, restores provider accounts
   signed out with an empty config dir, never deletes an account on a
   tombstone.
4. **The real scenario, in the app:** on a QA identifier, sign in, set the
   things listed above (prompt, project note, rotation policy, autonomy, theme,
   a provider account), wipe the QA data directory (a reinstall), launch, sign
   in: everything is back, the provider account signed out.
5. Settings → Account shows the sync status, in both themes and languages.
6. `pnpm verify` green.

## Plan

1. Server: `identity_documents`, `PUT /v1/sync/documents`, documents in
   `stateFor`, new setting keys in the allowlist; document validation as a pure
   module with `node --test` tests.
2. Deploy to Railway `jarvis-account-sync` (Postgres + service, env, domain);
   run the acceptance-2 script against it.
3. Desktop: `ORIGIN` to the new domain; migration 22 `sync_documents`;
   `identity/documents.rs` (collect, ledger, apply) with tests; wire into
   apply/push/pull and the 60 s loop.
4. Desktop: re-create a lost server account on sign-in; record sync status;
   commands for status and "sync now".
5. UI: Settings → Account status line, "Sync now", the re-sign-in notice; i18n
   en + pt-BR.
6. App QA (acceptance 4–5), `pnpm verify`, docs (this file, HANDOFF, DECISIONS
   D52, M23 pointer).
