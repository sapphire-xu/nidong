import { invoke } from "@tauri-apps/api/core";
import type { Agenda, AppSettings, LoginOutcome, PlatformInfo, SavedCreds, Snapshot } from "./types";

export const api = {
  bootstrap: () => invoke<Snapshot>("bootstrap"),
  platformInfo: () => invoke<PlatformInfo>("platform_info"),
  desktopNotify: (title: string, body: string) => invoke<void>("desktop_notify", { title, body }),
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
  meterStatus: () => invoke<MeterStatus>("meter_status"),
  publicIpv4: () => invoke<string>("public_ipv4"),
  meterLog: () => invoke<MeterDay[]>("meter_log"),
  meterEnable: () => invoke<void>("meter_enable"),
  meterDisable: () => invoke<void>("meter_disable"),
  meterOpenLog: () => invoke<void>("meter_open_log"),
  fetchStatus: () => invoke<FetchStatus>("fetch_status"),
  fetchArm: (urls: string, time: string, dir: string, shutdown: boolean, useProxy: boolean) =>
    invoke<FetchStatus>("fetch_arm", { urls, time, dir, shutdown, useProxy }),
  fetchCancel: () => invoke<FetchStatus>("fetch_cancel"),
  fetchResume: () => invoke<FetchStatus>("fetch_resume"),
  pluginList: () => invoke<PluginInfo[]>("plugin_list"),
  pluginInstall: (path: string) => invoke<PluginInfo>("plugin_install", { path }),
  pluginToggle: (pluginId: string, enabled: boolean) =>
    invoke<PluginInfo[]>("plugin_toggle", { pluginId, enabled }),
  pluginUninstall: (pluginId: string, removeAgenda: boolean) =>
    invoke<PluginInfo[]>("plugin_uninstall", { pluginId, removeAgenda }),
  pluginLoadHtml: (pluginId: string, theme: string) =>
    invoke<string>("plugin_load_html", { pluginId, theme }),
  pluginSetBadge: (pluginId: string, badge: string | null) =>
    invoke<void>("plugin_set_badge", { pluginId, badge }),
  pluginStorageGet: (pluginId: string, key: string) =>
    invoke<unknown>("plugin_storage_get", { pluginId, key }),
  pluginStorageSet: (pluginId: string, key: string, value: unknown) =>
    invoke<void>("plugin_storage_set", { pluginId, key, value }),
  pluginAgendaList: (pluginId: string) =>
    invoke<Agenda[]>("plugin_agenda_list", { pluginId }),
  pluginAgendaUpsert: (pluginId: string, item: Agenda) =>
    invoke<Agenda[]>("plugin_agenda_upsert", { pluginId, item }),
  pluginAgendaDelete: (pluginId: string, itemId: string) =>
    invoke<Agenda[]>("plugin_agenda_delete", { pluginId, itemId }),
  pluginIslandNotify: (pluginId: string, kind: "hint" | "remind", text: string, start?: string) =>
    invoke<void>("plugin_island_notify", { pluginId, kind, text, start }),
  pluginHttpFetch: (pluginId: string, req: PluginHttpRequest) =>
    invoke<PluginHttpResponse>("plugin_http_fetch", { pluginId, req }),
  pluginGetCookies: (pluginId: string) =>
    invoke<Record<string, string>>("plugin_get_cookies", { pluginId }),
  pluginSetCookies: (pluginId: string, cookies: unknown) =>
    invoke<Record<string, string>>("plugin_set_cookies", { pluginId, cookies }),
  pluginOpenAuthWindow: (
    pluginId: string,
    url: string,
    username?: string,
    password?: string
  ) => invoke<void>("plugin_open_auth_window", { pluginId, url, username, password }),
  pluginMcpListTools: (pluginId: string) =>
    invoke<unknown>("plugin_mcp_list_tools", { pluginId }),
  pluginMcpCall: (pluginId: string, tool: string, args: unknown) =>
    invoke<unknown>("plugin_mcp_call", { pluginId, tool, arguments: args }),
  pluginAppendLog: (pluginId: string, line: string) =>
    invoke<void>("plugin_append_log", { pluginId, line }),
  pluginOpenLog: (pluginId: string) =>
    invoke<void>("plugin_open_log", { pluginId }),
  pluginOpenExternal: (pluginId: string, url: string) =>
    invoke<void>("plugin_open_external", { pluginId, url }),
};

export type McpConfig = {
  command: string;
  args?: string[];
  env?: Record<string, string>;
};

export type PluginManifest = {
  id: string;
  name: string;
  version: string;
  author?: string;
  description?: string;
  runtime: "webview" | "mcp" | "hybrid" | string;
  entry?: string | null;
  background?: boolean;
  permissions?: string[];
  mcp?: McpConfig | null;
};

export type PluginInfo = {
  manifest: PluginManifest;
  enabled: boolean;
  hasUi: boolean;
  statusBadge?: string | null;
};

export type PluginHttpRequest = {
  url: string;
  method?: string;
  headers?: Record<string, string>;
  body?: string;
};

export type PluginHttpResponse = {
  status: number;
  headers: Record<string, string>;
  body: string;
  cookies?: Record<string, string>;
};

export type ShareFile = { id?: number; path: string; name: string; bytes: number };

export type ShareDescribe = { files: ShareFile[]; note?: string | null };

export type MeterTotals = {
  v4Day: number;
  v4Night: number;
  v4Up: number;
  v6Down: number;
  v6Up: number;
};

export type MeterStatus = {
  supported?: boolean;
  enabled: boolean;
  running: boolean;
  onCampus: boolean;
  ipv4: string;
  ipv6: string;
  v4DownBps: number;
  v6DownBps: number;
  v4DownTcpBps: number;
  v4DownUdpBps: number;
  v4DownCampusBps: number;
  v6DownTcpBps: number;
  v6DownUdpBps: number;
  v6DownCampusBps: number;
  v4UpBps: number;
  v6UpBps: number;
  night: boolean;
  elevated: boolean;
  udpReady: boolean;
  udpGap: boolean;
  today: MeterTotals | null;
  month: MeterTotals | null;
  error: string;
};

export type MeterDay = { date: string; totals: MeterTotals };

export type FetchFile = {
  url: string;
  name: string;
  phase: string;
  received: number;
  total?: number | null;
  error: string;
};

export type FetchStatus = {
  phase: string;
  when: string;
  dir: string;
  shutdown: boolean;
  useProxy: boolean;
  proxy: string;
  proxyKind: string;
  note: string;
  links: string;
  files: FetchFile[];
};

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
