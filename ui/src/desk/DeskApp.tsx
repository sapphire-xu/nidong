import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Button,
  FluentProvider,
  NavDrawer,
  NavDrawerBody,
  NavItem,
} from "@fluentui/react-components";
import { relaunch } from "@tauri-apps/plugin-process";
import type { Update } from "@tauri-apps/plugin-updater";
import {
  CalendarLtr20Regular,
  DoorArrowRight20Regular,
  Home20Regular,
  Info20Regular,
  Receipt20Regular,
  Settings20Regular,
} from "@fluentui/react-icons";
import { api } from "../api";
import { EventForm } from "../EventForm";
import { cacheDayLabel } from "../course";
import { resolveTheme, withSettingDefaults } from "../prefs";
import type { Agenda, AppSettings, DaySpend, LoginOutcome, Snapshot } from "../types";
import { useClock } from "../useClock";
import { lookForUpdate } from "./updateCheck";
import { nidongDarkTheme, nidongLightTheme } from "./theme";
import { About } from "./pages/About";
import { Bills } from "./pages/Bills";
import { Doors } from "./pages/Doors";
import { Home } from "./pages/Home";
import { Login, type Sms } from "./pages/Login";
import { Schedule } from "./pages/Schedule";
import { SettingsPage } from "./pages/Settings";

type Page = "home" | "schedule" | "bills" | "doors" | "settings" | "about";

const NAV: { value: Page; label: string; icon: JSX.Element }[] = [
  { value: "home", label: "主页", icon: <Home20Regular /> },
  { value: "schedule", label: "日程", icon: <CalendarLtr20Regular /> },
  { value: "bills", label: "账单", icon: <Receipt20Regular /> },
  { value: "doors", label: "任意门", icon: <DoorArrowRight20Regular /> },
  { value: "settings", label: "设置", icon: <Settings20Regular /> },
  { value: "about", label: "关于", icon: <Info20Regular /> },
];

const emptySms = (): Sms => ({ phone: "", png: "", code: "", graphic: "", trust: true });

type CheckState = "idle" | "checking" | "current" | "failed";

export function DeskApp() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [page, setPage] = useState<Page>("home");
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
    if (import.meta.env.PROD) void runLook(false);
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
  const showApp = !!snap && loggedIn;

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

  function updateSettings(next: AppSettings) {
    const normalized = withSettingDefaults(next);
    if (normalized.rememberPassword === false) setPass("");
    setRemember(normalized.rememberPassword !== false);
    setAutoLogin(!!normalized.autoLogin && normalized.rememberPassword !== false);
    setSettings(normalized);
  }

  function selectPage(next: Page) {
    if (page === "settings" && next !== "settings" && settings && roomAtSettings.current !== null && settings.roomNo !== roomAtSettings.current) {
      doRefresh();
    }
    if (next === "settings" && settings) roomAtSettings.current = settings.roomNo;
    setPage(next);
  }

  async function doRefresh() {
    if (refreshUi === "spin") return;
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
            <NavDrawer
              open
              type="inline"
              selectedValue={page}
              onNavItemSelect={(_, data) => {
                const value = String(data.value || "");
                if (value === "home" || value === "schedule" || value === "bills" || value === "doors" || value === "settings" || value === "about") {
                  selectPage(value);
                }
              }}
            >
              <NavDrawerBody>
                {NAV.map((item) => (
                  <NavItem key={item.value} value={item.value} icon={item.icon}>
                    {item.label}
                  </NavItem>
                ))}
              </NavDrawerBody>
            </NavDrawer>
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
              <div key={page} className="page-enter">
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
                    onOpenSchedule={() => selectPage("schedule")}
                    onOpenEvent={openEvent}
                    onSyncCourses={doSyncCourses}
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
                {page === "doors" && <Doors />}
                {page === "bills" && (
                  <Bills
                    snap={snap}
                    now={now}
                    tab={billTab}
                    onTab={setBillTab}
                    onSnap={(s) => setSnap(s)}
                  />
                )}
                {page === "settings" && settings && (
                  <SettingsPage
                    settings={settings}
                    setSettings={updateSettings}
                    semester={snap.semester}
                    cacheLabel={cacheLabel}
                    courseSync={courseSync}
                    courseSyncErr={courseSyncErr}
                    offline={offline}
                    onSync={doSyncCourses}
                    onPreviewRemind={() => { api.previewRemind().catch(() => undefined); }}
                    onLogout={async () => {
                      try { await api.logout(); } catch { /* ignore */ }
                      if (!remember) setPass("");
                      setSnap({ fetchedAt: "", network: "unknown", loggedIn: false, offline: false, agenda: [], errors: {} });
                      setStep("login");
                      setPage("home");
                    }}
                  />
                )}
                {page === "about" && (
                  <About
                    updateVersion={updateVersion}
                    updateBusy={updatePhase === "busy"}
                    updateFailed={updatePhase === "fail"}
                    checkState={checkState}
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
                onLoggedIn={(username) => {
                  setSnap((s) =>
                    s
                      ? { ...s, loggedIn: true, offline: false, username: username || s.username, errors: { ...s.errors, auth: undefined } }
                      : { fetchedAt: "", network: "unknown", loggedIn: true, username, offline: false, errors: {} }
                  );
                  setStep("login");
                  setPage("home");
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
      </div>
    </FluentProvider>
  );
}
