//! 防社死 · 智能静音。
//!
//! 看护线程每秒读一次默认输出设备的峰值和静音状态。课程、日程按本机缓存判断；
//! 「长时间无音频」按真实经过的时间计时。用户手动取消静音后，本段时间不再自动静音。

use crate::models::{AgendaItem, RepetRule, SemesterTable};
use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, Timelike};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const IDLE_MIN: u32 = 20;
const IDLE_MAX: u32 = 200;
/// 数字静音是 0；约 -66 dBFS 就当作有声音。
const AUDIO_PEAK: f32 = 0.0005;

pub struct SmartMute {
    stop: Arc<AtomicBool>,
    done: Arc<(Mutex<bool>, Condvar)>,
}

impl SmartMute {
    /// 退出前把系统静音恢复原状，最多等 1.5 秒。
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let (lock, cv) = &*self.done;
        let guard = lock.lock().unwrap();
        let _ = cv.wait_timeout_while(guard, Duration::from_millis(1500), |done| !*done);
    }
}

pub fn start(app: AppHandle) -> SmartMute {
    let stop = Arc::new(AtomicBool::new(false));
    let done = Arc::new((Mutex::new(false), Condvar::new()));
    let stop_thread = stop.clone();
    let done_thread = done.clone();
    let done_fail = done.clone();
    let spawned = std::thread::Builder::new()
        .name("nidong-smart-mute".into())
        .spawn(move || {
            let _com = ComGuard::init();
            let mut worker = Worker::new();
            while !stop_thread.load(Ordering::SeqCst) {
                worker.tick(&app);
                for _ in 0..4 {
                    if stop_thread.load(Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
            worker.restore();
            let (lock, cv) = &*done_thread;
            *lock.lock().unwrap() = true;
            cv.notify_all();
        });
    if spawned.is_err() {
        *done_fail.0.lock().unwrap() = true;
    }
    SmartMute { stop, done }
}

struct Worker {
    auto_muted: bool,
    saved_mute: bool,
    override_key: String,
    idle_hold: bool,
    idle_prev: bool,
    last_sound: Instant,
    prev_muted: Option<bool>,
}

impl Worker {
    fn new() -> Self {
        Self {
            auto_muted: false,
            saved_mute: false,
            override_key: String::new(),
            idle_hold: false,
            idle_prev: false,
            last_sound: Instant::now(),
            prev_muted: None,
        }
    }

    fn restore(&mut self) {
        if !self.auto_muted {
            return;
        }
        if let Some(ep) = Endpoint::open() {
            if ep.muted().unwrap_or(false) {
                let _ = ep.set_mute(self.saved_mute);
            }
        }
        self.auto_muted = false;
    }

    fn tick(&mut self, app: &AppHandle) {
        let Some(state) = app.try_state::<crate::AppState>() else {
            return;
        };
        let settings = state.settings.lock().unwrap().clone();
        let now = crate::clock::now();
        let (class_key, agenda_key) = {
            let snap = state.snapshot.lock().unwrap();
            let class_key = if settings.smart_mute_class {
                active_class_key(snap.semester.as_ref(), &settings.week1_monday, &now)
            } else {
                None
            };
            let agenda_key = if settings.smart_mute_agenda {
                active_agenda_key(&snap.agenda, &now)
            } else {
                None
            };
            (class_key, agenda_key)
        };

        if settings.smart_mute_idle && !self.idle_prev {
            self.last_sound = Instant::now();
            self.idle_hold = false;
        }
        if !settings.smart_mute_idle {
            self.idle_hold = false;
        }
        self.idle_prev = settings.smart_mute_idle;

        if !settings.smart_mute_idle && !settings.smart_mute_class && !settings.smart_mute_agenda {
            self.restore();
            self.prev_muted = None;
            self.last_sound = Instant::now();
            return;
        }

        let Some(ep) = Endpoint::open() else {
            return;
        };
        if ep.peak().unwrap_or(0.0) > AUDIO_PEAK {
            self.last_sound = Instant::now();
            self.idle_hold = false;
        }

        let minutes = settings
            .smart_mute_idle_minutes
            .clamp(IDLE_MIN, IDLE_MAX) as u64;
        let mut idle_want = settings.smart_mute_idle
            && !self.idle_hold
            && self.last_sound.elapsed() >= Duration::from_secs(minutes * 60);

        let active_key = match (class_key, agenda_key) {
            (Some(a), Some(b)) => format!("{a}|{b}"),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => String::new(),
        };
        if self.override_key != active_key {
            self.override_key.clear();
        }

        let muted_now = ep.muted().unwrap_or(false);
        if (!active_key.is_empty() || idle_want) && self.prev_muted == Some(true) && !muted_now {
            // 用户手动取消了静音。本段课程/日程，或这次无音频静音不再动手。
            self.auto_muted = false;
            if !active_key.is_empty() {
                self.override_key = active_key.clone();
            }
            if idle_want {
                self.idle_hold = true;
                idle_want = false;
            }
            self.last_sound = Instant::now();
        }

        let active_want = !active_key.is_empty() && self.override_key != active_key;
        let want = active_want || idle_want;
        if want {
            if self.auto_muted {
                if !muted_now {
                    // 默认设备换了，或静音被别人取消。按手动取消处理。
                    self.auto_muted = false;
                    if !active_key.is_empty() {
                        self.override_key = active_key.clone();
                    }
                    if idle_want {
                        self.idle_hold = true;
                    }
                }
            } else if !muted_now {
                self.saved_mute = muted_now;
                if ep.set_mute(true) {
                    self.auto_muted = true;
                }
            }
        } else if self.auto_muted {
            if muted_now {
                let _ = ep.set_mute(self.saved_mute);
            }
            self.auto_muted = false;
        }

        self.prev_muted = Some(ep.muted().unwrap_or(muted_now));
    }
}

fn parse_hm(s: &str) -> Option<u32> {
    let mut p = s.trim().split(':');
    let h: u32 = p.next()?.parse().ok()?;
    let m: u32 = p.next()?.parse().ok()?;
    if p.next().is_some() || h > 23 || m > 59 {
        return None;
    }
    Some(h * 60 + m)
}

fn now_minutes(now: &DateTime<Local>) -> u32 {
    now.hour() * 60 + now.minute()
}

fn teaching_week_for(
    sem: &SemesterTable,
    week1_override: &str,
    today: NaiveDate,
) -> Option<u32> {
    let override_week = week1_override.trim();
    if override_week.is_empty() {
        if let Some(w) = crate::course::teaching_week_from_term(&sem.term_weeks, today) {
            return Some(w);
        }
    }
    let week1 = if !override_week.is_empty() {
        override_week
    } else if !sem.week1_monday.trim().is_empty() {
        sem.week1_monday.trim()
    } else {
        return crate::course::teaching_week(
            &crate::course::default_week1(&sem.semester_id),
            today,
        );
    };
    crate::course::teaching_week(week1, today)
}

fn active_class_key(
    sem: Option<&SemesterTable>,
    week1_override: &str,
    now: &DateTime<Local>,
) -> Option<String> {
    let sem = sem?;
    let today = now.date_naive();
    let week = teaching_week_for(sem, week1_override, today)?;
    let weekday = crate::course::weekday_of(today);
    let minute = now_minutes(now);
    crate::course::courses_today(sem, Some(week), weekday)
        .into_iter()
        .find(|c| {
            let (Some(start), Some(end)) = (parse_hm(&c.start), parse_hm(&c.end)) else {
                return false;
            };
            minute >= start && minute < end
        })
        .map(|c| format!("{today}|{}|{}|{}", c.start, c.end, c.name))
}

fn active_agenda_key(items: &[AgendaItem], now: &DateTime<Local>) -> Option<String> {
    let naive_now = now.naive_local();
    items.iter().find_map(|a| active_occurrence(a, naive_now))
}

fn active_occurrence(a: &AgendaItem, now: NaiveDateTime) -> Option<String> {
    let start = crate::agenda::parse_local(&a.start)?;
    let end = crate::agenda::parse_local(&a.end)?;
    let duration = end - start;
    if duration.num_seconds() <= 0 {
        return None;
    }
    let Some(rule) = repeat_rule(a) else {
        return (now >= start && now < end).then(|| format!("{}|{}", a.id, a.start));
    };

    let series_day = start.date();
    if now.date() < series_day {
        return None;
    }
    let limit_day = if rule.endmode == "DATE" {
        let text = rule.time.as_deref().unwrap_or("");
        Some(crate::course::parse_ymd(text.get(..10)?)?)
    } else {
        None
    };
    let max_occurrences = if rule.endmode == "NUMBER" {
        let n = rule.num.min(999);
        if n == 0 {
            return None;
        }
        Some(n)
    } else {
        None
    };

    let today = now.date();
    let mut day = series_day;
    let mut seen = 0u32;
    let mut guard = 0u32;
    while day <= today {
        guard += 1;
        if guard > 12000 {
            return None;
        }
        if let Some(limit) = limit_day {
            if day > limit {
                return None;
            }
        }
        if day_matches(day, rule) {
            seen += 1;
            if let Some(max) = max_occurrences {
                if seen > max {
                    return None;
                }
            }
            if day == today {
                let occ_start = NaiveDateTime::new(day, start.time());
                let occ_end = occ_start + duration;
                if now >= occ_start && now < occ_end {
                    return Some(format!(
                        "{}|{}T{}",
                        a.id,
                        day.format("%Y-%m-%d"),
                        start.format("%H:%M")
                    ));
                }
                return None;
            }
        }
        day = day.succ_opt()?;
    }
    None
}

fn repeat_rule(a: &AgendaItem) -> Option<&RepetRule> {
    if !a.is_repet {
        return None;
    }
    let rule = a.repet_rule.as_ref()?;
    if rule.freq != "DAILY" && rule.freq != "WEEKLY" {
        return None;
    }
    if rule.endmode != "DATE" && rule.endmode != "NUMBER" {
        return None;
    }
    if a.start.get(..10)? != a.end.get(..10)? {
        return None;
    }
    Some(rule)
}

fn day_matches(day: NaiveDate, rule: &RepetRule) -> bool {
    match rule.freq.as_str() {
        "DAILY" => true,
        "WEEKLY" => {
            let code = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"]
                [day.weekday().num_days_from_sunday() as usize];
            rule.byday.iter().any(|d| d == code)
        }
        _ => false,
    }
}

#[cfg(windows)]
struct Endpoint {
    volume: windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume,
    meter: windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation,
}

#[cfg(windows)]
impl Endpoint {
    fn open() -> Option<Self> {
        use windows::Win32::Media::Audio::{
            eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
        };
        use windows::Win32::Media::Audio::Endpoints::{
            IAudioEndpointVolume, IAudioMeterInformation,
        };
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            let volume = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()?;
            let meter = device
                .Activate::<IAudioMeterInformation>(CLSCTX_ALL, None)
                .ok()?;
            Some(Self { volume, meter })
        }
    }

    fn peak(&self) -> Option<f32> {
        unsafe { self.meter.GetPeakValue().ok() }
    }

    fn muted(&self) -> Option<bool> {
        unsafe { self.volume.GetMute().ok().map(|b| b.as_bool()) }
    }

    fn set_mute(&self, muted: bool) -> bool {
        unsafe { self.volume.SetMute(muted, std::ptr::null()).is_ok() }
    }
}

#[cfg(windows)]
struct ComGuard;

#[cfg(windows)]
impl ComGuard {
    fn init() -> Option<Self> {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .is_ok()
                .then_some(Self)
        }
    }
}

#[cfg(windows)]
impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    }
}

