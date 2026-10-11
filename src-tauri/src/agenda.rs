//! 本机日程 `agenda.json`。不上传、不拉取。退出登录时由用户决定是否删这个文件。

use crate::error::Result;
use crate::models::AgendaItem;
use chrono::NaiveDateTime;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn load(dir: &Path) -> Vec<AgendaItem> {
    fs::read(dir.join("agenda.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save(dir: &Path, items: &[AgendaItem]) -> Result<()> {
    fs::write(
        dir.join("agenda.json"),
        serde_json::to_vec_pretty(items).unwrap_or_default(),
    )?;
    Ok(())
}

pub fn parse_local(s: &str) -> Option<NaiveDateTime> {
    let s = s.trim().replace('T', " ");
    if s.len() >= 16 {
        NaiveDateTime::parse_from_str(&s.get(..16)?, "%Y-%m-%d %H:%M").ok()
    } else {
        None
    }
}

pub fn minutes_for_code(code: &str, start: &str, set_time: Option<&str>) -> Option<u32> {
    match code {
        "1" => Some(1),
        "2" => Some(10),
        "15" => Some(15),
        "3" => Some(30),
        "4" => Some(60),
        "5" => Some(120),
        "6" => Some(1440),
        "7" => minutes_before_set_time(start, set_time?),
        _ => None,
    }
}

fn minutes_before_set_time(start: &str, set_time: &str) -> Option<u32> {
    let naive = parse_local(start)?;
    let (h, m) = parse_hhmm(set_time)?;
    let prev = naive.date().pred_opt()?;
    let when = prev.and_hms_opt(h, m, 0)?;
    let mins = (naive - when).num_minutes();
    if mins < 1 {
        None
    } else {
        u32::try_from(mins).ok()
    }
}

fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    let mut p = s.trim().split(':');
    let h: u32 = p.next()?.parse().ok()?;
    let m: u32 = p.next()?.parse().ok()?;
    if p.next().is_some() || h > 23 || m > 59 {
        return None;
    }
    Some((h, m))
}

pub fn normalize(item: &mut AgendaItem) -> Result<()> {
    item.name = item.name.trim().to_string();
    if item.name.is_empty() {
        return Err("请填写日程名称".into());
    }
    let start = parse_local(&item.start).ok_or("请设置开始时间")?;
    let end = parse_local(&item.end).ok_or("请设置结束时间")?;
    if end <= start {
        return Err("结束时间必须晚于开始时间".into());
    }
    item.start = start.format("%Y-%m-%dT%H:%M").to_string();
    item.end = end.format("%Y-%m-%dT%H:%M").to_string();
    match item.location.as_mut() {
        Some(loc) => {
            *loc = loc.trim().to_string();
            if loc.is_empty() {
                item.location = None;
            } else if loc.chars().count() > 80 {
                return Err("地点最多 80 个字".into());
            }
        }
        None => {}
    }
    match item.note.as_mut() {
        Some(n) => {
            *n = n.trim().to_string();
            if n.is_empty() {
                item.note = None;
            }
        }
        None => {}
    }
    normalize_rule(item)?;
    if !item.remind {
        item.remind_minutes = None;
        item.remind_code = None;
        item.set_time = None;
        return Ok(());
    }
    let code = item.remind_code.clone().ok_or("提醒选项不对")?;
    let mins = minutes_for_code(&code, &item.start, item.set_time.as_deref()).ok_or("提醒选项不对")?;
    item.remind_code = Some(code.clone());
    item.remind_minutes = Some(mins);
    if code != "7" {
        item.set_time = None;
    } else if let Some(st) = item.set_time.as_mut() {
        let (h, m) = parse_hhmm(st).ok_or("请填写前一天的提醒时间")?;
        *st = format!("{h:02}:{m:02}");
    }
    Ok(())
}

fn normalize_rule(item: &mut AgendaItem) -> Result<()> {
    if !item.is_repet {
        item.repet_rule = None;
        return Ok(());
    }
    let mut rule = item.repet_rule.clone().ok_or("请选择重复频率")?;
    let freq = rule.freq.trim().to_ascii_uppercase();
    if !matches!(freq.as_str(), "DAILY" | "WEEKLY") {
        return Err("只能每天重复，或按星期重复".into());
    }
    rule.freq = freq.clone();
    if freq == "WEEKLY" {
        const ORDER: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
        let mut days = Vec::new();
        for raw in &rule.byday {
            if let Some(canon) = ORDER.iter().find(|x| x.eq_ignore_ascii_case(raw.trim())).copied() {
                if !days.iter().any(|d| d == canon) {
                    days.push(canon.to_string());
                }
            }
        }
        days.sort_by_key(|d| ORDER.iter().position(|x| x == d).unwrap_or(0));
        if days.is_empty() {
            return Err("请选择重复的星期".into());
        }
        rule.byday = days;
    } else {
        rule.byday.clear();
    }
    let mode = rule.endmode.trim().to_ascii_uppercase();
    if mode == "DATE" {
        let raw = rule.time.clone().unwrap_or_default();
        let day = raw.trim();
        if day.len() < 10 || parse_local(&format!("{day}T00:00")).is_none() {
            return Err("请设置重复何时结束".into());
        }
        let day = &day[..10];
        if day < &item.start[..10] {
            return Err("结束日期不能早于开始日期".into());
        }
        rule.endmode = "DATE".into();
        rule.time = Some(day.to_string());
        rule.num = 0;
    } else if mode == "NUMBER" {
        if !(1..=999).contains(&rule.num) {
            return Err("重复次数须在 1–999".into());
        }
        rule.endmode = "NUMBER".into();
        rule.time = None;
    } else {
        return Err("请设置重复何时结束".into());
    }
    item.repet_rule = Some(rule);
    Ok(())
}

fn new_id() -> String {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{n:x}{seq:x}")
}

pub fn upsert(dir: &Path, mut item: AgendaItem) -> Result<Vec<AgendaItem>> {
    normalize(&mut item)?;
    if item.id.trim().is_empty() {
        item.id = new_id();
    }
    let mut items = load(dir);
    if let Some(old) = items.iter_mut().find(|x| x.id == item.id) {
        *old = item;
    } else {
        items.push(item);
    }
    items.sort_by(|a, b| a.start.cmp(&b.start).then(a.id.cmp(&b.id)));
    save(dir, &items)?;
    Ok(items)
}

pub fn delete(dir: &Path, id: &str) -> Result<Vec<AgendaItem>> {
    let mut items = load(dir);
    items.retain(|x| x.id != id);
    save(dir, &items)?;
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AgendaItem, RepetRule};

    fn item(name: &str, start: &str, end: &str) -> AgendaItem {
        AgendaItem {
            id: "a".into(),
            name: name.into(),
            note: None,
            start: start.into(),
            end: end.into(),
            remind: false,
            remind_minutes: None,
            location: None,
            remind_code: None,
            set_time: None,
            is_repet: false,
            repet_rule: None,
        }
    }

    fn daily(num: u32) -> RepetRule {
        RepetRule {
            freq: "DAILY".into(),
            byday: Vec::new(),
            endmode: "NUMBER".into(),
            time: None,
            num,
        }
    }

    fn checked(mut item: AgendaItem) -> Result<()> {
        normalize(&mut item)
    }

    #[test]
    fn rejects_empty_name_and_inverted_range() {
        assert!(checked(item(" ", "2026-09-21T18:00", "2026-09-21T19:00")).is_err());
        assert!(checked(item("班会", "2026-09-21T19:00", "2026-09-21T18:00")).is_err());
        assert!(checked(item("班会", "2026-09-21T18:00", "2026-09-21T19:00")).is_ok());
    }

    #[test]
    fn parses_t_separator() {
        let t = parse_local("2026-09-21T18:05").unwrap();
        assert_eq!(t.format("%H:%M").to_string(), "18:05");
    }

    #[test]
    fn remind_codes_map_to_minutes() {
        assert_eq!(minutes_for_code("6", "2026-10-06T21:00", None), Some(1440));
        assert_eq!(minutes_for_code("15", "2026-10-06T21:00", None), Some(15));
        let mins = minutes_for_code("7", "2026-10-06T21:00", Some("18:00")).unwrap();
        assert_eq!(mins, 27 * 60);
        let mut ev = item("班会", "2026-10-06T21:00", "2026-10-06T22:00");
        ev.remind = true;
        ev.remind_code = Some("6".into());
        normalize(&mut ev).unwrap();
        assert_eq!(ev.remind_minutes, Some(1440));
    }

    #[test]
    fn weekly_rule_sorts_days() {
        let mut ev = item("例会", "2026-10-05T10:00", "2026-10-05T11:00");
        ev.is_repet = true;
        ev.repet_rule = Some(RepetRule {
            freq: "weekly".into(),
            byday: vec!["We".into(), "Mo".into(), "nope".into()],
            endmode: "date".into(),
            time: Some("2026-12-31".into()),
            num: 4,
        });
        normalize(&mut ev).unwrap();
        let rule = ev.repet_rule.unwrap();
        assert_eq!(rule.freq, "WEEKLY");
        assert_eq!(rule.byday, vec!["Mo".to_string(), "We".to_string()]);
        assert_eq!(rule.endmode, "DATE");
        assert_eq!(rule.time.as_deref(), Some("2026-12-31"));
        assert_eq!(rule.num, 0);
    }

    #[test]
    fn monthly_yearly_and_bad_rules_are_rejected() {
        let mut month = item("月报", "2026-10-06T09:00", "2026-10-06T10:00");
        month.is_repet = true;
        month.repet_rule = Some(RepetRule {
            freq: "MONTHLY".into(),
            byday: Vec::new(),
            endmode: "NUMBER".into(),
            time: None,
            num: 3,
        });
        assert!(normalize(&mut month).is_err());

        let mut year = item("年会", "2026-10-06T09:00", "2026-10-06T10:00");
        year.is_repet = true;
        year.repet_rule = Some(RepetRule {
            freq: "YEARLY".into(),
            byday: Vec::new(),
            endmode: "NUMBER".into(),
            time: None,
            num: 2,
        });
        assert!(normalize(&mut year).is_err());

        let mut bad = item("班会", "2026-10-06T21:00", "2026-10-06T22:00");
        bad.is_repet = true;
        bad.repet_rule = Some(daily(0));
        assert!(normalize(&mut bad).is_err());
        bad.repet_rule = Some(RepetRule {
            freq: "DAILY".into(),
            byday: Vec::new(),
            endmode: "DATE".into(),
            time: Some("2026-10-01".into()),
            num: 0,
        });
        assert!(normalize(&mut bad).is_err());
        bad.repet_rule = Some(RepetRule {
            freq: "WEEKLY".into(),
            byday: Vec::new(),
            endmode: "NUMBER".into(),
            time: None,
            num: 2,
        });
        assert!(normalize(&mut bad).is_err());
    }
}
