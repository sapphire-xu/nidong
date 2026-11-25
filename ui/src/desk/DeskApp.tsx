import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Button, FluentProvider } from "@fluentui/react-components";
import { relaunch } from "@tauri-apps/plugin-process";
import type { Update } from "@tauri-apps/plugin-updater";
import {
  ArrowDownload20Filled,
  ArrowDownload20Regular,
  CalendarLtr20Filled,
  CalendarLtr20Regular,
  DoorArrowRight20Filled,
  DoorArrowRight20Regular,
  Home20Filled,
  Home20Regular,
  Info20Filled,
  Info20Regular,
  PlugConnected20Filled,
  PlugConnected20Regular,
  Receipt20Filled,
  Receipt20Regular,
  Settings20Filled,
  Settings20Regular,
  Share20Filled,
  Share20Regular,
  SpeakerMute20Filled,
  SpeakerMute20Regular,
  Toolbox20Filled,
  Toolbox20Regular,
  Wifi120Filled,
  Wifi120Regular,
} from "@fluentui/react-icons";
import { api, type PluginInfo } from "../api";
import { EventForm } from "../EventForm";
import { cacheDayLabel } from "../course";
import { getPlatformInfo, usePlatformInfo } from "../platform";
import { resolveTheme, withSettingDefaults } from "../prefs";
import type { Agenda, AppSettings, DaySpend, LoginOutcome, Snapshot } from "../types";
import { useClock } from "../useClock";
import { DeskNav, type DeskNavItem } from "./DeskNav";
import {
  isCoreNavId,
  normalizeNavPins,
  pinToolNav,
  pluginIdFromToolNavId,
  reorderVisibleNav,
  resolveVisibleNavIds,
  subPageFromToolNavId,
  toolNavIdFromSubPage,
  unpinToolNav,
  type BuiltinToolKey,
  type CoreNavId,
  type NavId,
  type ToolNavId,
  type ToolSubPage,
} from "./navModel";
import { lookForUpdate } from "./updateCheck";
import { nidongDarkTheme, nidongLightTheme } from "./theme";
import { About } from "./pages/About";
import { Bills } from "./pages/Bills";
import { Doors } from "./pages/Doors";
import { Home } from "./pages/Home";
import { Login, type Sms } from "./pages/Login";
import { PluginBridge } from "./pages/Plugins";
import { Schedule } from "./pages/Schedule";
import { SettingsPage } from "./pages/Settings";
import { Toolbox } from "./pages/Toolbox";

const CORE_NAV_META: Record<CoreNavId, { label: string; icon: JSX.Element; activeIcon: JSX.Element }> = {
  home: { label: "主页", icon: <Home20Regular />, activeIcon: <Home20Filled /> },
  schedule: { label: "日程", icon: <CalendarLtr20Regular />, activeIcon: <CalendarLtr20Filled /> },
  bills: { label: "账单", icon: <Receipt20Regular />, activeIcon: <Receipt20Filled /> },
  toolbox: { label: "工具箱", icon: <Toolbox20Regular />, activeIcon: <Toolbox20Filled /> },
  doors: { label: "任意门", icon: <DoorArrowRight20Regular />, activeIcon: <DoorArrowRight20Filled /> },
  settings: { label: "设置", icon: <Settings20Regular />, activeIcon: <Settings20Filled /> },
  about: { label: "关于", icon: <Info20Regular />, activeIcon: <Info20Filled /> },
};

const TOOL_NAV_META: Record<BuiltinToolKey, { label: string; icon: JSX.Element; activeIcon: JSX.Element }> = {
  campus: { label: "校园网自动登录", icon: <Wifi120Regular />, activeIcon: <Wifi120Filled /> },
  share: { label: "免流文件分享", icon: <Share20Regular />, activeIcon: <Share20Filled /> },
  mute: { label: "防社死设置", icon: <SpeakerMute20Regular />, activeIcon: <SpeakerMute20Filled /> },
  fetch: { label: "定时下载", icon: <ArrowDownload20Regular />, activeIcon: <ArrowDownload20Filled /> },
};

