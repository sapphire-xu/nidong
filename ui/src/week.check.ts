import type { Agenda, Course } from "./types";
import { agendaDayMinutes, clockMinutes, layoutDay, courseLane, sectionText } from "./week";

function assert(cond: unknown, msg: string) {
  if (!cond) throw new Error(msg);
}

function near(a: number, b: number, msg: string) {
  assert(Math.abs(a - b) < 1e-9, `${msg}: ${a} vs ${b}`);
}

const course: Course = {
  name: "高等数学",
  place: "生命 B213",
  teacher: "张三",
  start: "08:00",
  end: "08:45",
  weekday: 1,
  weeks: [1, 2, 3],
  sections: [1, 2],
};

const overlap: Agenda = {
  id: "a1",
  name: "班会",
  start: "2026-09-07T08:20",
  end: "2026-09-07T09:00",
  remind: false,
};

const cross: Agenda = {
  id: "a2",
  name: "通宵实验",
  start: "2026-09-07T21:00",
  end: "2026-09-08T09:00",
  remind: false,
};

const late: Agenda = {
  id: "a3",
  name: "夜谈",
  start: "2026-09-07T23:00",
  end: "2026-09-07T23:30",
  remind: false,
};

const one = layoutDay([courseLane(course)]);
assert(one.placed.length === 1, "一节课应落在轴上");
assert(one.outside.length === 0, "课不应进轴外");
near(one.placed[0].top, (clockMinutes("08:00") - 450) / 900, "课的顶边");
near(one.placed[0].height, 45 / 900, "课的高度");
assert(one.placed[0].columns === 1, "单独一节只有一列");
assert(sectionText(course.sections) === "1–2 节", "节次文案");

const both = layoutDay([
  courseLane(course),
  {
    key: "e",
    kind: "event",
    name: overlap.name,
    startMin: agendaDayMinutes(overlap, "2026-09-07")!.startMin,
    endMin: agendaDayMinutes(overlap, "2026-09-07")!.endMin,
    event: overlap,
  },
]);
assert(both.placed.length === 2, "重叠的课和日程都要画");
assert(both.placed.every((b) => b.columns === 2), "相交的块按簇分两列");
assert(new Set(both.placed.map((b) => b.column)).size === 2, "两列不能叠在同一列");

const day1 = agendaDayMinutes(cross, "2026-09-07");
const day2 = agendaDayMinutes(cross, "2026-09-08");
assert(day1 && day2, "跨天日程要切成两天");
const laid1 = layoutDay([{ key: "d1", kind: "event", name: cross.name, ...day1!, event: cross }]);
const laid2 = layoutDay([{ key: "d2", kind: "event", name: cross.name, ...day2!, event: cross }]);
near(laid1.placed[0].startMin, 21 * 60, "第一天从 21:00 画");
near(laid1.placed[0].endMin, 22 * 60 + 30, "第一天收到 22:30");
near(laid2.placed[0].startMin, 7 * 60 + 30, "第二天从 07:30 画");
near(laid2.placed[0].endMin, 9 * 60, "第二天画到 09:00");

const lateSpan = agendaDayMinutes(late, "2026-09-07")!;
const lateLay = layoutDay([{ key: "late", kind: "event", name: late.name, ...lateSpan, event: late }]);
assert(lateLay.placed.length === 0 && lateLay.outside.length === 1, "23:00 的日程进轴外");

const chain = layoutDay([
  { key: "a", kind: "event", name: "A", startMin: 8 * 60, endMin: 10 * 60 },
  { key: "b", kind: "event", name: "B", startMin: 9 * 60, endMin: 11 * 60 },
  { key: "c", kind: "event", name: "C", startMin: 10 * 60, endMin: 12 * 60 },
]);
assert(chain.placed.length === 3, "传递相交仍是一簇");
assert(chain.placed.every((b) => b.columns === 2), "A 与 C 不重叠，簇内两列就够");
const colA = chain.placed.find((b) => b.name === "A")!.column;
const colC = chain.placed.find((b) => b.name === "C")!.column;
assert(colA === colC, "C 应复用 A 空出来的列");

console.log("week checks ok");
