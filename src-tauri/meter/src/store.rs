//! 按日留下五项合计。没有发生流量的日子不写行，也不补 0。

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub v4_day: u64,
    pub v4_night: u64,
    pub v4_up: u64,
    pub v6_down: u64,
    pub v6_up: u64,
}

impl Totals {
    pub fn add(&mut self, other: Self) {
        self.v4_day += other.v4_day;
        self.v4_night += other.v4_night;
        self.v4_up += other.v4_up;
        self.v6_down += other.v6_down;
        self.v6_up += other.v6_up;
    }

    pub fn is_zero(self) -> bool {
        self == Self::default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Day {
    pub date: String,
    pub totals: Totals,
}

#[derive(Clone, Debug, Default)]
pub struct Ledger {
    pub month: String,
    pub totals: Totals,
    pub days: Vec<Day>,
}

#[repr(C)]
struct SystemTime {
    year: u16,
    month: u16,
    dow: u16,
    day: u16,
    hour: u16,
    min: u16,
    sec: u16,
    ms: u16,
}

extern "system" {
    fn GetLocalTime(out: *mut SystemTime);
}

pub fn app_dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("edu.neu.nidong")
}

pub fn meter_dir() -> PathBuf {
    app_dir().join("meter")
}

pub fn now_parts() -> (String, String, bool) {
    let mut t = SystemTime {
        year: 0,
        month: 0,
        dow: 0,
        day: 0,
        hour: 0,
        min: 0,
        sec: 0,
        ms: 0,
    };
    unsafe { GetLocalTime(&mut t) };
    let month = format!("{:04}-{:02}", t.year, t.month);
    let day = format!("{:04}-{:02}-{:02}", t.year, t.month, t.day);
    (month, day, t.hour < 6)
}

impl Ledger {
    pub fn add(&mut self, day: &str, delta: Totals) {
        if delta.is_zero() {
            return;
        }
        self.totals.add(delta);
        if let Some(slot) = self.days.iter_mut().find(|d| d.date == day) {
            slot.totals.add(delta);
        } else {
            self.days.push(Day {
                date: day.to_string(),
                totals: delta,
            });
        }
    }
}

pub fn usage_path() -> PathBuf {
    meter_dir().join("usage.txt")
}

pub fn load_ledger(path: &Path) -> Ledger {
    let text = fs::read_to_string(path).unwrap_or_default();
    parse_ledger(&text)
}

pub fn parse_ledger(text: &str) -> Ledger {
    let mut ledger = Ledger::default();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(v) = line.strip_prefix("month=") {
            ledger.month = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("total=") {
            if let Some(t) = parse_five(v) {
                ledger.totals = t;
            }
        } else if let Some(v) = line.strip_prefix("day=") {
            let mut parts = v.split_whitespace();
            let Some(date) = parts.next() else { continue };
            let rest: Vec<&str> = parts.collect();
            if let Some(totals) = parse_five(&rest.join(" ")) {
                if !totals.is_zero() {
                    ledger.days.push(Day {
                        date: date.to_string(),
                        totals,
                    });
                }
            }
        }
    }
    ledger
}

fn parse_five(text: &str) -> Option<Totals> {
    let mut n = text.split_whitespace();
    Some(Totals {
        v4_day: n.next()?.parse().ok()?,
        v4_night: n.next()?.parse().ok()?,
        v4_up: n.next()?.parse().ok()?,
        v6_down: n.next()?.parse().ok()?,
        v6_up: n.next()?.parse().ok()?,
    })
}

fn five(t: Totals) -> String {
    format!("{} {} {} {} {}", t.v4_day, t.v4_night, t.v4_up, t.v6_down, t.v6_up)
}

pub fn save_ledger(path: &Path, ledger: &Ledger) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("txt.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        writeln!(f, "# 字节。顺序：IPv4下载(白天校外) IPv4下载(0-6点) IPv4上传 IPv6下载 IPv6上传")?;
        writeln!(f, "month={}", ledger.month)?;
        writeln!(f, "total={}", five(ledger.totals))?;
        for day in &ledger.days {
            if day.totals.is_zero() {
                continue;
            }
            writeln!(f, "day={} {}", day.date, five(day.totals))?;
        }
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn archive_if_needed(path: &Path, ledger: Ledger, month_now: &str) -> Ledger {
    if ledger.month.is_empty() {
        let mut fresh = Ledger::default();
        fresh.month = month_now.to_string();
        return fresh;
    }
    if ledger.month == month_now {
        return ledger;
    }
    if let Some(dir) = path.parent() {
        let archived = dir.join(format!("{}.txt", ledger.month));
        let _ = save_ledger(&archived, &ledger);
    }
    let mut fresh = Ledger::default();
    fresh.month = month_now.to_string();
    fresh
}

pub fn all_days() -> Vec<Day> {
    let dir = meter_dir();
    let mut map: BTreeMap<String, Totals> = BTreeMap::new();
    let Ok(rd) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = Vec::new();
    for ent in rd.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        let archived = name.len() == 11 && name.ends_with(".txt") && name.as_bytes().get(4) == Some(&b'-');
        if name == "usage.txt" || archived {
            files.push(ent.path());
        }
    }
    files.sort();
    let usage = dir.join("usage.txt");
    files.retain(|p| p != &usage);
    files.push(usage);
    for path in files {
        for day in load_ledger(&path).days {
            map.insert(day.date, day.totals);
        }
    }
    map.into_iter()
        .map(|(date, totals)| Day { date, totals })
        .collect()
}

pub fn summary(month: &str, today: &str) -> (Option<Totals>, Option<Totals>) {
    let mut month_tot = Totals::default();
    let mut month_any = false;
    let mut today_tot = None;
    let prefix = format!("{month}-");
    for day in all_days() {
        if day.date == today {
            today_tot = Some(day.totals);
        }
        if day.date.starts_with(&prefix) {
            month_tot.add(day.totals);
            month_any = true;
        }
    }
    (today_tot, month_any.then_some(month_tot))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_delta_does_not_create_a_day() {
        let mut ledger = Ledger::default();
        ledger.month = "2026-10".into();
        ledger.add("2026-10-12", Totals::default());
        assert!(ledger.days.is_empty());
        ledger.add(
            "2026-10-12",
            Totals {
                v4_day: 10,
                ..Totals::default()
            },
        );
        ledger.add("2026-10-13", Totals::default());
        assert_eq!(ledger.days.len(), 1);
        assert_eq!(ledger.days[0].date, "2026-10-12");
        assert_eq!(ledger.totals.v4_day, 10);
    }

    #[test]
    fn ledger_roundtrip_skips_blank_days() {
        let mut ledger = Ledger::default();
        ledger.month = "2026-10".into();
        ledger.add(
            "2026-10-11",
            Totals {
                v4_night: 4,
                v6_down: 9,
                ..Totals::default()
            },
        );
        let dir = std::env::temp_dir().join("nidong-meter-ledger-test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("usage.txt");
        save_ledger(&path, &ledger).unwrap();
        let loaded = load_ledger(&path);
        assert_eq!(loaded.days.len(), 1);
        assert_eq!(loaded.days[0].totals.v4_night, 4);
        assert_eq!(loaded.days[0].totals.v6_down, 9);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn parse_ignores_a_zero_day_line() {
        let text = "month=2026-10\ntotal=0 0 0 0 0\nday=2026-10-01 0 0 0 0 0\nday=2026-10-02 1 0 0 0 0\n";
        let ledger = parse_ledger(text);
        assert_eq!(ledger.days.len(), 1);
        assert_eq!(ledger.days[0].date, "2026-10-02");
    }
}
