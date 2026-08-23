# BLOCKERS

Things that genuinely need you — a purchase, a credential, or an interactive
sign-in. Per §73 none of these stopped the rest of the work; each entry says
exactly what is missing and what happens when it arrives.

---

## B1 — Authenticode code-signing certificate (§12, §60)
**Status:** Blocked. Needs a purchase; nothing technical is missing.

The installer builds, installs, updates and uninstalls. The binaries inside it
are **unsigned**, so Windows SmartScreen shows an "unknown publisher" warning
the first time someone installs it.

**What I need from you:** an OV or EV code-signing certificate (DigiCert,
Sectigo, SSL.com, …). EV generally requires a hardware token or a cloud HSM.

**What to do when you have it:** set `bundle.windows.certificateThumbprint` (or
the `TAURI_SIGNING_*` environment variables for a cloud HSM) in
`apps/desktop/src-tauri/tauri.conf.json`. No code changes — the build pipeline
already has the shape for it.

**Not the same thing:** the updater's own signing key. That is a minisign
keypair, it was generated locally, and update integrity verification works
today. The private half lives in `.keys/` and is gitignored — **it must never be
committed, and losing it means no existing installation can ever be updated
again.** Back it up somewhere safe.

---

## B2 — A place to publish updates (§62)
**Status:** Partially blocked. The mechanism works; it has nowhere public to point.

The updater is configured against
`https://github.com/alanaraujo-bit/jarvis-control-plane/releases/latest/download/latest.json`.
The repository is **private**, so that URL is not reachable without
authentication and update checks will fail against it as things stand.

**Your options, in order of least effort:**
1. Make a separate public repository just for release artifacts, and point the
   endpoint at it. The source stays private.
2. Serve `latest.json` and the installers from the cloud service once B3 is
   resolved.

Until then, `Check for updates` reports a failure honestly rather than
pretending to be up to date.

---

## B3 — Vercel / cloud provisioning (§59)
**Status:** Blocked on an interactive sign-in.

The Vercel connector for this session is unauthenticated, and the session is
non-interactive, so the OAuth flow cannot be completed here.

**What I need from you:** authorize the Vercel connector, or run `vercel login`
in a terminal.

**Impact: none on the product as it stands.** This is local-first paying off
(§3) — projects, sessions, the terminal, agents, conversation, Git and the
local database all work fully offline with no account. Cloud only gates the
mobile companion, push notifications, and billing.

---

## B4 — Provider account limits
**Status:** Informational, no action needed.

The Codex CLI on this machine reported `usage_limit_exceeded` in a recent
session. That affects running Codex live here; it does not affect the Codex
adapter, which is verified against real recorded rollout transcripts.

---

## B5 — `pnpm tauri build`'s NSIS step hangs on this machine
**Status:** Blocked on investigation. A workaround exists and is not urgent.

Two full `pnpm tauri build` runs both hung the same way: the Rust binary
compiled in about two minutes each time, `installer.nsi` was generated in
`target/release/nsis/`, and then nothing happened — no installer `.exe`, no
`makensis.exe` process ever appeared in the process tree, and the whole
process tree sat at effectively zero CPU for 25+ minutes both times. Both runs
had to be killed by hand.

**Impact: small.** `pnpm tauri build --no-bundle` produces the same
`jarvis-desktop.exe` the installer would package, in the same ~2 minutes,
skipping only the NSIS step — that is what this session used to verify Global
Search (§51) against the real app. Day-to-day development and verification are
unaffected.

**What actually needs it:** producing a real, installable `.exe` — which is
gated on B1 (code signing) anyway, so nothing ships through this path yet
regardless.

**What I need from you, when you have a spare half hour:** check whether
`makensis.exe` is on this machine and runs standalone against the generated
script (`makensis apps\desktop\src-tauri\target\release\nsis\x64\installer.nsi`
from a plain terminal, outside this tool), and whether Windows Defender's
real-time scanner is holding a lock on the freshly-built multi-megabyte binary
long enough to starve NSIS of the read it needs. Both are guesses from the
symptom, not confirmed causes.
