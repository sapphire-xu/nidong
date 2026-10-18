import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalPosition } from "@tauri-apps/api/window";
import { api } from "./api";
import { upcomingAgenda } from "./agenda";
import { upcomingCourse } from "./course";
import { abortFold, HIDE_AFTER_MS, HIDE_POLL_MS, shouldResetIdle, stepHide, type HidePhase } from "./autoHide";
import { deriveDay } from "./dayModel";
import { fmtMoney, ymd } from "./format";
import { clampVolume, resolveTheme, withSettingDefaults } from "./prefs";
import type { AppSettings, Course, Snapshot } from "./types";
import { useClock } from "./useClock";

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
  const remindRef = useRef<Remind | null>(null);
  const autoHideRef = useRef(false);
  const autoHideHoverRef = useRef(false);
  const hideGen = useRef(0);
  const remindTimer = useRef(0);
  const hoverRevealTimer = useRef(0);
  const preRemindRef = useRef<{ hidden: boolean } | null>(null);
  const remindedId = useRef("");
  const startRemindRef = useRef<(c: Course & { kind?: "class" | "event" }) => void>(() => undefined);
  const remindSoundRef = useRef(false);
  const remindVolumeRef = useRef(1);
  const revealRef = useRef<() => void>(() => undefined);
  const armHideRef = useRef<() => void>(() => undefined);
  const cancelStowRef = useRef<() => void>(() => undefined);
  const tickRef = useRef<() => void>(() => undefined);
  const hideFsRef = useRef<() => void>(() => undefined);
  const phaseRef = useRef<HidePhase>("shown");
  const idleSince = useRef(0);
  const stowSince = useRef(0);
  const closingRef = useRef(false);
  const busyRef = useRef(false);
  const sawReadyRef = useRef(false);
  const prevAutoHideRef = useRef<boolean | null>(null);
  const settingsRef = useRef<AppSettings | null>(null);
  const gameFsRef = useRef(false);
  const hiddenBeforeFs = useRef(false);
  const openingDeskRef = useRef(false);
  const todayCoursesRef = useRef<Course[]>([]);
  const campusHintRef = useRef("");
  const silentLoginRef = useRef(false);
  const preHintHidden = useRef(false);
  const hintTimer = useRef(0);

  const theme = resolveTheme(settings?.theme);
  const loggedIn = !!snap?.loggedIn;
  settingsRef.current = settings;
  remindRef.current = remind;
  autoHideRef.current = !!settings && settings.autoHide !== false;
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

  const paint = (phase: HidePhase) => {
    phaseRef.current = phase;
    if (phase !== "stowing") stowSince.current = 0;
    const stow = phase === "stowing";
    const hid = phase === "hidden";
    setStowing((v) => (v === stow ? v : stow));
    setHidden((v) => (v === hid ? v : hid));
  };

  const showIsland = () => {
    void api.raiseTopmost().catch(() => {
      void getCurrentWindow().show().catch(() => undefined);
    });
  };

  const pointerOnIsland = () => !!document.getElementById("root")?.matches(":hover") || draggingRef.current;

  const reveal = () => {
    hideGen.current += 1;
    closingRef.current = false;
    paint("shown");
    idleSince.current = performance.now();
    void stopHoverWatch();
    showIsland();
  };

  // 原先是藏着的：这一轮直接折，不再另等 3 秒。指针还在岛上时，巡检会把这一下抹掉并重新计时。
  const armHide = () => {
    if (phaseRef.current === "hidden" || gameFsRef.current) return;
    idleSince.current = performance.now() - HIDE_AFTER_MS - HIDE_POLL_MS;
  };

  const cancelStow = () => {
    idleSince.current = performance.now();
    if (phaseRef.current !== "stowing" && !closingRef.current) return;
    hideGen.current += 1;
    closingRef.current = false;
    paint("shown");
    void stopHoverWatch();
  };

  const finishHide = async () => {
    const gen = ++hideGen.current;
    closingRef.current = true;
    const dropped = () => abortFold({
      superseded: hideGen.current !== gen,
      remind: !!remindRef.current,
      fullscreen: gameFsRef.current,
      autoHide: autoHideRef.current,
      hover: pointerOnIsland(),
    });
    // 折叠类名清掉之后不能再把窗口留在隐藏。全屏自己负责藏，这里不要把它叫回来。
    const undoHide = () => {
      closingRef.current = false;
      if (gameFsRef.current) return;
      paint("shown");
      idleSince.current = performance.now();
      void stopHoverWatch();
      showIsland();
    };
    const keepShown = () => {
      if (gameFsRef.current) {
        closingRef.current = false;
        return;
      }
      if (hideGen.current !== gen && phaseRef.current !== "stowing") return;
      closingRef.current = false;
      paint("shown");
      idleSince.current = performance.now();
      void stopHoverWatch();
    };
    if (dropped()) {
      keepShown();
      return;
    }
    const w = getCurrentWindow();
    try {
      if (autoHideHoverRef.current) {
        const pos = await w.outerPosition();
        const size = await w.outerSize();
        if (dropped()) {
          keepShown();
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
      if (dropped()) {
        keepShown();
        return;
      }
      await w.hide();
      if (gameFsRef.current && hideGen.current === gen) {
        closingRef.current = false;
        phaseRef.current = "hidden";
        stowSince.current = 0;
        setStowing(false);
        setHidden(true);
        return;
      }
      if (dropped() || pointerOnIsland()) {
        undoHide();
        return;
      }
      closingRef.current = false;
      paint("hidden");
    } catch {
      undoHide();
    }
  };

  const hideForFullscreen = async () => {
    const gen = ++hideGen.current;
    closingRef.current = true;
    phaseRef.current = "hidden";
    stowSince.current = 0;
    try {
      await getCurrentWindow().hide();
    } catch {
      if (hideGen.current !== gen) return;
      closingRef.current = false;
      if (!gameFsRef.current) paint("shown");
      return;
    }
    if (hideGen.current !== gen) {
      closingRef.current = false;
      if (!gameFsRef.current) showIsland();
      return;
    }
    closingRef.current = false;
    setStowing(false);
    setHidden(true);
  };

  const tick = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    try {
      let now = performance.now();
      if (closingRef.current && stowSince.current && now - stowSince.current > 3000 && !gameFsRef.current) {
        hideGen.current += 1;
        closingRef.current = false;
        paint("shown");
        idleSince.current = now;
        showIsland();
      }
      const ready = settingsRef.current !== null;
      const autoHide = ready && settingsRef.current?.autoHide !== false;
      if (shouldResetIdle(sawReadyRef.current, prevAutoHideRef.current, ready, autoHide)) {
        idleSince.current = now;
      }
      if (ready) {
        sawReadyRef.current = true;
        prevAutoHideRef.current = autoHide;
      }
      let windowVisible = phaseRef.current !== "hidden";
      if (phaseRef.current === "hidden" || gameFsRef.current) {
        try {
          windowVisible = await getCurrentWindow().isVisible();
        } catch {
          windowVisible = true;
        }
        now = performance.now();
      }
      const hover = pointerOnIsland();
      hoveredRef.current = hover;
      const step = stepHide({
        ready,
        autoHide,
        phase: phaseRef.current,
        windowVisible,
        hover,
        remind: !!remindRef.current,
        hint: !!campusHintRef.current,
        login: silentLoginRef.current,
        fullscreen: gameFsRef.current,
        closing: closingRef.current,
        idleMs: idleSince.current ? now - idleSince.current : 0,
        stowMs: stowSince.current ? now - stowSince.current : 0,
      });
      if (step.kind === "wait") {
        if (step.resetIdle) idleSince.current = now;
        else if (!idleSince.current && ready && phaseRef.current === "shown" && !gameFsRef.current) idleSince.current = now;
        return;
      }
      if (step.kind === "begin-stow") {
        stowSince.current = now;
        paint("stowing");
        return;
      }
      if (step.kind === "cancel-stow") {
        hideGen.current += 1;
        closingRef.current = false;
        paint("shown");
        idleSince.current = now;
        void stopHoverWatch();
        return;
      }
      if (step.kind === "sync-shown") {
        hideGen.current += 1;
        closingRef.current = false;
        paint("shown");
        idleSince.current = now;
        void stopHoverWatch();
        return;
      }
      if (step.kind === "show") {
        hideGen.current += 1;
        closingRef.current = false;
        paint("shown");
        idleSince.current = now;
        void stopHoverWatch();
        showIsland();
        return;
      }
      if (step.kind === "fullscreen-hide") {
        void hideForFullscreen();
        return;
      }
      if (step.kind === "finish-hide") void finishHide();
    } finally {
      busyRef.current = false;
    }
  };

  revealRef.current = reveal;
  armHideRef.current = armHide;
  cancelStowRef.current = cancelStow;
  tickRef.current = () => { void tick(); };
  hideFsRef.current = () => { void hideForFullscreen(); };

  remindSoundRef.current = settings?.remindSound === true;
  remindVolumeRef.current = clampVolume(settings?.remindVolume) / 100;

  const startRemind = (course: Course & { kind?: "class" | "event" }) => {
    if (remindSoundRef.current) playRemindSound(remindVolumeRef.current);
    const next = { name: course.name, start: course.start, kind: course.kind || "class" as const };
    const wasHidden = phaseRef.current === "hidden";
    remindRef.current = next;
    preRemindRef.current = { hidden: wasHidden };
    reveal();
    setRemind(next);
    window.clearTimeout(remindTimer.current);
    remindTimer.current = window.setTimeout(() => {
      remindRef.current = null;
      preRemindRef.current = null;
      setRemind(null);
      if (wasHidden && autoHideRef.current) armHideRef.current();
    }, 10000);
  };
  startRemindRef.current = startRemind;

  useEffect(() => {
    const unsubs: Array<() => void> = [];
    listen<Snapshot>("snapshot-updated", (e) => {
      setSnap(e.payload);
      silentLoginRef.current = false;
      setSilentLogin(false);
      setRefreshUi((u) => (u === "spin" ? "ok" : u));
    }).then((f) => unsubs.push(f));
    listen<AppSettings>("settings-updated", (e) => {
      setSettings(withSettingDefaults(e.payload));
    }).then((f) => unsubs.push(f));
    listen<string>("auth-status", (e) => {
      const p = e.payload;
      if (p === "logging") {
        silentLoginRef.current = true;
        setSilentLogin(true);
        setRefreshUi("idle");
        if (phaseRef.current !== "hidden") cancelStowRef.current();
      } else if (p === "syncing") {
        silentLoginRef.current = true;
        setSilentLogin(true);
        setRefreshUi("spin");
        if (phaseRef.current !== "hidden") cancelStowRef.current();
      } else if (p === "ok" || p === "idle" || p === "need_sms") {
        silentLoginRef.current = false;
        setSilentLogin(false);
        if (p !== "ok") setRefreshUi("idle");
      } else if (typeof p === "string" && p.startsWith("fail:")) {
        silentLoginRef.current = false;
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
      if (!campusHintRef.current) preHintHidden.current = phaseRef.current === "hidden";
      campusHintRef.current = text;
      setCampusHint(text);
      if (!gameFsRef.current) revealRef.current();
      if (!text.startsWith("正在")) {
        hintTimer.current = window.setTimeout(() => {
          campusHintRef.current = "";
          setCampusHint("");
          if (preHintHidden.current && autoHideRef.current) armHideRef.current();
        }, 4000);
      }
    }).then((f) => unsubs.push(f));
    listen("island-reveal", () => {
      revealRef.current();
    }).then((f) => unsubs.push(f));
    listen("hotkey-reveal", () => {
      if (gameFsRef.current) return;
      revealRef.current();
    }).then((f) => unsubs.push(f));
    listen<boolean>("exclusive-fullscreen", (e) => {
      const on = !!e.payload;
      if (on) {
        hiddenBeforeFs.current = phaseRef.current === "hidden";
        gameFsRef.current = true;
        hideFsRef.current();
        return;
      }
      gameFsRef.current = false;
      if (!hiddenBeforeFs.current) revealRef.current();
    }).then((f) => unsubs.push(f));
    listen<boolean>("hover-zone", (e) => {
      window.clearTimeout(hoverRevealTimer.current);
      if (!e.payload || gameFsRef.current) return;
      if (phaseRef.current !== "hidden" || !autoHideHoverRef.current) return;
      hoverRevealTimer.current = window.setTimeout(() => {
        if (phaseRef.current !== "hidden" || !autoHideHoverRef.current || gameFsRef.current) return;
        revealRef.current();
      }, 1000);
    }).then((f) => unsubs.push(f));
    listen("island-reset", () => {
      window.clearTimeout(hoverRevealTimer.current);
      window.clearTimeout(remindTimer.current);
      remindRef.current = null;
      preRemindRef.current = null;
      setRemind(null);
      revealRef.current();
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
      cancelStowRef.current();
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

  // 不挂在设置、提醒、同步这些依赖上。那些一变，旧的 setTimeout 链会被拆掉，拆掉之后没人再装回去。
  useEffect(() => {
    const id = window.setInterval(() => {
      tickRef.current();
    }, HIDE_POLL_MS);
    tickRef.current();
    const onVisible = () => {
      if (document.visibilityState !== "visible") return;
      if (gameFsRef.current || phaseRef.current !== "hidden") return;
      hideGen.current += 1;
      closingRef.current = false;
      phaseRef.current = "shown";
      stowSince.current = 0;
      idleSince.current = performance.now();
      setStowing(false);
      setHidden(false);
      void stopHoverWatch();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.clearInterval(id);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, []);

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
    cancelStowRef.current();
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
    cancelStowRef.current();
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
