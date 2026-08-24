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
**Status:** Still open, but the obstacle changed. B3 is resolved, so option 2
below is now reachable — the relay deployment could serve `latest.json` and
the installers. Not done in this pass: the updater endpoint is a separate
decision from the companion relay, and pointing release artifacts at a
function that exists to pass small JSON around deserves its own thought
rather than being bolted on because the account happened to be available.

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
**Status: RESOLVED, 2026-08-23.** Not by a change of plan — the CLIs turned
out to be authenticated all along.

`vercel whoami` answers `alanarauj0`, `railway whoami` answers
`alanvitoraraujo2a@gmail.com`, and `gh auth status` answers
`alanaraujo-bit`. The earlier entry said the connector was unauthenticated
and the OAuth flow could not be completed here; that was true of the *MCP
connector* and never checked against the CLIs, which were already logged in.

**The lesson is the same one item 29 records:** a status written from what
was assumed rather than probed. One command would have settled it.

**What was built on it:** the M11 relay, deployed to the Vercel project
`jarvis-desktop-relay` with a private Blob store `jarvis-relay-mailbox`.

### Two things a future session must not do

1. **Do not touch the Vercel project `jarvis` or the Railway project
   `jarvis-guardian`.** They look like this product and are something else
   entirely — Alan said so directly. `jarvis.aionixdev.com` is live and
   serving. New resources for this product get names that cannot be
   confused with those.
2. **Railway bills monthly and Vercel does not.** The Railway account has an
   active subscription (next invoice USD 18.21 at the time of writing) and
   ten projects; Vercel `aionixdev` has no contract and no recorded costs.
   That is why the relay went to Vercel — the directive asks for no
   recurring cost where an adequate free path exists, and for a relay there
   is one.

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

---

## B6 — Signing in to the second, third and fourth Claude accounts (§66, M13)
**Status:** Blocked on an interactive browser sign-in. Blocks one verification,
not the feature.

M13 gives each account its own provider configuration directory
(`CLAUDE_CONFIG_DIR` / `CODEX_HOME`) — see `docs/M13-ACCOUNTS.md` §2.5 for why
rewriting `~/.claude/.credentials.json` was rejected. Registering an account is
therefore: create the directory, then **sign in to it**, and signing in means
`claude auth login` opening a browser and you completing an OAuth flow. No
agent can do that for you, and `claude setup-token` is not a way around it —
its first use needs the same interactive login.

**Impact: narrow.** The machinery is built and exercised against your live
account plus an alternate configuration directory: the registry, the config-dir
plumbing, the quota model, the panel, the manual switch and the automatic
switch can all be verified without a second subscription. What cannot be
verified until this is done is the one end-to-end pass that matters most —
account A exhausting, work moving to account B, and the agent carrying on.

**What I need from you:** with J.A.R.V.I.S. open on the Accounts screen, add an
account, then complete the sign-in in the browser window it opens — once per
subscription, three times in total. Nothing else. The product never sees a
password and never reads a token; it asks the provider who the directory is
signed in as (`claude auth status --json`) and stores the email, organisation
and plan so you can tell four accounts apart.

**What happens when it arrives:** the automatic-switch path can be run for real
against two genuine allowances instead of a simulated rejection, which is the
last thing standing between M13 and "verified" rather than "built".
