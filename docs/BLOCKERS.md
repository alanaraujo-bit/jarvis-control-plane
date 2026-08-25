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
**Status: RESOLVED, 2026-08-24.** The failure was not reproduced by the M13
release build.

`pnpm tauri build` compiled the release binary, regenerated
`target/release/nsis/x64/installer.nsi`, launched `makensis.exe` and produced
`J.A.R.V.I.S_0.2.0_x64-setup.exe` in one uninterrupted run. The NSIS process
was observed consuming CPU rather than sitting stalled. The resulting binary
reports file and product version `0.2.0`; the installer is 6,738,870 bytes and
its updater signature was generated successfully.

The updater key has an empty password. On this PowerShell host, assigning an
empty environment variable removes it, so Tauri reached an interactive
password prompt after the bundle was ready. Running `tauri signer sign` with
`--password=` and absolute key/artifact paths completed the signature without
rebuilding. This was signing transport, not an NSIS failure.

**The whole thing signs in one run from bash** (M14, 2026-08-24), which is
worth preferring over building and then signing separately — a two-step release
is a release somebody eventually forgets the second half of:

```bash
TAURI_SIGNING_PRIVATE_KEY="$(cat .keys/jarvis-updater.key)" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
pnpm tauri build
```

Bash keeps an empty variable as an empty value, which is exactly what an empty
key password needs; PowerShell deletes it, which is what sent the earlier run
to an interactive prompt. Same key, same command, different shell. Produced
`J.A.R.V.I.S_0.3.0_x64-setup.exe` and its `.sig` together, with no prompt.

B1 still applies independently: the updater artifact is cryptographically
signed for Tauri integrity checks, but the Windows executable has no
Authenticode certificate and SmartScreen may show “unknown publisher”.

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

**Impact: narrow; M13 is complete despite it.** The machinery is built and
exercised against your live account plus alternate configuration directories:
registry, config-dir plumbing, quota model, panel, manual/automatic switch and
the Brain relay are covered by real-infrastructure and deterministic tests. The
release app was also validated in both locales/themes under an isolated app
identifier. What cannot be executed until this is done is one external-state
verification: account A genuinely exhausting, work moving to a separately
authenticated account B, and the agent carrying on.

**What I need from you:** with J.A.R.V.I.S. open on the Accounts screen, add an
account, then complete the sign-in in the browser window it opens — once per
subscription, three times in total. Nothing else. The product never sees a
password and never reads a token; it asks the provider who the directory is
signed in as (`claude auth status --json`) and stores the email, organisation
and plan so you can tell four accounts apart.

**What happens when it arrives:** repeat the already-built automatic-switch path
against two genuine allowances and record the external E2E evidence. It does not
change the architecture or reopen M13.

### B6 — update after M16 (2026-08-24)

Still blocked, still narrow, and now with more of it built underneath.

Two things changed in what a sign-in unlocks. Both were verified against the
real CLIs (`docs/M16-QUOTA.md` §1.3):

* **A directory that has never been signed into is now a definite answer, not a
  guess.** Claude replies `rate_limits_available: false` and Codex replies
  `-32600 authentication required`. Neither one returns the ambient account's
  numbers under the new account's name — which was the thing that could have
  made this feature unsafe to ship half-configured. So the cards for accounts
  2–4 are correct *today*: they say "not signed in — no quota to read" and
  offer the sign-in, rather than showing a plausible number that belongs to
  somebody else.
* **The moment you finish a sign-in, that account starts showing real official
  figures with no further work.** Nothing is waiting on a first refusal any
  more; the probe answers immediately.

What still cannot be executed without you is unchanged and is one thing: account
A genuinely running out, new work moving to a separately authenticated account
B, and the agent carrying on. Everything up to that point — the registry, the
config-dir plumbing, the quota model, the panel, the status-bar chips, manual
and automatic switching, and the Brain relay — is built and covered.

**What I need from you, unchanged:** Accounts screen → Add account → complete
the browser sign-in. Once per subscription. The product never sees a password
and never reads a token.

### B6 — update after M18 (2026-08-25)

**Partly done, and the remaining half is no longer blocked on you.**

Alan has since signed in. What is registered on this machine now:

| Card | Subscription | State when checked |
| --- | --- | --- |
| Claude 1 | `alanvitoraraujo1@icloud.com` (adopted `~/.claude`) | 71% left |
| Claude 2 | `alanvitoraraujo2a@gmail.com` | **0% — exhausted** |
| Claude 3 | `alanvitoraraujo1@icloud.com` — the *same* subscription as Claude 1 (D46) | 71% left |
| Codex | `alanvitoraraujo1a@outlook.com` | 100% left |

