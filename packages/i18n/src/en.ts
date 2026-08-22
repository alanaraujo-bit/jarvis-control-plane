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

  // ---- Generic -------------------------------------------------------------
  "common.cancel": "Cancel",
  "common.confirm": "Confirm",
  "common.retry": "Retry",
  "common.dismiss": "Dismiss",
  "common.loading": "Loading…",
} as const;

export type MessageKey = keyof typeof en;
