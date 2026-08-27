import { useCallback, useEffect, useMemo, useState } from "react";
import { Bell, Languages, Moon, NotebookPen, RefreshCw, Search, Sun, SunMoon } from "lucide-react";
import { LOCALES, LOCALE_NAMES } from "@jarvis/i18n";
import { Rail, RAIL_ITEMS, type SurfaceId } from "./shell/Rail";
import { TitleBar } from "./shell/TitleBar";
import { StatusBar } from "./shell/StatusBar";
import { CommandPalette, type Command } from "./shell/CommandPalette";
import { GlobalSearch } from "./shell/GlobalSearch";
import { NotificationBell } from "./shell/notify/NotificationBell";
import { NotificationCentre } from "./shell/notify/NotificationCentre";
import { Toasts } from "./shell/notify/Toasts";
import { useNotificationFeed } from "./shell/notify/useNotificationFeed";
import {
  setWindowFocused,
  useNotifications,
  type Notification,
} from "./app/notifications";
import { usePreferences } from "./surfaces/settings/usePreferences";
import { invoke } from "./app/platform";
import type { SearchResult } from "./app/search";
import { Activity } from "./surfaces/activity/Activity";
import { Friends } from "./surfaces/social/Friends";
import { History } from "./surfaces/history/History";
import { kindOf } from "./surfaces/history/format";
import { Analytics } from "./surfaces/analytics/Analytics";
import { Accounts } from "./surfaces/accounts/Accounts";
import { MissionControl } from "./surfaces/mission-control/MissionControl";
import { Missions } from "./surfaces/missions/Missions";
import { Auth } from "./surfaces/identity/Auth";
import { registerLocaleSetter, useIdentity } from "./surfaces/identity/useIdentity";
import { Onboarding } from "./surfaces/onboarding/Onboarding";
import { useOnboarding } from "./surfaces/onboarding/useOnboarding";
import { Projects } from "./surfaces/projects/Projects";
import { ProjectWorkspace, type Area } from "./surfaces/project/ProjectWorkspace";
import type { Project } from "./surfaces/projects/useProjects";
import type { SessionKind } from "./app/sessions";
import { Settings } from "./surfaces/settings/Settings";
import { Notebook } from "./surfaces/notebook/Notebook";
import { CATEGORIES, CATEGORY } from "./surfaces/settings/categories";
import { openSettingsCategory } from "./surfaces/settings/settingsNav";
import { useEnvironmentStore } from "./surfaces/environment/useEnvironment";
import { useProjects } from "./surfaces/projects/useProjects";
import { useTerminals } from "./surfaces/terminal/useTerminals";
import { useI18n, useT } from "./app/i18n";
import { useTheme } from "./app/theme";
import "./App.css";

/**
 * Surfaces that actually exist.
 *
 * Navigation is derived from this list, so the product never advertises a
 * destination it cannot deliver (§81). Milestones add entries here as they
 * land; nothing has to be restructured when they do.
 */
const IMPLEMENTED: SurfaceId[] = [
  "mission-control",
  "projects",
  "missions",
  "activity",
  "friends",
  "history",
  "analytics",
  "accounts",
  "settings",
];

