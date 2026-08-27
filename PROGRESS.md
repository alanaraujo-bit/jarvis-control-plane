# Social network delivery log

## Current objective

Deliver J.A.R.V.I.S. Friends: real profiles, bilateral friendships, opted-in
presence, private-by-default aggregate metrics, activity feed and a polished
desktop surface.

## Infrastructure

- Railway project: `jarvis-social` (isolated from existing projects).
- PostgreSQL service: `Postgres`.
- Public API: `https://social-api-production-edb6.up.railway.app`.
- API health was verified on 2026-08-27.

## Decisions

- The existing mobile relay remains a private desktop-to-phone mailbox.
- Social data is stored in Railway Postgres; projects, prompts, transcripts,
  terminal output and credentials never leave the desktop.
- Social participation is additive: normal J.A.R.V.I.S. work never requires
  signing into, or enabling, the social layer.
- Presence and metrics are opt-in and can be private, friends-only or public.

## Completed in this pass

- Railway Postgres provisioned in isolated `jarvis-social` project.
- `apps/social-api` deployed with profile creation, bearer authentication,
  presence, metrics visibility, friendships and profile privacy endpoints.
- Railway domain health verified.
- Desktop `Friends` surface wired into the existing rail and validated in a
  separate Tauri instance in dark and light themes.
- TypeScript workspace typecheck and Rust `cargo check` pass.

## Next work

1. Add automatic desktop heartbeat and metric publication with local privacy
   controls.
2. Add profile detail modal and social activity timeline.
3. Add integration tests against a disposable Postgres database.