const emptySms = (): Sms => ({ phone: "", png: "", code: "", graphic: "", trust: true });

type CheckState = "idle" | "checking" | "current" | "failed";

export function DeskApp() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [page, setPage] = useState<CoreNavId>("home");
  const [toolSub, setToolSub] = useState<ToolSubPage>("hub");
  const [pageAnim, setPageAnim] = useState<{ key: string; subMotion: "none" | "in" | "back" }>({
    key: "home",
    subMotion: "none",
  });
  const [settingsStart, setSettingsStart] = useState<"hub" | "dorm">("hub");
  const [billTab, setBillTab] = useState<"card" | "net">("card");
  const [err, setErr] = useState("");
  const [refreshUi, setRefreshUi] = useState<"idle" | "spin" | "ok">("idle");
  const [courseSync, setCourseSync] = useState<"idle" | "spin" | "ok" | "fail">("idle");
  const [courseSyncErr, setCourseSyncErr] = useState("");
  const [homeSpend, setHomeSpend] = useState<DaySpend[]>([]);
  const [user, setUser] = useState("");
  const [pass, setPass] = useState("");
  const [remember, setRemember] = useState(false);
  const [autoLogin, setAutoLogin] = useState(false);
  const [step, setStep] = useState<"login" | "sms">("login");
  const [sms, setSms] = useState<Sms>(emptySms);
  const [eventOpen, setEventOpen] = useState(false);
  const [editing, setEditing] = useState<Agenda | null>(null);
  const [eventNonce, setEventNonce] = useState(0);
  const [eventErr, setEventErr] = useState("");
  const [eventBusy, setEventBusy] = useState(false);
  const hydrated = useRef(false);
  const pageRef = useRef(page);
  const roomAtSettings = useRef<string | null>(null);
  const refreshOkTimer = useRef(0);
  const updateRef = useRef<Update | null>(null);
  const checkGen = useRef(0);
  const installing = useRef(false);
  const mounted = useRef(true);
  const [updateVersion, setUpdateVersion] = useState<string | null>(null);
  const [updatePhase, setUpdatePhase] = useState<"idle" | "busy" | "fail">("idle");
  const [checkState, setCheckState] = useState<CheckState>("idle");
  const [updateHidden, setUpdateHidden] = useState(false);
  const [shareOn, setShareOn] = useState(false);
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [pluginsLoaded, setPluginsLoaded] = useState(false);
  const platform = usePlatformInfo();
  pageRef.current = page;

  async function runLook(manual: boolean) {
    const gen = ++checkGen.current;
    if (manual) setCheckState("checking");
    const look = await lookForUpdate();
    if (!mounted.current || gen !== checkGen.current) {
      look.update?.close().catch(() => undefined);
      return;
    }
    const prev = updateRef.current;
    updateRef.current = look.update;
    if (prev && prev !== look.update) prev.close().catch(() => undefined);
    if (look.update) {
      setUpdateVersion(look.update.version);
      setUpdatePhase("idle");
      setCheckState("idle");
      return;
    }
    setUpdateVersion(null);
    if (look.failed) {
      if (manual) setCheckState("failed");
      return;
    }
    setUpdatePhase("idle");
    setCheckState("current");
  }

  async function installUpdate() {
    const update = updateRef.current;
    if (!update || installing.current) return;
    installing.current = true;
    setUpdatePhase("busy");
    try {
      await update.downloadAndInstall();
      await relaunch();
    } catch {
      installing.current = false;
      if (mounted.current) setUpdatePhase("fail");
    }
  }

  useEffect(() => {
    mounted.current = true;
    if (import.meta.env.PROD) {
      void getPlatformInfo().then((info) => {
        if (mounted.current && info.autoUpdateSupported) void runLook(false);
      });
    }
    return () => {
      mounted.current = false;
      const pending = updateRef.current;
      updateRef.current = null;
      pending?.close().catch(() => undefined);
    };
  }, []);

  const { now, overridden } = useClock(!!settings?.classRemind || page === "schedule");
  const mode = resolveTheme(settings?.theme);
  const loggedIn = !!snap?.loggedIn;
  const offline = !!snap?.offline;
  const showApp = !!snap && (loggedIn || !!settings?.skipLogin);

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", mode);
  }, [mode]);

  useEffect(() => {
    if (settings?.theme === "light" || settings?.theme === "dark") return;
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    const onChange = () => document.documentElement.setAttribute("data-theme", resolveTheme("system"));
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [settings?.theme]);

  useEffect(() => {
    (async () => {
      const [s, st, creds] = await Promise.all([api.bootstrap(), api.settings(), api.savedCreds()]);
      setSnap(s);
      const st2 = withSettingDefaults(st);
      setSettings(st2);
      if (creds.username) {
        setRemember(creds.rememberPassword);
        setAutoLogin(creds.autoLogin);
      } else {
        setRemember(st2.rememberPassword !== false);
        setAutoLogin(!!st2.autoLogin && st2.rememberPassword !== false);
      }
      if (creds.username) setUser(creds.username);
      else if (s.username) setUser(s.username);
      if (creds.rememberPassword) {
        const saved = await api.savedPassword();
        if (saved) setPass(saved);
      }
    })().catch((e) => setErr(String(e)));
  }, []);

  useEffect(() => {
    if (!settings) return;
    if (!hydrated.current) {
      hydrated.current = true;
      return;
    }
    const delay = settings.rememberPassword === false ? 0 : 400;
    const t = window.setTimeout(() => {
      api.saveSettings(settings).catch((e) => setErr(String(e)));
    }, delay);
    return () => window.clearTimeout(t);
  }, [settings]);

  useEffect(() => {
    const current = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}`;
    if (snap?.cardMonth === current && (snap.cardSpend?.length ?? 0) > 0) {
      setHomeSpend(snap.cardSpend || []);
    }
  }, [snap?.cardMonth, snap?.cardSpend, now]);

  function applySnap(next: Snapshot) {
    setSnap((prev) => {
      if (pageRef.current !== "bills") return next;
      return {
        ...next,
        cardSpend: prev?.cardSpend ?? next.cardSpend,
        cardMonth: prev?.cardMonth ?? next.cardMonth,
        cardBills: prev?.cardBills ?? next.cardBills,
        billsTruncated: prev?.billsTruncated ?? next.billsTruncated,
      };
    });
  }

  useEffect(() => {
    const unsubs: Array<() => void> = [];
    listen<Snapshot>("snapshot-updated", (e) => {
      applySnap(e.payload);
      if (e.payload.loggedIn) {
        setSettings((prev) => (prev?.skipLogin ? { ...prev, skipLogin: false } : prev));
      }
      setRefreshUi((u) => (u === "spin" ? "ok" : u));
    }).then((f) => unsubs.push(f));
    listen<AppSettings>("settings-updated", (e) => {
      const next = withSettingDefaults(e.payload);
      setSettings((prev) => (prev && JSON.stringify(prev) === JSON.stringify(next) ? prev : next));
    }).then((f) => unsubs.push(f));
    listen<string>("auth-status", (e) => {
      const p = e.payload;
      if (p === "syncing") setRefreshUi("spin");
      else if (p === "ok") setRefreshUi("ok");
      else if (p === "idle" || p === "need_sms") setRefreshUi("idle");
      else if (typeof p === "string" && p.startsWith("fail:")) {
        setRefreshUi("idle");
        setErr(p.slice(5));
      }
    }).then((f) => unsubs.push(f));
    api.shareOffer().then((offer) => setShareOn(!!offer)).catch(() => undefined);
    listen<boolean>("share-changed", (e) => {
      setShareOn(!!e.payload);
    }).then((f) => unsubs.push(f));
    api.pluginList()
      .then((list) => {
        setPlugins(list);
        setPluginsLoaded(true);
      })
      .catch(() => setPluginsLoaded(true));
    listen<PluginInfo[]>("plugins-updated", (e) => {
      setPlugins(e.payload || []);
      setPluginsLoaded(true);
    }).then((f) => unsubs.push(f));
    listen<LoginOutcome>("need-sms", (e) => {
      const out = e.payload;
      if (out.status !== "need_sms") return;
      setSms({
        phone: out.phoneMask,
        png: out.captchaPng,
        graphic: "",
        code: "",
        trust: true,
      });
      setStep("sms");
      setErr("");
    }).then((f) => unsubs.push(f));
    return () => unsubs.forEach((f) => f());
  }, []);

  useEffect(() => {
    if (refreshUi !== "ok") return;
    window.clearTimeout(refreshOkTimer.current);
    refreshOkTimer.current = window.setTimeout(() => setRefreshUi("idle"), 1400);
    return () => window.clearTimeout(refreshOkTimer.current);
  }, [refreshUi]);

  const uiPlugins = useMemo(() => plugins.filter((p) => p.enabled && p.hasUi), [plugins]);
  const availablePluginIds = useMemo(() => uiPlugins.map((p) => p.manifest.id), [uiPlugins]);
  const pinnedNavIds = useMemo(() => normalizeNavPins(settings?.navPins), [settings?.navPins]);
  const visibleNavIds = useMemo(
    () => resolveVisibleNavIds(settings?.navOrder, settings?.navPins, availablePluginIds),
    [settings?.navOrder, settings?.navPins, availablePluginIds],
  );

  const navItems = useMemo<DeskNavItem[]>(() => {
    const pluginMap = new Map(uiPlugins.map((p) => [p.manifest.id, p]));
    const items: DeskNavItem[] = [];
    for (const id of visibleNavIds) {
      if (isCoreNavId(id)) {
        const meta = CORE_NAV_META[id];
        items.push({ id, label: meta.label, icon: meta.icon, activeIcon: meta.activeIcon });
        continue;
      }
      const pluginId = pluginIdFromToolNavId(id);
      if (pluginId) {
        const plugin = pluginMap.get(pluginId);
        if (!plugin) continue;
        items.push({
          id,
          label: plugin.manifest.name,
          icon: <PlugConnected20Regular />,
          activeIcon: <PlugConnected20Filled />,
          pinned: true,
          badge: plugin.statusBadge || undefined,
        });
        continue;
      }
      const sub = subPageFromToolNavId(id);
      if (sub && sub in TOOL_NAV_META) {
        const meta = TOOL_NAV_META[sub as BuiltinToolKey];
        items.push({
          id,
          label: meta.label,
          icon: meta.icon,
          activeIcon: meta.activeIcon,
          pinned: true,
        });
      }
    }
    return items;
  }, [visibleNavIds, uiPlugins]);

  const activePluginId =
    page === "toolbox" && toolSub.startsWith("plugin:")
      ? toolSub.slice("plugin:".length)
      : null;

  useEffect(() => {
    if (!pluginsLoaded || !activePluginId) return;
    if (!uiPlugins.some((p) => p.manifest.id === activePluginId)) {
      setToolSub("hub");
      setPageAnim((prev) => ({ ...prev, subMotion: "back" }));
    }
  }, [pluginsLoaded, activePluginId, uiPlugins]);

  const selectedNavId: NavId = useMemo(() => {
    if (page === "toolbox") {
      const candidate = toolNavIdFromSubPage(toolSub);
      if (candidate && visibleNavIds.includes(candidate)) {
        return candidate;
      }
      return "toolbox";
    }
    return page;
  }, [page, toolSub, visibleNavIds]);

  function updateSettings(next: AppSettings) {
    const normalized = withSettingDefaults(next);
    if (normalized.rememberPassword === false) setPass("");
    setRemember(normalized.rememberPassword !== false);
    setAutoLogin(!!normalized.autoLogin && normalized.rememberPassword !== false);
    setSettings(normalized);
  }

  function handleSettingsTransition(nextCore: CoreNavId) {
    if (
      page === "settings" &&
      nextCore !== "settings" &&
      settings &&
      roomAtSettings.current !== null &&
      settings.roomNo !== roomAtSettings.current
    ) {
      doRefresh();
    }
    if (nextCore === "settings" && settings) roomAtSettings.current = settings.roomNo;
  }

  function selectPage(next: CoreNavId) {
    handleSettingsTransition(next);
    if (next === "toolbox") {
      if (page === "toolbox") {
        if (toolSub === "hub") return;
        setToolSub("hub");
        setPageAnim((prev) => ({ ...prev, subMotion: "back" }));
        return;
      }
      setPage("toolbox");
      setToolSub("hub");
      setPageAnim({ key: "toolbox:hub", subMotion: "none" });
      return;
    }
    if (page === next) return;
    setPage(next);
    setPageAnim({ key: next, subMotion: "none" });
  }

  function selectNav(id: NavId) {
    if (id === "settings") setSettingsStart("hub");
    if (isCoreNavId(id)) {
      selectPage(id);
      return;
    }
    const sub = subPageFromToolNavId(id);
    if (!sub) return;
    if (page === "toolbox" && toolSub === sub) return;
    handleSettingsTransition("toolbox");
    setPage("toolbox");
    setToolSub(sub);
    setPageAnim({ key: `toolbox:${sub}`, subMotion: "none" });
  }

  function openToolSubPage(next: ToolSubPage) {
    if (page === "toolbox" && toolSub === next) return;
    const dir = next === "hub" && toolSub !== "hub" ? "back" : "in";
    handleSettingsTransition("toolbox");
    setPage("toolbox");
    setToolSub(next);
    setPageAnim((prev) => ({ ...prev, subMotion: dir }));
  }

  function openShareFromPill() {
    if (page === "toolbox" && toolSub === "share") return;
    handleSettingsTransition("toolbox");
    setPage("toolbox");
    setToolSub("share");
    setPageAnim({ key: "toolbox:share", subMotion: "none" });
  }

  function toggleNavCollapse() {
    if (!settings) return;
    updateSettings({ ...settings, navCollapsed: !settings.navCollapsed });
  }

  function reorderNav(fromIndex: number, toIndex: number) {
    if (!settings) return;
    const nextOrder = reorderVisibleNav(
      settings.navOrder,
      settings.navPins,
      visibleNavIds,
      fromIndex,
      toIndex,
    );
    updateSettings({ ...settings, navOrder: nextOrder });
  }

  function togglePinTool(toolId: ToolNavId) {
    if (!settings) return;
    const isPinned = pinnedNavIds.includes(toolId);
    const next = isPinned
      ? unpinToolNav(settings.navOrder, settings.navPins, toolId)
      : pinToolNav(settings.navOrder, settings.navPins, toolId);
    updateSettings({ ...settings, navOrder: next.navOrder, navPins: next.navPins });
  }

  function unpinTool(toolId: ToolNavId) {
    if (!settings) return;
    const next = unpinToolNav(settings.navOrder, settings.navPins, toolId);
    updateSettings({ ...settings, navOrder: next.navOrder, navPins: next.navPins });
  }

  function requestLogin() {
    if (!settings) return;
    updateSettings({ ...settings, skipLogin: false });
    setPage("home");
    setToolSub("hub");
    setPageAnim({ key: "home", subMotion: "none" });
  }

  async function doRefresh() {
    if (refreshUi === "spin") return;
    if (!snap?.loggedIn) {
      setErr("校园卡、网费、电费和课程需要登录。日程仍在这台电脑上。");
      setRefreshUi("idle");
      return;
    }
    setRefreshUi("spin");
    setErr("");
    try {
      const s = await api.refresh();
      applySnap(s);
      if (s.offline) setRefreshUi("idle");
      else setRefreshUi("ok");
    } catch (e) {
      setErr(String(e));
      setRefreshUi("idle");
    }
  }

  async function doSyncCourses() {
    if (courseSync === "spin") return;
    setCourseSync("spin");
    setCourseSyncErr("");
    setErr("");
    try {
      const s = await api.syncCourses();
      applySnap(s);
      setCourseSync("ok");
      window.setTimeout(() => setCourseSync((u) => (u === "ok" ? "idle" : u)), 1600);
    } catch (e) {
      const msg = String(e);
      setCourseSync("fail");
      setCourseSyncErr(msg);
      setErr(msg);
    }
  }

  function openEvent(item: Agenda | null) {
    setEditing(item);
    setEventErr("");
    setEventNonce((n) => n + 1);
    setEventOpen(true);
  }

  async function saveEvent(item: Agenda) {
    if (eventBusy) return;
    setEventBusy(true);
    setEventErr("");
    try {
      const s = await api.saveAgenda(item);
      applySnap(s);
      setEventBusy(false);
      setEventOpen(false);
    } catch (e) {
      setEventErr(String(e));
      setEventBusy(false);
    }
  }

  async function deleteEvent(id: string) {
    if (eventBusy) return;
    setEventBusy(true);
    setEventErr("");
    try {
      const s = await api.deleteAgenda(id);
      applySnap(s);
      setEventBusy(false);
      setEventOpen(false);
    } catch (e) {
      setEventErr(String(e));
      setEventBusy(false);
    }
  }

  const theme = mode === "light" ? nidongLightTheme : nidongDarkTheme;
  const cacheLabel = cacheDayLabel(snap?.semester?.fetchedAt);

  return (
    <FluentProvider theme={theme} style={{ height: "100%" }}>
      <div className="desk-root">
        {!snap ? (
          <div className="desk-main desk-pending" role="status">
            <i className="status-spin" aria-hidden="true" />
            <span>正在打开…</span>
          </div>
        ) : showApp ? (
          <div className="desk-shell">
            <DeskNav
              items={navItems}
              selectedId={selectedNavId}
              collapsed={!!settings?.navCollapsed}
              shareOn={shareOn}
              onToggleCollapse={toggleNavCollapse}
              onSelect={selectNav}
              onReorder={reorderNav}
              onUnpin={unpinTool}
              onOpenShare={openShareFromPill}
            />
            <main className="desk-main">
              {updateVersion && !updateHidden ? (
                <div className="update-bar" role="status">
                  <span>有新版本 {updateVersion}</span>
                  <div className="update-bar-actions">
                    {updatePhase === "fail" ? <span className="hint warn">更新没有完成，稍后再试。</span> : null}
                    <Button appearance="subtle" disabled={updatePhase === "busy"} onClick={() => { void installUpdate(); }}>
                      {updatePhase === "busy" ? "正在下载…" : "更新并重启"}
                    </Button>
                    <Button appearance="subtle" disabled={updatePhase === "busy"} onClick={() => setUpdateHidden(true)}>稍后</Button>
                  </div>
                </div>
              ) : null}
              <div key={pageAnim.key} className="page-enter">
                {page === "home" && (
                  <Home
                    snap={snap}
                    settings={settings}
                    now={now}
                    clockOverridden={overridden}
                    refreshUi={refreshUi}
                    err={err}
                    homeSpend={homeSpend}
                    courseSync={courseSync}
                    onRefresh={doRefresh}
                    onOpenBills={(tab) => {
                      setBillTab(tab);
                      selectPage("bills");
                    }}
                    onOpenDorm={() => {
                      setSettingsStart("dorm");
                      selectPage("settings");
                    }}
                    onOpenSchedule={() => selectPage("schedule")}
                    onOpenEvent={openEvent}
                    onSyncCourses={doSyncCourses}
                    onRequestLogin={requestLogin}
                  />
                )}
                {page === "schedule" && (
                  <Schedule
                    snap={snap}
                    settings={settings}
                    now={now}
                    onOpenEvent={openEvent}
                    onSyncCourses={doSyncCourses}
                    courseSync={courseSync}
                  />
                )}
                {page === "toolbox" && settings && (
                  <Toolbox
                    settings={settings}
                    setSettings={updateSettings}
                    subPage={toolSub}
                    subMotion={pageAnim.subMotion}
                    onOpenSubPage={openToolSubPage}
                    pinnedNavIds={pinnedNavIds}
                    onTogglePin={togglePinTool}
                    plugins={plugins}
                    theme={mode}
                  />
                )}
                {page === "doors" && <Doors />}
                {page === "bills" && (
                  <Bills
                    snap={snap}
                    now={now}
                    tab={billTab}
                    onTab={setBillTab}
                    onSnap={(s) => setSnap(s)}
                    onRequestLogin={requestLogin}
                  />
                )}
                {page === "settings" && settings && (
                  <SettingsPage
                    key={settingsStart}
                    start={settingsStart}
                    settings={settings}
                    setSettings={updateSettings}
                    semester={snap.semester}
                    cacheLabel={cacheLabel}
                    courseSync={courseSync}
                    courseSyncErr={courseSyncErr}
                    offline={offline}
                    onSync={doSyncCourses}
                    onPreviewRemind={() => { api.previewRemind().catch(() => undefined); }}
                    loggedIn={loggedIn}
                    onRequestLogin={requestLogin}
                    onLogout={async (keepAgenda) => {
                      const kept = keepAgenda ? snap?.agenda ?? [] : [];
                      try {
                        await api.logout(keepAgenda);
                      } catch {
                        return;
                      }
                      if (!remember) setPass("");
                      setSnap({ fetchedAt: "", network: "unknown", loggedIn: false, offline: false, agenda: kept, errors: {} });
                      setStep("login");
                      setPage("home");
                      setToolSub("hub");
                      setPageAnim({ key: "home", subMotion: "none" });
                    }}
                  />
                )}
                {page === "about" && (
                  <About
                    updateVersion={updateVersion}
                    updateBusy={updatePhase === "busy"}
                    updateFailed={updatePhase === "fail"}
                    checkState={checkState}
                    autoUpdateSupported={platform.autoUpdateSupported}
                    isLinux={platform.os === "linux"}
                    onCheck={() => { void runLook(true); }}
                    onInstall={() => { void installUpdate(); }}
                  />
                )}
              </div>
            </main>
          </div>
        ) : (
          <div className="desk-main">
            <div key={step} className="page-enter">
              <Login
                settings={settings}
                setSettings={updateSettings}
                user={user}
                setUser={setUser}
                pass={pass}
                setPass={setPass}
                remember={remember}
                setRemember={setRemember}
                autoLogin={autoLogin}
                setAutoLogin={setAutoLogin}
                step={step}
                setStep={setStep}
                sms={sms}
                setSms={setSms}
                onSkip={() => {
                  if (!settings) return;
                  updateSettings({ ...settings, skipLogin: true });
                  setPage("home");
                  setToolSub("hub");
                  setPageAnim({ key: "home", subMotion: "none" });
                }}
                onLoggedIn={(username) => {
                  setSettings((s) => (s ? { ...s, skipLogin: false } : s));
                  setSnap((s) =>
                    s
                      ? { ...s, loggedIn: true, offline: false, username: username || s.username, errors: { ...s.errors, auth: undefined } }
                      : { fetchedAt: "", network: "unknown", loggedIn: true, username, offline: false, errors: {} }
                  );
                  setStep("login");
                  setPage("home");
                  setToolSub("hub");
                  setPageAnim({ key: "home", subMotion: "none" });
                  setRefreshUi("spin");
                }}
              />
            </div>
          </div>
        )}
        <EventForm
          key={eventNonce}
          open={eventOpen}
          initial={editing}
          now={now}
          busy={eventBusy}
          err={eventErr}
          onClose={() => setEventOpen(false)}
          onSave={saveEvent}
          onDelete={editing?.id ? deleteEvent : undefined}
        />
        <PluginBridge
          plugins={plugins}
          theme={mode}
          activePluginId={activePluginId}
        />
      </div>
    </FluentProvider>
  );
}
