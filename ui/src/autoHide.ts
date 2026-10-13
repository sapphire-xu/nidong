/** 岛露出后，无人使用再过多久收起。 */
export const HIDE_AFTER_MS = 3000;

export type HideTick =
  | { kind: "stop" }
  | { kind: "wait"; ms: number; resetIdle: boolean }
  | { kind: "hide" };

/**
 * 下一次自动隐藏该怎么排。
 * `hover` 必须是这一刻指针是否在岛上，不能传卡住的 mouseenter 标记。
 * 挡住时返回 wait，而不是停机；停机只有自动隐藏关着或岛已经藏起。
 */
export function nextHideTick(s: {
  autoHide: boolean;
  hidden: boolean;
  hover: boolean;
  remind: boolean;
  hint: boolean;
  login: boolean;
  stowing: boolean;
  idleMs: number;
}): HideTick {
  if (!s.autoHide || s.hidden) return { kind: "stop" };
  if (s.stowing) return { kind: "wait", ms: 200, resetIdle: true };
  if (s.hover || s.remind || s.hint || s.login) return { kind: "wait", ms: 250, resetIdle: true };
  if (s.idleMs < HIDE_AFTER_MS) return { kind: "wait", ms: HIDE_AFTER_MS - s.idleMs, resetIdle: false };
  return { kind: "hide" };
}

/** 岛已经露着时再次叫出，也要重新算 3 秒。只清掉计时、不改 hidden，效果不会再跑。 */
export function revealShouldRearm(autoHide: boolean, hidden: boolean): boolean {
  return autoHide && !hidden;
}