So there are **two** genuine Claude subscriptions, not three: the third
directory landed on the account that already existed, which is what M18 exists
to make visible. A third subscription would need a third email.

**What this unblocks.** B6 was waiting on exactly one thing: account A genuinely
exhausting, work moving to a separately authenticated account B, and the agent
carrying on. The two allowances that scenario needs now exist and are *already
at opposite ends* — Claude 2 is refusing and Claude 1/3 have room. The
verification no longer needs a sign-in from Alan; it needs a real run.

**What it costs, which is why it has not been run unasked.** It means making
Claude 2 active, starting a real agent session against it, and letting the
provider refuse a turn — `maybe_rotate` fires from the transcript tailer, so
nothing short of a genuine refusal exercises the real path. That spends tokens
on Alan's own accounts and writes real sessions to history. It is his call, not
a thing to do while he is away.

**What is left for Alan on B6:** nothing, unless he wants a third genuine Claude
subscription. The remaining item is a run to schedule, not a credential to
supply.

### B2 — update after the 0.5.0 build (2026-08-25)

**`tauri build` does not produce `latest.json`.** Measured, not assumed: the
0.5.0 run finished with exactly two artifacts —
`J.A.R.V.I.S_0.5.0_x64-setup.exe` and its `.sig`. `createUpdaterArtifacts: true`
generates the **signature**; the manifest the updater actually fetches is
normally written by `tauri-action` in CI, and there is no CI here.

So the endpoint in `tauri.conf.json` would 404 on the manifest **even if the
repository were public**. That is a second, independent half of B2 that nobody
had noticed, because update checks have always failed for the first reason.

A correct manifest is now written by hand at
`apps/desktop/src-tauri/target/release/bundle/nsis/latest.json` — version,
`pub_date` from the installer's own mtime, the signature read out of the `.sig`
file, and the release-asset URL. Any future release has to produce one the same
way, or the release process needs the CI action that does it.

**Both halves still need you:**

1. The repository is private, so `releases/latest/download/…` is unreachable
   without authentication. Making a source repository public is not a decision
   an agent should take on somebody's behalf — the least-effort fix stays a
   separate public repository holding only release artifacts, with the endpoint
   pointed at it.
2. Whoever cuts a release has to attach `latest.json` beside the installer.

### B2 — RESOLVED, 2026-08-25. The premise was wrong.

**The repository is public.** It has been the whole time. Every version of this
entry has opened by stating it is private and reasoning from there, and nobody
ever ran the one command that checks:

```
gh repo view alanaraujo-bit/jarvis-control-plane --json isPrivate
{"isPrivate": false, "visibility": "PUBLIC"}
```

This is the second time in this file — B3 was the first — that a blocker was
written from what was assumed rather than probed, and it is the same shape:
*one command would have settled it*, and the wrong premise then survived every
re-reading because each one built on the last.

**What actually blocked updates was the missing manifest**, which is the half
recorded in the entry above and which nobody could see, because the check failed
for a reason that turned out not to exist.

**Proved end to end, unauthenticated, after the 0.5.0 release:**

```
GET releases/latest/download/latest.json                 → 200, correct manifest
GET releases/latest/download/J.A.R.V.I.S_0.5.0_x64-setup.exe → 200, 6,888,429 bytes
```

Which is exactly what `Verificar atualizações` fetches. **The updater works.**

**What a future release must not forget:** `tauri build` produces the installer
and its `.sig` and *nothing else*. `latest.json` is written by hand or by a CI
action, and a release without it puts every installation back where this entry
started — an update check that fails against a URL that looks right.

---

## B-DOC1 — An unanswered AnyDesk remote-session request (2026-08-25, 14:3x)

**Not a project blocker. Read this one first anyway.**

While the documentation site was being built, an **incoming AnyDesk remote
session request** appeared on top of the application window:

```
AnyDesk — 1604852548
"-" (1604852548) deseja se conectar ao seu dispositivo.
   [ Aceitar ]  [ Rejeitar ]
```

It was **not** initiated from this session and nothing here asked for it. It
was left **unanswered on purpose** — accepting or rejecting a request for
remote control of this machine is not a decision an agent should make on
somebody's behalf. The window was minimised so it stopped covering the
application being photographed; minimising answers nothing and the request is
still exactly where it was.

**What to do when you are back:** decide whether you recognise ID
`1604852548`. If you do not, reject it and consider setting an AnyDesk
unattended-access password, or closing AnyDesk when you are away. A screenshot
of the request is at `.tmp/docshots/probe-settings.png`.
