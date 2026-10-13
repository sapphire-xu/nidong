import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalPosition } from "@tauri-apps/api/window";
import { api } from "./api";
import { upcomingAgenda } from "./agenda";
import { upcomingCourse } from "./course";
import { HIDE_AFTER_MS, nextHideTick, revealShouldRearm } from "./autoHide";
import { deriveDay } from "./dayModel";
import { fmtMoney, ymd } from "./format";
import { clampVolume, resolveTheme, withSettingDefaults } from "./prefs";
import type { AppSettings, Course, Snapshot } from "./types";
import { useClock } from "./useClock";

const sleep = (ms: number) => new Promise<void>((r) => window.setTimeout(r, ms));

let remindAudio: HTMLAudioElement | null = null;
function playRemindSound(volume: number) {
  if (remindAudio) {
    remindAudio.pause();
    remindAudio.currentTime = 0;
  }
  const a = new Audio("/sounds/argon.ogg");
  const level = Number.isFinite(volume) ? Math.min(1, Math.max(0, volume)) : 1;
  a.volume = level;
  remindAudio = a;
  void a.play().catch(() => undefined);
}

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
  const remindSoundRef = useRef(false);
  const remindVolumeRef = useRef(1);
  const revealIslandRef = useRef<() => void>(() => undefined);
  const scheduleRef = useRef<(ms: number) => void>(() => undefined);
  const idleSince = useRef(0);
  const stowSince = useRef(0);
  const stowingRef = useRef(false);
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
  stowingRef.current = stowing;
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

  const abortStow = (gen: number) => {
    if (hideGen.current !== gen) return;
    stowingRef.current = false;
    stowSince.current = 0;
    setStowing(false);
  };

  const hideIsland = useCallback(async () => {
    if (!autoHideRef.current || remindRef.current || campusHintRef.current || silentLoginRef.current || hiddenRef.current) return;
    if (gameFsRef.current || document.getElementById("root")?.matches(":hover")) return;
    const gen = ++hideGen.current;
    stowingRef.current = true;
    stowSince.current = performance.now();
    setStowing(true);
    await sleep(320);
    // 折叠途中如果来了校园网提示或同步文案，reveal 会把这一代作废。
    // 位置查询也是 await。作废之后不能再 hide()，否则动画被掐掉，窗口直接没了。
    const cancelled = () =>
      hideGen.current !== gen || !!remindRef.current || gameFsRef.current || !!campusHintRef.current || silentLoginRef.current
      || !!document.getElementById("root")?.matches(":hover");
    if (cancelled()) {
      abortStow(gen);
      return;
    }
    const w = getCurrentWindow();
    try {
      if (autoHideHoverRef.current) {
        const pos = await w.outerPosition();
        const size = await w.outerSize();
        if (cancelled()) {
          abortStow(gen);
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
        abortStow(gen);
        return;
      }
      hiddenRef.current = true;
      setHidden(true);
      await w.hide();
      // hide() 返回时，提示可能已经把折叠类名清掉。这时再留着隐藏，岛就是直接没了。
      if (cancelled()) {
        hiddenRef.current = false;
        stowingRef.current = false;
        stowSince.current = 0;
        setHidden(false);
        setStowing(false);
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
        hiddenRef.current = false;
        setHidden(false);
        abortStow(gen);
      }
    }
  }, []);

  const revealIsland = useCallback(async () => {
    hideGen.current += 1;
    window.clearTimeout(hideTimer.current);
    window.clearTimeout(hoverRevealTimer.current);
    stowingRef.current = false;
    stowSince.current = 0;
    hiddenRef.current = false;
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
    // 岛本来就露着时，hidden 不变，上面的效果不会重跑。这里必须自己把 3 秒排回去。
    if (revealShouldRearm(autoHideRef.current, hiddenRef.current)) {
      idleSince.current = performance.now();
      scheduleRef.current(HIDE_AFTER_MS);
    }
  }, []);

  remindSoundRef.current = settings?.remindSound === true;
  remindVolumeRef.current = clampVolume(settings?.remindVolume) / 100;

  const startRemind = useCallback(async (course: Course & { kind?: "class" | "event" }) => {
    if (remindSoundRef.current) playRemindSound(remindVolumeRef.current);
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
      stowingRef.current = false;
      stowSince.current = 0;
      hiddenRef.current = false;
      setStowing(false);
      setHidden(false);
      setSettings((st) => (st ? { ...st, islandX: null, islandY: null } : st));
      if (revealShouldRearm(autoHideRef.current, false)) {
        idleSince.current = performance.now();
        scheduleRef.current(HIDE_AFTER_MS);
      }
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
      if (stowingRef.current) {
        hideGen.current += 1;
        stowingRef.current = false;
        stowSince.current = 0;
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
  }, []);

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
    let dead = false;
    const arm = (ms: number) => {
      window.clearTimeout(hideTimer.current);
      if (dead) return;
      hideTimer.current = window.setTimeout(tick, ms);
    };
    const tick = () => {
      if (dead) return;
      const now = performance.now();
      if (stowingRef.current && stowSince.current && now - stowSince.current > 2000 && !hiddenRef.current) {
        hideGen.current += 1;
        stowingRef.current = false;
        stowSince.current = 0;
        setStowing(false);
      }
      const over = !!document.getElementById("root")?.matches(":hover");
      hoveredRef.current = over;
      const idleMs = idleSince.current ? now - idleSince.current : 0;
      const decision = nextHideTick({
        autoHide: autoHideRef.current,
        hidden: hiddenRef.current,
        hover: over,
        remind: !!remindRef.current,
        hint: !!campusHintRef.current,
        login: silentLoginRef.current,
        stowing: stowingRef.current,
        idleMs,
      });
      if (decision.kind === "stop") return;
      if (decision.kind === "wait") {
        if (decision.resetIdle) idleSince.current = 0;
        else if (!idleSince.current) idleSince.current = now;
        arm(decision.ms);
        return;
      }
      idleSince.current = 0;
      void hideIsland().finally(() => {
        if (dead || hiddenRef.current || !autoHideRef.current) return;
        idleSince.current = performance.now();
        arm(HIDE_AFTER_MS);
      });
    };
    scheduleRef.current = (ms: number) => {
      if (!autoHideRef.current) return;
      idleSince.current = performance.now();
      arm(ms);
    };
    if (settings?.autoHide && !hidden) {
      idleSince.current = performance.now();
      arm(HIDE_AFTER_MS);
    }
    return () => {
      dead = true;
      window.clearTimeout(hideTimer.current);
      scheduleRef.current = () => undefined;
    };
  }, [settings?.autoHide, hidden, remind, campusHint, silentLogin, hideIsland]);

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
