import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalPosition } from "@tauri-apps/api/window";
import { api } from "./api";
import { upcomingAgenda } from "./agenda";
import { upcomingCourse } from "./course";
import { deriveDay } from "./dayModel";
import { fmtMoney, ymd } from "./format";
import { resolveTheme, withSettingDefaults } from "./prefs";
import type { AppSettings, Course, Snapshot } from "./types";
import { useClock } from "./useClock";

const sleep = (ms: number) => new Promise<void>((r) => window.setTimeout(r, ms));

function stopHoverWatch() {
  return api.setHoverWatch({ enabled: false, x: 0, y: 0, width: 0, height: 0 }).catch(() => undefined);
}

type Remind = { name: string; start: string; kind?: "class" | "event" };

export default function App() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [hidden, setHidden] = useState(false);
  const [stowing, setStowing] = useState(false);
  const [remind, setRemind] = useState<Remind | null>(null);
  const [silentLogin, setSilentLogin] = useState(false);
  const [campusHint, setCampusHint] = useState("");
  const [refreshUi, setRefreshUi] = useState<"idle" | "spin" | "ok">("idle");
  const [hovered, setHovered] = useState(false);
  const { now } = useClock(!!settings?.classRemind || !!remind);

  const draggingRef = useRef(false);
  const dragOrigin = useRef<{ x: number; y: number; pointerId: number } | null>(null);
  const dragSession = useRef<{ pointerId: number; startX: number; startY: number; winX: number; winY: number } | null>(null);
  const dragGen = useRef(0);
  const dragRaf = useRef(0);
  const dragPending = useRef<{ x: number; y: number } | null>(null);
  const hoveredRef = useRef(false);
  const hiddenRef = useRef(false);
  const remindRef = useRef<Remind | null>(null);
  const autoHideRef = useRef(false);
  const autoHideHoverRef = useRef(false);
  const hideGen = useRef(0);
  const hideTimer = useRef(0);
  const remindTimer = useRef(0);
  const hoverRevealTimer = useRef(0);
  const preRemindRef = useRef<{ hidden: boolean } | null>(null);
  const remindedId = useRef("");
  const startRemindRef = useRef<(c: Course & { kind?: "class" | "event" }) => void>(() => undefined);
  const revealIslandRef = useRef<() => void>(() => undefined);
  const gameFsRef = useRef(false);
  const hiddenBeforeFs = useRef(false);
  const openingDeskRef = useRef(false);
  const todayCoursesRef = useRef<Course[]>([]);
  const campusHintRef = useRef("");
  const silentLoginRef = useRef(false);
  const preHintHidden = useRef(false);
  const hintTimer = useRef(0);
  const hideIslandRef = useRef<() => void>(() => undefined);

  const theme = resolveTheme(settings?.theme);
  const loggedIn = !!snap?.loggedIn;
  hiddenRef.current = hidden;
  remindRef.current = remind;
  hoveredRef.current = hovered;
  autoHideRef.current = !!settings?.autoHide;
  autoHideHoverRef.current = !!settings?.autoHideHover;
  silentLoginRef.current = silentLogin;

  const day = deriveDay(snap, settings, now);
  todayCoursesRef.current = day.todayCourses;

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
  }, [theme]);

  useEffect(() => {
    if (settings?.theme === "light" || settings?.theme === "dark") return;
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    const onChange = () => document.documentElement.setAttribute("data-theme", resolveTheme("system"));
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [settings?.theme]);

  useEffect(() => {
    api.bootstrap().then(setSnap).catch(() => undefined);
    api.settings().then((s) => setSettings(withSettingDefaults(s))).catch(() => undefined);
  }, []);

  const hideIsland = useCallback(async () => {
    if (!autoHideRef.current || remindRef.current || campusHintRef.current || silentLoginRef.current || hiddenRef.current) return;
    const gen = ++hideGen.current;
    setStowing(true);
    await sleep(320);
    // 折叠途中如果来了校园网提示或同步文案，reveal 会把这一代作废。
    // 位置查询也是 await。作废之后不能再 hide()，否则动画被掐掉，窗口直接没了。
    const cancelled = () =>
      hideGen.current !== gen || !!remindRef.current || gameFsRef.current || !!campusHintRef.current || silentLoginRef.current;
    if (cancelled()) {
      if (hideGen.current === gen) setStowing(false);
      return;
    }
    const w = getCurrentWindow();
    try {
      if (autoHideHoverRef.current) {
        const pos = await w.outerPosition();
        const size = await w.outerSize();
        if (cancelled()) {
          if (hideGen.current === gen) setStowing(false);
          return;
        }
        const pad = 24;
        await api.setHoverWatch({
          enabled: true,
          x: pos.x - pad,
          y: pos.y - pad,
          width: size.width + pad * 2,
          height: size.height + pad * 2,
        });
      } else {
        await stopHoverWatch();
      }
      if (cancelled()) {
        if (hideGen.current === gen) setStowing(false);
        return;
      }
      setHidden(true);
      await w.hide();
      // hide() 返回时，提示可能已经把折叠类名清掉。这时再留着隐藏，岛就是直接没了。
      if (cancelled()) {
        setHidden(false);
        if (hideGen.current === gen) setStowing(false);
        try {
          await api.raiseTopmost();
        } catch {
          try {
            await getCurrentWindow().show();
          } catch {
            /* ignore */
          }
        }
      }
    } catch {
      if (hideGen.current === gen) {
        setStowing(false);
        setHidden(false);
      }
    }
  }, []);

  const revealIsland = useCallback(async () => {
    hideGen.current += 1;
    window.clearTimeout(hideTimer.current);
    window.clearTimeout(hoverRevealTimer.current);
    setStowing(false);
    setHidden(false);
    await stopHoverWatch();
    try {
      await api.raiseTopmost();
    } catch {
      try {
        await getCurrentWindow().show();
      } catch {
        /* ignore */
      }
    }
  }, []);

  const startRemind = useCallback(async (course: Course & { kind?: "class" | "event" }) => {
    hideGen.current += 1;
    window.clearTimeout(hideTimer.current);
    window.clearTimeout(hoverRevealTimer.current);
    preRemindRef.current = { hidden: hiddenRef.current };
    setStowing(false);
    setHidden(false);
    await stopHoverWatch();
    try {
      await api.raiseTopmost();
    } catch {
      try {
        await getCurrentWindow().show();
      } catch {
        /* ignore */
      }
    }
    setRemind({ name: course.name, start: course.start, kind: course.kind || "class" });
    window.clearTimeout(remindTimer.current);
    remindTimer.current = window.setTimeout(() => {
      const pre = preRemindRef.current;
      setRemind(null);
      if (pre?.hidden && autoHideRef.current) hideIsland();
    }, 10000);
  }, [hideIsland]);

  revealIslandRef.current = revealIsland;
  startRemindRef.current = startRemind;
  hideIslandRef.current = () => { void hideIsland(); };

  useEffect(() => {
    const unsubs: Array<() => void> = [];
    listen<Snapshot>("snapshot-updated", (e) => {
      setSnap(e.payload);
      setSilentLogin(false);
      setRefreshUi((u) => (u === "spin" ? "ok" : u));
    }).then((f) => unsubs.push(f));
    listen<AppSettings>("settings-updated", (e) => {
      setSettings(withSettingDefaults(e.payload));
    }).then((f) => unsubs.push(f));
    listen<string>("auth-status", (e) => {
      const p = e.payload;
      if (p === "logging") {
        setSilentLogin(true);
        setRefreshUi("idle");
      } else if (p === "syncing") {
        setSilentLogin(true);
        setRefreshUi("spin");
      } else if (p === "ok" || p === "idle" || p === "need_sms") {
        setSilentLogin(false);
        if (p !== "ok") setRefreshUi("idle");
      } else if (typeof p === "string" && p.startsWith("fail:")) {
        setSilentLogin(false);
        setRefreshUi("idle");
      }
    }).then((f) => unsubs.push(f));
    listen<string>("campus-hint", (e) => {
      const text = typeof e.payload === "string" ? e.payload : "";
      window.clearTimeout(hintTimer.current);
      if (!text) {
        campusHintRef.current = "";
        setCampusHint("");
        return;
      }
      if (!campusHintRef.current) preHintHidden.current = hiddenRef.current;
      campusHintRef.current = text;
      setCampusHint(text);
      if (!gameFsRef.current) revealIslandRef.current();
      if (!text.startsWith("正在")) {
        hintTimer.current = window.setTimeout(() => {
          campusHintRef.current = "";
          setCampusHint("");
          if (preHintHidden.current && autoHideRef.current && !hoveredRef.current) hideIslandRef.current();
        }, 4000);
      }
    }).then((f) => unsubs.push(f));
    listen("island-reveal", () => {
      revealIslandRef.current();
    }).then((f) => unsubs.push(f));
    listen("hotkey-reveal", () => {
      if (gameFsRef.current) return;
      revealIslandRef.current();
    }).then((f) => unsubs.push(f));
    listen<boolean>("exclusive-fullscreen", (e) => {
      const on = !!e.payload;
      gameFsRef.current = on;
      if (on) {
        hiddenBeforeFs.current = hiddenRef.current;
        getCurrentWindow().hide().catch(() => undefined);
        return;
      }
      if (!hiddenBeforeFs.current) {
        setHidden(false);
        setStowing(false);
        api.raiseTopmost().catch(() => undefined);
      }
    }).then((f) => unsubs.push(f));
    listen<boolean>("hover-zone", (e) => {
      window.clearTimeout(hoverRevealTimer.current);
      if (!e.payload || gameFsRef.current) return;
      if (!hiddenRef.current || !autoHideHoverRef.current) return;
      hoverRevealTimer.current = window.setTimeout(() => {
        if (!hiddenRef.current || !autoHideHoverRef.current || gameFsRef.current) return;
        revealIslandRef.current();
      }, 1000);
    }).then((f) => unsubs.push(f));
    listen("island-reset", () => {
      hideGen.current += 1;
      window.clearTimeout(hideTimer.current);
      window.clearTimeout(hoverRevealTimer.current);
      window.clearTimeout(remindTimer.current);
      stopHoverWatch();
      setRemind(null);
      setStowing(false);
      setHidden(false);
      setSettings((st) => (st ? { ...st, islandX: null, islandY: null } : st));
    }).then((f) => unsubs.push(f));
    listen("preview-remind", () => {
      const sample = todayCoursesRef.current[0] || {
        name: "示例课程",
        place: "",
        start: "08:30",
        end: "09:15",
        weekday: 1,
      };
      startRemindRef.current({ ...sample, kind: "class" });
    }).then((f) => unsubs.push(f));
    return () => unsubs.forEach((f) => f());
  }, []);

  useEffect(() => {
    const root = document.getElementById("root");
    if (!root) return;
    const onEnter = () => {
      hoveredRef.current = true;
      setHovered(true);
      window.clearTimeout(hideTimer.current);
      if (stowing) {
        hideGen.current += 1;
        setStowing(false);
      }
    };
    const onLeave = () => {
      hoveredRef.current = false;
      setHovered(false);
    };
    root.addEventListener("mouseenter", onEnter);
    root.addEventListener("mouseleave", onLeave);
    return () => {
      root.removeEventListener("mouseenter", onEnter);
      root.removeEventListener("mouseleave", onLeave);
    };
  }, [stowing]);

  useEffect(() => {
    if (settings?.autoHide !== false) return;
    if (!hidden && !stowing) return;
    hideGen.current += 1;
    setStowing(false);
    setHidden(false);
    stopHoverWatch();
    api.raiseTopmost().catch(() => undefined);
  }, [settings?.autoHide, hidden, stowing]);

  useEffect(() => {
    window.clearTimeout(hideTimer.current);
    if (!settings?.autoHide || hidden || stowing || remind || hovered || campusHint || silentLogin) return;
    hideTimer.current = window.setTimeout(() => {
      hideIsland();
    }, 3000);
    return () => window.clearTimeout(hideTimer.current);
  }, [hidden, stowing, remind, hovered, campusHint, silentLogin, settings?.autoHide, hideIsland]);

  useEffect(() => {
    if (!remind) return;
    api.raiseTopmost().catch(() => undefined);
    const t = window.setInterval(() => {
      api.raiseTopmost().catch(() => undefined);
    }, 1000);
    return () => window.clearInterval(t);
  }, [remind]);

  const todayKey = day.todayCourses.map((c) => `${c.name}@${c.start}`).join("|");
  useEffect(() => {
    if (remind) return;
    const mins = Math.max(1, Math.floor(Number(settings?.classRemindMinutes) || 15));
    if (settings?.classRemind) {
      const c = upcomingCourse(day.todayCourses, now, mins);
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
    const id = `event-${ev.item.id}-${ev.start}`;
    if (remindedId.current === id) return;
    remindedId.current = id;
    startRemindRef.current({
      name: ev.item.name,
      place: "",
      start: ev.start.slice(11, 16),
      end: ev.end.slice(11, 16),
      weekday: day.weekdayNow || 1,
      kind: "event",
    });
  }, [now, settings?.classRemind, settings?.classRemindMinutes, todayKey, remind, day.todayCourses, day.weekdayNow, snap?.agenda]);

  function finishDrag() {
    draggingRef.current = false;
    dragOrigin.current = null;
    dragSession.current = null;
    if (dragRaf.current) {
      cancelAnimationFrame(dragRaf.current);
      dragRaf.current = 0;
    }
    const parked = dragPending.current;
    dragPending.current = null;
    if (parked) {
      getCurrentWindow().setPosition(new LogicalPosition(parked.x, parked.y)).catch(() => undefined);
    }
    api.persistIsland().catch(() => undefined);
    const over = document.getElementById("root")?.matches(":hover");
    hoveredRef.current = !!over;
    setHovered(!!over);
  }

  function queueIslandPos(x: number, y: number) {
    dragPending.current = { x, y };
    if (dragRaf.current) return;
    dragRaf.current = requestAnimationFrame(() => {
      dragRaf.current = 0;
      const p = dragPending.current;
      dragPending.current = null;
      if (!p) return;
      getCurrentWindow().setPosition(new LogicalPosition(p.x, p.y)).catch(() => undefined);
    });
  }

  function onDragPointerDown(e: React.PointerEvent) {
    if (e.button !== 0) return;
    dragOrigin.current = { x: e.screenX, y: e.screenY, pointerId: e.pointerId };
    draggingRef.current = false;
    dragSession.current = null;
    try {
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    } catch {
      /* capture unavailable */
    }
  }

  function onDragPointerMove(e: React.PointerEvent) {
    const o = dragOrigin.current;
    if (!o || e.pointerId !== o.pointerId) return;
    const session = dragSession.current;
    if (session) {
      queueIslandPos(session.winX + (e.screenX - session.startX), session.winY + (e.screenY - session.startY));
      return;
    }
    if (draggingRef.current || (e.buttons & 1) === 0) return;
    if (Math.hypot(e.screenX - o.x, e.screenY - o.y) < 6) return;
    // 不用 startDragging：Windows 会把它当成标题栏拖动，画出最小化按钮，松手后还可能卡死。
    draggingRef.current = true;
    const gen = ++dragGen.current;
    const pointerId = o.pointerId;
    const screenX = e.screenX;
    const screenY = e.screenY;
    const win = getCurrentWindow();
    void (async () => {
      try {
        const scale = await win.scaleFactor();
        const pos = await win.outerPosition();
        if (dragGen.current !== gen) return;
        dragSession.current = {
          pointerId,
          startX: screenX,
          startY: screenY,
          winX: pos.x / scale,
          winY: pos.y / scale,
        };
      } catch {
        /* position unavailable */
      }
    })();
  }

  function onDragPointerUp(e: React.PointerEvent) {
    dragGen.current += 1;
    const dragged = draggingRef.current || !!dragSession.current;
    dragOrigin.current = null;
    dragSession.current = null;
    if ((e.currentTarget as HTMLElement).hasPointerCapture?.(e.pointerId)) {
      try {
        (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
      } catch {
        /* already released */
      }
    }
    if (dragged) {
      finishDrag();
      return;
    }
    if (e.button !== 0 || e.type === "pointercancel") return;
    hideGen.current += 1;
    setStowing(false);
    openDesk();
  }

  function openDesk() {
    if (openingDeskRef.current) return;
    openingDeskRef.current = true;
    api.openDesk()
      .catch(() => undefined)
      .finally(() => {
        window.setTimeout(() => {
          openingDeskRef.current = false;
        }, 350);
      });
  }

  const cardWarn = (snap?.card?.balance ?? 999) < (settings?.cardWarn ?? 20);
  const netWarn = (snap?.net?.balance ?? 999) < (settings?.netWarn ?? 5);
  const elecCached = !!snap?.elec && !snap.elec?.unbound && snap.elec?.remainKwh != null;
  const elecWarn = elecCached && (snap?.elec?.remainKwh ?? 999) < (settings?.elecWarn ?? 10);
  const collapsedMid = remind
    ? `${remind.kind === "event" ? "即将开始" : "即将上课"} · ${remind.name}`
    : campusHint
      ? campusHint
      : silentLogin
      ? refreshUi === "spin"
        ? "同步中…"
        : "登录中…"
      : !loggedIn && !settings?.skipLogin
        ? "登录泥洞"
        : day.collapsed?.mid || "今天没课";
  const collapsedAux = remind
    ? remind.start
    : campusHint
      ? ""
      : !loggedIn && !settings?.skipLogin
      ? "→"
      : day.collapsed?.aux
        ? day.collapsed.aux
        : netWarn && snap?.net
          ? `网费 ${fmtMoney(snap.net.balance)} 元`
          : snap?.card
            ? `卡 ${fmtMoney(snap.card.balance)}`
            : "";
  const showLiveDot = !remind && !!day.collapsed?.live;
  const showSoonWarn = !remind && !!day.collapsed?.soon;
  const showBalWarn = !remind && loggedIn && (cardWarn || netWarn || elecWarn) && !showLiveDot;
  const flushTop = (settings?.islandY ?? 0) < 2;

  return (
    <div
      className={`island collapsed${flushTop ? " flush" : ""}${stowing || hidden ? " stowed" : ""}${remind ? " remind" : ""}`}
      onPointerDown={onDragPointerDown}
      onPointerMove={onDragPointerMove}
      onPointerUp={onDragPointerUp}
      onPointerCancel={onDragPointerUp}
    >
      <div className="collapsed-row drag">
        <div className="logo">泥</div>
        <div className="mid">{collapsedMid}</div>
        {remind ? <span className="dot warn pulse" /> : showLiveDot ? <span className="dot blue" /> : showSoonWarn || showBalWarn ? <span className="dot warn" /> : null}
        <div className={`aux ${showSoonWarn || remind ? "warn" : ""}`}>{collapsedAux}</div>
        {remind ? <i className="remind-bar" /> : null}
      </div>
    </div>
  );
}
