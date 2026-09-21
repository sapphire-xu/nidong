import type { Course, SemesterTable } from "./types";

export type Campus = "nanhu" | "hunnan";

export const BLOCKS: { id: string; label: string; sections: [number, number] }[] = [
  { id: "1-2", label: "1–2", sections: [1, 2] },
  { id: "3-4", label: "3–4", sections: [3, 4] },
  { id: "5-6", label: "5–6", sections: [5, 6] },
  { id: "7-8", label: "7–8", sections: [7, 8] },
  { id: "9-10", label: "9–10", sections: [9, 10] },
  { id: "11-12", label: "11–12", sections: [11, 12] },
];

const PERIODS: Record<Campus, Record<number, [string, string]>> = {
  nanhu: {
    1: ["08:00", "08:45"],
    2: ["08:55", "09:40"],
    3: ["10:00", "10:45"],
    4: ["10:55", "11:40"],
    5: ["14:00", "14:45"],
    6: ["14:55", "15:40"],
    7: ["16:00", "16:45"],
    8: ["16:55", "17:40"],
    9: ["18:30", "19:15"],
    10: ["19:25", "20:10"],
    11: ["20:20", "21:05"],
    12: ["21:15", "22:00"],
  },
  hunnan: {
    1: ["08:30", "09:15"],
    2: ["09:25", "10:10"],
    3: ["10:30", "11:15"],
    4: ["11:25", "12:10"],
    5: ["14:00", "14:45"],
    6: ["14:55", "15:40"],
    7: ["16:00", "16:45"],
    8: ["16:55", "17:40"],
    9: ["18:30", "19:15"],
    10: ["19:25", "20:10"],
    11: ["20:20", "21:05"],
    12: ["21:15", "22:00"],
  },
};

export function parseCampus(s?: string): Campus | undefined {
  if (!s) return undefined;
  if (s.includes("hunnan") || s.includes("浑南")) return "hunnan";
  if (s.includes("nanhu") || s.includes("南湖")) return "nanhu";
  return undefined;
}

export function blockTime(campus: Campus, id: string): string {
  const b = BLOCKS.find((x) => x.id === id);
  if (!b) return "";
  const a = PERIODS[campus][b.sections[0]];
  const z = PERIODS[campus][b.sections[1]];
  return a && z ? `${a[0]}–${z[1]}` : "";
}

export function courseCampus(c: Course): Campus {
  return parseCampus(c.campus) || "hunnan";
}

export function inWeek(c: Course, week: number): boolean {
  if (!c.weeks || c.weeks.length === 0) return true;
  return c.weeks.includes(week);
}

export function majorityCampus(courses: Course[], week?: number): Campus {
  let hunnan = 0;
  let nanhu = 0;
  for (const c of courses) {
    if (week != null && !inWeek(c, week)) continue;
    const camp = parseCampus(c.campus);
    if (camp === "hunnan") hunnan++;
    else if (camp === "nanhu") nanhu++;
  }
  return hunnan >= nanhu ? "hunnan" : "nanhu";
}

export function datesOfTeachingWeek(week1Monday: string, week: number): Date[] {
  const p = week1Monday.slice(0, 10).split("-").map(Number);
  if (p.length < 3 || !p[0] || week < 1) return [];
  const start = mondayOf(new Date(p[0], p[1] - 1, p[2]));
  return Array.from({ length: 7 }, (_, i) => {
    const d = new Date(start);
    d.setDate(start.getDate() + (week - 1) * 7 + i);
    return d;
  });
}

export function mondayOf(d: Date): Date {
  const x = new Date(d.getFullYear(), d.getMonth(), d.getDate());
  const wd = (x.getDay() + 6) % 7;
  x.setDate(x.getDate() - wd);
  return x;
}

