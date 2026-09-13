import { invoke } from "@tauri-apps/api/core";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { AppSettings, LoginOutcome, SavedCreds, Snapshot } from "./types";

export const api = {
  bootstrap: () => invoke<Snapshot>("bootstrap"),
  clockNow: () => invoke<{ iso: string; overridden: boolean }>("clock_now"),
  settings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (settings: AppSettings) => invoke<AppSettings>("save_settings", { settings }),
  savedCreds: () => invoke<SavedCreds>("saved_creds"),
  savedPassword: () => invoke<string | null>("saved_password"),
  persistIsland: () => invoke<void>("persist_island"),
  login: (username: string, password: string, rememberPassword: boolean, autoLogin: boolean) =>
    invoke<LoginOutcome>("login", { username, password, rememberPassword, autoLogin }),
  refreshCaptcha: () => invoke<[string, string | null]>("refresh_captcha"),
  sendSms: (captcha: string) => invoke<void>("send_sms", { captcha }),
  cancelSms: () => invoke<void>("cancel_sms"),
  submitSms: (sms: string, trust: boolean) => invoke<LoginOutcome>("submit_sms", { sms, trust }),
  refresh: () => invoke<Snapshot>("refresh"),
  loadMonth: (year: number, month: number) => invoke<Snapshot>("load_month", { year, month }),
  syncCourses: () => invoke<Snapshot>("sync_courses"),
  logout: () => invoke<void>("logout"),
  resetIsland: () => invoke<void>("reset_island"),
  raiseTopmost: () => invoke<void>("raise_topmost"),
  setHoverWatch: (watch: { enabled: boolean; x: number; y: number; width: number; height: number }) =>
    invoke<void>("set_hover_watch", { watch }),
  setBillsOpen: (open: boolean) => invoke<void>("set_bills_open", { open }),
};

export async function setExpanded(expanded: boolean, height?: number) {
  const w = getCurrentWindow();
  const h = expanded ? Math.min(560, Math.max(40, Math.ceil(height ?? 560))) : 40;
  await w.setSize(new LogicalSize(420, h));
  try {
    await invoke("set_expanded", { expanded, height: h });
  } catch {
    /* size already set */
  }
}

export async function focusIsland() {
  try {
    await getCurrentWindow().setFocus();
  } catch {
    /* ignore */
  }
}