export function App() {
  const t = useT();
  const { locale, setLocale } = useI18n();
  const { preference, setPreference } = useTheme();
  const rescanEnvironment = useEnvironmentStore((state) => state.scan);
  const projects = useProjects((state) => state.projects);
  const openTerminal = useTerminals((state) => state.openTerminal);
  const onboardingSeen = useOnboarding((state) => state.seen);
  const loadOnboarding = useOnboarding((state) => state.load);
  const identity = useIdentity((state) => state.report);
  const loadIdentity = useIdentity((state) => state.load);
  const authOpen = useIdentity((state) => state.authOpen);

  /**
   * Whether the auth screen is what is on screen (M20).
   *
   * Two ways in, and they are different questions. `prompted === false` is the
   * product offering an account **once**, on a machine that has never been
   * asked; `authOpen` is somebody deliberately going there from Settings, which
   * has to keep working forever after that one offer is spent.
   *
   * It is not a gate either way: the screen's own "Continue without an account"
   * is a real destination, and nothing in the core asks who is signed in before
   * deciding whether work may happen. See `identity`'s module note.
   */
  const authShowing = identity !== null && (identity.prompted === false || authOpen);

  const [surface, setSurface] = useState<SurfaceId>("mission-control");
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const [notebookOpen, setNotebookOpen] = useState(false);
  // An open project takes over the surface area. The rail stays put, so the
  // user never loses their bearings when they go deeper (§85).
  const [openProject, setOpenProject] = useState<Project | null>(null);
  // Set when arriving at Missions from somewhere that already knows which one.
  const [focusMission, setFocusMission] = useState<string | undefined>();
  // Where inside a project Global Search (§51) wants to land — which area, and
  // which past session's conversation, if any.
  const [focusArea, setFocusArea] = useState<Area | undefined>();
  const [focusSessionId, setFocusSessionId] = useState<string | undefined>();
  const [focusSessionProvider, setFocusSessionProvider] = useState<SessionKind | undefined>();
  const [focusSessionTitle, setFocusSessionTitle] = useState<string | undefined>();
  // Distinguishes one "take me there" from the next — see `focusToken` in
  // ProjectWorkspace for what silently did not happen without it.
  const [focusToken, setFocusToken] = useState(0);
  const [accountsProjectId, setAccountsProjectId] = useState<string | null>(null);
  const [notificationsOpen, setNotificationsOpen] = useState(false);
  const outstanding = useNotifications((state) => state.outstanding);
  const { toasts, dismiss, clearToasts, setChannels } = useNotificationFeed();
  const { prefs } = usePreferences();

  /**
   * Where the Notebook's "Send to agent" would type (M19).
   *
   * The active tab of the project on screen, and nothing otherwise. The
   * notebook is global — it opens from Mission Control, from Settings, from
   * anywhere — so most of the time there is no answer, and the surface says so
   * rather than offering a button that goes nowhere (§81).
   *
   * A shell is deliberately included. Pasting a prompt into a shell is not
   * useful, but deciding *for* somebody which of their own terminals may
   * receive their own text is a judgement this does not get to make.
   */
  const terminalTabs = useTerminals((state) => state.tabs);
  const activeTabs = useTerminals((state) => state.activeTab);
  const notebookTarget = useMemo(() => {
    if (!openProject) return null;
    const sessionId = activeTabs[openProject.id];
    if (!sessionId) return null;
    const tab = terminalTabs[openProject.id]?.find((candidate) => candidate.sessionId === sessionId);
    // A historical tab is a transcript of a session that already ended — there
    // is no process behind it to type into (§88).
    if (!tab || tab.historical) return null;
    return { sessionId, agent: tab.title };
  }, [openProject, activeTabs, terminalTabs]);

  type Focus = {
    area?: Area;
    sessionId?: string;
    sessionProvider?: SessionKind;
    sessionTitle?: string;
  };

  /**
   * Open a project workspace from a `Project` already in hand.
   *
   * This is the *only* place `openProject` is set to a real project, so it is
   * also the one place that has to decide the focus fields — every caller
   * that does not pass one clears it, or a stale search result would leak its
   * focus into the next, unrelated project somebody opens by hand.
   */
  const openProjectDirect = useCallback((project: Project, focus?: Focus) => {
    setOpenProject(project);
    setFocusArea(focus?.area);
    setFocusSessionId(focus?.sessionId);
    setFocusSessionProvider(focus?.sessionProvider);
    setFocusSessionTitle(focus?.sessionTitle);
    if (focus?.area) setFocusToken((token) => token + 1);
  }, []);

  /**
   * Open a project workspace by id, looked up in the already-loaded list.
   *
   * Missions and Global Search know their project only as an id, so this is
   * the join between the two (§86). **Do not use this for a project that was
   * just created in the same gesture** — `projects` here is whatever this
   * render closed over, and a project added a moment ago by `openFolder()`
   * can lose that race and simply not be found. `openProjectDirect` with the
   * `Project` the caller already has is what those callers want instead.
   * Found by opening a brand-new folder from this screen and landing back on
   * Mission Control instead of inside it.
   */
  const openProjectById = useCallback(
    (projectId: string, focus?: Focus) => {
      const project = projects.find((p) => p.id === projectId);
      if (project) openProjectDirect(project, focus);
      return project;
    },
    [projects, openProjectDirect],
  );

  /**
   * Go to a project by id, **even one that has been archived**.
   *
   * `openProjectById` looks the id up in the loaded list, and that list is
   * `list_projects`, which filters archived projects out — correctly, because
   * it is the list of places to work. Anything that arrives holding only an id
   * therefore silently does nothing when the project has been archived: no
   * error, no navigation, a click that is simply inert.
   *
   * That is not hypothetical. Archiving is what happens to a scratch project
   * and to a removed worktree (§45), and their sessions are still history.
   * Found by clicking a Session History row in the real app and watching
   * nothing at all happen — the same silent-fall-through shape as HANDOFF
   * item 33, with the id missing for a different reason.
   *
   * The store is still tried first: it is already in memory, and this is only
   * a fallback for the case it cannot answer.
   */
  const openProjectAnywhere = useCallback(
    async (projectId: string, focus?: Focus) => {
      if (openProjectById(projectId, focus)) return;
      try {
        const project = await invoke<Project | null>("get_project", { id: projectId });
        if (project) openProjectDirect(project, focus);
      } catch {
        // Nothing to navigate to. Staying put is the honest outcome — better
        // than a blank workspace for a project that is genuinely gone.
      }
    },
    [openProjectById, openProjectDirect],
  );

  const handleSearchResult = useCallback(
    (result: SearchResult) => {
      if (result.kind === "mission" && result.missionId) {
        setOpenProject(null);
        setFocusMission(result.missionId);
        setSurface("missions");
        return;
      }
      if (result.kind === "activity") {
        setOpenProject(null);
        setFocusMission(undefined);
        setSurface("activity");
        return;
      }
      if (!result.projectId) return;
      if (result.kind === "conversation" && result.sessionId) {
        // `openProjectAnywhere`, not `openProjectById`: a conversation can
        // perfectly well have happened in a project since archived, and this
        // is the one gesture that has to reach it.
        void openProjectAnywhere(result.projectId, {
          area: "sessions",
          sessionId: result.sessionId,
          sessionProvider: (result.sessionProvider as SessionKind | null) ?? "shell",
          sessionTitle: result.heading || undefined,
        });
      } else {
        openProjectById(result.projectId, { area: "brain" });
      }
    },
    [openProjectById, openProjectAnywhere],
  );

  const goTo = useCallback(
    (id: SurfaceId) => {
      if (id === "accounts") setAccountsProjectId(openProject?.id ?? null);
      setOpenProject(null);
      setFocusMission(undefined);
      setSurface(id);
    },
    [openProject],
  );

  // Who is signed in, and whether an account has ever been offered (M20).
  // Fetched up front for the same reason onboarding is: the reveal below waits
  // on it, so the window never shows the shell for a frame and then swaps to
  // the auth screen.
  useEffect(() => {
    void loadIdentity();
  }, [loadIdentity]);

  // The locale lives in React context and `useIdentity` is not a component, so
  // signing in cannot reach the setter by itself. Registered once, here.
  useEffect(() => {
    registerLocaleSetter(setLocale);
  }, [setLocale]);

  // Whether this machine has ever gotten past the welcome screen (§13).
  // Fetched once, up front, so the reveal below never shows the normal shell
  // for one frame before swapping to onboarding.
  useEffect(() => {
    void loadOnboarding();
  }, [loadOnboarding]);

  // Reveal the window only once the first frame has painted, so launching
  // never shows an empty white rectangle (§11) — and now, only once it is
  // also known whether that first frame is the welcome screen or the normal
  // shell, so the window never shows one and then silently swaps to the
  // other. `onboardingSeen` starts `null` and `load()` always resolves it to
  // a real boolean, even on failure (see `useOnboarding`'s own comment) —
  // this must never be the reason the window stays hidden (item 31, HANDOFF).
  useEffect(() => {
    // `identity` is the same kind of gate as `onboardingSeen`, and `load()`
    // always resolves it — even on failure — so this cannot be the reason the
    // window stays hidden either.
    if (onboardingSeen === null || identity === null) return;
    let cancelled = false;
    const reveal = async () => {
      if (!("__TAURI_INTERNALS__" in window)) return;
      const { invoke } = await import("@tauri-apps/api/core");
      if (!cancelled) await invoke("window_ready");
    };
    const frame = requestAnimationFrame(() => void reveal());
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
    };
  }, [onboardingSeen, identity]);

  // Keep the presentation channels current without rebuilding the listener.
  useEffect(() => {
    setChannels(prefs.notificationsSystem, prefs.notificationsSound);
  }, [prefs.notificationsSystem, prefs.notificationsSound, setChannels]);

  // Whether the window has focus is half of the suppression rule (§49), and
  // the core cannot see it. Reported here rather than per-surface because it
  // is a fact about the window, not about what is open in it.
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void (async () => {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      const win = getCurrentWindow();
      setWindowFocused(await win.isFocused());
      const stop = await win.onFocusChanged(({ payload }) => {
        setWindowFocused(payload);
        // Coming back to the window is the person arriving. Whatever the
        // toasts were telling them is now something they can see for
        // themselves, and a stack of stale toasts over the work is the exact
        // clutter this feature has to avoid being.
        if (payload) clearToasts();
      });
      if (cancelled) stop();
      else unlisten = stop;
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [clearToasts]);

  /**
   * Go to what a notification is about (§49).
   *
   * The one click-through in the feature, because it is the only one there can
   * be: a Windows toast has no activation callback on the desktop. A
   * notification with a session opens that project's workspace; one with only
   * a mission goes to the mission; one with neither has nowhere to go and is
   * never given a hit target in the first place.
   */
  const openNotification = useCallback(
    (notification: Notification) => {
      dismiss(notification.id);
      if (notification.missionId && !notification.sessionId) {
        setOpenProject(null);
        setFocusMission(notification.missionId);
        setSurface("missions");
        return;
      }
      if (notification.projectId) {
        // The session as well as the area. Landing on Sessions and leaving
        // whichever tab happened to be active is not "take me to it" — with
        // four terminals open it is a coin toss, and the whole promise of a
        // notification is that it puts you in front of the thing it is about.
        //
        // `sessionProvider` is deliberately **not** passed. It is what tells
        // ProjectWorkspace to open a *historical, read-only* tab (§51), and a
        // notification is about a session that is very much alive. Passing it
        // would reopen a live agent as a transcript.
        openProjectById(notification.projectId, {
          area: "sessions",
          sessionId: notification.sessionId ?? undefined,
        });
      }
    },
    [dismiss, openProjectById],
  );

  const togglePalette = useCallback(() => setPaletteOpen((open) => !open), []);

  /**
   * Ctrl/Cmd+K opens the command palette (§50), from anywhere.
   *
   * Registered in the **capture** phase, and it stops the event there. A
   * bubble-phase listener is not enough: Monaco treats Ctrl+K as a chord prefix
   * and calls `stopPropagation`, so the event never reaches `window` and the
   * palette silently stopped opening whenever the editor had focus — while the
   * titlebar went on advertising the shortcut. Found by pressing it in the real
   * app and watching "claro" get typed into a source file.
   *
   * A shortcut the whole product advertises has to be resolved before any
   * widget gets an opinion about it.
   */
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // Nothing the shell owns opens over the auth screen. These are capture
      // phase and `preventDefault` — they answer before any widget does — so
      // without this guard Ctrl+K would open the command palette on top of a
      // login form, over a product the person has not reached yet. The same
      // reasoning gates the bell and the Notebook below.
      if (authShowing) return;
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        event.stopPropagation();
        togglePalette();
      } else if (
        (event.ctrlKey || event.metaKey) &&
        event.shiftKey &&
        event.key.toLowerCase() === "n"
      ) {
        // The Notebook (M19). Registered here, in the capture phase, for the
        // same reason Ctrl+K is: the terminal and Monaco both claim keys before
        // a bubble-phase listener ever sees them — and a terminal holding focus
        // is exactly when somebody reaches for their prompt library.
        event.preventDefault();
        event.stopPropagation();
        setNotebookOpen((open) => !open);
      } else if (
        (event.ctrlKey || event.metaKey) &&
        event.shiftKey &&
        event.key.toLowerCase() === "f"
      ) {
        // Global Search (§51). Same capture-phase reasoning as Ctrl+K above —
        // Monaco and the terminal both have their own ideas about Ctrl+Shift+F
        // otherwise.
        event.preventDefault();
        event.stopPropagation();
        setSearchOpen((open) => !open);
      }
    };
    window.addEventListener("keydown", onKeyDown, { capture: true });
    return () => window.removeEventListener("keydown", onKeyDown, { capture: true });
  }, [togglePalette, authShowing]);

  const commands = useMemo<Command[]>(() => {
    const navigation: Command[] = RAIL_ITEMS.filter((item) =>
      IMPLEMENTED.includes(item.id),
    ).map((item) => ({
      id: `go.${item.id}`,
      title: t(item.label),
      group: "Go to",
      icon: item.icon,
      run: () => goTo(item.id),
    }));

    const appearance: Command[] = [
      {
        id: "theme.dark",
        title: t("settings.theme.dark"),
        group: t("settings.appearance"),
        icon: Moon,
        keywords: "theme dark tema escuro",
        hint: preference === "dark" ? "•" : undefined,
        run: () => setPreference("dark"),
      },
      {
        id: "theme.light",
        title: t("settings.theme.light"),
        group: t("settings.appearance"),
        icon: Sun,
        keywords: "theme light tema claro",
        hint: preference === "light" ? "•" : undefined,
        run: () => setPreference("light"),
      },
      {
        id: "theme.system",
        title: t("settings.theme.system"),
        group: t("settings.appearance"),
        icon: SunMoon,
        keywords: "theme system tema sistema",
        hint: preference === "system" ? "•" : undefined,
        run: () => setPreference("system"),
      },
      ...LOCALES.map((value) => ({
        id: `locale.${value}`,
        title: LOCALE_NAMES[value],
        group: t("settings.language"),
        icon: Languages,
        keywords: `language idioma locale ${value}`,
        hint: locale === value ? "•" : undefined,
        run: () => setLocale(value),
      })),
    ];

    const actions: Command[] = [
      {
        // The palette is this product's primary keyboard entry point (§50), and
        // a surface that can only be reached by aiming at a 14px bell is one
        // half the people who need it will not find.
        id: "notify.open",
        title: t("notify.title"),
        group: "Go to",
        icon: Bell,
        keywords: "notifications alerts notificações alertas avisos bell sino",
        hint: outstanding > 0 ? t("notify.unread", { count: outstanding }) : undefined,
        run: () => setNotificationsOpen(true),
      },
      {
        id: "notebook.open",
        title: t("notebook.title"),
        group: "Go to",
        icon: NotebookPen,
        keywords: "notebook notes prompts ideas bloco notas anotações prompts ideias",
        hint: "Ctrl Shift N",
        run: () => setNotebookOpen(true),
      },
      {
        id: "search.open",
        title: t("search.title"),
        group: "Go to",
        icon: Search,
        keywords: "search find buscar procurar everywhere tudo",
        hint: "Ctrl Shift F",
        run: () => setSearchOpen(true),
      },
      {
        id: "env.rescan",
        title: t("env.rescan"),
        group: t("env.title"),
        icon: RefreshCw,
        keywords: "environment scan doctor ambiente verificar",
        run: () => {
          // Land on the section that owns the scan. Before Settings had
          // sections this navigated to a page where everything was visible at
          // once and the distinction did not exist; now arriving on Appearance
          // while a scan runs three rooms away would be a lie about where the
          // command went.
          goTo("settings");
          openSettingsCategory("environment");
          void rescanEnvironment(true);
        },
      },
    ];

    /**
     * Every section of Settings, reachable by name (§50).
     *
     * The left column is only a map once you are already in Settings. This is
     * the same map from anywhere — typing "guardrails" or "proteções" goes
     * straight to the section rather than to the screen it lives on.
     */
    const settings: Command[] = CATEGORIES.map((id) => {
      const category = CATEGORY[id];
      return {
        id: `settings.${id}`,
        title: t(category.label),
        group: t("nav.settings"),
        icon: category.icon,
        keywords: category.keywords,
        run: () => {
          goTo("settings");
          openSettingsCategory(id);
        },
      };
    });

    return [...navigation, ...appearance, ...actions, ...settings];
  }, [t, locale, preference, setLocale, setPreference, rescanEnvironment, goTo, outstanding]);

  return (
    <div className="app">
      <TitleBar
        onOpenPalette={authShowing ? undefined : togglePalette}
        // Gated the same way the bell is: onboarding is one screen with one
        // question on it, and a control that opens a library of prompts on a
        // first-run install is a door to somewhere that does not exist yet.
        onOpenNotebook={
          onboardingSeen !== false && !authShowing ? () => setNotebookOpen(true) : undefined
        }
        notifications={
          onboardingSeen !== false && !authShowing ? (
            <NotificationBell
              count={outstanding}
              open={notificationsOpen}
              onToggle={() => setNotificationsOpen((open) => !open)}
            />
          ) : undefined
        }
      />

      <NotificationCentre
        open={notificationsOpen}
        onClose={() => setNotificationsOpen(false)}
        onOpenNotification={openNotification}
      />

      {/* Over everything, including a project workspace. Nothing behind it
          unmounts or resizes, so an agent mid-turn is untouched — which is the
          whole point of it being an overlay (M19). */}
      <Notebook
        // One gate rather than two: this covers the shortcut as well as the
        // titlebar button, so neither can open a library of prompts over a
        // first-run screen that exists to ask one question.
        open={notebookOpen && onboardingSeen !== false && !authShowing}
        onClose={() => setNotebookOpen(false)}
        target={notebookTarget}
      />

      {/* Ordered as somebody meets the product: who you are, then what is on
          this machine, then the product itself. Identity comes first because
          the welcome screen's environment scan is about the machine, and being
          asked who you are *after* being shown a machine report reads as an
          interruption rather than as an opening. */}
      {authShowing ? (
        <Auth />
      ) : onboardingSeen === false ? (
        <Onboarding onOpenProject={(project) => openProjectDirect(project)} />
      ) : (
        <div className="app__body">
          <Rail active={surface} available={IMPLEMENTED} onNavigate={goTo} />

          <main className="app__surface" key={openProject ? openProject.id : surface}>
            {openProject ? (
              <ProjectWorkspace
                project={openProject}
                onBack={() => setOpenProject(null)}
                onOpenProject={openProjectById}
                focusArea={focusArea}
                focusSessionId={focusSessionId}
                focusSessionProvider={focusSessionProvider}
                focusSessionTitle={focusSessionTitle}
                focusToken={focusToken}
              />
            ) : surface === "settings" ? (
              <Settings />
            ) : surface === "projects" ? (
              <Projects onOpen={(project) => openProjectDirect(project)} />
            ) : surface === "activity" ? (
              <Activity />
            ) : surface === "friends" ? (
              <Friends />
            ) : surface === "history" ? (
              <History
                // Routed through the *same* path Global Search uses to reopen a
                // past session (§51): the project workspace, the sessions area,
                // and a read-only historical tab. A second mechanism for
                // opening a session is exactly what §23 exists to prevent -- and
                // `focusToken` is what makes it work when the project is
                // already open (HANDOFF item 53).
                onOpenSession={(entry) => {
                  // `openProjectAnywhere`: a history row very often belongs to
                  // a project that has since been archived -- a scratch folder,
                  // a removed worktree -- and its sessions are still history.
                  void openProjectAnywhere(entry.projectId, {
                    area: "sessions",
                    sessionId: entry.id,
                    // A live session is opened as itself; only a finished one
                    // is reopened as a transcript. Passing the provider is what
                    // marks a tab historical, and doing it for a running agent
                    // would turn it into a read-only view of a session that is
                    // still going.
                    sessionProvider: entry.live ? undefined : kindOf(entry),
                    sessionTitle: entry.title ?? undefined,
                  });
                }}
                // Still running: rejoin it. There is nothing to hand back — the
                // agent never stopped — so this is the ordinary "take me to
                // that session" path with no provider passed, which is what
                // keeps it from opening as a read-only transcript.
                onGoToTerminal={(entry) => {
                  void openProjectAnywhere(entry.projectId, {
                    area: "sessions",
                    sessionId: entry.id,
                  });
                }}
                // Finished: continue it (§88, D41). A new agent process, handed
                // this conversation, in a tab named after it.
                //
                // The project is opened **first and awaited**, because
                // `openTerminal` needs the workspace to exist for the tab to
                // land in — and `openProjectAnywhere` reaches an archived
                // project, which a continued session very often belongs to.
                onContinue={async (entry) => {
                  await openProjectAnywhere(entry.projectId, { area: "sessions" });
                  await openTerminal(
                    entry.projectId,
                    kindOf(entry),
                    { cols: 120, rows: 30 },
                    // Not the old session's mission. A continuation is new work
                    // by default; inheriting a mission would quietly attach
                    // evidence to something nobody chose (§86).
                    undefined,
                    { sessionId: entry.id, title: entry.title ?? undefined },
                  );
                }}
                onOpenMission={(missionId) => {
                  setOpenProject(null);
                  setFocusMission(missionId);
                  setSurface("missions");
                }}
              />
            ) : surface === "analytics" ? (
              <Analytics />
            ) : surface === "accounts" ? (
              <Accounts projectId={accountsProjectId} />
            ) : surface === "missions" ? (
              <Missions
                initialMissionId={focusMission}
                // Starting an agent from a mission tags the session with it, so
                // the terminal, the conversation and the evidence all belong to
                // the same thread of work (§86).
                onLaunchAgent={(projectId, missionId) => {
                  if (openProjectById(projectId)) {
                    void openTerminal(projectId, "claude-code", { cols: 120, rows: 30 }, missionId);
                  }
                }}
                onOpenSession={(projectId) => {
                  openProjectById(projectId);
                }}
              />
            ) : (
              <MissionControl
                onOpenProject={() => goTo("projects")}
                onOpenMission={(mission) => {
                  setOpenProject(null);
                  setFocusMission(mission.id);
                  setSurface("missions");
                }}
              />
            )}
          </main>
        </div>
      )}

      {onboardingSeen !== false && !authShowing && (
        <StatusBar onOpenAccounts={() => goTo("accounts")} />
      )}

      <Toasts items={toasts} onOpen={openNotification} onDismiss={dismiss} />

      <CommandPalette
        open={paletteOpen && !authShowing}
        commands={commands}
        onClose={() => setPaletteOpen(false)}
      />
      <GlobalSearch
        open={searchOpen && !authShowing}
        onClose={() => setSearchOpen(false)}
        onSelect={handleSearchResult}
      />
    </div>
  );
}
