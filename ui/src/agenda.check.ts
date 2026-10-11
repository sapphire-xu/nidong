import { agendasOnDate, occurrencesOnDate, upcomingAgenda } from "./agenda";
import type { Agenda, RepetRule } from "./types";

function assert(cond: unknown, msg: string) {
  if (!cond) throw new Error(msg);
}

function ev(patch: Partial<Agenda>): Agenda {
  return {
    id: "a",
    name: "例会",
    start: "2026-10-05T10:00",
    end: "2026-10-05T11:00",
    remind: false,
    ...patch,
  };
}

function rule(patch: Partial<RepetRule> & Pick<RepetRule, "freq" | "endmode">): RepetRule {
  return { ...patch };
}

const daily = ev({
  isRepet: true,
  repetRule: rule({ freq: "DAILY", endmode: "NUMBER", num: 3 }),
});
assert(occurrencesOnDate(daily, "2026-10-05").length === 1, "daily day 1");
assert(occurrencesOnDate(daily, "2026-10-07").length === 1, "daily day 3");
assert(occurrencesOnDate(daily, "2026-10-08").length === 0, "daily stops at 3");

const weekly = ev({
  isRepet: true,
  repetRule: rule({ freq: "WEEKLY", endmode: "NUMBER", num: 2, byday: ["We"] }),
});
assert(occurrencesOnDate(weekly, "2026-10-05").length === 0, "monday is not selected");
assert(occurrencesOnDate(weekly, "2026-10-07")[0]?.start === "2026-10-07T10:00", "first wednesday");
assert(occurrencesOnDate(weekly, "2026-10-14").length === 1, "second wednesday");
assert(occurrencesOnDate(weekly, "2026-10-21").length === 0, "third wednesday is past the count");
assert(agendasOnDate([weekly], "2026-10-07")[0]?.start === "2026-10-05T10:00", "edit still opens the series");

const dated = ev({
  isRepet: true,
  repetRule: rule({ freq: "DAILY", endmode: "DATE", time: "2026-10-07" }),
});
assert(occurrencesOnDate(dated, "2026-10-07").length === 1, "end date included");
assert(occurrencesOnDate(dated, "2026-10-08").length === 0, "after end date");

const overnight = ev({
  start: "2026-10-05T22:00",
  end: "2026-10-06T01:00",
  isRepet: true,
  repetRule: rule({ freq: "DAILY", endmode: "NUMBER", num: 1 }),
});
assert(occurrencesOnDate(overnight, "2026-10-05").length === 1, "starts the first night");
assert(occurrencesOnDate(overnight, "2026-10-06").length === 1, "spills into the next morning");
assert(occurrencesOnDate(overnight, "2026-10-07").length === 0, "only one occurrence");

const soon = ev({
  start: "2026-10-06T21:00",
  end: "2026-10-06T21:30",
  remind: true,
  remindMinutes: 15,
});
const now = new Date("2026-10-06T20:50:00");
const timed = { ...soon, isRepet: true, repetRule: rule({ freq: "DAILY", endmode: "NUMBER", num: 2 }) };
const hit = upcomingAgenda([timed], now);
assert(hit?.start === "2026-10-06T21:00", "next occurrence is inside the remind window");
assert(upcomingAgenda([timed], new Date("2026-10-06T21:10:00")) === null, "the next day is outside the remind window");
assert(upcomingAgenda([timed], new Date("2026-10-07T20:50:00"))?.start === "2026-10-07T21:00", "second occurrence can remind");

console.log("agenda checks ok");
