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
  refreshCaptcha: () => invoke<string>("refresh_captcha"),
  sendSms: (captcha: string) => invoke<void>("send_sms", { captcha }),
  cancelSms: () => invoke<void>("cancel_sms"),
  submitSms: (sms: string, trust: boolean) => invoke<LoginOutcome>("submit_sms", { sms, trust }),
  refresh: () => invoke<Snapshot>("refresh"),
  loadMonth: (year: number, month: number) => invoke<Snapshot>("load_month", { year, month }),
  syncCourses: () => invoke<Snapshot>("sync_courses"),
  saveAgenda: (item: Agenda) => invoke<Snapshot>("save_agenda", { item }),
  deleteAgenda: (id: string) => invoke<Snapshot>("delete_agenda", { id }),
  logout: (keepAgenda: boolean) => invoke<void>("logout", { keepAgenda }),
  raiseTopmost: () => invoke<void>("raise_topmost"),
  setHoverWatch: (watch: { enabled: boolean; x: number; y: number; width: number; height: number }) =>
    invoke<void>("set_hover_watch", { watch }),
  setBillsOpen: (open: boolean) => invoke<void>("set_bills_open", { open }),
  openDesk: () => invoke<void>("open_desk"),
  openDoor: (url: string, policy: "direct" | "intranet" | "campus-only") =>
    invoke<void>("open_door", { url, policy }),
  previewRemind: () => invoke<void>("preview_remind"),
  currentWifi: () => invoke<string>("current_wifi"),
  openFeedback: () => invoke<void>("open_feedback"),
  shareDescribe: (paths: string[]) => invoke<ShareDescribe>("share_describe", { paths }),
  shareStart: (paths: string[]) => invoke<ShareOffer>("share_start", { paths }),
  shareStop: () => invoke<void>("share_stop"),
  shareOffer: () => invoke<ShareOffer | null>("share_offer"),
  shareRevise: (paths: string[]) => invoke<ShareOffer>("share_revise", { paths }),
  shareOnCampus: () => invoke<boolean>("share_on_campus"),
};

export type ShareFile = { id?: number; path: string; name: string; bytes: number };

export type ShareDescribe = { files: ShareFile[]; note?: string | null };

export type ShareOffer = {
  url: string;
  code: string;
  host: string;
  port: number;
  files: ShareFile[];
  unlocks: number;
  activeDownloads: number;
  lastPeer?: string | null;
};
