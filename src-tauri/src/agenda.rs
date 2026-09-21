//! 用户自定义日程。只存在本机 `agenda.json`，不打学校。

use crate::error::Result;
use crate::models::AgendaItem;
use chrono::NaiveDateTime;
use std::fs;
use std::path::Path;
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

pub fn validate(item: &AgendaItem) -> Result<()> {
    if item.name.trim().is_empty() {
        return Err("请填写日程名称".into());
    }
    let start = parse_local(&item.start).ok_or("请设置开始时间")?;
    let end = parse_local(&item.end).ok_or("请设置结束时间")?;
    if end <= start {
        return Err("结束时间必须晚于开始时间".into());
    }
    if item.remind {
        let m = item.remind_minutes.unwrap_or(15);
        if m < 1 || m > 180 {
            return Err("提醒时间须在 1–180 分钟".into());
        }
    }
    Ok(())
}

fn new_id() -> String {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{n:x}")
}

pub fn upsert(dir: &Path, mut item: AgendaItem) -> Result<Vec<AgendaItem>> {
    validate(&item)?;
    if item.id.trim().is_empty() {
        item.id = new_id();
    }
    item.name = item.name.trim().to_string();
    match &mut item.note {
        Some(n) => {
            *n = n.trim().to_string();
            if n.is_empty() {
                item.note = None;
            }
        }
        None => {}
    }
    if !item.remind {
        item.remind_minutes = None;
    } else if item.remind_minutes.unwrap_or(0) == 0 {
        item.remind_minutes = Some(15);
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
    use crate::models::AgendaItem;

    fn item(name: &str, start: &str, end: &str) -> AgendaItem {
        AgendaItem {
            id: "a".into(),
            name: name.into(),
            note: None,
            start: start.into(),
            end: end.into(),
            remind: false,
            remind_minutes: None,
        }
    }

    #[test]
    fn rejects_empty_name_and_inverted_range() {
        assert!(validate(&item(" ", "2026-09-21T18:00", "2026-09-21T19:00")).is_err());
        assert!(validate(&item("班会", "2026-09-21T19:00", "2026-09-21T18:00")).is_err());
        assert!(validate(&item("班会", "2026-09-21T18:00", "2026-09-21T19:00")).is_ok());
    }

    #[test]
    fn parses_t_separator() {
        let t = parse_local("2026-09-21T18:05").unwrap();
        assert_eq!(t.format("%H:%M").to_string(), "18:05");
    }
}
