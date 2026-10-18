import assert from "node:assert/strict";
import test from "node:test";
import { repeatText } from "./agenda.ts";
import type { Agenda, RepetRule } from "./types.ts";

function ev(rule: RepetRule): Agenda {
  return {
    id: "a",
    name: "例会",
    start: "2026-10-05T10:00",
    end: "2026-10-05T11:00",
    remind: false,
    isRepet: true,
    repetRule: rule,
  };
}

test("daily repeat count is not read as times per day", () => {
  assert.equal(
    repeatText(ev({ freq: "DAILY", endmode: "NUMBER", num: 2 })),
    "重复 2 次 每天",
  );
});

test("weekly repeat count stays in front of the weekdays", () => {
  assert.equal(
    repeatText(ev({ freq: "WEEKLY", endmode: "NUMBER", num: 2, byday: ["Mo", "We"] })),
    "重复 2 次 按星期 周一、周三",
  );
});

test("a dated repeat still names the last day", () => {
  assert.equal(
    repeatText(ev({ freq: "DAILY", endmode: "DATE", time: "2026-10-07" })),
    "每天 到 2026-10-07",
  );
});
