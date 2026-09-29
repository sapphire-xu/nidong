import type { Bill } from "./types";

export const fmtMoney = (n: number) => n.toFixed(2);
export const fmtElec = (n: number) => String(Math.round(n));

export function pad(n: number) {
  return String(n).padStart(2, "0");
}

export function ymd(d: Date) {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export function formatBytes(n: number) {
  if (n >= 1024 ** 3) return (n / 1024 ** 3).toFixed(1) + " GB";
  if (n >= 1024 ** 2) return Math.round(n / 1024 ** 2) + " MB";
  if (n >= 1024) return Math.round(n / 1024) + " KB";
  return n + " B";
}

export function placeLabel(p?: string) {
  const t = (p || "").trim();
  if (!t || t === "浑南校区" || t === "南湖校区" || t === "浑南" || t === "南湖") {
    return "地点未定";
  }
  return t;
}

export function isJunkBill(b: Pick<Bill, "subject" | "amount" | "station">) {
  const sub = (b.subject || "").trim();
  const st = b.station || "";
  if (st.includes("物联网采集") && Math.abs(b.amount) < 0.005) return true;
  if (/^\d{4}\s*[-/年.]\s*\d{1,2}\s*[-/月.]\s*\d{1,2}/.test(sub)) return true;
  if (Math.abs(b.amount) < 0.005 && /\d{1,2}:\d{2}:\d{2}/.test(sub)) return true;
  return false;
}