#[cfg(not(windows))]
struct Endpoint;

#[cfg(not(windows))]
impl Endpoint {
    fn open() -> Option<Self> {
        None
    }

    fn peak(&self) -> Option<f32> {
        None
    }

    fn muted(&self) -> Option<bool> {
        None
    }

    fn set_mute(&self, _muted: bool) -> bool {
        false
    }
}

#[cfg(not(windows))]
struct ComGuard;

#[cfg(not(windows))]
impl ComGuard {
    fn init() -> Option<Self> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, m, d, h, min, 0).single().unwrap()
    }

    fn semester(courses: Vec<crate::models::Course>) -> SemesterTable {
        SemesterTable {
            semester_id: "2026-2027-1".into(),
            label: "2026-2027 秋".into(),
            week1_monday: "2026-09-07".into(),
            fetched_at: String::new(),
            courses,
            term_weeks: Vec::new(),
        }
    }

    fn course(name: &str, start: &str, end: &str, weekday: u8, weeks: Option<Vec<u32>>) -> crate::models::Course {
        crate::models::Course {
            name: name.into(),
            place: String::new(),
            teacher: None,
            start: start.into(),
            end: end.into(),
            weekday,
            weeks,
            sections: None,
            campus: None,
        }
    }

    fn agenda(value: serde_json::Value) -> AgendaItem {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn class_silences_only_during_the_lesson() {
        let sem = semester(vec![course("高等数学", "08:00", "09:35", 6, None)]);
        assert!(active_class_key(Some(&sem), "", &at(2026, 10, 31, 9, 0)).is_some());
        assert!(active_class_key(Some(&sem), "", &at(2026, 10, 31, 9, 35)).is_none());
        assert!(active_class_key(Some(&sem), "", &at(2026, 10, 30, 9, 0)).is_none());
    }

    #[test]
    fn class_respects_teaching_weeks() {
        let sem = semester(vec![course("程序设计", "14:00", "15:40", 6, Some(vec![1, 2, 3]))]);
        assert!(active_class_key(Some(&sem), "", &at(2026, 9, 12, 14, 30)).is_some());
        assert!(active_class_key(Some(&sem), "", &at(2026, 10, 31, 14, 30)).is_none());
    }

    #[test]
    fn daily_agenda_is_active_inside_its_window() {
        let item = agenda(json!({
            "id": "a1",
            "name": "班会",
            "start": "2026-10-01T08:00",
            "end": "2026-10-01T09:00",
            "isRepet": true,
            "repetRule": { "freq": "DAILY", "endmode": "DATE", "time": "2026-12-31" }
        }));
        assert!(active_agenda_key(&[item.clone()], &at(2026, 10, 31, 8, 30)).is_some());
        assert!(active_agenda_key(&[item], &at(2026, 10, 31, 9, 30)).is_none());
    }

    #[test]
    fn weekly_agenda_matches_its_weekday() {
        let item = agenda(json!({
            "id": "a2",
            "name": "社团",
            "start": "2026-10-03T18:00",
            "end": "2026-10-03T19:00",
            "isRepet": true,
            "repetRule": { "freq": "WEEKLY", "byday": ["Sa"], "endmode": "DATE", "time": "2026-12-31" }
        }));
        assert!(active_agenda_key(&[item.clone()], &at(2026, 10, 31, 18, 30)).is_some());
        assert!(active_agenda_key(&[item], &at(2026, 10, 26, 18, 30)).is_none());
    }

    #[test]
    fn numbered_agenda_stops_after_its_count() {
        let item = agenda(json!({
            "id": "a3",
            "name": "例会",
            "start": "2026-10-03T18:00",
            "end": "2026-10-03T19:00",
            "isRepet": true,
            "repetRule": { "freq": "WEEKLY", "byday": ["Sa"], "endmode": "NUMBER", "num": 2 }
        }));
        assert!(active_agenda_key(&[item.clone()], &at(2026, 10, 10, 18, 30)).is_some());
        assert!(active_agenda_key(&[item], &at(2026, 10, 17, 18, 30)).is_none());
    }

    #[test]
    fn cross_day_repeat_falls_back_to_one_span() {
        let item = agenda(json!({
            "id": "a4",
            "name": "通宵",
            "start": "2026-10-30T22:00",
            "end": "2026-10-31T02:00",
            "isRepet": true,
            "repetRule": { "freq": "DAILY", "endmode": "DATE", "time": "2026-12-31" }
        }));
        assert!(active_agenda_key(&[item.clone()], &at(2026, 10, 31, 1, 0)).is_some());
        assert!(active_agenda_key(&[item], &at(2026, 11, 1, 1, 0)).is_none());
    }
}
