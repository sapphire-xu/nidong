import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, focusIsland, PANEL_MAX, setExpanded } from "./api";
import { SettingsPanel } from "./Settings";
import { EventForm } from "./EventForm";
import type { Agenda, AppSettings, Course, DaySpend, LoginOutcome, Snapshot } from "./types";
import {
  BLOCKS,
  WEEKDAY_SHORT,
  addDays,
  allCoursesEnded,
  blockTime,
  cacheDayLabel,
  collapsedFromCourses,
  coursesOnDay,
  datesOfTeachingWeek,
  defaultWeek1,
  inWeek,
  majorityCampus,
  maxWeek,
  overlapsBlock,
  teachingWeek,
  teachingWeekFromTerm,
  upcomingCourse,
  weekdayOf,
} from "./course";
import { agendaOnDate, agendasOnDate, mixDay, upcomingAgenda, weekdayLabel, type MixRow } from "./agenda";

const sleep = (ms: number) => new Promise<void>((r) => window.setTimeout(r, ms));

function stopHoverWatch() {
  return api.setHoverWatch({ enabled: false, x: 0, y: 0, width: 0, height: 0 }).catch(() => undefined);
}

function withSettingDefaults(s: AppSettings): AppSettings {
  return {
    ...s,
    autoHide: s.autoHide !== false,
    autoHideHotkey: s.autoHideHotkey !== false,
    autoHideHover: s.autoHideHover !== false,
    revealHotkey: s.revealHotkey === "Tab+Q" || !s.revealHotkey ? "Ctrl+Q" : s.revealHotkey,
    classRemind: s.classRemind !== false,
    classRemindMinutes: Math.max(1, Math.floor(Number(s.classRemindMinutes) || 15)),
  };
}

function resolveTheme(theme: string | undefined) {
  if (theme === "light" || theme === "dark") return theme;
  if (typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: light)").matches) {
    return "light";
  }
  return "dark";
}

type View = "main" | "bills" | "net" | "about" | "login" | "sms" | "settings" | "semester" | "event";

const WEEK = ["日", "一", "二", "三", "四", "五", "六"];
const fmt = (n: number) => n.toFixed(2);
const fmtElec = (n: number) => String(Math.round(n));
const pad = (n: number) => String(n).padStart(2, "0");

function dragIgnore(t: EventTarget | null) {
  return !!(t as HTMLElement | null)?.closest?.(
    "button, input, textarea, a, label, .sw, .card.clickable, .col, .bills, .cell, .course, .text-btn, .tt, .brand, .row.event"
  );
}

function placeLabel(p?: string) {
  const t = (p || "").trim();
  if (!t || t === "浑南校区" || t === "南湖校区" || t === "浑南" || t === "南湖") {
    return "地点未定";
  }
  return t;
}

function courseMeta(c: Course) {
  return [c.teacher, placeLabel(c.place)].filter((s) => s && s.trim()).join(" · ");
}

function courseTip(c: Course) {
  return [`${c.start}–${c.end}`, c.teacher, placeLabel(c.place)].filter((s) => s && String(s).trim()).join(" · ");
}

function MixRowView({ row, onEvent }: { row: MixRow; onEvent?: () => void }) {
  const event = row.type === "event";
  return (
    <div
      className={`row ${row.kind === "later" ? "" : row.kind}${event ? " event" : ""}`}
      onClick={event ? onEvent : undefined}
    >
      <div className="when">
        <span className="num">{row.start}–{row.end}</span>
        <span>{row.label}</span>
      </div>
      <div className="cname">
        <span className="cname-main">
          {event ? <i className="kind-dot" aria-hidden="true" /> : null}
          {row.name}
        </span>
        <span className="place">{row.course ? courseMeta(row.course) : row.meta || "日程"}</span>
      </div>
    </div>
  );
}

function formatBytes(n: number) {
  if (n >= 1024 ** 3) return (n / 1024 ** 3).toFixed(1) + " GB";
  if (n >= 1024 ** 2) return Math.round(n / 1024 ** 2) + " MB";
  if (n >= 1024) return Math.round(n / 1024) + " KB";
  return n + " B";
}

