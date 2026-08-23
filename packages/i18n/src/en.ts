/**
 * English catalogue — the source of truth for message keys.
 *
 * Every other locale is typed as `Record<MessageKey, string>`, so a missing or
 * stray translation is a compile error rather than a runtime blank (§65).
 */
export const en = {
  "app.name": "J.A.R.V.I.S.",
  "app.tagline": "The control plane for AI-agent development.",

  // ---- Global navigation (§87) --------------------------------------------
  "nav.missionControl": "Mission Control",
  "nav.projects": "Projects",
  "nav.missions": "Missions",
  "nav.activity": "Activity",
  "nav.analytics": "Analytics",
  "nav.settings": "Settings",

  // ---- Window chrome -------------------------------------------------------
  "window.minimize": "Minimize",
  "window.maximize": "Maximize",
  "window.restore": "Restore",
  "window.close": "Close",
  "window.search": "Search or run a command",

  // ---- Agent + mission states (§21, §29) -----------------------------------
  "state.working": "Working",
  "state.waiting": "Waiting",
  "state.idle": "Idle",
  "state.completed": "Completed",
  "state.blocked": "Blocked",
  "state.failed": "Failed",
  "state.ready": "Ready",
  "state.running": "Running",
  "state.verifying": "Verifying",

  "mission.agents": "Agents",
  "mission.launchAgent": "Start an agent",
  "mission.noAgents": "No agent has worked on this mission yet.",
  "mission.openSession": "Open",

  // ---- Mission Control (§18) -----------------------------------------------
  "missionControl.title": "Mission Control",
  "missionControl.needsAttention": "Needs attention",
  "missionControl.working": "Working now",
  "missionControl.recentlyCompleted": "Recently completed",
  "missionControl.activeProjects": "Active projects",
  "missionControl.empty.title": "Nothing is running",
  "missionControl.empty.body":
    "When agents are working, this is where you will watch them. Start by opening a project.",
  "missionControl.empty.action": "Open a project",

  // ---- Environment scan (§14) ----------------------------------------------
  "env.title": "Environment",
  "env.rescan": "Rescan",
  "env.scanning": "Scanning your environment…",
  "env.ready": "Ready",
  "env.missing": "Not found",
  "env.degraded": "Needs attention",
  "env.required": "Required",
  "env.recommended": "Recommended",
  "env.optional": "Optional",
  "env.signedIn": "Signed in",
  "env.signedOut": "Not signed in",
  "env.installHint": "Install with",
  "env.copy": "Copy",
  "env.copied": "Copied",
  "env.learnMore": "Learn more",
  "env.allReady": "Your environment is ready.",
  "env.someMissing": "Some tools are missing.",

  // ---- Settings (§64) ------------------------------------------------------
  "settings.appearance": "Appearance",
  "settings.theme": "Theme",
  "settings.theme.dark": "Dark",
  "settings.theme.light": "Light",
  "settings.theme.system": "System",
  "settings.language": "Language",


  // ---- Projects (§16) ------------------------------------------------------
  "projects.openFolder": "Open folder",
  "projects.empty.title": "No projects yet",
  "projects.empty.body":
    "Open a folder on this machine to get started. Nothing is uploaded — J.A.R.V.I.S. works against your files where they already are.",
  "projects.missing": "Folder missing",
  "projects.back": "All projects",

  // ---- Terminal (§21) ------------------------------------------------------
  "terminal.title": "Terminal",
  "terminal.new": "New terminal",
  "terminal.shell": "Shell",
  "terminal.claudeCode": "Claude Code",
  "terminal.codex": "Codex",
  "terminal.close": "Close tab",
  "terminal.empty.title": "No terminal running",
  "terminal.empty.body": "Start a shell, or launch an agent in this project.",
  "terminal.notInstalled": "{name} is not installed",


  // ---- Conversation (§24) --------------------------------------------------
  "conversation.title": "Conversation",
  "conversation.you": "You",
  "conversation.agent": "Agent",
  "conversation.thinking": "Thinking",
  "conversation.in": "in",
  "conversation.out": "out",
  "conversation.cached": "cached",
  "conversation.usageOfficial": "Reported by the provider",
  "conversation.empty.title": "Nothing to show yet",
  "conversation.empty.body":
    "This session has not produced structured output yet. The terminal shows everything as it happens.",
  "view.terminal": "Terminal",
  "view.conversation": "Conversation",

  // ---- Project areas (§19/§85) ---------------------------------------------
  // Project-scoped, so they live inside a project rather than on the rail.
  "project.sessions": "Sessions",
  "project.files": "Files",
  "project.review": "Review",

  // ---- Files and the editor (§41/§42) --------------------------------------
  "files.loading": "Reading…",
  "files.emptyFolder": "Empty folder",
  "files.close": "Close file",
  "files.save": "Save",
  "files.reload": "Reload from disk",
  "files.editorLoading": "Opening the editor…",
  "files.empty.title": "No file open",
  "files.empty.body": "Choose a file from the tree to read or edit it.",
  "files.binary.title": "Not a text file",
  "files.binary.body":
    "This file contains binary data. Showing it as text would be misleading, so it is not shown.",
  "files.tooLarge.title": "Too large to open",
  "files.tooLarge.body": "This file is {size}, past the editor's limit of 2 MB.",

  // ---- Diff / Review (§43) -------------------------------------------------
  "review.changedFiles": "{count} changed files",
  "review.changedFiles_one": "{count} changed file",
  "review.refresh": "Re-read the working tree",
  "review.against": "Against {branch}",
  "review.clean": "Nothing has changed since the last commit.",
  "review.noCommits":
    "This repository has no commits yet, so there is nothing to compare against.",
  "review.notARepo.title": "Not a Git repository",
  "review.notARepo.body":
    "Review shows what changed since the last commit. This project folder is not under Git, so there is nothing to compare against.",
  "review.empty.title": "Nothing selected",
  "review.empty.body": "Choose a file to see what changed in it.",
  "review.loadingDiff": "Reading the diff…",
  "review.binary": "A binary file changed. There is no text diff to show.",
  "review.binaryShort": "binary",
  "review.tooLarge":
    "This new file is larger than the 2 MB the editor will read, so its contents are not shown.",
  "review.tooLargeShort": "too large",
  "review.noTextChange": "No line changed — only the file's mode or metadata.",
  "review.truncated": "The rest of this diff was left out; it is longer than {count} lines.",
  "review.andOthers": "+{count} more sessions",
  "review.andOthers_one": "+{count} more session",
  // The letter is a code, so the word travels with it as a tooltip and as the
  // accessible name — an interface that has to be learned before it can be
  // read is not finished.
  "review.kindFull.added": "Added",
  "review.kindFull.modified": "Modified",
  "review.kindFull.deleted": "Deleted",
  "review.kindFull.renamed": "Renamed",
  "review.kindFull.untracked": "New, not yet tracked by Git",
  "review.kindFull.conflicted": "Conflicted",
  // One letter per row: the list is long and the word would crowd it.
  "review.kind.added": "A",
  "review.kind.modified": "M",
  "review.kind.deleted": "D",
  "review.kind.renamed": "R",
  "review.kind.untracked": "N",
  "review.kind.conflicted": "!",


  // ---- Updates (§62) -------------------------------------------------------
  "update.title": "Updates",
  "update.current": "You are on version {version}.",
  "update.check": "Check for updates",
  "update.checking": "Checking…",
  "update.upToDate": "J.A.R.V.I.S. is up to date.",
  "update.available": "Version {version} is available.",
  "update.downloading": "Downloading… {percent}%",
  "update.ready": "Version {version} is ready to install.",
  "update.install": "Restart and update",
  "update.failed": "Could not check for updates.",
  "update.unsigned":
    "This build is not code-signed, so Windows may warn on install. See BLOCKERS.md.",
  "update.notes": "What changed",


  // ---- Missions (§29-§35) --------------------------------------------------
  "mission.new": "New mission",
  "mission.title": "Title",
  "mission.goal": "Goal",
  "mission.tasks": "Tasks",
  "mission.criteria": "Acceptance criteria",
  "mission.evidence": "Evidence",
  "mission.autonomy": "Autonomy",
  "mission.autonomy.guided": "Guided",
  "mission.autonomy.autonomous": "Autonomous",
  "mission.autonomy.unattended": "Unattended",
  "mission.autonomy.inherited": "Inherited",
  "mission.verify": "Verify now",
  "mission.verifying": "Verifying…",
  "mission.complete": "Mark complete",
  "mission.start": "Start",
  "mission.block": "Blocked",
  "mission.required": "Required",
  "mission.optional": "Optional",
  "mission.confirm": "Confirm",
  "mission.confirmManual": "Only a person can confirm this",
  "mission.withdrawn": "Withdrawn",
  "mission.withdrawnBy": "Withdrawn by {who}: {reason}",
  "mission.notVerified_one":
    "{count} required criterion is not verified yet. A mission is complete when there is evidence, not when it is claimed.",
  "mission.notVerified":
    "{count} required criteria are not verified yet. A mission is complete when there is evidence, not when it is claimed.",
  "mission.noCriteria": "No acceptance criteria. Nothing will be checked automatically.",
  "mission.blockedReason": "Why it is blocked",
  "mission.empty.title": "No missions yet",
  "mission.empty.body":
    "A mission is work that needs finishing, with criteria that say what finished means.",
  "mission.create": "Create mission",
  "mission.cancel": "Cancel",
  "mission.titlePlaceholder": "What needs to be done?",
  "mission.goalPlaceholder": "What does success look like?",
  "mission.criterionPlaceholder": "A check that must pass",
  "mission.commandPlaceholder": "e.g. pnpm test",
  "mission.addCriterion": "Add criterion",
  "mission.checkType.command": "Command",
  "mission.checkType.fileExists": "File exists",
  "mission.checkType.manual": "Manual",

  // ---- Mission Control (§18) -----------------------------------------------
  "missionControl.blocked": "Blocked",
  "missionControl.verified": "verified",
  "missionControl.openCriteria_one": "{count} unverified",
  "missionControl.openCriteria": "{count} unverified",


  // ---- Analytics (§52, §53) ------------------------------------------------
  "analytics.title": "Analytics",
  "analytics.window": "Last {days} days",
  "analytics.leverage": "Human leverage",
  "analytics.humanActive": "Human active",
  "analytics.agentRuntime": "Agent execution",
  "analytics.leverageNote":
    "Human time counts minutes in which you actually typed into a session. Agent time is how long agent sessions were alive.",
  "analytics.tokens": "Tokens",
  "analytics.input": "Input",
  "analytics.output": "Output",
  "analytics.cacheRead": "Cache read",
  "analytics.cacheWrite": "Cache write",
  "analytics.byProvider": "By provider",
  "analytics.byModel": "By model",
  "analytics.byProject": "By project",
  "analytics.byDay": "By day",
  "analytics.filesChanged": "Files changed",
  "analytics.sessions": "Sessions",
  "analytics.empty.title": "Nothing measured yet",
  "analytics.empty.body":
    "Run an agent and its token usage, runtime and file changes show up here.",
  "analytics.confidence.official": "Reported by the provider",
  "analytics.confidence.observed": "Measured by J.A.R.V.I.S.",
  "analytics.confidence.estimated": "Estimated",
  "analytics.confidence.unknown": "Unknown provenance",

  // ---- Activity (§48) ------------------------------------------------------
  "activity.title": "Activity",
  "activity.all": "Everything",
  "activity.attention": "Needs attention",
  "activity.empty.title": "Nothing has happened yet",
  "activity.empty.body": "Sessions, missions and verifications show up here as they happen.",
  "activity.kind.session.started": "Agent started",
  "activity.kind.session.ended": "Agent finished",
  "activity.kind.mission.completed": "Mission completed",
  "activity.kind.mission.blocked": "Mission blocked",
  "activity.kind.mission.failed": "Mission failed",
  "activity.kind.mission.waiting": "Mission waiting",


  // ---- Guardrails (§35) ----------------------------------------------------
  "guardrail.title": "Guardrails",
  "guardrail.subtitle":
    "What an agent must ask about before it does it. Rules apply per project, and fall back to the global setting.",
  "guardrail.scope.global": "Everywhere",
  "guardrail.scope.project": "This project",
  "guardrail.scope.default": "Default",
  "guardrail.inherited": "Inherited: {decision}",
  "guardrail.decision.ask": "Always ask",
  "guardrail.decision.allow": "Allow",
  "guardrail.decision.deny": "Never allow",
  "guardrail.clear": "Use the global rule",

  "guardrail.op.git.force-push": "Force push",
  "guardrail.op.git.force-push.detail":
    "Overwriting a branch on a remote. Excludes --force-with-lease, which refuses when the remote has moved.",
  "guardrail.op.git.history-rewrite": "Rewrite history",
  "guardrail.op.git.history-rewrite.detail": "reset --hard, rebase, filter-branch.",
  "guardrail.op.git.branch-delete": "Delete a branch",
  "guardrail.op.git.branch-delete.detail": "Deleting a remote branch, or forcing a local one.",
  "guardrail.op.fs.recursive-delete": "Delete a directory tree",
  "guardrail.op.fs.recursive-delete.detail": "rm -rf, Remove-Item -Recurse, git clean -f.",
  "guardrail.op.secrets.access": "Read a credential file",
  "guardrail.op.secrets.access.detail": ".env, private keys, .npmrc. Example files are not included.",
  "guardrail.op.deploy.production": "Deploy to production",
  "guardrail.op.deploy.production.detail": "vercel --prod, wrangler deploy, terraform apply.",
  "guardrail.op.package.publish": "Publish a package",
  "guardrail.op.package.publish.detail": "npm publish, cargo publish, docker push.",
  "guardrail.op.remote.execute": "Run something downloaded",
  "guardrail.op.remote.execute.detail": "curl | sh, iex (iwr …).",

  "guardrail.pending.title": "Waiting for you",
  "guardrail.pending.body_one": "{count} operation needs your decision before it can run.",
  "guardrail.pending.body": "{count} operations need your decision before they can run.",
  "guardrail.matched": "Matched",
  "guardrail.choice.allowOnce": "Allow once",
  "guardrail.choice.allowForProject": "Allow for this project",
  "guardrail.choice.alwaysAllow": "Always allow",
  "guardrail.choice.neverAllow": "Never allow",

  "guardrail.history": "Guardrail history",
  "guardrail.empty.title": "Nothing has been stopped",
  "guardrail.empty.body":
    "When an agent reaches for something sensitive, what happened shows up here.",
  "guardrail.status.pending": "Waiting",
  "guardrail.status.allowed": "Allowed",
  "guardrail.status.denied": "Refused",
  "guardrail.status.asked": "Asked",
  "guardrail.origin.agent": "Agent",
  "guardrail.origin.verification": "Verification",
  "guardrail.reason.policyDenies": "A rule says never",
  "guardrail.reason.nobodyToAsk": "Needed approval, nobody was attached",
  "guardrail.reason.askedHuman": "Put to you",
  "guardrail.reason.policyAllows": "A rule allows it",
  "guardrail.reason.allowedOnce": "Allowed once",
  "guardrail.reason.allowedForProject": "Allowed for this project",
  "guardrail.reason.allowedAlways": "Always allowed",
  "guardrail.reason.neverAllowed": "Never allowed",

  "guardrail.coverage.preExecution": "Stopped before it runs",
  "guardrail.coverage.preExecutionWhenTrusted": "Stopped once you trust the hook in {provider}",
  "guardrail.coverage.observed": "Recorded, not prevented",
  "guardrail.coverage.none": "Not governed",
  "guardrail.coverage.note":
    "Guardrails govern agents, not what you type yourself. Verification commands J.A.R.V.I.S. runs are always enforced.",

  "activity.kind.guardrail.denied": "Guardrail refused an operation",
  "activity.kind.guardrail.allowed": "Guardrail approved an operation",

  // ---- Evidence, worded here rather than in Rust (§65) ----------------------
  "evidence.guardrailRefused": "Not checked: a guardrail refused {operation}",


  // ---- Autopilot (§32) -----------------------------------------------------
  "autopilot.title": "Run unattended",
  "autopilot.description":
    "J.A.R.V.I.S. takes the seat in front of the agent: after every turn it verifies the criteria and either sends the next instruction or stops and tells you why.",
  "autopilot.start": "Run until done",
  "autopilot.stop": "Take over",
  "autopilot.running": "Running unattended",
  "autopilot.turn": "Turn {turns} of {budget}",
  "autopilot.state.working": "Agent is working",
  "autopilot.state.deciding": "Verifying",
  "autopilot.state.finished": "Finished",
  "autopilot.requiresUnattended":
    "Set this mission's autonomy to Unattended first. Running it unsupervised is your decision to make, not ours.",
  "autopilot.alreadyRunning": "This mission is already being driven.",
  "autopilot.alreadyFinished": "This mission has already finished.",
  "autopilot.outOfTurns":
    "Stopped after the turn budget ran out. It was not converging on the criteria.",
  "autopilot.needsManualCheck": "Everything left can only be confirmed by a person.",
  "autopilot.awaitingApproval": "A guardrail needs your decision before it can continue.",
  "autopilot.notConverging":
    "Stopped: the same criteria kept failing with no progress between attempts.",
  "autopilot.missionBlocked": "The mission is blocked and needs you.",
  "autopilot.completed": "Finished unattended, with every required criterion verified.",

  "activity.kind.autopilot.turn": "Autopilot sent an instruction",
  "activity.kind.autopilot.completed": "Mission finished unattended",
  "activity.kind.autopilot.outOfTurns": "Autopilot ran out of turns",
  "activity.kind.autopilot.notConverging": "Autopilot stopped, not converging",
  "activity.kind.autopilot.needsManualCheck": "Autopilot needs a person",
  "activity.kind.autopilot.awaitingApproval": "Autopilot is waiting on approval",
  "activity.kind.autopilot.missionBlocked": "Autopilot stopped, mission blocked",

  // ---- Generic -------------------------------------------------------------
  "common.cancel": "Cancel",
  "common.confirm": "Confirm",
  "common.retry": "Retry",
  "common.dismiss": "Dismiss",
  "common.loading": "Loading…",
} as const;

export type MessageKey = keyof typeof en;
