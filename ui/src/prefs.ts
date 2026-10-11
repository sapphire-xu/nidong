import type { AppSettings } from "./types";

const WIFI_MODES = ["off", "offline", "boot", "hotspot", "manual"] as const;

export function withSettingDefaults(s: AppSettings): AppSettings {
  const mode = WIFI_MODES.find((m) => m === s.campusWifiMode) || "off";
  return {
    ...s,
    autoHide: s.autoHide !== false,
    autoHideHotkey: s.autoHideHotkey !== false,
    autoHideHover: s.autoHideHover !== false,
    revealHotkey: s.revealHotkey === "Tab+Q" || !s.revealHotkey ? "Ctrl+Q" : s.revealHotkey,
    classRemind: s.classRemind !== false,
    classRemindMinutes: Math.max(1, Math.floor(Number(s.classRemindMinutes) || 15)),
    campusWifiMode: mode,
    campusWifiSsid: s.campusWifiSsid || "",
    campusHotspotSsid: s.campusHotspotSsid || "",
    skipLogin: s.skipLogin === true,
    shareHintDismissed: s.shareHintDismissed === true,
  };
}

export function resolveTheme(theme: string | undefined) {
  if (theme === "light" || theme === "dark") return theme;
  if (typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: light)").matches) {
    return "light";
  }
  return "dark";
}
