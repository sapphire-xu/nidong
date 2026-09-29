import { parseAgenda } from "./agenda";
import type { Agenda, Course } from "./types";

/** 时间轴从 07:30 到 22:30，共 900 分钟。 */
export const AXIS_START_MIN = 7 * 60 + 30;
export const AXIS_END_MIN = 22 * 60 + 30;
export const AXIS_SPAN_MIN = AXIS_END_MIN - AXIS_START_MIN;

export type LaneItem = {
  key: string;
  kind: "course" | "event";
  name: string;
  /** 当天 0:00 起的分钟，尚未裁到轴内。 */
  startMin: number;
  endMin: number;
  place?: string;
  teacher?: string;
  sections?: [number, number];
  weeks?: number[];
  course?: Course;
  event?: Agenda;
};

export type PlacedBlock = LaneItem & {
  /** 裁进 07:30–22:30 之后的分钟。 */
  startMin: number;
  endMin: number;
  top: number;
  height: number;
  column: number;
  columns: number;
};

export type DayLayout = {
  placed: PlacedBlock[];
  outside: LaneItem[];
};

export function clockMinutes(hhmm: string) {
  const [h, m] = (hhmm || "").split(":").map(Number);
  return (h || 0) * 60 + (m || 0);
}

export function sectionText(sections?: [number, number]) {
  if (!sections) return "";
  const [a, b] = sections;
  if (!Number.isFinite(a) || !Number.isFinite(b)) return "";
  return a === b ? `${a} 节` : `${a}–${b} 节`;
}

export function weeksText(weeks?: number[]) {
  if (!weeks?.length) return "每周";
  const s = [...new Set(weeks)].sort((a, b) => a - b);
  const parts: string[] = [];
  let i = 0;
  while (i < s.length) {
    let j = i;
    while (j + 1 < s.length && s[j + 1] === s[j] + 1) j++;
    parts.push(i === j ? `${s[i]}` : `${s[i]}–${s[j]}`);
    i = j + 1;
  }
  return `${parts.join("、")} 周`;
}

export function clipToAxis(startMin: number, endMin: number) {
  const start = Math.max(startMin, AXIS_START_MIN);
  const end = Math.min(endMin, AXIS_END_MIN);
  if (end <= start) return null;
  return { start, end };
}

/** 日程落在某一天上的分钟范围。跨天按天切开。不碰这一天则返回 null。 */
export function agendaDayMinutes(a: Agenda, ymd: string): { startMin: number; endMin: number } | null {
  const start = parseAgenda(a.start);
  const end = parseAgenda(a.end);
  if (!start || !end || end.getTime() <= start.getTime()) return null;
  const startKey = (a.start || "").slice(0, 10);
  const endKey = (a.end || "").slice(0, 10);
  if (!startKey || !endKey || ymd < startKey || ymd > endKey) return null;
  const startMin = ymd === startKey ? start.getHours() * 60 + start.getMinutes() : 0;
  const endMin = ymd === endKey ? end.getHours() * 60 + end.getMinutes() : 24 * 60;
  if (endMin <= startMin) return null;
  return { startMin, endMin };
}

export function courseLane(c: Course, key?: string): LaneItem {
  return {
    key: key || `c-${c.name}-${c.start}-${(c.weeks || []).join(",")}-${c.place}`,
    kind: "course",
    name: c.name,
    startMin: clockMinutes(c.start),
    endMin: clockMinutes(c.end),
    place: c.place,
    teacher: c.teacher,
    sections: c.sections,
    weeks: c.weeks,
    course: c,
  };
}

function byStart(a: LaneItem, b: LaneItem) {
  return a.startMin - b.startMin || a.endMin - b.endMin || a.name.localeCompare(b.name, "zh");
}

function clusters(items: LaneItem[]): LaneItem[][] {
  const sorted = items.slice().sort(byStart);
  const groups: LaneItem[][] = [];
  let cur: LaneItem[] = [];
  let curEnd = -1;
  for (const item of sorted) {
    if (cur.length && item.startMin < curEnd) {
      cur.push(item);
      curEnd = Math.max(curEnd, item.endMin);
    } else {
      if (cur.length) groups.push(cur);
      cur = [item];
      curEnd = item.endMin;
    }
  }
  if (cur.length) groups.push(cur);
  return groups;
}

/** 按开始时间贪心占列。返回与 group 下标对齐的列号。 */
function assignColumns(group: LaneItem[]): number[] {
  const order = group
    .map((item, index) => ({ item, index }))
    .sort((a, b) => byStart(a.item, b.item));
  const colEnds: number[] = [];
  const cols = new Array<number>(group.length).fill(0);
  for (const { item, index } of order) {
    let col = colEnds.findIndex((end) => item.startMin >= end);
    if (col < 0) {
      col = colEnds.length;
      colEnds.push(item.endMin);
    } else {
      colEnds[col] = item.endMin;
    }
    cols[index] = col;
  }
  return cols;
}

export function layoutDay(items: LaneItem[]): DayLayout {
  const inside: LaneItem[] = [];
  const outside: LaneItem[] = [];
  for (const item of items) {
    const clip = clipToAxis(item.startMin, item.endMin);
    if (!clip) {
      outside.push(item);
      continue;
    }
    inside.push({ ...item, startMin: clip.start, endMin: clip.end });
  }
  const placed: PlacedBlock[] = [];
  for (const group of clusters(inside)) {
    const cols = assignColumns(group);
    const count = Math.max(1, cols.reduce((m, c) => Math.max(m, c + 1), 0));
    group.forEach((item, i) => {
      placed.push({
        ...item,
        top: (item.startMin - AXIS_START_MIN) / AXIS_SPAN_MIN,
        height: (item.endMin - item.startMin) / AXIS_SPAN_MIN,
        column: cols[i],
        columns: count,
      });
    });
  }
  return { placed, outside };
}
