//! 可覆盖的“当前时间”。仅当存在 `dev-now.txt` 或环境变量 `NIDONG_NOW` 时生效。
//! 文件不存在则与系统时钟完全一致。

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn init(dir: PathBuf) {
    let _ = DATA_DIR.set(dir);
}

pub fn now() -> DateTime<Local> {
    read_override().unwrap_or_else(Local::now)
}

pub fn today() -> NaiveDate {
    now().date_naive()
}

pub fn is_overridden() -> bool {
    read_override().is_some()
}

fn read_override() -> Option<DateTime<Local>> {
    if let Ok(s) = std::env::var("NIDONG_NOW") {
        if let Some(t) = parse_now(s.trim()) {
            return Some(t);
        }
    }
    let paths = [
        DATA_DIR.get().map(|d| d.join("dev-now.txt")),
        Some(PathBuf::from("tmp/now.txt")),
    ];
    for p in paths.into_iter().flatten() {
        if let Some(t) = read_file(&p) {
            return Some(t);
        }
    }
    None
}

fn read_file(p: &Path) -> Option<DateTime<Local>> {
    let s = std::fs::read_to_string(p).ok()?;
    let line = s.lines().find(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))?;
    parse_now(line.trim())
}

fn parse_now(s: &str) -> Option<DateTime<Local>> {
    let s = s.replace('T', " ");
    let naive = if s.len() >= 19 {
        NaiveDateTime::parse_from_str(&s[..19], "%Y-%m-%d %H:%M:%S").ok()
    } else if s.len() >= 16 {
        NaiveDateTime::parse_from_str(&s[..16], "%Y-%m-%d %H:%M").ok()
    } else {
        NaiveDate::parse_from_str(&s[..10], "%Y-%m-%d")
            .ok()
            .and_then(|d| d.and_hms_opt(12, 0, 0))
    }?;
    Local.from_local_datetime(&naive).single()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_date_and_time() {
        let t = parse_now("2026-09-15 08:20").unwrap();
        assert_eq!(t.format("%Y-%m-%d %H:%M").to_string(), "2026-09-15 08:20");
        let d = parse_now("2026-03-01").unwrap();
        assert_eq!(d.date_naive().to_string(), "2026-03-01");
    }
}
