# M24 — Account isolation and resilient quota rotation

## Problem found

The first Claude Code and Codex accounts registered by J.A.R.V.I.S. were not
isolated accounts. Their database rows pointed directly at the provider's
machine-wide configuration (`~/.claude` or `~/.codex`), and adopted accounts
were deliberately launched without an environment override. Consequently, a
login or logout from VS Code or another terminal changed the credentials that
J.A.R.V.I.S. used too.

Codex has a second boundary: even with a distinct `CODEX_HOME`, its automatic
credential-store selection may use the operating-system credential store. That
store is shared between processes and therefore is not an account boundary.

Automatic rotation also depended on transcript output. A provider that stopped
emitting output after exhausting quota could never reach that check. Candidate
selection followed list order rather than choosing the account with the most
remaining quota.

## Invariants after M24

1. Every registered provider process receives an explicit
   `CLAUDE_CONFIG_DIR` or `CODEX_HOME`, including the account imported from the
   machine.
2. A legacy machine account is copied once into an app-owned directory. The
   global source is never modified, deleted, or subsequently synchronized.
3. Credentials are copied as opaque files. Session histories and caches are
   excluded, and symbolic links are not followed.
4. Every private Codex home forces
   `cli_auth_credentials_store = "file"`, so the selected `CODEX_HOME` is the
   authentication boundary rather than the shared OS keyring.
5. The database points at the private directory only after a complete staging
   copy is atomically installed. If isolation cannot be completed, startup
   fails visibly instead of silently using shared credentials.
6. Live provider quota refreshes and transcript events both run the same
   rotation policy and audit path.
7. Rotation skips signed-out, paused, exhausted, and duplicate-subscription
   accounts, then chooses the candidate with the most headroom. List position
   is only the deterministic fallback when quota is unknown.

## Runtime boundary

Account switching changes the credentials used by newly spawned work. A Claude
Code or Codex process that is already running has already loaded its account;
mutating credential files under that process is unsafe and does not reliably
change it. Autopilot can relay onto a fresh process with preserved context. An
ordinary interactive terminal must start a fresh session/tab to use the newly
active account; it no longer needs a manual provider `/login`.

## Verification

- Account unit suite: 48 passed, 4 ignored machine diagnostics.
- The opt-in real-machine identity diagnostic distinguishes the registered
  accounts and no longer treats a Codex token without decoded e-mail claims as
  permanently stale.
- The diagnostic also identified two current Claude configuration directories
  as the same provider subscription. They are correctly rejected as rotation
  alternatives until one is signed into a distinct subscription.

The Codex authentication behavior and configuration keys are documented in the
official authentication guide: <https://learn.chatgpt.com/docs/auth>.
