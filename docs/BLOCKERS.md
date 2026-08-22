# BLOCKERS

Things that genuinely require the user (physical action, purchase, credential,
or an interactive auth flow). Per §73 these do **not** stop the rest of the work.

---

## B1 — Authenticode code-signing certificate (§12, §60)
**Status:** Blocked — requires purchase.

The installer and updater are built and functional, but binaries are
**unsigned**. Windows SmartScreen will warn on install.

**Needed from you:** an OV/EV code-signing certificate (DigiCert, Sectigo,
SSL.com…). EV now generally requires a hardware token or cloud HSM.

**When you have it:** set `TAURI_SIGNING_*` / configure `windows.certificateThumbprint`
in `tauri.conf.json`. No code changes required — the pipeline is already shaped for it.

**Not blocked by this:** the Tauri updater's own minisign keypair is separate and
is generated locally. Update integrity verification works today.

---

## B2 — Vercel / cloud provisioning (§59)
**Status:** Partially blocked — the Vercel MCP connector is unauthenticated and
this session is non-interactive, so the OAuth flow cannot run here.

**Needed from you:** authorize the Vercel connector (or run `vercel login`
interactively), then the cloud app can be deployed.

**Impact:** none on local product. Local-first (§3) means every core surface —
projects, sessions, terminal, missions, git, brain, analytics — works fully
offline with no account. Cloud only gates mobile relay, push, and billing.

---

## B3 — Provider account usage limits
**Status:** Informational.

The installed Codex CLI reported `usage_limit_exceeded` in a recent local
session transcript. This affects live end-to-end Codex runs on this machine, not
the adapter implementation (which is verified against real recorded transcripts).
