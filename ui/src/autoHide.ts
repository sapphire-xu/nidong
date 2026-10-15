/** 岛露出后，无人使用再过多久收起。 */
export const HIDE_AFTER_MS = 3000;
/** 向上折起的动画时长。折完才许 hide()。 */
export const STOW_MS = 320;
/** 收起状态机的巡检间隔。不靠一条会断的 setTimeout 链。 */
export const HIDE_POLL_MS = 200;

export type HidePhase = "shown" | "stowing" | "hidden";

export type HideView = {
  /** 设置还没读到时不要动窗口。更新后第一次启动会先经过这里。 */
  ready: boolean;
  autoHide: boolean;
  phase: HidePhase;
  /** 系统报告窗口可见。phase 说已经藏起时，以这个为准。 */
  windowVisible: boolean;
  hover: boolean;
  remind: boolean;
  hint: boolean;
  login: boolean;
  fullscreen: boolean;
  /** finish-hide 进行中，不要再发一次收起。 */
  closing: boolean;
  idleMs: number;
  stowMs: number;
};

export type HideStep =
  | { kind: "wait"; resetIdle: boolean }
  | { kind: "show" }
  | { kind: "sync-shown" }
  | { kind: "begin-stow" }
  | { kind: "cancel-stow" }
  | { kind: "finish-hide" }
  | { kind: "fullscreen-hide" };

function blocked(s: HideView): boolean {
  return s.hover || s.remind || s.hint || s.login;
}

/**
 * 设置刚读到，或自动隐藏从关到开，重新给满 3 秒。
 * 更新后第一次启动会先经过「还没读到设置」。
 */
export function shouldResetIdle(
  prevReady: boolean,
  prevAutoHide: boolean | null,
  ready: boolean,
  autoHide: boolean,
): boolean {
  if (!prevReady && ready) return true;
  if (ready && prevAutoHide === false && autoHide) return true;
  return false;
}

/**
 * 自动隐藏的唯一下一步。
 * 停机不再存在：关着就维持露出，藏着就盯住窗口是否又被显示出来。
 */
export function stepHide(s: HideView): HideStep {
  if (!s.ready || s.closing) return { kind: "wait", resetIdle: false };
  if (s.fullscreen) {
    if (s.windowVisible) return { kind: "fullscreen-hide" };
    return { kind: "wait", resetIdle: false };
  }
  if (!s.autoHide) {
    if (s.phase !== "shown" || !s.windowVisible) return { kind: "show" };
    return { kind: "wait", resetIdle: false };
  }
  if (s.phase === "hidden") {
    if (s.windowVisible) return { kind: "sync-shown" };
    return { kind: "wait", resetIdle: false };
  }
  if (s.phase === "stowing") {
    if (blocked(s) || s.stowMs > 2000) return { kind: "cancel-stow" };
    if (s.stowMs >= STOW_MS) return { kind: "finish-hide" };
    return { kind: "wait", resetIdle: false };
  }
  if (blocked(s)) return { kind: "wait", resetIdle: true };
  if (s.idleMs < HIDE_AFTER_MS) return { kind: "wait", resetIdle: false };
  return { kind: "begin-stow" };
}
