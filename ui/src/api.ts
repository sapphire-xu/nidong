import { invoke } from "@tauri-apps/api/core";
import type { Agenda, AppSettings, LoginOutcome, SavedCreds, Snapshot } from "./types";

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
  saveAgenda: (item: Agenda) => invoke<Snapshot>("save_agenda", { item }),
  deleteAgenda: (id: string) => invoke<Snapshot>("delete_agenda", { id }),
  logout: () => invoke<void>("logout"),
  raiseTopmost: () => invoke<void>("raise_topmost"),
  setHoverWatch: (watch: { enabled: boolean; x: number; y: number; width: number; height: number }) =>
    invoke<void>("set_hover_watch", { watch }),
  setBillsOpen: (open: boolean) => invoke<void>("set_bills_open", { open }),
  openDesk: () => invoke<void>("open_desk"),
  openDoor: (url: string, policy: "direct" | "intranet" | "campus-only") =>
    invoke<void>("open_door", { url, policy }),
  previewRemind: () => invoke<void>("preview_remind"),
};
