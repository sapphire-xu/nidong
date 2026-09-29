import type { AppSettings } from "./types";

export function withSettingDefaults(s: AppSettings): AppSettings {
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

export function resolveTheme(theme: string | undefined) {
  if (theme === "light" || theme === "dark") return theme;
  if (typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: light)").matches) {
    return "light";
  }
  return "dark";
}
