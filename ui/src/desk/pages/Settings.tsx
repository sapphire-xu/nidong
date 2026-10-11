import { SettingsPanel } from "../../Settings";
import type { AppSettings, SemesterTable } from "../../types";

export function SettingsPage(props: {
  settings: AppSettings;
  setSettings: (s: AppSettings) => void;
  semester?: SemesterTable;
  cacheLabel: string;
  courseSync: "idle" | "spin" | "ok" | "fail";
  courseSyncErr?: string;
  offline?: boolean;
  onSync: () => void;
  onLogout: (keepAgenda: boolean) => void;
  onPreviewRemind: () => void;
  loggedIn: boolean;
  onRequestLogin: () => void;
}) {
  return <SettingsPanel {...props} />;
}