function ymd(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export function defaultWeek1(term?: string): string {
  if (!term) return "";
  const parts = term.split("-");
  if (parts.length < 3) return "";
  const y = Number(parts[0]);
  if (!y) return "";
  if (parts[2].startsWith("1")) return ymd(mondayOf(new Date(y, 8, 1)));
  if (parts[2].startsWith("2")) return ymd(mondayOf(new Date(y + 1, 1, 24)));
  return "";
}

export function teachingWeek(week1Monday: string | undefined, now: Date): number | undefined {
  if (!week1Monday) return undefined;
  const p = week1Monday.slice(0, 10).split("-").map(Number);
  if (p.length < 3 || !p[0]) return undefined;
  const start = mondayOf(new Date(p[0], p[1] - 1, p[2]));
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const diff = Math.round((today.getTime() - start.getTime()) / 86400000);
  if (diff < 0) return undefined;
  return Math.floor(diff / 7) + 1;
}

export function teachingWeekFromTerm(
  termWeeks: { n: number; start: string; end: string }[] | undefined,
  now: Date
): number | undefined {
  if (!termWeeks?.length) return undefined;
  const t = ymd(now);
  for (const w of termWeeks) {
    const s = (w.start || "").slice(0, 10);
    const e = (w.end || "").slice(0, 10);
    if (s && e && t >= s && t <= e) return w.n;
  }
  return undefined;
}

export function weekdayOf(now: Date): number {
  const d = now.getDay();
  return d === 0 ? 7 : d;
}

export function coursesOnDay(sem: SemesterTable, week: number | undefined, weekday: number): Course[] {
  if (week == null) return [];
  return sem.courses
    .filter((c) => c.weekday === weekday)
    .filter((c) => inWeek(c, week))
    .slice()
    .sort((a, b) => a.start.localeCompare(b.start) || a.name.localeCompare(b.name));
}

export function maxWeek(sem: SemesterTable): number {
  let m = 18;
  for (const c of sem.courses) {
    if (!c.weeks) continue;
    for (const w of c.weeks) m = Math.max(m, w);
  }
  return Math.min(22, Math.max(m, 18));
}

export function overlapsBlock(c: Course, begin: number, end: number): boolean {
  const sec = c.sections;
  if (!sec) return false;
  return sec[0] <= end && sec[1] >= begin;
}

function hm(s: string, now: Date): Date {
  const [h, m] = s.split(":").map(Number);
  const d = new Date(now);
  d.setHours(h || 0, m || 0, 0, 0);
  return d;
}

export function minutesUntil(hhmm: string, now: Date): number {
  return Math.max(0, Math.round((hm(hhmm, now).getTime() - now.getTime()) / 60000));
}

export function addDays(d: Date, n: number): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
}

/** 今天本来有课，并且最后一节已经下课。没课的日子不算「上完」。 */
export function allCoursesEnded(courses: Course[], now: Date): boolean {
  if (!courses.length) return false;
  return courses.every((c) => now >= hm(c.end, now));
}

/** 尚未开始、且距离开课不超过 withinMinutes 的最近一节。 */
export function upcomingCourse(courses: Course[], now: Date, withinMinutes: number): Course | null {
  const sorted = courses.slice().sort((a, b) => a.start.localeCompare(b.start));
  for (const c of sorted) {
    if (now >= hm(c.start, now)) continue;
    if (minutesUntil(c.start, now) <= withinMinutes) return c;
  }
  return null;
}

export type CourseKind = "past" | "now" | "next" | "later";

export function classifyToday(courses: Course[], now: Date): { course: Course; kind: CourseKind; label: string }[] {
  const sorted = courses.slice().sort((a, b) => a.start.localeCompare(b.start));
  const next = sorted.find((c) => now < hm(c.start, now));
  return sorted.map((c) => {
    const start = hm(c.start, now);
    const end = hm(c.end, now);
    if (now >= end) return { course: c, kind: "past" as const, label: "已结束" };
    if (now >= start && now < end) {
      return { course: c, kind: "now" as const, label: `还剩 ${minutesUntil(c.end, now)} 分钟` };
    }
    if (next && next === c) {
      const m = minutesUntil(c.start, now);
      return { course: c, kind: "next" as const, label: `下一节 · ${m} 分钟` };
    }
    return { course: c, kind: "later" as const, label: "" };
  });
}

export function collapsedFromCourses(
  courses: Course[],
  now: Date
): { mid: string; aux: string; live?: boolean; soon?: boolean } | null {
  if (!courses.length) return null;
  const rows = classifyToday(courses, now);
  const live = rows.find((r) => r.kind === "now");
  if (live) {
    return {
      mid: live.course.name,
      aux: `还剩 ${minutesUntil(live.course.end, now)} 分钟`,
      live: true,
    };
  }
  const next = rows.find((r) => r.kind === "next");
  if (next) {
    const m = minutesUntil(next.course.start, now);
    return {
      mid: `${next.course.name}  ${next.course.start}`,
      aux: `${m} 分钟`,
      soon: m <= 15,
    };
  }
  return { mid: "今天课上完了", aux: "" };
}

export function cacheDayLabel(iso?: string): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) {
    const p = iso.slice(0, 10).split("-");
    if (p.length >= 3) return `${Number(p[1])}月${Number(p[2])}日`;
    return "";
  }
  return `${d.getMonth() + 1}月${d.getDate()}日`;
}

export const WEEKDAY_SHORT = ["一", "二", "三", "四", "五", "六", "日"];
