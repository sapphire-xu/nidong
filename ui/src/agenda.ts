import type { Agenda, Course } from "./types";
import type { CourseKind } from "./course";

export function parseAgenda(s: string): Date | null {
  const t = (s || "").trim().replace(" ", "T");
  if (t.length < 16) return null;
  const d = new Date(t.length === 16 ? `${t}:00` : t);
  return Number.isNaN(d.getTime()) ? null : d;
}

export function hmOf(s: string): string {
  const p = (s || "").replace("T", " ");
  return p.slice(11, 16);
}

export function agendaOnDate(a: Agenda, ymd: string): boolean {
  const s = (a.start || "").slice(0, 10);
  const e = (a.end || "").slice(0, 10);
  return !!s && !!e && ymd >= s && ymd <= e;
}

export function agendasOnDate(items: Agenda[] | undefined, ymd: string): Agenda[] {
  return (items || [])
    .filter((a) => agendaOnDate(a, ymd))
    .slice()
    .sort((a, b) => a.start.localeCompare(b.start) || a.name.localeCompare(b.name));
}

export function upcomingAgenda(items: Agenda[], now: Date): Agenda | null {
  let best: Agenda | null = null;
  let bestUntil = Infinity;
  for (const a of items) {
    if (!a.remind) continue;
    const start = parseAgenda(a.start);
    if (!start || now >= start) continue;
    const mins = Math.max(1, Math.floor(Number(a.remindMinutes) || 15));
    const until = (start.getTime() - now.getTime()) / 60000;
    if (until > mins) continue;
    if (until < bestUntil) {
      bestUntil = until;
      best = a;
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
    const start = parseAgenda(e.start);
    const end = parseAgenda(e.end);
    if (!start || !end) continue;
    raw.push({
      type: "event",
      start,
      end,
      displayStart: hmOf(e.start),
      displayEnd: hmOf(e.end),
      name: e.name,
      meta: (e.note || "").trim(),
      event: e,
      key: `e-${e.id}`,
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
