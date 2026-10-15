import type { Agenda, Course, RepetRule } from "./types";
import type { CourseKind } from "./course";

const BYDAY = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"] as const;
const BYDAY_LABEL: Record<string, string> = {
  Mo: "周一",
  Tu: "周二",
  We: "周三",
  Th: "周四",
  Fr: "周五",
  Sa: "周六",
  Su: "周日",
};

export function parseAgenda(s: string): Date | null {
  const t = (s || "").trim().replace(" ", "T");
  if (t.length < 16) return null;
  const d = new Date(t.length === 16 ? `${t}:00` : t);
  return Number.isNaN(d.getTime()) ? null : d;
}

function pad2(n: number) {
  return String(n).padStart(2, "0");
}

export function ymdOf(d: Date): string {
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
}

function stampOf(d: Date): string {
  return `${ymdOf(d)}T${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
}

function covers(start: string, end: string, ymd: string): boolean {
  const s = (start || "").slice(0, 10);
  const e = (end || "").slice(0, 10);
  return !!s && !!e && ymd >= s && ymd <= e;
}

type Span = { start: Date; end: Date };

/** 开始日和结束日不是同一天。这种日程不允许再叠加重复。 */
export function repeatSpansDays(start: string, end: string): boolean {
  const s = (start || "").slice(0, 10);
  const e = (end || "").slice(0, 10);
  return s.length === 10 && e.length === 10 && s !== e;
}

function ruleOf(a: Agenda): RepetRule | null {
  const freq = a.repetRule?.freq;
  if (!a.isRepet || (freq !== "DAILY" && freq !== "WEEKLY")) return null;
  if (a.repetRule?.endmode !== "DATE" && a.repetRule?.endmode !== "NUMBER") return null;
  // 跨天重复展开会重叠、漏天。非法组合不当成系列，只留原来那一段。
  if (repeatSpansDays(a.start, a.end)) return null;
  return a.repetRule || null;
}

function matches(day: Date, seriesStart: Date, rule: RepetRule): boolean {
  if (ymdOf(day) < ymdOf(seriesStart)) return false;
  if (rule.freq === "DAILY") return true;
  if (rule.freq === "WEEKLY") return (rule.byday || []).includes(BYDAY[day.getDay()]);
  return false;
}

/** 从系列起点数到 `untilYmd`（含）为止的发生。次数和结束日都到此封顶。 */
function collectUntil(a: Agenda, untilYmd: string): Span[] {
  const seriesStart = parseAgenda(a.start);
  const seriesEnd = parseAgenda(a.end);
  if (!seriesStart || !seriesEnd) return [];
  const dur = seriesEnd.getTime() - seriesStart.getTime();
  if (dur <= 0) return [];
  const rule = ruleOf(a);
  if (!rule) return [{ start: seriesStart, end: seriesEnd }];
  const numbered = rule.endmode === "NUMBER";
  const maxN = numbered ? Math.min(999, Math.max(0, Math.floor(Number(rule.num) || 0))) : 4000;
  const endDay = !numbered && rule.endmode === "DATE" ? (rule.time || "").slice(0, 10) : "";
  if (!maxN || (rule.endmode === "DATE" && endDay.length < 10)) return [];
  const out: Span[] = [];
  const cursor = new Date(seriesStart);
  cursor.setHours(12, 0, 0, 0);
  let guard = 0;
  while (guard++ < 12000 && out.length < maxN) {
    const day = ymdOf(cursor);
    if (day > untilYmd) break;
    if (endDay && day > endDay) break;
    if (matches(cursor, seriesStart, rule)) {
      const start = new Date(cursor);
      start.setHours(seriesStart.getHours(), seriesStart.getMinutes(), 0, 0);
      out.push({ start, end: new Date(start.getTime() + dur) });
    }
    cursor.setDate(cursor.getDate() + 1);
  }
  return out;
}

export function occurrencesOnDate(a: Agenda, ymd: string): { start: string; end: string }[] {
  return collectUntil(a, ymd)
    .filter((s) => covers(stampOf(s.start), stampOf(s.end), ymd))
    .map((s) => ({ start: stampOf(s.start), end: stampOf(s.end) }));
}

export function repeatText(a: Agenda | undefined): string {
  const rule = a ? ruleOf(a) : null;
  if (!a || !rule) return "";
  const freq = rule.freq === "DAILY" ? "每天" : rule.freq === "WEEKLY" ? "按星期" : "";
  let detail = "";
  if (rule.freq === "WEEKLY") {
    const order = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
    detail = [...(rule.byday || [])].sort((x, y) => order.indexOf(x) - order.indexOf(y)).map((d) => BYDAY_LABEL[d] || d).join("、");
  }
  let end = "";
  if (rule.endmode === "DATE" && rule.time) end = `到 ${rule.time.slice(0, 10)}`;
  if (rule.endmode === "NUMBER" && rule.num) end = `${rule.num} 次`;
  return [freq, detail, end].filter(Boolean).join(" ");
}

export function whenText(a: Agenda): string {
  const s = (a.occStart || a.start).slice(11, 16);
  const e = (a.occEnd || a.end).slice(11, 16);
  return `${s}–${e}`;
}

export function hmOf(s: string): string {
  const p = (s || "").replace("T", " ");
  return p.slice(11, 16);
}

export function agendaOnDate(a: Agenda, ymd: string): boolean {
  return occurrencesOnDate(a, ymd).length > 0;
}

export function agendasOnDate(items: Agenda[] | undefined, ymd: string): Agenda[] {
  const hits: Agenda[] = [];
  for (const a of items || []) {
    for (const occ of occurrencesOnDate(a, ymd)) {
      hits.push({ ...a, occStart: occ.start, occEnd: occ.end });
    }
  }
  return hits.sort((a, b) => (a.occStart || a.start).localeCompare(b.occStart || b.start) || a.name.localeCompare(b.name));
}

export function upcomingAgenda(items: Agenda[], now: Date): { item: Agenda; start: string; end: string } | null {
  let best: { item: Agenda; start: string; end: string } | null = null;
  let bestUntil = Infinity;
  const horizon = new Date(now.getTime() + 3 * 86400000);
  const until = ymdOf(horizon);
  for (const a of items) {
    if (!a.remind) continue;
    const mins = Math.max(1, Math.floor(Number(a.remindMinutes) || 15));
    for (const occ of collectUntil(a, until)) {
      if (occ.start.getTime() <= now.getTime()) continue;
      const untilMin = (occ.start.getTime() - now.getTime()) / 60000;
      if (untilMin <= mins && untilMin < bestUntil) {
        bestUntil = untilMin;
        best = { item: a, start: stampOf(occ.start), end: stampOf(occ.end) };
      }
      break;
    }
  }
  return best;
}

export type MixRow = {
  key: string;
  type: "course" | "event";
  kind: CourseKind;
  label: string;
  start: string;
  end: string;
  name: string;
  meta: string;
  course?: Course;
  event?: Agenda;
};

function minutesBetween(from: Date, to: Date): number {
  return Math.max(0, Math.round((to.getTime() - from.getTime()) / 60000));
}

function hmOnDay(hhmm: string, day: Date): Date {
  const [h, m] = hhmm.split(":").map(Number);
  const d = new Date(day);
  d.setHours(h || 0, m || 0, 0, 0);
  return d;
}

export function mixDay(courses: Course[], events: Agenda[], now: Date, day = now): MixRow[] {
  type Raw = {
    type: "course" | "event";
    start: Date;
    end: Date;
    displayStart: string;
    displayEnd: string;
    name: string;
    meta: string;
    course?: Course;
    event?: Agenda;
    key: string;
  };
  const raw: Raw[] = [];
  for (const c of courses) {
    raw.push({
      type: "course",
      start: hmOnDay(c.start, day),
      end: hmOnDay(c.end, day),
      displayStart: c.start,
      displayEnd: c.end,
      name: c.name,
      meta: [c.teacher, c.place].filter((s) => s && s.trim()).join(" · "),
      course: c,
      key: `c-${c.name}-${c.start}-${(c.weeks || []).join(",")}-${c.place}`,
    });
  }
  for (const e of events) {
    const start = parseAgenda(e.occStart || e.start);
    const end = parseAgenda(e.occEnd || e.end);
    if (!start || !end) continue;
    raw.push({
      type: "event",
      start,
      end,
      displayStart: hmOf(e.occStart || e.start),
      displayEnd: hmOf(e.occEnd || e.end),
      name: e.name,
      meta: [repeatText(e), e.location, e.note].map((s) => (s || "").trim()).filter(Boolean).join(" · "),
      event: e,
      key: `e-${e.id}-${e.occStart || e.start}`,
    });
  }
  raw.sort((a, b) => a.start.getTime() - b.start.getTime() || a.name.localeCompare(b.name));
  const next = raw.find((r) => now < r.start);
  return raw.map((r) => {
    let kind: CourseKind = "later";
    let label = "";
    if (now >= r.end) {
      kind = "past";
      label = "已结束";
    } else if (now >= r.start) {
      kind = "now";
      label = `还剩 ${minutesBetween(now, r.end)} 分钟`;
    } else if (next && next.key === r.key) {
      kind = "next";
      const m = minutesBetween(now, r.start);
      label = r.type === "event" ? `即将开始 · ${m} 分钟` : `下一节 · ${m} 分钟`;
    }
    return {
      key: r.key,
      type: r.type,
      kind,
      label,
      start: r.displayStart,
      end: r.displayEnd,
      name: r.name,
      meta: r.meta,
      course: r.course,
      event: r.event,
    };
  });
}

export function weekdayLabel(dow: number): string {
  return ["", "一", "二", "三", "四", "五", "六", "日"][dow] || "";
}
