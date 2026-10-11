import { agendasOnDate, mixDay } from "./agenda";
import {
  addDays,
  allCoursesEnded,
  collapsedFromCourses,
  coursesOnDay,
  defaultWeek1,
  maxWeek,
  teachingWeek,
  teachingWeekFromTerm,
  weekdayOf,
} from "./course";
import type { AppSettings, Snapshot } from "./types";
import { ymd } from "./format";

export function week1Of(settings: AppSettings | null, snap: Snapshot | null) {
  return (settings?.week1Monday || snap?.semester?.week1Monday || defaultWeek1(snap?.semester?.semesterId) || "").trim();
}

export function weekNumber(settings: AppSettings | null, snap: Snapshot | null, now: Date) {
  const week1 = week1Of(settings, snap);
  return (
    (!settings?.week1Monday ? teachingWeekFromTerm(snap?.semester?.termWeeks, now) : undefined) ??
    teachingWeek(week1, now) ??
    snap?.teachingWeek
  );
}

export function deriveDay(snap: Snapshot | null, settings: AppSettings | null, now: Date) {
  const week1 = week1Of(settings, snap);
  const weekNow = weekNumber(settings, snap, now);
  const weekdayNow = weekdayOf(now);
  const signedIn = !!snap?.loggedIn;
  const todayCourses = !signedIn
    ? []
    : snap?.semester
      ? coursesOnDay(snap.semester, weekNow, weekdayNow)
      : snap?.coursesToday || [];
  const tomorrowDate = addDays(now, 1);
  const weekTomorrow =
    (!settings?.week1Monday ? teachingWeekFromTerm(snap?.semester?.termWeeks, tomorrowDate) : undefined) ??
    teachingWeek(week1, tomorrowDate) ??
    (weekdayNow === 7 && weekNow ? weekNow + 1 : weekNow);
  const tomorrowCourses = signedIn && snap?.semester ? coursesOnDay(snap.semester, weekTomorrow, weekdayOf(tomorrowDate)) : [];
  const todayDone = allCoursesEnded(todayCourses, now);
  const todayEvents = agendasOnDate(snap?.agenda, ymd(now));
  const tomorrowEvents = agendasOnDate(snap?.agenda, ymd(tomorrowDate));
  const todayMix = mixDay(todayDone ? [] : todayCourses, todayEvents, now);
  const tomorrowMix = mixDay(tomorrowCourses, tomorrowEvents, now, tomorrowDate);
  const remainingTodayEvents = mixDay([], todayEvents, now).filter((r) => r.kind !== "past");
  const collapsed = (() => {
    if (!snap?.loggedIn) return null;
    const live = collapsedFromCourses(todayCourses, now);
    const next = tomorrowCourses[0];
    if (live && live.mid !== "今天课上完了") return live;
    if (next && !todayCourses.length) return { mid: next.name, aux: `明天 ${next.start}` };
    if (live?.mid === "今天课上完了" && next) return { ...live, aux: `明天 ${next.start}` };
    return live;
  })();
  const semMax = snap?.semester ? maxWeek(snap.semester) : 18;
  return {
    week1,
    weekNow,
    weekdayNow,
    todayCourses,
    tomorrowCourses,
    todayDone,
    todayMix,
    tomorrowMix,
    remainingTodayEvents,
    collapsed,
    semMax,
  };
}
