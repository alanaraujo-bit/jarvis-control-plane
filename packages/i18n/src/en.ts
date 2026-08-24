/**
 * English catalogue — the source of truth for message keys.
 *
 * Every other locale is typed as `Record<MessageKey, string>`, so a missing or
 * stray translation is a compile error rather than a runtime blank (§65).
 */
export const en = {
  "app.name": "J.A.R.V.I.S.",
  "app.tagline": "The control plane for AI-agent development.",

  // ---- Onboarding (§13), shown once ------------------------------------------
  "onboarding.intro": "Here's what J.A.R.V.I.S. found on this machine.",
  "onboarding.continue": "Continue without opening a project",

  // ---- Voice dictation (§54) ------------------------------------------------
  "voice.start": "Dictate into the terminal",
  "voice.recording": "Listening — click to stop",
  "voice.listening": "Listening…",
  "voice.transcribing": "Transcribing…",
  "voice.error": "Voice dictation failed",
  "voice.downloadNeeded": "Download voice dictation to use it",
  "voice.download.title": "Voice dictation",
  "voice.download.body": "Downloads a local speech-to-text model (about 490 MB, once). Your voice never leaves this machine.",
  "voice.download.action": "Download",
  "voice.download.verifying": "Verifying…",
  "voice.download.retry": "Retry",

  // ---- Global navigation (§87) --------------------------------------------
  "nav.missionControl": "Mission Control",
  "nav.projects": "Projects",
  "nav.missions": "Missions",
  "nav.activity": "Activity",
  "nav.analytics": "Analytics",
  "nav.accounts": "Accounts",
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
  "settings.agents": "Agents",
  "settings.companion": "Phone",
  "companion.title": "Mobile companion",
  "companion.subtitle": "Watch your agents from your phone, and answer when they need you.",
  "companion.off": "Not connected. Nothing is sent anywhere until you connect a device.",
  "companion.paired": "Connected. This desktop publishes a summary every minute.",
  "companion.connect": "Connect a device",
  "companion.disconnect": "Disconnect",
  "companion.code": "Type this on your phone",
  "companion.codeHint": "Open {url} on your phone and enter the code. It is valid for 5 minutes and can be used once.",
  "companion.failed": "Could not reach the relay. Nothing on this machine is affected.",
  "companion.whatIsSent": "What is sent: which missions need attention and which approvals are waiting. Never your files, terminal output or conversations.",
  "settings.terminal": "Terminal",
  "settings.fontSize": "Type size",
  "settings.scrollback": "History kept",
  "settings.scrollbackUnit": "{0} lines",
  "settings.scrollbackHelp": "Per terminal. A split can hold four at once.",
  "settings.turnBudget": "Turn limit",
  "settings.turnBudgetUnit": "{0} turns",
  "settings.turnBudgetHelp": "How far an unattended run goes before it stops and says so. A run keeps the limit it started with.",
  "settings.default": "Default",
  "settings.reset": "Reset to default",
  "autonomy.title": "Autonomy",
  "autonomy.subtitle": "How much an agent does before it asks.",
  "autonomy.default": "Default for new work",
  "autonomy.project": "This project",
  "autonomy.inherit": "Inherit",
  // Shown under a project set to Inherit, so the word points at something the
  // reader can actually see rather than at a value with no surface.
  "autonomy.inherits": "Follows the global default: {0}",
  "autonomy.appliesTo": "Applies to every mission that has not chosen its own.",
  "autonomy.guided.help": "The agent checks in often. The safe assumption.",
  "autonomy.autonomous.help": "The agent handles what falls within its remit and asks when it does not.",
  "autonomy.unattended.help": "The agent keeps going until the work is verified, blocked or failed.",


  // ---- Projects (§16) ------------------------------------------------------
  "projects.openFolder": "Open folder",
  "projects.empty.title": "No projects yet",
  "projects.empty.body":
    "Open a folder on this machine to get started. Nothing is uploaded — J.A.R.V.I.S. works against your files where they already are.",
  "projects.missing": "Folder missing",
  "projects.back": "All projects",
  "projects.worktreeOf": "worktree of {project}",
  "projects.archive": "Remove {project} from J.A.R.V.I.S. — the folder stays on disk",

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
  "terminal.find.placeholder": "Search scrollback",
  "terminal.find.matchCase": "Match case",
  "terminal.find.previous": "Previous match",
  "terminal.find.next": "Next match",
  "terminal.find.close": "Close search",
  "terminal.find.noResults": "No results",
  // xterm stops counting past its highlight limit. Saying "1000+" is honest;
  // a total that is quietly wrong is not.
  "terminal.find.manyResults": "1000+",
  "terminal.split.add": "Show two terminals side by side",
  "terminal.split.addThis": "Alt+click to show this terminal alongside",
  "terminal.split.remove": "Close this pane — the session keeps running",
  "terminal.split.layout": "Layout",
  "terminal.split.columns": "Side by side",
  "terminal.split.rows": "Stacked",
  "terminal.split.grid": "Grid",
  "terminal.paste.attached": "Image attached",
  "terminal.paste.remove": "Hide preview",
  "terminal.paste.tooLarge": "That image is too large to attach.",
  "terminal.paste.unsupported": "That is not an image J.A.R.V.I.S. can attach.",
  "terminal.paste.empty": "The clipboard image was empty.",
  "terminal.paste.outsideSession": "That file does not belong to this session.",
  "terminal.paste.failed": "The image could not be attached.",

  "preview.title": "Preview",
  "preview.open": "Open preview",
  "preview.reload": "Reload",
  "preview.close": "Close preview",
  "preview.detected": "This session is serving {0}",
  "preview.searching": "No dev server yet",
  "preview.hint": "Start one in a terminal — {name} will find its address in the output.",
  "preview.openWindow": "Preview opens in its own window, beside this one.",
  "preview.notLocal": "Preview only opens addresses on this machine.",
  "preview.invalidUrl": "That is not an address Preview can open.",
  "preview.failed": "The preview could not be opened.",
  "preview.choose": "This session is serving more than one thing.",


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
  "project.worktrees": "Worktrees",
  "project.brain": "Brain",
  "project.settings": "Settings",

  // ---- Project Brain (§36–§39) and Notes (§40) -----------------------------
  "brain.title": "Brain",
  "brain.subtitle":
    "What is known about this project. Knowledge is handed to every agent that starts here; notes are yours.",
  "brain.tab.knowledge": "Knowledge",
  "brain.tab.notes": "Notes",
  "brain.tab.timeline": "History",

  "brain.kind.what": "What this project is",
  "brain.kind.convention": "How work is done here",
  "brain.kind.gotcha": "Things that will bite you",
  "brain.kind.glossary": "What words mean here",

  "brain.source.human": "You wrote this",
  "brain.source.agent": "An agent recorded this",
  "brain.add": "Add",
  "brain.addPlaceholder": "Something that stays true about this project",
  "brain.placeholder.what": "e.g. Next.js 15 with Prisma, deployed on Vercel",
  "brain.placeholder.convention": "e.g. Tests live beside the code they cover",
  "brain.placeholder.gotcha": "e.g. The seed depends on insertion order",
  "brain.placeholder.glossary": "e.g. \"Tenant\" here means a billing account, not a user",
  "brain.save": "Save",
  "brain.cancel": "Cancel",
  "brain.edit": "Edit",
  "brain.archive": "Retire",
  "brain.archiveTitle": "Retire this — it stops being briefed, and the record stays",
  "brain.empty.title": "Nothing recorded yet",
  "brain.empty.body":
    "Write down what someone joining this project would need to know. Every agent that starts here is told the same thing.",

  "brain.notes.placeholder": "A reminder, a link, a thing to come back to",
  "brain.notes.add": "Add note",
  "brain.notes.pin": "Pin",
  "brain.notes.unpin": "Unpin",
  "brain.notes.delete": "Delete",
  "brain.notes.promote": "Move to knowledge",
  "brain.notes.promoteAs": "Move to knowledge as…",
  "brain.notes.empty.title": "No notes",
  "brain.notes.empty.body":
    "Notes are your working memory and are never sent to an agent. Anything that turns out to stay true can be moved into knowledge.",

  "brain.facts.title": "What the record shows",
  "brain.facts.derived": "Worked out from this project's own history, not written by anyone.",
  "brain.facts.empty": "Nothing has happened in this project yet.",
  "brain.fact.sessions": "{0} has run {1} sessions here",
  "brain.fact.sessions_one": "{0} has run {1} session here",
  "brain.fact.completed": "{0} missions completed",
  "brain.fact.completed_one": "{0} mission completed",
  "brain.fact.blocked": "{0} missions blocked or failed",
  "brain.fact.blocked_one": "{0} mission blocked or failed",
  "brain.fact.revoked": "{0} completions were later taken back",
  "brain.fact.revoked_one": "{0} completion was later taken back",
  "brain.fact.hotFile": "{0} changed {1} times",
  "brain.fact.hotFile_one": "{0} changed once",
  "brain.fact.refused": "{0} was refused {1} times",
  "brain.fact.refused_one": "{0} was refused once",

  "brain.timeline.empty": "Nothing has happened in this project yet.",

  "brain.brief.title": "What an agent is told",
  "brain.brief.show": "Preview the briefing",
  "brain.brief.hide": "Hide",
  "brain.brief.size": "{count} characters, sent once per session",
  "brain.brief.none":
    "Nothing is recorded yet, so agents start here with no briefing at all.",
  "brain.brief.notSupported":
    "{provider} has no way to take a briefing before it starts, so a session with it begins unbriefed.",

  // ---- Worktrees (§45) -----------------------------------------------------
  "worktree.title": "Worktrees",
  "worktree.subtitle":
    "A second checkout of this repository, on its own branch, in its own folder. An agent can work in one without touching the tree you are reading. Each is a project, so opening one opens it everywhere.",
  "worktree.main": "Repository",
  "worktree.current": "You are here",
  "worktree.detached": "Detached HEAD",
  "worktree.locked": "Locked",
  "worktree.gone": "Folder missing",
  "worktree.open": "Open",
  "worktree.remove": "Remove",
  "worktree.notOpened": "Made outside J.A.R.V.I.S.",
  "worktree.refused": "A guardrail refuses this. Change the rule in Settings if you meant it.",
  "worktree.notARepo.title": "Not a Git repository",
  "worktree.notARepo.body":
    "Worktrees are a Git feature. This project folder is not under Git, so there are none to show.",

  "worktree.confirm.title": "Remove the worktree for {branch}?",
  "worktree.confirm.body":
    "Git refused because this worktree has uncommitted work in it. Removing it deletes the folder and everything in it that was never committed.",
  "worktree.confirm.anyway": "Remove it anyway",

  "worktree.new.title": "New worktree",
  "worktree.new.placeholder": "branch name",
  "worktree.new.create": "New branch",
  "worktree.new.existing": "Existing branch",
  "worktree.new.action": "Create",
  "worktree.new.hint": "Created beside this repository, in a folder named after the branch.",

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

  "files.stale":
    "This file changed on disk after you opened it — an agent may have edited it. Nothing was saved. Reload to take the new version, or save again to overwrite it.",
  "files.unsaved.title": "Unsaved changes",
  "files.unsaved.saveAndClose": "Save and close",
  "files.unsaved.discard": "Close without saving",

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

  // ---- Acting on a change (§44) --------------------------------------------
  "review.stage": "Stage",
  "review.unstage": "Unstage",
  "review.staged": "Staged",
  "review.partlyStaged": "Partly staged",
  // The same operation, named for what it does to *this* file. Returning a
  // deleted file to HEAD brings it back; returning a modified one throws work
  // away. One word for both would be wrong half the time.
  "review.discard": "Discard",
  "review.restore": "Restore",
  "review.discardTitle": "Throw away the changes to {file}",
  "review.restoreTitle": "Bring {file} back as it was committed",
  "review.stageAll": "Stage everything",
  "review.unstageAll": "Unstage everything",

  "review.commit.title": "Commit",
  "review.commit.staged": "{count} staged",
  "review.commit.staged_one": "{count} staged",
  "review.commit.placeholder": "What changed, and why",
  "review.commit.action": "Commit staged changes",
  "review.commit.nothingStaged": "Stage something first — a commit takes what is in the index.",
  "review.commit.done": "Committed.",

  // The confirmation. It says what will run, because approving a paraphrase is
  // not approving anything (§35).
  "review.confirm.discardTitle": "Discard the changes to {file}?",
  "review.confirm.restoreTitle": "Restore {file}?",
  "review.confirm.body":
    "Uncommitted work is the one thing Git keeps no copy of. Once this runs there is nothing to recover it from.",
  // Restoring a deleted file is the case where this command *recovers*. The
  // discard wording would frighten someone out of the one action they want,
  // and it would also be untrue: what is thrown away is the deletion.
  "review.confirm.bodyDeleted":
    "The file comes back exactly as it was committed. Anything uncommitted about it — the deletion included — is thrown away.",
  "review.confirm.bodyUntracked":
    "Git has never seen this file, so there is no committed version to go back to. It will be removed from disk.",
  "review.confirm.willRun": "This will run",
  "review.confirm.cancel": "Cancel",
  "review.refused":
    "A guardrail refuses this. Change the rule in Settings if you meant it.",


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
  // ---- Provider accounts and quota (§66) -------------------------------
  "accounts.title": "Accounts & usage",
  "accounts.subtitle": "Persistent provider identities, honest quota signals, and where new work starts.",
  "accounts.provider.claude-code": "Claude Code",
  "accounts.provider.codex": "Codex",
  "accounts.add": "Add account",
  "accounts.refresh": "Refresh identities",
  "accounts.active": "Active",
  "accounts.machineAccount": "Machine account",
  "accounts.unnamed": "Unnamed account",
  "accounts.identityMissing": "Identity not available",
  "accounts.signIn": "Sign in",
  "accounts.useAccount": "Use for new work",
  "accounts.pause": "Pause",
  "accounts.pauseNeedsReplacement": "Add or resume another signed-in account before pausing this one.",
  "accounts.resume": "Resume",
  "accounts.rename": "Rename account",
  "accounts.remove": "Remove account",
  "accounts.removeConfirm": "Remove {name} from J.A.R.V.I.S.? The machine account is never deleted from disk.",
  "accounts.health.unverified": "Not verified",
  "accounts.health.ready": "Ready",
  "accounts.health.nearing": "Nearing limit",
  "accounts.health.exhausted": "Exhausted",
  "accounts.health.paused": "Paused",
  "accounts.health.signedOut": "Signed out",
  "accounts.window.five_hour": "5-hour window",
  "accounts.window.weekly": "Weekly window",
  "accounts.window.seven_day": "7-day window",
  "accounts.window.opus_weekly": "Weekly Opus",
  "accounts.window.seven_day_opus": "Weekly Opus",
  "accounts.window.primary": "Primary window",
  "accounts.window.secondary": "Secondary window",
  "accounts.window.unknown": "Provider window",
  "accounts.confidence.official": "Official",
  "accounts.confidence.observed": "Observed",
  "accounts.confidence.estimated": "Estimated",
  "accounts.confidence.unknown": "Allowance unknown",
  "accounts.tokens.used": "{tokens} tokens observed",
  "accounts.tokens.today": "{tokens} tokens in the last 24 hours",
  "accounts.reset.now": "Resetting now",
  "accounts.reset.hours": "Resets in {hours}h {minutes}m",
  "accounts.reset.minutes": "Resets in {minutes}m",
  "accounts.calibration_one": "Learned from {count} refusal",
  "accounts.calibration": "Learned from {count} refusals",
  "accounts.liveSessions_one": "{count} running session stays on this account",
  "accounts.liveSessions": "{count} running sessions stay on this account",
  "accounts.folderUntrusted": "This account does not trust the current folder yet. Open Claude Code here once and decide in its own trust prompt.",
  "accounts.trustIsPerAccount": "Folder trust belongs to this account. A new folder must be approved in Claude Code before an unattended switch can use it.",
  "accounts.autoSwitch.title": "Automatic switching",
  "accounts.autoSwitch.body": "Running sessions keep their account. New work can move when the provider reports exhaustion.",
  "accounts.autoSwitch.estimatedDisclosure": "Also switches near {percent}% using J.A.R.V.I.S.'s estimate. Claude Code does not report a live percentage.",
  "accounts.autoSwitch.off": "Off",
  "accounts.autoSwitch.onExhaustion": "When exhausted",
  "accounts.autoSwitch.onThreshold": "Near the limit",
  "accounts.add.title": "Add a separate identity",
  "accounts.add.body": "A private configuration directory is created, then the provider opens its own browser sign-in. No credential passes through J.A.R.V.I.S.",
  "accounts.label": "Account name",
  "accounts.labelPlaceholder": "e.g. Personal 2",
  "accounts.emailOptional": "Email (optional, pre-fills Claude login)",
  "accounts.continueToLogin": "Continue to sign in",
  "accounts.openingLogin": "Opening sign-in…",
  "accounts.empty.title": "No account for this provider",
  "accounts.empty.body": "Add one to give new sessions an isolated, persistent sign-in.",
  "accounts.error": "The account operation did not complete. Nothing was changed silently.",

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
  "guardrail.op.git.discard-changes": "Throw away uncommitted work",
  "guardrail.op.git.discard-changes.detail":
    "git restore, checkout -- <file>, stash drop. The only thing Git keeps no copy of.",
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
  "guardrail.origin.surface": "You asked for it",
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

  "activity.kind.git.worktreeAdded": "Created a worktree",
  "activity.kind.git.worktreeRemoved": "Removed a worktree",
  "activity.kind.git.staged": "Staged a change",
  "activity.kind.git.unstaged": "Unstaged a change",
  "activity.kind.git.discarded": "Discarded uncommitted work",
  "activity.kind.git.committed": "Committed",
  "activity.kind.guardrail.denied": "Guardrail refused an operation",
  "activity.kind.guardrail.allowed": "Guardrail approved an operation",
  "activity.kind.brain.agentRecorded": "An agent recorded something it learned",
  "activity.kind.account.autoSwitched": "Switched the active account automatically",
  "activity.kind.account.relayStarted": "Continued an unattended run on another account",

  // ---- Evidence, worded here rather than in Rust (§65) ----------------------
  "evidence.guardrailRefused": "Not checked: a guardrail refused {operation}",
  "evidence.command.passed": "`{command}` exited {exitCode} in {seconds}s",
  "evidence.command.failed": "`{command}` exited {exitCode}, expected {expectExitCode}",
  "evidence.command.timedOut": "`{command}` did not finish within {seconds}s",
  "evidence.command.notRun": "`{command}` could not be run",
  "evidence.file.exists": "{path} exists",
  "evidence.file.missing": "{path} does not exist",
  "evidence.file.contains": "{path} contains the expected text",
  "evidence.file.doesNotContain": "{path} does not contain the expected text",
  "evidence.file.unreadable": "{path} could not be read",
  "evidence.manual.needsConfirmation": "Needs a person to confirm",
  "evidence.manual.confirmedBy": "Confirmed by {who}",


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
  "autopilot.folderNotTrusted":
    "Claude Code has not been told this folder is safe, so it would open with its trust question and wait for an answer nobody is here to give. Open an agent in this project once and accept it, then start the run.",
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

  // ---- Global Search (§51) --------------------------------------------------
  "search.title": "Search everywhere",
  "search.placeholder": "Search knowledge, notes, missions, activity and conversations…",
  "search.empty.prompt": "Search across every project — not just this one",
  "search.empty.noResults": "Nothing found",
  "search.empty.tooShort": "Keep typing — at least two characters",
  "search.group.conversation": "Conversations",
  "search.group.knowledge": "Knowledge",
  "search.group.note": "Notes",
  "search.group.mission": "Missions",
  "search.group.activity": "Activity",
  "search.everyProject": "Every project",
  "search.historicalTab": "Opened from Search — read-only",


  // ---- Notifications (§49) --------------------------------------------------
  //
  // `notify.title.<reason>` mirrors `notify::Reason` in the core exactly. The
  // core sends a stable identifier and never prose, so a reason added there
  // without a line here is a compile error in pt-BR rather than a raw
  // identifier rendered on screen (§65).
  "notify.title": "Notifications",
  "notify.open": "Notifications",
  "notify.unread": "{count} waiting",
  "notify.unread_one": "1 waiting",
  "notify.empty": "Nothing is waiting.",
  "notify.emptyHint": "When an agent finishes, or stops to ask you something, it shows up here.",
  "notify.markAllSeen": "Mark all as read",
  "notify.clear": "Clear",
  "notify.goTo": "Open",
  "notify.disabled": "Notifications are off.",
  "notify.disabledHint": "Turn them back on in Settings.",
  "notify.more": "and {count} more",
  "notify.more_one": "and 1 more",
  "notify.coalesced": "{count} agents want you",
  "notify.someProject": "this project",

  // What kind of stop it was. The agent's own words go underneath, verbatim.
  "notify.title.providerPrompt": "{agent} is asking",
  "notify.title.guardrailPending": "A guardrail is holding this",
  "notify.title.guardrailAsked": "{agent} needs approval",
  "notify.title.turnEnded": "{agent} finished",
  "notify.title.missionCompleted": "Mission complete",
  "notify.title.runCompleted": "Ran to done on its own",
  "notify.title.sessionEnded": "{agent} closed",
  "notify.title.sessionFailed": "{agent} stopped unexpectedly",
  "notify.title.missionBlocked": "A mission needs you",
  "notify.title.runStopped": "An unattended run stopped",

  // Where the fact came from (§28). Shown quietly, and only where there is room.
  "notify.from.official": "Reported by the agent",
  "notify.from.observed": "Read from the terminal",

  "notify.group.now": "Just now",
  "notify.group.today": "Earlier today",
  "notify.group.earlier": "Earlier",

  "settings.notifications": "Notifications",
  "settings.notifications.enabled": "Tell me when an agent stops",
  "settings.notifications.enabledHelp": "Only when you are not already watching that session. An agent finishing in front of you is not news.",
  "settings.notifications.system": "Also show a desktop notification",
  "settings.notifications.systemHelp": "So it reaches you while J.A.R.V.I.S. is behind another window.",
  "settings.notifications.sound": "Play a sound",
  "settings.notifications.test": "Send a test notification",
  "settings.notifications.testSent": "Sent",
  "settings.notifications.testPreview": "Do you want to proceed?",

  // ---- Generic -------------------------------------------------------------
  "common.cancel": "Cancel",
  "common.confirm": "Confirm",
  "common.retry": "Retry",
  "common.dismiss": "Dismiss",
  "common.loading": "Loading…",
} as const;

export type MessageKey = keyof typeof en;