function ymd(d: Date) {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

function isJunkBill(b: { subject: string; amount: number; station?: string }) {
  const sub = (b.subject || "").trim();
  const st = b.station || "";
  if (st.includes("物联网采集") && Math.abs(b.amount) < 0.005) return true;
  if (/^\d{4}\s*[-/年.]\s*\d{1,2}\s*[-/月.]\s*\d{1,2}/.test(sub)) return true;
  if (Math.abs(b.amount) < 0.005 && /\d{1,2}:\d{2}:\d{2}/.test(sub)) return true;
  return false;
}

export default function App() {
  const [expanded, setExp] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [view, setView] = useState<View>("main");
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [user, setUser] = useState("");
  const [pass, setPass] = useState("");
  const [remember, setRemember] = useState(false);
  const [autoLogin, setAutoLogin] = useState(false);
  const [silentLogin, setSilentLogin] = useState(false);
  const draggingRef = useRef(false);
  const dragOrigin = useRef<{ x: number; y: number } | null>(null);
  const skipClick = useRef(false);
  const justDragged = useRef(false);
  const hoveredRef = useRef(false);
  const pinnedRef = useRef(false);
  const pinBeforeOverlay = useRef<boolean | null>(null);
  const expandGen = useRef(0);
  const loginInFlight = useRef(false);
  const smsInFlight = useRef(false);
  const settingsHydrated = useRef(false);
  const roomAtSettingsOpen = useRef("");
  const [sms, setSms] = useState({ phone: "", png: "", ocr: "", code: "", graphic: "", trust: true });
  const [smsWait, setSmsWait] = useState(0);
  const [smsSending, setSmsSending] = useState(false);
  const [captchaBusy, setCaptchaBusy] = useState(false);
  const [smsHint, setSmsHint] = useState("");
  const [refreshUi, setRefreshUi] = useState<"idle" | "spin" | "ok">("idle");
  const [courseSync, setCourseSync] = useState<"idle" | "spin" | "ok" | "fail">("idle");
  const [courseSyncErr, setCourseSyncErr] = useState("");
  const [editingEvent, setEditingEvent] = useState<Agenda | null>(null);
  const [eventNonce, setEventNonce] = useState(0);
  const [eventErr, setEventErr] = useState("");
  const [eventBusy, setEventBusy] = useState(false);
  const eventBackRef = useRef<View>("main");
  const [clockOverridden, setClockOverridden] = useState(false);
  const refreshOkTimer = useRef(0);
  const [hover, setHover] = useState<string | null>(null);
  const [selected, setSelected] = useState(ymd(new Date()));
  const [ym, setYm] = useState(() => {
    const d = new Date();
    return { y: d.getFullYear(), m: d.getMonth() + 1 };
  });
  const [now, setNow] = useState(() => new Date());
  const [semWeek, setSemWeek] = useState(1);
  const [cellTip, setCellTip] = useState<{ text: string; x: number; y: number } | null>(null);
  const islandRef = useRef<HTMLDivElement>(null);
  const [hidden, setHidden] = useState(false);
  const [stowing, setStowing] = useState(false);
  const [remind, setRemind] = useState<{ name: string; start: string; kind?: "class" | "event" } | null>(null);
  const hiddenRef = useRef(false);
  const remindRef = useRef<{ name: string; start: string; kind?: "class" | "event" } | null>(null);
  const expandedRef = useRef(false);
  const autoHideRef = useRef(false);
  const autoHideHoverRef = useRef(false);
  const hideGen = useRef(0);
  const hideTimer = useRef(0);
  const remindTimer = useRef(0);
  const hoverExpandTimer = useRef(0);
  const tookOverRef = useRef(false);
  const preRemindRef = useRef<{ expanded: boolean; hidden: boolean; pinned: boolean } | null>(null);
  const remindedId = useRef("");
  const startRemindRef = useRef<(c: Course & { kind?: "class" | "event" }) => void>(() => undefined);
  const revealAndPinRef = useRef<(pin?: boolean) => void>(() => undefined);
  const hotkeyHoldingRef = useRef(false);
  const openOverlayRef = useRef<(next: View) => void>(() => undefined);
  const viewRef = useRef(view);
  const gameFsRef = useRef(false);

  const theme = resolveTheme(settings?.theme);
  const loggedIn = !!snap?.loggedIn;
  const hasCachedData = !!(snap?.card || snap?.net || snap?.elec || snap?.semester);
  const authMsg = snap?.errors?.auth;
  pinnedRef.current = pinned;
  hiddenRef.current = hidden;
  remindRef.current = remind;
  expandedRef.current = expanded;
  viewRef.current = view;
  autoHideRef.current = !!settings?.autoHide;
  autoHideHoverRef.current = !!settings?.autoHideHover;

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
  }, [theme]);

  useEffect(() => {
    let t = 0;
    let cancelled = false;
    const tick = async () => {
      try {
        const c = await api.clockNow();
        if (cancelled) return;
        const d = new Date(c.iso);
        if (!Number.isNaN(d.getTime())) setNow(d);
        setClockOverridden(!!c.overridden);
        const fast = !!c.overridden || !!settings?.classRemind;
        t = window.setTimeout(tick, fast ? 1000 : 5000);
      } catch {
        if (cancelled) return;
        setNow(new Date());
        setClockOverridden(false);
        t = window.setTimeout(tick, settings?.classRemind ? 1000 : 5000);
      }
    };
    tick();
    return () => {
      cancelled = true;
      window.clearTimeout(t);
    };
  }, [settings?.classRemind]);

  const clockDay = ymd(now);
  useEffect(() => {
    if (!clockOverridden) return;
    setYm({ y: now.getFullYear(), m: now.getMonth() + 1 });
    setSelected(clockDay);
  }, [clockOverridden, clockDay]);

  useEffect(() => {
    if (smsWait <= 0) return;
    const t = window.setTimeout(() => setSmsWait((n) => Math.max(0, n - 1)), 1000);
    return () => window.clearTimeout(t);
  }, [smsWait]);

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
      if (s.cardMonth) {
        const [y, m] = s.cardMonth.split("-").map(Number);
        setYm({ y, m });
      }
    })().catch((e) => setErr(String(e)));
  }, []);

  useEffect(() => {
    if (!settings) return;
    if (!settingsHydrated.current) {
      settingsHydrated.current = true;
      return;
    }
    const delay = settings.rememberPassword === false ? 0 : 400;
    const t = window.setTimeout(() => {
      api.saveSettings(settings).catch((e) => setErr(String(e)));
    }, delay);
    return () => window.clearTimeout(t);
  }, [settings]);

  const [panelIn, setPanelIn] = useState(false);
  const [islandH, setIslandH] = useState(40);

  useEffect(() => {
    if (!expanded) {
      setIslandH(40);
      return;
    }
    const panel = islandRef.current?.querySelector(".panel") as HTMLElement | null;
    if (!panel) return;
    const measure = () => {
      const h = Math.min(PANEL_MAX, Math.max(160, Math.ceil(panel.scrollHeight)));
      setIslandH(h);
    };
    const id = requestAnimationFrame(measure);
    const ro = new ResizeObserver(measure);
    ro.observe(panel);
    return () => {
      cancelAnimationFrame(id);
      ro.disconnect();
    };
  }, [expanded, view, panelIn, snap, selected, semWeek, refreshUi, courseSync, editingEvent]);

  const winH = useRef(40);
  useEffect(() => {
    if (hidden || stowing || remind) return;
    if (!expanded) {
      const t = window.setTimeout(() => {
        winH.current = 40;
        setExpanded(false);
      }, 260);
      return () => window.clearTimeout(t);
    }
    if (islandH >= winH.current - 1) {
      winH.current = islandH;
      setExpanded(true, islandH);
      return;
    }
    const t = window.setTimeout(() => {
      winH.current = islandH;
      setExpanded(true, islandH);
    }, 260);
    return () => window.clearTimeout(t);
  }, [expanded, islandH, hidden, stowing, remind]);

  useEffect(() => {
    if (view !== "bills") return;
    const want = `${ym.y}-${String(ym.m).padStart(2, "0")}`;
    if (snap?.cardMonth === want && (snap.cardSpend?.length ?? 0) > 0) return;
    let cancel = false;
    api.loadMonth(ym.y, ym.m).then((s) => { if (!cancel) setSnap(s); }).catch(() => {});
    return () => { cancel = true; };
  }, [view, ym.y, ym.m, snap?.cardMonth, snap?.cardSpend?.length]);

  useEffect(() => {
    const open = view === "bills";
    api.setBillsOpen(open).catch(() => undefined);
    return () => {
      if (open) api.setBillsOpen(false).catch(() => undefined);
    };
  }, [view]);

  const expand = useCallback(async (on: boolean) => {
    const gen = ++expandGen.current;
    if (on) {
      setPanelIn(true);
      await setExpanded(true);
      if (expandGen.current !== gen) return;
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          if (expandGen.current === gen) setExp(true);
        });
      });
    } else {
      setExp(false);
      window.setTimeout(() => {
        if (expandGen.current !== gen) return;
        setPanelIn(false);
        setExpanded(false);
      }, 260);
    }
  }, []);

  const hideIsland = useCallback(async () => {
    if (!autoHideRef.current || remindRef.current || pinnedRef.current) return;
    if (expandedRef.current || hiddenRef.current) return;
    const gen = ++hideGen.current;
    setStowing(true);
    await sleep(320);
    if (hideGen.current !== gen || remindRef.current || pinnedRef.current || expandedRef.current) {
      setStowing(false);
      return;
    }
    setHidden(true);
    const w = getCurrentWindow();
    try {
      if (autoHideHoverRef.current) {
        const pos = await w.outerPosition();
        const scale = await w.scaleFactor();
        const pad = Math.round(80 * scale);
        await api.setHoverWatch({
          enabled: true,
          x: pos.x - pad,
          y: Math.max(0, pos.y - Math.round(12 * scale)),
          width: Math.round(420 * scale) + pad * 2,
          height: Math.round(80 * scale),
        });
      } else {
        await stopHoverWatch();
      }
      await w.hide();
    } catch {
      /* ignore */
    }
  }, []);

  const revealAndPin = useCallback(async (pin = true) => {
    hideGen.current += 1;
    window.clearTimeout(hideTimer.current);
    window.clearTimeout(hoverExpandTimer.current);
    window.clearTimeout(remindTimer.current);
    tookOverRef.current = true;
    setRemind(null);
    setStowing(false);
    setHidden(false);
    if (pin) {
      setPinned(true);
      pinBeforeOverlay.current = true;
    }
    await stopHoverWatch();
    try {
      await api.raiseTopmost();
    } catch {
      try { await getCurrentWindow().show(); } catch { /* ignore */ }
    }
    if (!expandedRef.current) expand(true);
    if (pin) focusIsland();
  }, [expand]);

  const startRemind = useCallback(async (course: Course & { kind?: "class" | "event" }) => {
    hideGen.current += 1;
    window.clearTimeout(hideTimer.current);
    window.clearTimeout(hoverExpandTimer.current);
    preRemindRef.current = {
      expanded: expandedRef.current,
      hidden: hiddenRef.current,
      pinned: pinnedRef.current,
    };
    tookOverRef.current = false;
    setStowing(false);
    setHidden(false);
    await stopHoverWatch();
    try {
      await api.raiseTopmost();
    } catch {
      try { await getCurrentWindow().show(); } catch { /* ignore */ }
    }
    setRemind({ name: course.name, start: course.start, kind: course.kind || "class" });
    setExp(false);
    setIslandH(40);
    try { await setExpanded(false); } catch { /* ignore */ }
    window.clearTimeout(remindTimer.current);
    remindTimer.current = window.setTimeout(() => {
      const pre = preRemindRef.current;
      setRemind(null);
      if (tookOverRef.current) return;
      if (pre?.hidden && autoHideRef.current) {
        hideIsland();
      } else if (pre?.expanded) {
        expand(true);
      }
    }, 10000);
  }, [expand, hideIsland]);

  revealAndPinRef.current = revealAndPin;
  startRemindRef.current = startRemind;

  const openOverlay = useCallback((next: View) => {
    const pinOverlay = next !== "main" && next !== "settings";
    if (pinOverlay && pinBeforeOverlay.current === null) {
      pinBeforeOverlay.current = pinned;
    }
    if (pinOverlay) setPinned(true);
    if (next === "settings" && settings) roomAtSettingsOpen.current = settings.roomNo;
    hideGen.current += 1;
    setStowing(false);
    setHidden(false);
    api.raiseTopmost().catch(() => undefined);
    setView(next);
    if (!expanded) expand(true);
  }, [pinned, settings, expanded, expand]);
  openOverlayRef.current = openOverlay;

  const closeOverlay = useCallback(async () => {
    if (viewRef.current === "event") {
      const back = eventBackRef.current || "main";
      setEditingEvent(null);
      setEventErr("");
      setView(back);
      if (back !== "main") return;
    }
    const restore = pinBeforeOverlay.current;
    pinBeforeOverlay.current = null;
    const roomChanged = settings && settings.roomNo !== roomAtSettingsOpen.current;
    if (settings) {
      try { await api.saveSettings(settings); } catch { /* ignore */ }
    }
    setView("main");
    setYm({ y: now.getFullYear(), m: now.getMonth() + 1 });
    if (restore !== null) setPinned(restore);
    if (restore === false && !hoveredRef.current) expand(false);
    if (roomChanged) doRefresh();
  }, [settings, expand, now]);

  const togglePin = useCallback(() => {
    if (pinned) {
      pinBeforeOverlay.current = false;
      setPinned(false);
    } else {
      pinBeforeOverlay.current = true;
      setPinned(true);
    }
  }, [pinned]);

  useEffect(() => {
    const unsubs: Array<() => void> = [];
    listen<Snapshot>("snapshot-updated", (e) => {
      setSnap((prev) => {
        if (viewRef.current !== "bills") return e.payload;
        return {
          ...e.payload,
          cardSpend: prev?.cardSpend ?? e.payload.cardSpend,
          cardMonth: prev?.cardMonth ?? e.payload.cardMonth,
          cardBills: prev?.cardBills ?? e.payload.cardBills,
          billsTruncated: prev?.billsTruncated ?? e.payload.billsTruncated,
        };
      });
      setSilentLogin(false);
      setBusy(false);
      setRefreshUi((u) => (u === "spin" ? "ok" : u));
    }).then((f) => unsubs.push(f));
    listen<string>("auth-status", (e) => {
      const p = e.payload;
      if (p === "logging") {
        setSilentLogin(true);
        setBusy(true);
        setRefreshUi("idle");
      } else if (p === "syncing") {
        setSilentLogin(true);
        setBusy(false);
        setRefreshUi("spin");
      } else if (p === "ok" || p === "idle") {
        setSilentLogin(false);
        setBusy(false);
        if (p === "ok") markRefreshOk();
        else setRefreshUi("idle");
      } else if (typeof p === "string" && p.startsWith("fail:")) {
        setSilentLogin(false);
        setBusy(false);
        setErr(p.slice(5));
      } else if (p === "need_sms") {
        setSilentLogin(false);
        setBusy(false);
      }
    }).then((f) => unsubs.push(f));
    listen<LoginOutcome>("need-sms", (e) => {
      const out = e.payload;
      if (out.status !== "need_sms") return;
      setSms({
        phone: out.phoneMask,
        png: out.captchaPng,
        ocr: out.ocrHint || "",
        graphic: out.ocrHint || "",
        code: "",
        trust: true,
      });
      setSmsWait(0);
      setSmsHint("");
      setErr("");
      openOverlayRef.current("sms");
    }).then((f) => unsubs.push(f));
    listen("island-expand", () => {
      revealAndPinRef.current();
    }).then((f) => unsubs.push(f));
    listen("hotkey-reveal", () => {
      if (gameFsRef.current) return;
      hotkeyHoldingRef.current = true;
      revealAndPinRef.current(false);
    }).then((f) => unsubs.push(f));
    listen<boolean>("exclusive-fullscreen", (e) => {
      const on = !!e.payload;
      gameFsRef.current = on;
      if (on) {
        getCurrentWindow().hide().catch(() => undefined);
        return;
      }
      if (!hiddenRef.current) {
        api.raiseTopmost().catch(() => undefined);
      }
    }).then((f) => unsubs.push(f));
    listen("hotkey-release", () => {
      if (!hotkeyHoldingRef.current) return;
      hotkeyHoldingRef.current = false;
      if (pinnedRef.current) return;
      if (hoveredRef.current) return;
      if (viewRef.current !== "main") return;
      expand(false);
    }).then((f) => unsubs.push(f));
    listen<boolean>("hover-zone", (e) => {
      window.clearTimeout(hoverExpandTimer.current);
      if (!e.payload) return;
      if (gameFsRef.current) return;
      if (!hiddenRef.current || !autoHideHoverRef.current) return;
      hoverExpandTimer.current = window.setTimeout(() => {
        if (!hiddenRef.current || !autoHideHoverRef.current) return;
        revealAndPinRef.current(false);
      }, 1000);
    }).then((f) => unsubs.push(f));
    listen("island-reset", () => {
      hideGen.current += 1;
      window.clearTimeout(hideTimer.current);
      window.clearTimeout(hoverExpandTimer.current);
      window.clearTimeout(remindTimer.current);
      stopHoverWatch();
      setRemind(null);
      setStowing(false);
      setHidden(false);
      setPinned(false);
      pinBeforeOverlay.current = null;
      setView("main");
      expand(false);
      setSettings((st) => (st ? { ...st, islandX: null, islandY: null } : st));
    }).then((f) => unsubs.push(f));
    return () => unsubs.forEach((f) => f());
  }, [expand]);

  useEffect(() => {
    let enter: number | undefined;
    let leave: number | undefined;
    const root = document.getElementById("root");
    if (!root) return;
    const onEnter = () => {
      hoveredRef.current = true;
      window.clearTimeout(leave);
      window.clearTimeout(hideTimer.current);
      window.clearTimeout(hoverExpandTimer.current);
      if (stowing) {
        hideGen.current += 1;
        setStowing(false);
      }
      if (hidden) {
        if (!settings?.autoHideHover) return;
        hoverExpandTimer.current = window.setTimeout(() => {
          if (draggingRef.current || justDragged.current || silentLogin) return;
          revealAndPinRef.current(false);
        }, 1000);
        return;
      }
      enter = window.setTimeout(() => {
        if (draggingRef.current || dragOrigin.current || silentLogin || justDragged.current) return;
        if (remindRef.current) return;
        if (!expanded) {
          if (view !== "main") setView("main");
          expand(true);
        }
      }, 120);
    };
    const onLeave = () => {
      hoveredRef.current = false;
      window.clearTimeout(enter);
      window.clearTimeout(hoverExpandTimer.current);
      leave = window.setTimeout(() => {
        if (draggingRef.current || dragOrigin.current || justDragged.current) return;
        if (remindRef.current) return;
        if (hidden) return;
        if (!pinned) {
          pinBeforeOverlay.current = null;
          setView("main");
          expand(false);
        }
      }, 2000);
    };
    root.addEventListener("mouseenter", onEnter);
    root.addEventListener("mouseleave", onLeave);
    return () => {
      root.removeEventListener("mouseenter", onEnter);
      root.removeEventListener("mouseleave", onLeave);
      window.clearTimeout(enter);
      window.clearTimeout(leave);
      window.clearTimeout(hoverExpandTimer.current);
    };
  }, [expanded, pinned, loggedIn, view, expand, silentLogin, openOverlay, hidden, stowing, settings?.autoHideHover]);

  useEffect(() => {
    if (view === "login" || view === "sms") focusIsland();
  }, [view]);

  useEffect(() => {
    if (settings?.autoHide !== false) return;
    if (!hidden && !stowing) return;
    hideGen.current += 1;
    setStowing(false);
    setHidden(false);
    stopHoverWatch();
    api.raiseTopmost().catch(() => undefined);
    setExpanded(false).catch(() => undefined);
  }, [settings?.autoHide]);

  useEffect(() => {
    window.clearTimeout(hideTimer.current);
    if (!settings?.autoHide || expanded || hidden || stowing || remind || pinned) return;
    if (silentLogin || view === "login" || view === "sms") return;
    if (hoveredRef.current) return;
    hideTimer.current = window.setTimeout(() => {
      hideIsland();
    }, 3000);
    return () => window.clearTimeout(hideTimer.current);
  }, [expanded, hidden, stowing, remind, pinned, settings?.autoHide, hideIsland, silentLogin, view]);

  useEffect(() => {
    if (!remind) return;
    api.raiseTopmost().catch(() => undefined);
    const t = window.setInterval(() => {
      api.raiseTopmost().catch(() => undefined);
    }, 1000);
    return () => window.clearInterval(t);
  }, [remind]);

  function finishDrag() {
    draggingRef.current = false;
    dragOrigin.current = null;
    api.persistIsland().catch(() => undefined);
    const over = document.getElementById("root")?.matches(":hover");
    hoveredRef.current = !!over;
    if (!pinnedRef.current && !over) {
      expand(false);
    }
    window.setTimeout(() => {
      skipClick.current = false;
      justDragged.current = false;
    }, 400);
  }

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (remind) {
        tookOverRef.current = true;
        window.clearTimeout(remindTimer.current);
        setRemind(null);
        const pre = preRemindRef.current;
        if (pre?.expanded) expand(true);
        else if (pre?.hidden && autoHideRef.current && !hoveredRef.current) hideIsland();
        return;
      }
      if (view === "bills" || view === "net" || view === "about" || view === "settings" || view === "login" || view === "sms" || view === "semester" || view === "event") {
        closeOverlay();
        return;
      }
      pinBeforeOverlay.current = null;
      setView("main");
      expand(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [view, expand, closeOverlay, remind, hideIsland]);

  async function refreshCaptcha() {
    if (captchaBusy) return;
    setCaptchaBusy(true);
    setErr("");
    try {
      const [png, hint] = await api.refreshCaptcha();
      setSms((s) => ({
        ...s,
        png,
        ocr: hint || "",
        graphic: hint || "",
      }));
    } catch (e) {
      setErr(String(e));
    } finally {
      setCaptchaBusy(false);
    }
  }

  async function sendSmsCode() {
    if (smsSending || smsWait > 0 || captchaBusy) return;
    if (!sms.graphic.trim()) {
      setErr("请输入图形验证码");
      return;
    }
    setSmsSending(true);
    setErr("");
    setSmsHint("");
    try {
      await api.sendSms(sms.graphic.trim());
      setSmsWait(60);
      setSmsHint("验证码发送成功，请注意查收");
    } catch (e) {
      const msg = String(e);
      setErr(msg);
      const graphicWrong =
        /请输入图形验证码|图形码不对|图形验证码/.test(msg)
        || (/验证码错误|验证码有误|验证码不正确/.test(msg) && !/短信|动态|身份验证/.test(msg));
      if (graphicWrong) {
        await refreshCaptcha();
      }
    } finally {
      setSmsSending(false);
    }
  }

  async function submitSms() {
    if (smsInFlight.current || busy) return;
    if (!/^\d{6}$/.test(sms.code.trim())) {
      setErr("请输入 6 位短信码");
      return;
    }
    smsInFlight.current = true;
    setBusy(true);
    setErr("");
    try {
      const out = await api.submitSms(sms.code.trim(), sms.trust);
      if (out.status !== "ok") {
        setErr("短信码不对，请重新获取后再试");
        setBusy(false);
        return;
      }
      if (!remember) setPass("");
      setSnap((s) =>
        s
          ? { ...s, loggedIn: true, errors: { ...s.errors, auth: undefined } }
          : { fetchedAt: "", network: "unknown", loggedIn: true, errors: {} }
      );
      setSilentLogin(true);
      setRefreshUi("spin");
      await closeOverlay();
      setBusy(false);
    } catch (e) {
      setErr(String(e));
      setBusy(false);
    } finally {
      smsInFlight.current = false;
    }
  }

  async function doLogin() {
    if (loginInFlight.current || busy) return;
    if (!user.trim() || !pass) {
      setErr("请输入学号和密码");
      return;
    }
    loginInFlight.current = true;
    setBusy(true);
    setErr("");
    try {
      const out = await api.login(user.trim(), pass, remember, autoLogin);
      if (out.status === "need_sms") {
        setSms({
          phone: out.phoneMask,
          png: out.captchaPng,
          ocr: out.ocrHint || "",
          graphic: out.ocrHint || "",
          code: "",
          trust: true,
        });
        setSmsWait(0);
        setSmsHint("");
        openOverlay("sms");
        setBusy(false);
      } else if (out.status === "ok") {
        if (!remember) setPass("");
        setSnap((s) =>
          s
            ? { ...s, loggedIn: true, username: user.trim(), errors: { ...s.errors, auth: undefined } }
            : { fetchedAt: "", network: "unknown", loggedIn: true, username: user.trim(), errors: {} }
        );
        setSilentLogin(true);
        setRefreshUi("spin");
        await closeOverlay();
        setBusy(false);
      }
    } catch (e) {
      setErr(String(e));
      setBusy(false);
    } finally {
      loginInFlight.current = false;
    }
  }

  function markRefreshOk() {
    setRefreshUi("ok");
    window.clearTimeout(refreshOkTimer.current);
    refreshOkTimer.current = window.setTimeout(() => setRefreshUi("idle"), 1400);
  }

  useEffect(() => {
    if (refreshUi !== "ok") return;
    window.clearTimeout(refreshOkTimer.current);
    refreshOkTimer.current = window.setTimeout(() => setRefreshUi("idle"), 1400);
    return () => window.clearTimeout(refreshOkTimer.current);
  }, [refreshUi]);

  async function doRefresh() {
    if (refreshUi === "spin") return;
    setRefreshUi("spin");
    setErr("");
    try {
      const s = await api.refresh();
      setSnap(s);
      if (s.offline) setRefreshUi("idle");
      else markRefreshOk();
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
      setSnap(s);
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
    eventBackRef.current = viewRef.current === "event" ? eventBackRef.current : viewRef.current;
    setEditingEvent(item);
    setEventErr("");
    setEventNonce((n) => n + 1);
    openOverlay("event");
  }

  async function saveEvent(item: Agenda) {
    if (eventBusy) return;
    setEventBusy(true);
    setEventErr("");
    try {
      const s = await api.saveAgenda(item);
      setSnap(s);
      setEventBusy(false);
      await closeOverlay();
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
      setSnap(s);
      setEventBusy(false);
      await closeOverlay();
    } catch (e) {
      setEventErr(String(e));
      setEventBusy(false);
    }
  }

  async function shiftMonth(delta: number) {
    let y = ym.y;
    let m = ym.m + delta;
    if (m < 1) { m = 12; y--; }
    if (m > 12) { m = 1; y++; }
    if (y > now.getFullYear() || (y === now.getFullYear() && m > now.getMonth() + 1)) return;
    setYm({ y, m });
    try {
      const s = await api.loadMonth(y, m);
      setSnap(s);
      const days = s.cardSpend || [];
      const t = ymd(now);
      setSelected(days.find((d) => d.date === t)?.date || days[days.length - 1]?.date || t);
    } catch (e) {
      setErr(String(e));
    }
  }

  const spend = snap?.cardSpend || [];
  const currentYm = `${now.getFullYear()}-${pad(now.getMonth() + 1)}`;
  const [homeSpend, setHomeSpend] = useState<DaySpend[]>([]);
  useEffect(() => {
    if (snap?.cardMonth === currentYm && (snap.cardSpend?.length ?? 0) > 0) {
      setHomeSpend(snap.cardSpend || []);
    }
  }, [snap?.cardMonth, snap?.cardSpend, currentYm]);
  const previewSpend = homeSpend.length ? homeSpend : spend;
  const previewMax = Math.max(...previewSpend.map((d) => d.spend), 1);
  const maxSpend = Math.max(...spend.map((d) => d.spend), 1);
  const monthSpend = spend.reduce((a, d) => a + d.spend, 0);
  const todaySpend = spend.find((d) => d.date === ymd(now))?.spend || 0;
  const selectedDay: DaySpend | undefined = spend.find((d) => d.date === selected);
  const isCurrentMonth = ym.y === now.getFullYear() && ym.m === now.getMonth() + 1;
  const cardWarn = (snap?.card?.balance ?? 999) < (settings?.cardWarn ?? 20);
  const netWarn = (snap?.net?.balance ?? 999) < (settings?.netWarn ?? 5);
  const elecCached = !!snap?.elec && !snap.elec?.unbound && snap.elec?.remainKwh != null;
  const elecMaint = now.getHours() < 6;
  const elecSyncFail = !elecMaint && !!snap?.errors.elec;
  const elecWarn = elecCached && (snap?.elec?.remainKwh ?? 999) < (settings?.elecWarn ?? 10);
  const week1 = (settings?.week1Monday || snap?.semester?.week1Monday || defaultWeek1(snap?.semester?.semesterId) || "").trim();
  const weekNow =
    (!settings?.week1Monday
      ? teachingWeekFromTerm(snap?.semester?.termWeeks, now)
      : undefined) ??
    teachingWeek(week1, now) ??
    snap?.teachingWeek;
  const weekdayNow = weekdayOf(now);
  const todayCourses = snap?.semester
    ? coursesOnDay(snap.semester, weekNow, weekdayNow)
    : snap?.coursesToday || [];
  const tomorrowDate = addDays(now, 1);
  const weekTomorrow =
    (!settings?.week1Monday
      ? teachingWeekFromTerm(snap?.semester?.termWeeks, tomorrowDate)
      : undefined) ??
    teachingWeek(week1, tomorrowDate) ??
    (weekdayNow === 7 && weekNow ? weekNow + 1 : weekNow);
  const tomorrowCourses = snap?.semester
    ? coursesOnDay(snap.semester, weekTomorrow, weekdayOf(tomorrowDate))
    : [];
  const todayDone = allCoursesEnded(todayCourses, now);
  const todayEvents = agendasOnDate(snap?.agenda, clockDay);
  const tomorrowEvents = agendasOnDate(snap?.agenda, ymd(tomorrowDate));
  const todayMix = mixDay(todayDone ? [] : todayCourses, todayEvents, now);
  const tomorrowMix = mixDay(tomorrowCourses, tomorrowEvents, now, tomorrowDate);
  const remainingTodayEvents = mixDay([], todayEvents, now).filter((r) => r.kind !== "past");
  const collapsedCourse = loggedIn ? collapsedFromCourses(todayCourses, now) : null;
  const collapsed = collapsedCourse?.mid === "今天课上完了" && tomorrowCourses[0]
    ? { ...collapsedCourse, aux: `明天 ${tomorrowCourses[0].start}` }
    : collapsedCourse;
  const offline = !!snap?.offline;
  const cacheLabel = cacheDayLabel(snap?.semester?.fetchedAt);
  const semMax = snap?.semester ? maxWeek(snap.semester) : 18;
  const todayKey = todayCourses.map((c) => `${c.name}@${c.start}`).join("|");
  const flushTop = (settings?.islandY ?? 0) < 2;

  useEffect(() => {
    if (remind) return;
    const mins = Math.max(1, Math.floor(Number(settings?.classRemindMinutes) || 15));
    if (settings?.classRemind) {
      const c = upcomingCourse(todayCourses, now, mins);
      if (c) {
        const id = `${ymd(now)}-${c.name}-${c.start}`;
        if (remindedId.current === id) return;
        remindedId.current = id;
        startRemindRef.current({ ...c, kind: "class" });
        return;
      }
    }
    const ev = upcomingAgenda(snap?.agenda || [], now);
    if (!ev) return;
    const id = `event-${ev.id}-${ev.start}`;
    if (remindedId.current === id) return;
    remindedId.current = id;
    startRemindRef.current({
      name: ev.name,
      place: "",
      start: ev.start.slice(11, 16),
      end: ev.end.slice(11, 16),
      weekday: weekdayNow || 1,
      kind: "event",
    });
  }, [now, settings?.classRemind, settings?.classRemindMinutes, todayKey, remind, todayCourses, snap?.agenda, weekdayNow]);

  function onDragPointerDown(e: React.PointerEvent) {
    if (e.button !== 0 || dragIgnore(e.target)) return;
    dragOrigin.current = { x: e.clientX, y: e.clientY };
    draggingRef.current = false;
  }

  async function onDragPointerMove(e: React.PointerEvent) {
    const o = dragOrigin.current;
    if (!o || draggingRef.current) return;
    if (Math.hypot(e.clientX - o.x, e.clientY - o.y) < 6) return;
    draggingRef.current = true;
    skipClick.current = true;
    justDragged.current = true;
    dragOrigin.current = null;
    try {
      await getCurrentWindow().startDragging();
    } catch {
      /* native drag unavailable */
    } finally {
      finishDrag();
    }
  }

  function onDragPointerUp() {
    if (!justDragged.current && !draggingRef.current) {
      dragOrigin.current = null;
    }
  }

  function PinBtn() {
    return (
      <button
        type="button"
        className={`icon-btn pin ${pinned ? "on" : ""}`}
        title={pinned ? "取消钉住" : "钉住"}
        onClick={(e) => {
          e.stopPropagation();
          togglePin();
        }}
      >
        <svg viewBox="0 0 16 16" aria-hidden="true">
          {pinned ? (
            <path fill="currentColor" d="M8 2.2 13.8 8 8 13.8 2.2 8z" />
          ) : (
            <path fill="none" stroke="currentColor" strokeWidth="1.4" d="M8 2.8 13.2 8 8 13.2 2.8 8z" />
          )}
        </svg>
      </button>
    );
  }

  const collapsedMid = remind
    ? `${remind.kind === "event" ? "即将开始" : "即将上课"} · ${remind.name}`
    : silentLogin
      ? refreshUi === "spin"
        ? "同步中…"
        : "登录中…"
      : !loggedIn
        ? "登录泥洞"
        : collapsed?.mid || "今天没课";
  const collapsedAux = remind
    ? remind.start
    : !loggedIn
      ? "→"
      : collapsed?.aux
        ? collapsed.aux
        : netWarn && snap?.net
          ? `网费 ${fmt(snap.net.balance)} 元`
          : snap?.card
            ? `卡 ${fmt(snap.card.balance)}`
            : "";
  const showLiveDot = !remind && !!collapsed?.live;
  const showSoonWarn = !remind && !!collapsed?.soon;
  const showBalWarn = !remind && loggedIn && (cardWarn || netWarn || elecWarn) && !showLiveDot;

  return (
    <div
      ref={islandRef}
      className={`island ${expanded ? "expanded" : "collapsed"}${flushTop ? " flush" : ""}${stowing || hidden ? " stowed" : ""}${remind ? " remind" : ""}`}
      style={{ height: islandH }}
      onPointerDown={onDragPointerDown}
      onPointerMove={onDragPointerMove}
      onPointerUp={onDragPointerUp}
      onPointerCancel={onDragPointerUp}
      onClick={() => {
        if (skipClick.current || draggingRef.current || justDragged.current) return;
        if (hidden || remind) {
          revealAndPin();
          return;
        }
        if (!expanded) {
          setPinned(true);
          pinBeforeOverlay.current = true;
          if (!loggedIn) setView("login");
          else setView("main");
          expand(true);
          focusIsland();
        }
      }}
    >
      <div
        className="collapsed-row drag"
        onDoubleClick={(e) => {
          e.stopPropagation();
          api.resetIsland();
        }}
      >
        <div className="logo">泥</div>
        <div className="mid">{collapsedMid}</div>
        {remind ? <span className="dot warn pulse" /> : showLiveDot ? <span className="dot blue" /> : showSoonWarn || showBalWarn ? <span className="dot warn" /> : null}
        <div className={`aux ${showSoonWarn || remind ? "warn" : ""}`}>{collapsedAux}</div>
        {remind ? <i className="remind-bar" /> : null}
      </div>

      {(expanded || panelIn) && view === "main" && (
        <div className="panel">
          <div className="topbar drag">
            <button type="button" className="brand" onClick={(e) => { e.stopPropagation(); openOverlay("about"); }}>泥洞</button>
            <div className={`week${offline ? " offline" : ""}`}>
              {offline
                ? "离线状态"
                : loggedIn
                  ? weekNow
                    ? `第 ${weekNow} 周 · 周${WEEKDAY_SHORT[weekdayNow - 1] || ""}`
                    : snap?.semester
                      ? `周${WEEKDAY_SHORT[weekdayNow - 1] || ""}`
                      : "已登录"
                  : "未登录"}
            </div>
            <PinBtn />
            <button
              className={`icon-btn refresh-btn ${refreshUi === "spin" ? "spin" : ""} ${refreshUi === "ok" ? "ok" : ""}`}
              title={refreshUi === "spin" ? "同步中" : refreshUi === "ok" ? "已同步" : "刷新"}
              onClick={(e) => { e.stopPropagation(); doRefresh(); }}
            >
              {refreshUi === "ok" ? (
                <svg viewBox="0 0 16 16" aria-hidden="true"><path fill="currentColor" d="M6.3 11.1 3.2 8l.9-.9 2.2 2.2 5.6-5.6.9.9z"/></svg>
              ) : (
                <svg viewBox="0 0 16 16" aria-hidden="true">
                  <path fill="currentColor" d="M8 2.5a5.5 5.5 0 1 1-3.89 1.61l.7.71A4.5 4.5 0 1 0 8 3.5V5L5.5 3 8 1v1.5z"/>
                </svg>
              )}
            </button>
            <button className="icon-btn" title="设置" onClick={(e) => { e.stopPropagation(); openOverlay("settings"); }}>
              <svg viewBox="0 0 16 16" aria-hidden="true">
                <path fill="currentColor" d="M6.4 1.5h3.2l.3 1.6c.4.1.8.3 1.1.6l1.5-.7 1.6 2.8-1.3 1c.1.3.1.7.1 1s0 .7-.1 1l1.3 1-1.6 2.8-1.5-.7c-.3.3-.7.5-1.1.6l-.3 1.6H6.4l-.3-1.6a4.5 4.5 0 0 1-1.1-.6l-1.5.7L1.9 9.9l1.3-1a4 4 0 0 1-.1-1c0-.3 0-.7.1-1l-1.3-1 1.6-2.8 1.5.7c.3-.3.7-.5 1.1-.6l.3-1.6zM8 6.2A1.8 1.8 0 1 0 8 9.8 1.8 1.8 0 0 0 8 6.2z"/>
              </svg>
            </button>
          </div>
          {!loggedIn && !hasCachedData ? (
            <div className="empty clickable" onClick={() => openOverlay("login")}>
              {authMsg || "点这里登录泥洞"}
            </div>
          ) : (
            <>
              {offline && (
                <div className="offline-banner">离线状态 · 显示上次缓存</div>
              )}
              {!offline && (!loggedIn || authMsg) && (
                <div className="err">
                  {authMsg || "会话已失效，换了网络需要重新登录"}
                  <br />
                  <button className="retry" type="button" onClick={() => openOverlay("login")}>重新登录</button>
                </div>
              )}
              <div className="cards">
                <div className={`card clickable ${cardWarn ? "warn" : ""}`} onClick={() => openOverlay("bills")}>
                  <div className="k">校园卡</div>
                  <div className="v num">{snap?.card ? fmt(snap.card.balance) : "—"}</div>
                  <div className="u">元{snap?.errors.card ? " · 同步失败" : ""}</div>
                  <div className="mini">
                    {previewSpend.filter((d) => d.date <= ymd(now)).slice(-31).map((d) => {
                      const h = d.spend ? Math.max(3, Math.round((d.spend / previewMax) * 16)) : 0;
                      return <i key={d.date} className={d.date === ymd(now) ? "today" : ""} style={{ height: h }} />;
                    })}
                  </div>
                </div>
                <div className={`card clickable ${netWarn ? "warn" : ""}`} onClick={() => openOverlay("net")}>
                  <div className="k">网费</div>
                  <div className="v num">{snap?.net ? fmt(snap.net.balance) : "—"}</div>
                  <div className={`u ${netWarn ? "warn" : ""}`}>
                    {snap?.net && snap.net.balance <= 0 ? "元 · 见底了" : "元"}
                    {snap?.errors.net
                      ? /被拒绝|拦截/.test(snap.errors.net)
                        ? " · 访问被拒"
                        : " · 同步失败"
                      : ""}
                  </div>
                  <div className="u" style={{ marginTop: 4 }}>
                    {snap?.net
                      ? snap.net.flowFromPortal === false && !snap.net.monthBytes
                        ? "已用 —"
                        : `已用 ${formatBytes(snap.net.monthBytes)}`
                      : ""}
                  </div>
                </div>
                <div className={`card ${elecWarn ? "warn" : ""}`}>
                  <div className="k">电费</div>
                  {elecCached ? (
                    <>
                      <div className="v num">{fmtElec(snap?.elec?.remainKwh || 0)}</div>
                      <div className={`u ${elecSyncFail ? "warn" : ""}`}>
                        度{elecWarn ? " · 偏低" : ""}
                        {elecSyncFail ? " · 同步失败" : ""}
                      </div>
                    </>
                  ) : elecSyncFail ? (
                    <>
                      <div className="v">—</div>
                      <div className="u warn">同步失败</div>
                    </>
                  ) : snap?.elec?.unbound || !snap?.elec ? (
                    <>
                      <div className="v">未绑定</div>
                      <div className="u">去设置填房间码</div>
                    </>
                  ) : (
                    <>
                      <div className="v">—</div>
                      <div className="u">度</div>
                    </>
                  )}
                </div>
              </div>
              <div className="sec-title">
                <span>今日</span>
                <span className="sec-actions">
                  <button
                    className="icon-btn plus"
                    type="button"
                    title="添加日程"
                    onClick={(e) => {
                      e.stopPropagation();
                      openEvent(null);
                    }}
                  >
                    <svg viewBox="0 0 16 16" aria-hidden="true">
                      <path fill="currentColor" d="M7.2 3.2h1.6v4h4v1.6h-4v4H7.2v-4h-4V7.2h4z" />
                    </svg>
                  </button>
                  <button
                    className="text-btn"
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation();
                      setSemWeek(weekNow || 1);
                      openOverlay("semester");
                    }}
                  >
                    日程 ›
                  </button>
                </span>
              </div>
              {!snap?.semester && snap?.errors.course ? (
                <div className="err">
                  {snap.network === "offcampus" || /校外|超时/.test(snap.errors.course)
                    ? "校外连不上教务，连校园网后再试。"
                    : "出了洞，教务不理人"}
                  <br />
                  <button className="retry" type="button" onClick={() => doSyncCourses()}>重试</button>
                </div>
              ) : courseSync === "spin" && !snap?.semester ? (
                <div className="empty">正在同步课表…</div>
              ) : !snap?.semester && todayMix.length === 0 ? (
                <div className="empty">教务还没有本学期课表。军训结束、选课落定后再刷新。</div>
              ) : snap?.semester && snap.semester.courses.length === 0 && todayMix.length === 0 ? (
                <div className="empty">教务还没有本学期课表。军训结束、选课落定后再刷新。</div>
              ) : todayDone ? (
                <>
                  <div className="done-note">今日课程已上完</div>
                  {remainingTodayEvents.length ? (
                    <div className="course">
                      {remainingTodayEvents.map((r) => (
                        <MixRowView key={r.key} row={r} onEvent={() => r.event && openEvent(r.event)} />
                      ))}
                    </div>
                  ) : null}
                  <div className="sec-title sub">
                    <span>明日</span>
                  </div>
                  {tomorrowMix.length === 0 ? (
                    <div className="empty">明日没有课</div>
                  ) : (
                    <div className="course">
                      {tomorrowMix.map((r) => (
                        <MixRowView
                          key={r.key}
                          row={{ ...r, kind: "later", label: r.type === "event" ? "日程" : "明天" }}
                          onEvent={() => r.event && openEvent(r.event)}
                        />
                      ))}
                    </div>
                  )}
                </>
              ) : todayMix.length === 0 ? (
                <div className="empty">今天洞里没课</div>
              ) : (
                <div className="course">
                  {todayMix.map((r) => (
                    <MixRowView key={r.key} row={r} onEvent={() => r.event && openEvent(r.event)} />
                  ))}
                </div>
              )}
              <div className="foot">
                {offline ? "离线状态" : snap?.fetchedAt ? new Date(snap.fetchedAt).toLocaleTimeString() + " 同步" : "尚未同步"}
                {offline && snap?.fetchedAt ? ` · 上次 ${new Date(snap.fetchedAt).toLocaleTimeString()}` : ""}
                {clockOverridden ? " · 测试时钟" : ""}
                {!offline && snap?.network === "campus"
                  ? " · 校园网直连"
                  : !offline && snap?.network === "offcampus"
                    ? " · 校外"
                    : ""}
                {err ? ` · ${err}` : ""}
              </div>
            </>
          )}
        </div>
      )}

      {(expanded || panelIn) && view === "net" && (
        <div className="panel">
          <div className="bill-head drag">
            <button className="back" onClick={() => closeOverlay()}>‹ 网费</button>
            <div className="week" />
            <PinBtn />
          </div>
          <div className="net-doc">
            <p className="lead">东大校园网按「这个月用了多少校外 IPv4 下载流量」阶梯收费。</p>
            <h4>什么要钱，什么不要钱</h4>
            <ul>
              <li><b>要钱：</b>用校园网的 IPv4 访问校外网站时，<b>下载</b>的那部分流量。</li>
              <li><b>不要钱：</b>校内网站（教务、邮箱、门户等）、IPv6、你上传发出去的流量。</li>
              <li><b>夜里不要钱：</b>每天 <b>0:00–6:00</b> 访问外网也不计费，适合下大文件。</li>
            </ul>
            <h4>怎么算这个月要交多少</h4>
            <p>按自然月累计上面那种「要钱的流量」，对号入座。前面几档用满了，再按后面的档算。</p>
            <table>
              <thead>
                <tr><th>本月已用</th><th>这个月网费</th></tr>
              </thead>
              <tbody>
                <tr><td>0–10 GB（含）</td><td>免费</td></tr>
                <tr><td>10–25 GB（含）</td><td>超出 10 GB 的部分，1 元 / GB</td></tr>
                <tr><td>25–40 GB（含）</td><td>一口价 15 元</td></tr>
                <tr><td>40–45 GB（含）</td><td>15 元 + 超出 40 GB 的部分 1 元 / GB</td></tr>
                <tr><td>45–80 GB（含）</td><td>一口价 20 元</td></tr>
                <tr><td>80 GB 以上</td><td>20 元 + 超出 80 GB 的部分 1 元 / GB</td></tr>
              </tbody>
            </table>
            <h4>举几个例子</h4>
            <ul>
              <li>用了 8 GB → <b>0 元</b></li>
              <li>用了 18 GB → 超出免费的 8 GB × 1 元 → <b>8 元</b></li>
              <li>用了 30 GB → 落在第三档 → <b>15 元</b></li>
              <li>用了 42 GB → 15 元再加 2 GB → <b>17 元</b></li>
              <li>用了 50 GB → 第五档一口价 → <b>20 元</b></li>
              <li>用了 90 GB → 20 元再加 10 GB → <b>30 元</b></li>
            </ul>
            <h4>和看板上数字的关系</h4>
            <ul>
              <li>大数字是账户里<b>预存的余额</b>，流量费从这里扣。</li>
              <li>「已用」是门户统计的<b>本月流量合计</b>。夜间免费段会不会从合计里剔除，以学校网关为准。</li>
              <li>一个账号最多 <b>5 台</b>设备同时在线（电脑、手机都算）。</li>
            </ul>
            <p className="src">沈阳本校学生政策。秦皇岛分校不一样。收费依据：《东北大学会议纪要》（纪要〔2021〕13 号）。</p>
          </div>
        </div>
      )}

      {(expanded || panelIn) && view === "about" && (
        <div className="panel">
          <div className="bill-head drag">
            <button className="back" onClick={() => closeOverlay()}>‹ 关于</button>
            <div className="week" />
            <PinBtn />
          </div>
          <div className="about-doc">
            <div className="about-name">泥洞</div>
            <div className="about-ver">v1.2.0</div>
            <p>东北大学桌面看板，致力于解决官方平台不便捷的问题。</p>
            <p className="about-meta">作者 sapphire_fic</p>
          </div>
        </div>
      )}

      {(expanded || panelIn) && view === "bills" && (
        <div className="panel">
          <div className="bill-head drag">
            <button className="back" onClick={() => closeOverlay()}>‹ 校园卡</button>
            <div className="month-nav">
              <button onClick={() => shiftMonth(-1)}>‹</button>
              <span>{isCurrentMonth ? `${ym.m}月` : `${ym.y}年${ym.m}月`}</span>
              <button onClick={() => shiftMonth(1)} disabled={isCurrentMonth}>›</button>
            </div>
            <PinBtn />
          </div>
          <div className="metrics">
            <div><div className="k">余额</div><div className="v num">{snap?.card ? fmt(snap.card.balance) + " 元" : "—"}</div></div>
            <div><div className="k">{isCurrentMonth ? "本月" : `${ym.m}月`}</div><div className="v num">{fmt(monthSpend)} 元</div></div>
            <div><div className="k">{isCurrentMonth ? "今日" : "日均"}</div><div className="v num">{fmt(isCurrentMonth ? todaySpend : monthSpend / Math.max(spend.filter(d => d.date <= ymd(now)).length, 1))} 元</div></div>
          </div>
          <div className="chart-block">
            {hover && (() => {
              const d = spend.find((x) => x.date === hover);
              if (!d) return null;
              const i = spend.findIndex((x) => x.date === hover);
              const future = d.date > ymd(now);
              const text = future ? "还没到" : d.spend ? `¥${fmt(d.spend)} · ${d.count} 笔` : "没刷卡";
              const r = (i + 0.5) / spend.length;
              const pos =
                r < 0.22
                  ? { left: 8, transform: "none" as const, bottom: 28 }
                  : r > 0.78
                    ? { right: 8, left: "auto", transform: "none" as const, bottom: 28 }
                    : { left: `${r * 100}%`, bottom: 28 };
              return <div className="tip" style={pos}>{d.date.slice(5)} · {text}</div>;
            })()}
            <div className="chart">
              {spend.map((d) => {
                const h = d.spend ? Math.max(4, Math.round((d.spend / maxSpend) * 104)) : 0;
                const future = d.date > ymd(now);
                const cls = [d.date === ymd(now) ? "today" : "", d.date === selected ? "sel" : ""].filter(Boolean).join(" ");
                return (
                  <div key={d.date} className="col" onMouseEnter={() => setHover(d.date)} onMouseLeave={() => setHover(null)} onClick={() => setSelected(d.date)}>
                    {d.spend && !future ? <div className={`bar ${cls}`} style={{ height: h }} /> : null}
                  </div>
                );
              })}
            </div>
            <div className="xaxis"><span>1日</span><span>15日</span><span>{spend.length ? `${spend.length}日` : ""}</span></div>
          </div>
          {selectedDay && (
            <>
              <div className="day-title">{selectedDay.date} · {selectedDay.count} 笔 · {fmt(selectedDay.spend)} 元</div>
              <div className="bills">
                {selectedDay.date > ymd(now) ? (
                  <div className="bill-empty">还没到这一天</div>
                ) : selectedDay.bills.length === 0 ? (
                  <div className="bill-empty">这天没刷卡</div>
                ) : selectedDay.bills.filter((b) => !isJunkBill(b)).length === 0 ? (
                  <div className="bill-empty">这天没刷卡</div>
                ) : selectedDay.bills.filter((b) => !isJunkBill(b)).map((b, i) => (
                  <div className="bill" key={i}>
                    <span className="t num">{b.time.slice(11, 16)}</span>
                    <span className="s" title={[b.subject, b.station].filter(Boolean).join(" · ")}>
                      {b.subject}{b.station ? ` · ${b.station}` : ""}
                    </span>
                    <span className={`a num ${b.amount > 0 ? "pos" : ""}`}>{b.amount > 0 ? "+" : ""}{fmt(b.amount)}</span>
                  </div>
                ))}
              </div>
            </>
          )}
          <div className="foot">{snap?.billsTruncated ? "本月可能不完整 · " : ""}只计消费，充值不进柱高</div>
        </div>
      )}

      {(expanded || panelIn) && view === "semester" && (
        <div className="panel">
          <div className="bill-head drag">
            <button className="back" onClick={() => closeOverlay()}>‹ 日程</button>
            <div className="month-nav">
              <button type="button" onClick={() => setSemWeek((w) => Math.max(1, w - 1))} disabled={semWeek <= 1}>‹</button>
              <span>第 {semWeek} 周</span>
              <button type="button" onClick={() => setSemWeek((w) => Math.min(semMax, w + 1))} disabled={semWeek >= semMax}>›</button>
            </div>
            <button
              className="icon-btn plus"
              type="button"
              title="添加日程"
              onClick={(e) => {
                e.stopPropagation();
                openEvent(null);
              }}
            >
              <svg viewBox="0 0 16 16" aria-hidden="true">
                <path fill="currentColor" d="M7.2 3.2h1.6v4h4v1.6h-4v4H7.2v-4h-4V7.2h4z" />
              </svg>
            </button>
            <PinBtn />
          </div>
          {!snap?.semester && snap?.errors.course ? (
            <div className="err">
              出了洞，教务不理人
              <br />
              <button className="retry" type="button" onClick={() => doSyncCourses()}>重试</button>
            </div>
          ) : !snap?.semester ? (
            <div className="empty">教务还没有本学期课表。军训结束、选课落定后再刷新。</div>
          ) : (
            <>
              <div
                className="tt"
                onMouseLeave={() => setCellTip(null)}
                onMouseMove={(e) => {
                  const cell = (e.target as HTMLElement).closest("[data-ctip]") as HTMLElement | null;
                  const grid = e.currentTarget;
                  if (!cell) {
                    if (cellTip) setCellTip(null);
                    return;
                  }
                  const r = grid.getBoundingClientRect();
                  const text = cell.dataset.ctip || "";
                  const pad = 8;
                  const est = Math.min(r.width - pad * 2, Math.max(120, text.length * 7 + 16));
                  let x = e.clientX - r.left;
                  if (x + est > r.width - pad) x = r.width - pad - est;
                  if (x < pad) x = pad;
                  let y = e.clientY - r.top - 36;
                  if (y < 4) y = e.clientY - r.top + 16;
                  setCellTip({ text, x, y });
                }}
              >
                <div />
                {WEEKDAY_SHORT.map((w, i) => {
                  const ds = datesOfTeachingWeek(week1, semWeek);
                  const d = ds[i];
                  const isToday = d && ymd(d) === ymd(now);
                  const hasAgenda = d && agendasOnDate(snap?.agenda, ymd(d)).length > 0;
                  return (
                    <div className={`h${isToday ? " today" : ""}${hasAgenda ? " has-agenda" : ""}`} key={w}>
                      {w}
                      {d ? <small>{d.getMonth() + 1}/{d.getDate()}</small> : null}
                    </div>
                  );
                })}
                {BLOCKS.map((b) => {
                  const home = majorityCampus(snap.semester!.courses, semWeek);
                  const left = blockTime(home, b.id);
                  return (
                    <Fragment key={b.id}>
                      <div className="p">{b.label}<small>{left}</small></div>
                      {[1, 2, 3, 4, 5, 6, 7].map((dow) => {
                        const hits = snap.semester!.courses.filter(
                          (c) => c.weekday === dow && inWeek(c, semWeek) && overlapsBlock(c, b.sections[0], b.sections[1])
                        );
                        if (!hits.length) return <div className="cell empty" key={`${b.id}-${dow}`} />;
                        const tip = hits.map(courseTip).join(" / ");
                        return (
                          <div className="cell" data-ctip={tip} key={`${b.id}-${dow}`}>
                            {hits[0].name}
                          </div>
                        );
                      })}
                    </Fragment>
                  );
                })}
                {cellTip ? <div className="tip" style={{ left: cellTip.x, top: cellTip.y, bottom: "auto" }}>{cellTip.text}</div> : null}
              </div>
              {(() => {
                const ds = datesOfTeachingWeek(week1, semWeek);
                const weekEvents: { a: Agenda; day: Date; dow: number }[] = [];
                for (const a of snap?.agenda || []) {
                  for (const day of ds) {
                    if (agendaOnDate(a, ymd(day))) {
                      weekEvents.push({ a, day, dow: weekdayOf(day) });
                    }
                  }
                }
                weekEvents.sort((p, q) => p.dow - q.dow || p.a.start.localeCompare(q.a.start));
                if (!weekEvents.length) {
                  return <div className="week-agenda empty-lite">这周还没有日程，点右上角加号添加。</div>;
                }
                return (
                  <div className="week-agenda">
                    <div className="week-agenda-title">本周日程</div>
                    {weekEvents.map(({ a, day, dow }) => (
                      <button
                        key={`${a.id}-${ymd(day)}`}
                        type="button"
                        className="week-agenda-row"
                        onClick={() => openEvent(a)}
                      >
                        <span className="wa-day">周{weekdayLabel(dow)}</span>
                        <span className="wa-time num">{a.start.slice(11, 16)}–{a.end.slice(11, 16)}</span>
                        <span className="wa-name">{a.name}</span>
                        {a.remind ? <span className="wa-bell" title="会提醒">●</span> : null}
                      </button>
                    ))}
                  </div>
                );
              })()}
              <div className="foot">
                作息按课表自动：{majorityCampus(snap.semester.courses, semWeek) === "hunnan" ? "浑南" : "南湖"}（上午与另一校区不同）
                {cacheLabel ? ` · 课表缓存于 ${cacheLabel}` : ""}
                {" · 选课有变就重新同步"}
              </div>
            </>
          )}
        </div>
      )}

      {(expanded || panelIn) && view === "event" && (
        <EventForm
          key={eventNonce}
          pin={<PinBtn />}
          initial={editingEvent}
          now={now}
          busy={eventBusy}
          err={eventErr}
          onClose={closeOverlay}
          onSave={saveEvent}
          onDelete={editingEvent?.id ? deleteEvent : undefined}
        />
      )}

      {(expanded || panelIn) && view === "login" && (
        <div className="panel">
          <div className="topbar drag"><h1>进入泥洞</h1><div className="week" /><PinBtn /></div>
          <label>学号</label>
          <input
            type="text"
            name="username"
            autoComplete="username"
            inputMode="numeric"
            placeholder="一号通学号"
            spellCheck={false}
            value={user}
            onChange={(e) => setUser(e.target.value)}
          />
          <label>密码</label>
          <input
            type="password"
            name="password"
            autoComplete="current-password"
            placeholder="一号通密码"
            value={pass}
            onChange={(e) => setPass(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && doLogin()}
          />
          <div className="toggle" onClick={() => {
            const next = !remember;
            setRemember(next);
            if (!next) {
              setAutoLogin(false);
              setPass("");
            }
            if (settings) setSettings({ ...settings, rememberPassword: next, autoLogin: next && autoLogin });
          }}>
            <span>记住密码</span><div className={`sw ${remember ? "on" : ""}`}><i /></div>
          </div>
          <div className={`toggle ${remember ? "" : "off"}`} onClick={() => {
            if (!remember) return;
            const next = !autoLogin;
            setAutoLogin(next);
            if (settings) setSettings({ ...settings, autoLogin: next });
          }}>
            <span>自动登录</span><div className={`sw ${autoLogin && remember ? "on" : ""}`}><i /></div>
          </div>
          <button className="primary" disabled={busy} onClick={doLogin}>{busy ? "登录中…" : "进入泥洞"}</button>
          {err ? <div className="err-inline">{err}</div> : null}
          <div className="hint">勾选后密码存进这台电脑的系统凭据，下次打开会自动填入。关掉「记住密码」会删掉已存的密码。退出登录不会清。</div>
        </div>
      )}

      {(expanded || panelIn) && view === "sms" && (
        <div className="panel">
          <div className="bill-head drag">
            <button
              className="back"
              type="button"
              onClick={async () => {
                setErr("");
                setSmsWait(0);
                setSmsHint("");
                setSms((s) => ({ ...s, code: "", graphic: "", png: "", ocr: "" }));
                try { await api.cancelSms(); } catch { /* ignore */ }
                setView("login");
              }}
            >‹ 返回登录</button>
            <div className="week" />
            <PinBtn />
          </div>
          <div className="hint">绑定手机 {sms.phone}</div>
          <label>图形验证码 · 学校的，不是泥洞的</label>
          <div className="sms-row">
            <input
              type="text"
              autoComplete="off"
              value={sms.graphic}
              onChange={(e) => setSms((s) => ({ ...s, graphic: e.target.value, ocr: "" }))}
              onKeyDown={(e) => e.key === "Enter" && sendSmsCode()}
              placeholder="图形码"
            />
            <img
              className={`captcha${captchaBusy ? " loading" : ""}`}
              src={sms.png ? `data:image/png;base64,${sms.png}` : undefined}
              alt="captcha"
              title="点击换一张"
              onClick={() => refreshCaptcha()}
            />
          </div>
          {sms.ocr ? <div className="ocr-ok">本机已识别，不对就改 · 图不离开这台电脑</div> : null}
          <button className="link" type="button" disabled={captchaBusy} onClick={() => refreshCaptcha()}>
            {captchaBusy ? "正在换图…" : "看不清，换一张"}
          </button>
          <label>短信码</label>
          <div className="sms-row">
            <input
              type="text"
              inputMode="numeric"
              autoComplete="one-time-code"
              value={sms.code}
              onChange={(e) => setSms((s) => ({ ...s, code: e.target.value.replace(/\D/g, "").slice(0, 6) }))}
              onKeyDown={(e) => e.key === "Enter" && submitSms()}
              placeholder="6 位"
              maxLength={6}
            />
            <button
              className="link"
              type="button"
              disabled={smsSending || smsWait > 0 || captchaBusy}
              onClick={() => sendSmsCode()}
            >
              {smsSending ? "发送中…" : smsWait > 0 ? `${smsWait} 秒` : "发送"}
            </button>
          </div>
          {smsHint ? <div className="ocr-ok">{smsHint}</div> : null}
          <div className="toggle" onClick={() => setSms((s) => ({ ...s, trust: !s.trust }))}>
            <span>信任这台电脑</span><div className={`sw ${sms.trust ? "on" : ""}`}><i /></div>
          </div>
          <button className="primary" disabled={busy || sms.code.trim().length !== 6} onClick={() => submitSms()}>
            {busy ? "登录中…" : "继续"}
          </button>
          {err ? <div className="err-inline">{err}</div> : null}
        </div>
      )}

      {(expanded || panelIn) && view === "settings" && settings && (
        <SettingsPanel
          pin={<PinBtn />}
          settings={settings}
          setSettings={(next) => {
            setRemember(next.rememberPassword !== false);
            setAutoLogin(!!next.autoLogin && next.rememberPassword !== false);
            if (next.rememberPassword === false) setPass("");
            setSettings(withSettingDefaults(next));
          }}
          semester={snap?.semester}
          cacheLabel={cacheLabel}
          courseSync={courseSync}
          courseSyncErr={courseSyncErr}
          offline={offline}
          onClose={closeOverlay}
          onSync={() => doSyncCourses()}
          onLogout={async () => {
            setBusy(false);
            try { await api.logout(); } catch { /* ignore */ }
            if (!remember) setPass("");
            setSnap({ fetchedAt: "", network: "unknown", loggedIn: false, offline: false, agenda: [], errors: {} });
            pinBeforeOverlay.current = null;
            setPinned(true);
            setView("login");
          }}
          onPreviewRemind={() => {
            const sample = todayCourses[0] || {
              name: "示例课程",
              place: "",
              start: "08:30",
              end: "09:15",
              weekday: weekdayNow || 1,
            };
            startRemind(sample);
          }}
        />
      )}
    </div>
  );
}
