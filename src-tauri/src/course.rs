//! jwxt 整学期课表：解析、缓存、今日/周次推导。
//!
//! 网页周课表会打 `getMyScheduleDetail.do?type=week&week=n`，服务端按教学周筛格子。
//! 同步时按周拉齐，用点名结果当 `weeks[]`。金智 `wdkb`/`SKZC` 在东大未开通。
//! 周参数无效时回退：格子全文优先于 `weeksAndTeachers`；单/双只修饰紧挨着的周段。

use crate::models::{Course, SemesterTable, Snapshot, TermWeek};
use crate::periods::{
    majority_campus, resolve_campus, times_for, Campus,
};
use crate::error::Result;
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde_json::Value;
use std::fs;
use std::path::Path;

pub fn semester_path(dir: &Path) -> std::path::PathBuf {
    dir.join("semester.json")
}

pub fn load_semester(dir: &Path) -> Option<SemesterTable> {
    let raw = fs::read(semester_path(dir)).ok()?;
    serde_json::from_slice(&raw).ok()
}

pub fn save_semester(dir: &Path, sem: &SemesterTable) -> Result<()> {
    fs::write(semester_path(dir), serde_json::to_vec_pretty(sem)?)?;
    Ok(())
}

pub fn monday_of(d: NaiveDate) -> NaiveDate {
    let wd = d.weekday().num_days_from_monday() as i64;
    d - Duration::days(wd)
}

pub fn parse_ymd(s: &str) -> Option<NaiveDate> {
    let t = s.trim().replace('/', "-");
    let t = t.replace('年', "-").replace('月', "-").replace('日', "");
    let digits: String = t
        .chars()
        .map(|c| if c.is_ascii_digit() || c == '-' { c } else { ' ' })
        .collect();
    let parts: Vec<&str> = digits.split_whitespace().collect();
    let blob = parts.first().copied().unwrap_or(&t);
    let compact: String = blob.chars().filter(|c| c.is_ascii_digit()).collect();
    if compact.len() >= 8 {
        let y: i32 = compact.get(0..4)?.parse().ok()?;
        let m: u32 = compact.get(4..6)?.parse().ok()?;
        let d: u32 = compact.get(6..8)?.parse().ok()?;
        if let Some(date) = NaiveDate::from_ymd_opt(y, m, d) {
            return Some(date);
        }
    }
    let segs: Vec<&str> = blob.split('-').filter(|x| !x.is_empty()).collect();
    if segs.len() >= 3 {
        let y: i32 = segs[0].parse().ok()?;
        let m: u32 = segs[1].parse().ok()?;
        let d: u32 = segs[2].parse().ok()?;
        return NaiveDate::from_ymd_opt(y, m, d);
    }
    None
}

/// 秋季默认 9 月 1 日所在周周一；春季默认次年 2 月最后一个周一附近。
pub fn default_week1(term: &str) -> String {
    let parts: Vec<&str> = term.split('-').collect();
    if parts.len() >= 3 {
        let y: i32 = parts[0].parse().unwrap_or(0);
        if y >= 2000 {
            if parts[2].starts_with('1') {
                if let Some(d) = NaiveDate::from_ymd_opt(y, 9, 1) {
                    return monday_of(d).format("%Y-%m-%d").to_string();
                }
            } else if parts[2].starts_with('2') {
                if let Some(d) = NaiveDate::from_ymd_opt(y + 1, 3, 1) {
                    // 春季多在 2 月底，用 3/1 往前一个周一偏晚；改用 2/24 所在周
                    let start = NaiveDate::from_ymd_opt(y + 1, 2, 24).unwrap_or(d);
                    return monday_of(start).format("%Y-%m-%d").to_string();
                }
            }
        }
    }
    String::new()
}

fn weekday_digit(ch: &str) -> Option<u8> {
    Some(match ch {
        "一" | "1" => 1,
        "二" | "2" => 2,
        "三" | "3" => 3,
        "四" | "4" => 4,
        "五" | "5" => 5,
        "六" | "6" => 6,
        "日" | "天" | "7" => 7,
        _ => return None,
    })
}

/// 「14-15周」里的「周」不是「周四」；「周15」是第 15 周，不是周一。
fn zhou_is_weekday(s: &str, start: usize, whole: &str, captured: &str) -> bool {
    if !whole.starts_with('周') || whole.starts_with("星期") {
        return true;
    }
    if start > 0 {
        if s[..start].chars().last().is_some_and(|c| c.is_ascii_digit()) {
            return false;
        }
    }
    if captured.len() == 1 && captured.chars().all(|c| c.is_ascii_digit()) {
        let end = start + whole.len();
        if s[end..].chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return false;
        }
    }
    true
}

fn weekday_re() -> regex::Regex {
    regex::Regex::new(r"(?:星期|礼拜|周)([一二三四五六日天1-7])").expect("weekday")
}

/// 只认「星期X / 周X / 礼拜X」，不要把「体育(一)」「生成式…（一）」里的「一」当成周一。
pub fn parse_weekday(raw: &str) -> Option<u8> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(n) = s.parse::<u8>() {
        if (1..=7).contains(&n) {
            return Some(n);
        }
        if n == 0 {
            return Some(7);
        }
    }
    let re = weekday_re();
    let mut week_token = None;
    for cap in re.captures_iter(s) {
        let m = cap.get(0)?;
        let whole = m.as_str();
        let ch = cap.get(1).map(|x| x.as_str()).unwrap_or("");
        if !zhou_is_weekday(s, m.start(), whole, ch) {
            continue;
        }
        let Some(n) = weekday_digit(ch) else { continue };
        if whole.starts_with("星期") || whole.starts_with("礼拜") {
            return Some(n);
        }
        if week_token.is_none() {
            week_token = Some(n);
        }
    }
    if week_token.is_some() {
        return week_token;
    }
    let low = s.to_ascii_lowercase();
    if low.contains("monday") || low == "mon" {
        return Some(1);
    }
    if low.contains("tuesday") || low == "tue" {
        return Some(2);
    }
    if low.contains("wednesday") || low == "wed" {
        return Some(3);
    }
    if low.contains("thursday") || low == "thu" {
        return Some(4);
    }
    if low.contains("friday") || low == "fri" {
        return Some(5);
    }
    if low.contains("saturday") || low == "sat" {
        return Some(6);
    }
    if low.contains("sunday") || low == "sun" {
        return Some(7);
    }
    None
}

fn fold_name(s: &str) -> String {
    s.replace('（', "(")
        .replace('）', ")")
        .replace('㈠', "(一)")
        .replace('㈡', "(二)")
        .replace('㈢', "(三)")
        .replace([' ', '\u{3000}'], "")
}

fn names_eq(a: &str, b: &str) -> bool {
    fold_name(a) == fold_name(b)
}

pub fn teaching_week(week1_monday: &str, today: NaiveDate) -> Option<u32> {
    let start = parse_ymd(week1_monday).map(monday_of)?;
    if today < start {
        return None;
    }
    let days = (today - start).num_days();
    Some((days / 7 + 1) as u32)
}

pub fn teaching_week_from_term(weeks: &[TermWeek], today: NaiveDate) -> Option<u32> {
    let t = today.format("%Y-%m-%d").to_string();
    for w in weeks {
        if w.n == 0 {
            continue;
        }
        let s = w.start.get(..10).unwrap_or(&w.start);
        let e = w.end.get(..10).unwrap_or(&w.end);
        if t.as_str() >= s && t.as_str() <= e {
            return Some(w.n);
        }
    }
    None
}

pub fn parse_term_weeks(v: &Value) -> Vec<TermWeek> {
    let arr = walk_array(v, &["datas", "weeks", "rows", "list", "schoolCalendars"]);
    let Some(arr) = arr else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in arr {
        let n = json_u32(
            item,
            &["serialNumber", "week", "zc", "ZC", "SKZC", "name"],
        )
        .or_else(|| {
            json_str(item, &["name", "itemName"]).and_then(|s| {
                let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
                digits.parse().ok()
            })
        });
        let Some(n) = n.filter(|n| (1..=30).contains(n)) else {
            continue;
        };
        let start = json_str(item, &["startDate", "KSRQ", "beginDate", "XQKSRQ"])
            .and_then(|s| parse_ymd(&s))
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        let end = json_str(item, &["endDate", "JSRQ", "XQJSRQ"])
            .and_then(|s| parse_ymd(&s))
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        out.push(TermWeek { n, start, end });
    }
    out.sort_by_key(|w| w.n);
    out.dedup_by_key(|w| w.n);
    out
}

fn arranged_item_key(it: &Value) -> String {
    format!(
        "{}|{}|{}|{}",
        json_str(it, &["courseName", "KCMC", "kcmc", "name"]).unwrap_or_default(),
        json_str(it, &["dayOfWeek", "SKXQ", "weekday"]).unwrap_or_default(),
        json_u32(it, &["beginSection", "KSJC"]).unwrap_or(0),
        json_u32(it, &["endSection", "JSJC"]).unwrap_or(0)
    )
}

/// 带 `week=` 的格子课表若被服务端筛过，条数或课集合会与整学期不同。
pub fn week_param_filters(full: &[Value], weekly: &[Value]) -> bool {
    if weekly.is_empty() {
        return !full.is_empty();
    }
    if weekly.len() != full.len() {
        return true;
    }
    let a: std::collections::BTreeSet<_> = full.iter().map(arranged_item_key).collect();
    let b: std::collections::BTreeSet<_> = weekly.iter().map(arranged_item_key).collect();
    a != b
}

pub fn stamp_courses_week(mut courses: Vec<Course>, week: u32) -> Vec<Course> {
    for c in &mut courses {
        c.weeks = Some(vec![week]);
    }
    courses
}

fn place_merge_key(p: &str) -> String {
    let t = tidy_place(p);
    if place_has_room(&t) {
        t
    } else {
        String::new()
    }
}

/// 把「第 n 周接口返回的课」合成学期表：周次以服务端点名为准，地点不同则拆开。
pub fn merge_weekly_stamped(parts: Vec<Vec<Course>>) -> Vec<Course> {
    let mut map: Vec<Course> = Vec::new();
    for cs in parts {
        for c in cs {
            if let Some(ex) = map.iter_mut().find(|x| {
                names_eq(&x.name, &c.name)
                    && x.weekday == c.weekday
                    && x.sections == c.sections
                    && place_merge_key(&x.place) == place_merge_key(&c.place)
                    && x.teacher == c.teacher
            }) {
                let mut set: std::collections::BTreeSet<u32> =
                    ex.weeks.clone().unwrap_or_default().into_iter().collect();
                if let Some(ws) = &c.weeks {
                    set.extend(ws.iter().copied());
                }
                ex.weeks = Some(set.into_iter().collect());
                if !place_has_room(&ex.place) && place_has_room(&c.place) {
                    ex.place = c.place;
                }
            } else {
                map.push(c);
            }
        }
    }
    map
}

pub fn weekday_of(today: NaiveDate) -> u8 {
    match today.weekday() {
        Weekday::Mon => 1,
        Weekday::Tue => 2,
        Weekday::Wed => 3,
        Weekday::Thu => 4,
        Weekday::Fri => 5,
        Weekday::Sat => 6,
        Weekday::Sun => 7,
    }
}

pub fn in_week(c: &Course, week: u32) -> bool {
    match &c.weeks {
        Some(ws) if !ws.is_empty() => ws.contains(&week),
        _ => true,
    }
}

pub fn courses_today(sem: &SemesterTable, week: Option<u32>, weekday: u8) -> Vec<Course> {
    let week = match week {
        Some(w) => w,
        None => return Vec::new(),
    };
    let mut out: Vec<Course> = sem
        .courses
        .iter()
        .filter(|c| c.weekday == weekday)
        .filter(|c| in_week(c, week))
        .cloned()
        .collect();
    out.sort_by(|a, b| a.start.cmp(&b.start).then(a.name.cmp(&b.name)));
    out
}

pub fn apply_semester(snap: &mut Snapshot, mut sem: SemesterTable, week1_override: Option<&str>) {
    let today = crate::clock::today();
    if sem.week1_monday.trim().is_empty() {
        sem.week1_monday = default_week1(&sem.semester_id);
    }
    let week1 = week1_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&sem.week1_monday);
    let user_set_week1 = week1_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some();
    let week = if !user_set_week1 {
        if let Some(w) = teaching_week_from_term(&sem.term_weeks, today) {
            Some(w)
        } else if week1.is_empty() {
            teaching_week(&default_week1(&sem.semester_id), today)
        } else {
            teaching_week(week1, today)
        }
    } else if week1.is_empty() {
        teaching_week(&default_week1(&sem.semester_id), today)
    } else {
        teaching_week(week1, today)
    };
    let weekday = weekday_of(today);
    snap.teaching_week = week;
    snap.courses_today = Some(courses_today(&sem, week, weekday));
    snap.semester = Some(sem);
}

pub fn semester_cache_stale(sem: &SemesterTable) -> bool {
    let today = crate::clock::today();
    if let Some(d) = parse_ymd(&sem.week1_monday) {
        return today > d + Duration::days(140);
    }
    if let Some(d) = parse_ymd(&sem.fetched_at) {
        return today > d + Duration::days(120);
    }
    false
}

pub fn guess_term(today: NaiveDate) -> String {
    let y = today.year();
    let m = today.month();
    if (8..=12).contains(&m) {
        format!("{y}-{}-1", y + 1)
    } else if m == 1 {
        format!("{}-{y}-1", y - 1)
    } else {
        format!("{}-{y}-2", y - 1)
    }
}

pub fn term_label(id: &str, name: Option<&str>) -> String {
    if let Some(n) = name {
        let t = n.trim();
        if !t.is_empty() {
            return t
                .replace("学年", " ")
                .replace("第一学期", "秋季")
                .replace("第二学期", "春季")
                .replace("第1学期", "秋季")
                .replace("第2学期", "春季")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
        }
    }
    if let Some(rest) = id.strip_suffix("-1") {
        format!("{rest} 秋季")
    } else if let Some(rest) = id.strip_suffix("-2") {
        format!("{rest} 春季")
    } else {
        id.to_string()
    }
}

fn json_str(v: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(x) = v.get(*k) {
            if let Some(s) = x.as_str() {
                let t = s.trim();
                if !t.is_empty() && t != "null" {
                    return Some(t.to_string());
                }
            }
            if let Some(n) = x.as_i64() {
                return Some(n.to_string());
            }
            if let Some(n) = x.as_u64() {
                return Some(n.to_string());
            }
            if let Some(f) = x.as_f64() {
                return Some(f.to_string());
            }
        }
    }
    None
}

fn json_u32(v: &Value, keys: &[&str]) -> Option<u32> {
    json_str(v, keys)?.parse().ok()
}

fn walk_array<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a Vec<Value>> {
    if let Some(obj) = v.as_object() {
        for k in keys {
            if let Some(arr) = obj.get(*k).and_then(|x| x.as_array()) {
                return Some(arr);
            }
        }
        for val in obj.values() {
            if let Some(arr) = walk_array(val, keys) {
                return Some(arr);
            }
        }
    } else if let Some(arr) = v.as_array() {
        for val in arr {
            if let Some(found) = walk_array(val, keys) {
                return Some(found);
            }
        }
    }
    None
}

fn walk_obj<'a>(v: &'a Value, pred: &dyn Fn(&serde_json::Map<String, Value>) -> bool) -> Option<&'a Value> {
    match v {
        Value::Object(map) => {
            if pred(map) {
                return Some(v);
            }
            for val in map.values() {
                if let Some(hit) = walk_obj(val, pred) {
                    return Some(hit);
                }
            }
            None
        }
        Value::Array(arr) => {
            for val in arr {
                if let Some(hit) = walk_obj(val, pred) {
                    return Some(hit);
                }
            }
            None
        }
        _ => None,
    }
}

pub fn normalize_hhmm(raw: &str) -> Option<String> {
    let re = regex::Regex::new(r"(\d{1,2}):(\d{2})").ok()?;
    let c = re.captures(raw.trim())?;
    let h: u32 = c[1].parse().ok()?;
    let m: u32 = c[2].parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some(format!("{h:02}:{m:02}"))
}

/// 课号 `A115658` 常直接粘在 `14-15周` 前面，先切开再认周次。
fn peel_course_codes(s: &str) -> String {
    // 课号是字母+6位，不能把「18周」的 1 吃进课号。
    let re = regex::Regex::new(r"([A-Za-z]\d{6})(\d{1,2}(?:\s*[-~—至到]\s*\d{1,2})?周)")
        .expect("course code peel");
    re.replace_all(s, "$1 $2").into_owned()
}

/// 只认「N周 / N-M周 / 单双周 / 比特串」。不要把「1号B409」里的数字当周次。
pub fn parse_weeks(raw: &str) -> Option<Vec<u32>> {
    let s = peel_course_codes(raw);
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() >= 8 && compact.chars().all(|c| c == '0' || c == '1') {
        let mut weeks = Vec::new();
        for (i, c) in compact.chars().enumerate() {
            if c == '1' {
                weeks.push((i + 1) as u32);
            }
        }
        return if weeks.is_empty() { None } else { Some(weeks) };
    }

    let chunks = week_chunks(s);
    if chunks.is_empty() {
        // 只认「单周 / 双周」，不要把「李金双」里的「双」当成双周。
        if compact.contains("单周") {
            return Some((1..=22).filter(|w| w % 2 == 1).collect());
        }
        if compact.contains("双周") {
            return Some((1..=22).filter(|w| w % 2 == 0).collect());
        }
        return None;
    }
    let mut set = std::collections::BTreeSet::new();
    for c in chunks {
        set.extend(c);
    }
    if set.is_empty() {
        None
    } else {
        Some(set.into_iter().collect())
    }
}

fn week_expr_re() -> regex::Regex {
    // 「4周,7-8周」算一段；「14-15周 薛丽芳 … 16-17周 张昱」仍是两段。
    regex::Regex::new(
        r"(?:\d{1,2}(?:\s*[-~—至到]\s*\d{1,2})?(?:\s*[,，、]\s*\d{1,2}(?:\s*[-~—至到]\s*\d{1,2})?)*\s*周(?:\s*[,，、]\s*)?)+",
    )
    .expect("week expr")
}

fn is_kind_word(s: &str) -> bool {
    matches!(
        s.trim(),
        "理论"
            | "实验"
            | "实习"
            | "上机"
            | "实践"
            | "考查"
            | "考试"
            | "百分制"
            | "五级制"
            | "必修"
            | "选修"
            | "未知"
            | "主讲"
            | "辅讲"
            | "null"
            | "星期一"
            | "星期二"
            | "星期三"
            | "星期四"
            | "星期五"
            | "星期六"
            | "星期日"
            | "星期天"
            | "周一"
            | "周二"
            | "周三"
            | "周四"
            | "周五"
            | "周六"
            | "周日"
            | "周天"
    )
}

fn looks_like_person_token(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || is_kind_word(t) {
        return false;
    }
    if t.starts_with("星期") || t.starts_with("礼拜") {
        return false;
    }
    let n = t.chars().count();
    n >= 2
        && n <= 4
        && t.chars()
            .all(|c| ('\u{4e00}'..='\u{9fff}').contains(&c) || c == '·')
}

fn looks_like_teacher(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || is_kind_word(t) {
        return false;
    }
    if t.starts_with("星期") || t.starts_with("礼拜") || t.contains('节') {
        return false;
    }
    if looks_like_person_token(t) {
        return true;
    }
    let n = t.chars().count();
    n >= 2 && n <= 16 && !t.chars().any(|c| c.is_ascii_digit())
}

fn take_parity_prefix(s: &str) -> (bool, bool, &str) {
    let trimmed = s.trim_start();
    let lead = s.len() - trimmed.len();
    let eat = |p: &str| trimmed.strip_prefix(p).map(|_| &s[lead + p.len()..]);
    if let Some(rest) = eat("(单)").or_else(|| eat("（单）")).or_else(|| eat("单周")) {
        return (true, false, rest);
    }
    if let Some(rest) = eat("(双)").or_else(|| eat("（双）")).or_else(|| eat("双周")) {
        return (false, true, rest);
    }
    (false, false, s)
}

fn expand_week_range(lo: u32, hi: u32, odd: bool, even: bool) -> Vec<u32> {
    let (lo, hi) = (lo.min(hi), lo.max(hi).min(22));
    let mut out = Vec::new();
    for w in lo..=hi {
        if w == 0 {
            continue;
        }
        if odd && w % 2 == 0 {
            continue;
        }
        if even && w % 2 == 1 {
            continue;
        }
        out.push(w);
    }
    out
}

/// 「3-8周,11-13周(单)」里的单/双只作用在紧挨着的那段，不是整串。
fn parse_week_expr_per_range(expr: &str, after: &str) -> Vec<u32> {
    let re = regex::Regex::new(r"(\d{1,2})(?:\s*[-~—至到]\s*(\d{1,2}))?").unwrap();
    let mut atoms: Vec<(u32, u32, bool, bool)> = Vec::new();
    let mut pos = 0;
    while pos < expr.len() {
        let slice = &expr[pos..];
        let skip: usize = slice
            .chars()
            .take_while(|c| *c == ',' || *c == '，' || *c == '、' || c.is_whitespace())
            .map(|c| c.len_utf8())
            .sum();
        if skip > 0 {
            pos += skip;
            continue;
        }
        let Some(cap) = re.captures(slice) else { break };
        let m = cap.get(0).unwrap();
        if m.start() != 0 {
            break;
        }
        let lo: u32 = cap.get(1).unwrap().as_str().parse().unwrap_or(0);
        let hi: u32 = cap
            .get(2)
            .and_then(|x| x.as_str().parse().ok())
            .unwrap_or(lo);
        pos += m.end();
        if let Some(rest) = expr.get(pos..) {
            let t = rest.trim_start();
            let pad = rest.len() - t.len();
            if t.starts_with('周') {
                pos += pad + '周'.len_utf8();
            }
        }
        let rest = expr.get(pos..).unwrap_or("");
        let (odd, even, rest2) = take_parity_prefix(rest);
        pos += rest.len() - rest2.len();
        if lo > 0 {
            atoms.push((lo.min(hi), lo.max(hi), odd, even));
        }
    }
    if let Some(last) = atoms.last_mut() {
        if !last.2 && !last.3 {
            let (odd, even, _) = take_parity_prefix(after);
            last.2 = odd;
            last.3 = even;
        }
    }
    let mut set = std::collections::BTreeSet::new();
    for (lo, hi, odd, even) in atoms {
        set.extend(expand_week_range(lo, hi, odd, even));
    }
    set.into_iter().collect()
}

fn week_chunks(s: &str) -> Vec<Vec<u32>> {
    let re = week_expr_re();
    let mut out = Vec::new();
    for cap in re.captures_iter(s) {
        let m = cap.get(0).unwrap();
        let weeks = parse_week_expr_per_range(m.as_str(), s.get(m.end()..).unwrap_or(""));
        if !weeks.is_empty() {
            out.push(weeks);
        }
    }
    if out.is_empty() {
        // 「1-16」这种没有「周」的校历字段，但不要解析含校区/教室的句子
        if !s.contains("校区") && !s.contains('号') && !s.contains('楼') && !s.contains('室') {
            if s.contains('-')
                || s.contains('~')
                || s.contains(',')
                || s.contains('，')
                || s.contains('单')
                || s.contains('双')
            {
                let weeks = parse_week_expr_per_range(s, "");
                if !weeks.is_empty() {
                    out.push(weeks);
                }
            }
        }
    }
    out
}

#[derive(Debug, Clone)]
struct WeekSeg {
    weeks: Vec<u32>,
    teacher: Option<String>,
    place: Option<String>,
}

#[derive(Debug, Clone)]
struct SlashSlot {
    weeks: Vec<u32>,
    weekday: Option<u8>,
    sections: Option<(u32, u32)>,
    teacher: Option<String>,
    place: Option<String>,
}

fn cn_section_num(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Ok(n) = s.parse::<u32>() {
        return (1..=12).contains(&n).then_some(n);
    }
    let map = [
        ("十二", 12u32),
        ("十一", 11),
        ("十", 10),
        ("九", 9),
        ("八", 8),
        ("七", 7),
        ("六", 6),
        ("五", 5),
        ("四", 4),
        ("三", 3),
        ("两", 2),
        ("二", 2),
        ("一", 1),
    ];
    for (k, n) in map {
        if s == k {
            return Some(n);
        }
    }
    None
}

/// 「第三节-第四节」「第九节-第十节」「第5-6节」
fn parse_sections_cn(raw: &str) -> Option<(u32, u32)> {
    let re = regex::Regex::new(
        r"第?\s*([一二三四五六七八九十两]+|\d{1,2})\s*节?\s*[-~—至到]?\s*第?\s*([一二三四五六七八九十两]+|\d{1,2})?\s*节",
    )
    .ok()?;
    let c = re.captures(raw.trim())?;
    let a = cn_section_num(c.get(1)?.as_str())?;
    let b = c
        .get(2)
        .and_then(|m| cn_section_num(m.as_str()))
        .unwrap_or(a);
    Some((a.clamp(1, 12), b.max(a).clamp(1, 12)))
}

fn strip_kind_brackets(s: &str) -> String {
    regex::Regex::new(r"\[[^\]]*\]")
        .map(|re| re.replace_all(s, "").into_owned())
        .unwrap_or_else(|_| s.to_string())
}

fn parse_teacher_list(raw: &str) -> Option<String> {
    let s = strip_kind_brackets(raw);
    let people: Vec<&str> = s
        .split(|ch| ch == ',' || ch == '，' || ch == '/' || ch == '、')
        .map(str::trim)
        .filter(|p| looks_like_teacher(p))
        .collect();
    if people.is_empty() {
        None
    } else {
        Some(people.join("，"))
    }
}

/// 金智真实格式：
/// arrangedList `18周[理论]/李金双[主讲]`
/// courses.do `11周[理论]/星期五/第五节-第六节/杨晓翠[主讲]/1号B409-微机室`
/// 槽与槽之间是中文逗号「，」，周次列表才用英文逗号「,」。
fn parse_slash_schedule(s: &str) -> Vec<SlashSlot> {
    let s = peel_course_codes(s)
        .replace("<br/>", "\n")
        .replace("<br>", "\n")
        .replace('；', "\n")
        .replace(';', "\n");
    if !s.contains('/') {
        return split_week_segments(&s)
            .into_iter()
            .map(|seg| SlashSlot {
                weeks: seg.weeks,
                weekday: None,
                sections: None,
                teacher: seg.teacher,
                place: seg.place,
            })
            .collect();
    }
    let mut out = Vec::new();
    for piece in s.split(|c| c == '，' || c == '\n') {
        let piece = piece.trim();
        if piece.is_empty() {
            continue;
        }
        if let Some(slot) = parse_one_slash_slot(piece) {
            out.push(slot);
        }
    }
    out
}

fn parse_one_slash_slot(s: &str) -> Option<SlashSlot> {
    let parts: Vec<&str> = s.split('/').map(str::trim).collect();
    if parts.is_empty() || parts[0].is_empty() {
        return None;
    }
    let weeks = parse_weeks(&strip_kind_brackets(parts[0]))?;
    if parts.len() >= 3 {
        if let Some(wd) = parse_weekday(parts[1]) {
            let sections = parse_sections_cn(parts[2]);
            let teacher = parts.get(3).and_then(|p| parse_teacher_list(p));
            let place = parts
                .get(4)
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .map(|p| p.to_string());
            return Some(SlashSlot {
                weeks,
                weekday: Some(wd),
                sections,
                teacher,
                place,
            });
        }
    }
    if parts.len() == 1 {
        return Some(SlashSlot {
            weeks,
            weekday: None,
            sections: None,
            teacher: extract_teacher(parts[0]),
            place: extract_place(parts[0]),
        });
    }
    let teacher = parts.get(1).and_then(|p| parse_teacher_list(p));
    let place = parts
        .get(2)
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|p| p.to_string());
    Some(SlashSlot {
        weeks,
        weekday: None,
        sections: None,
        teacher,
        place,
    })
}

fn slot_to_seg(s: &SlashSlot) -> WeekSeg {
    WeekSeg {
        weeks: s.weeks.clone(),
        teacher: s.teacher.clone(),
        place: s.place.clone(),
    }
}

fn extract_teacher(s: &str) -> Option<String> {
    let re = regex::Regex::new(r"[\[【(（]([^\]】)）]{1,20})[\]】)）]").ok()?;
    if let Some(c) = re.captures(s) {
        let t = c[1].trim();
        if looks_like_person_token(t) {
            return Some(t.to_string());
        }
        let people: Vec<&str> = t
            .split(|ch| ch == ',' || ch == '，' || ch == '/' || ch == '、')
            .map(str::trim)
            .filter(|p| looks_like_person_token(p))
            .collect();
        if !people.is_empty() {
            return Some(people.join("，"));
        }
    }
    // 「7-16周 理论 张松伟 浑南校区」：跳过课程性质，取真实姓名。
    // 「4周,7-8周 曾觉吾」：从最后一段周次表达式之后开始取。
    let after = week_expr_re()
        .find(s)
        .map(|m| s[m.end()..].to_string())
        .unwrap_or_else(|| s.to_string());
    let cut = ["浑南", "南湖", "校区", "考试", "考查"]
        .iter()
        .filter_map(|k| after.find(k))
        .min()
        .unwrap_or(after.len());
    let mid = after[..cut].trim();
    let people: Vec<&str> = mid
        .split(|ch: char| {
            ch.is_whitespace() || ch == ',' || ch == '，' || ch == '/' || ch == '、' || ch == ';'
        })
        .map(str::trim)
        .filter(|p| looks_like_person_token(p))
        .collect();
    if people.is_empty() {
        None
    } else {
        Some(people.join("，"))
    }
}

pub fn sanitize_teacher(t: Option<String>) -> Option<String> {
    let s = t?.trim().to_string();
    if s.is_empty() || is_kind_word(&s) {
        return None;
    }
    if s.split(|c| c == ',' || c == '，').all(is_kind_word) {
        return None;
    }
    Some(s)
}

fn extract_place(s: &str) -> Option<String> {
    let keys = [
        "浑南", "南湖", "校区", "号楼", "教学", "体育馆", "体育场", "操场", "球场", "游泳",
        "风雨", "微机",
    ];
    let pos = keys
        .iter()
        .filter_map(|k| s.find(k))
        .min()
        .or_else(|| {
            regex::Regex::new(r"\d+号")
                .ok()
                .and_then(|re| re.find(s).map(|m| m.start()))
        })?;
    let rest = s[pos..].trim();
    let mut stop = rest.find('<').unwrap_or(rest.len());
    if let Some(m) = week_expr_re().find(rest) {
        if m.start() > 0 {
            stop = stop.min(m.start());
        }
    }
    for k in ["考试", "考查", "百分制", "五级制"] {
        if let Some(i) = rest.find(k) {
            if i > 0 {
                stop = stop.min(i);
            }
        }
    }
    let p = rest[..stop].trim().trim_end_matches(['/', '；', ';', '、', ' ']);
    if p.chars().count() >= 2 {
        Some(p.to_string())
    } else {
        None
    }
}

fn looks_like_room_fragment(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || t.chars().count() > 24 {
        return false;
    }
    if week_expr_re().is_match(t) {
        return false;
    }
    if t.contains("考试")
        || t.contains("考查")
        || t.contains("百分制")
        || t.contains("五级制")
        || t.contains('类')
        || t.contains('班')
    {
        return false;
    }
    if regex::Regex::new(r"^[A-Za-z]\d{1,4}(?:\s*[-－].{0,16})?$")
        .map(|re| re.is_match(t))
        .unwrap_or(false)
    {
        return true;
    }
    if t.contains('室') || t.contains('楼') || t.contains("微机") || t.contains("语音") {
        return true;
    }
    regex::Regex::new(r"^\d{1,2}号[A-Za-z]?\d{0,4}")
        .map(|re| re.is_match(t) && !t.contains('周'))
        .unwrap_or(false)
}

fn join_wrapped_schedule_lines(lines: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in lines {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(prev) = out.last_mut() {
            if looks_like_room_fragment(t)
                && (week_expr_re().is_match(prev)
                    || prev.contains("校区")
                    || prev.trim_end().ends_with('号'))
            {
                prev.push(' ');
                prev.push_str(t);
                continue;
            }
        }
        out.push(t.to_string());
    }
    out.join("\n")
}

fn place_has_room(p: &str) -> bool {
    let t = tidy_place(p);
    let t = t.trim();
    !t.is_empty() && !matches!(t, "浑南校区" | "南湖校区" | "浑南" | "南湖")
}

fn resolve_seg_place(cell_place: &str, seg: &WeekSeg, multi: bool, sibling_has_room: bool) -> String {
    let seg_p = seg
        .place
        .as_deref()
        .map(tidy_place)
        .filter(|p| !p.is_empty());
    if multi {
        if let Some(p) = seg_p {
            if place_has_room(&p) {
                return p;
            }
            return String::new();
        }
        if sibling_has_room {
            return String::new();
        }
        return cell_place.to_string();
    }
    if let Some(p) = seg_p {
        if place_has_room(&p) || cell_place.is_empty() {
            return p;
        }
    }
    cell_place.to_string()
}

/// 把「14-15周 薛丽芳 浑南校区1号B409」拆成多段，每段自己的周次和老师。
fn split_week_segments(s: &str) -> Vec<WeekSeg> {
    let s = peel_course_codes(s);
    let s = s
        .replace("<br/>", "\n")
        .replace("<br>", "\n")
        .replace("；", "\n")
        .replace(';', "\n");
    let lines: Vec<String> = s
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let s = join_wrapped_schedule_lines(&lines);
    let re = week_expr_re();
    let matches: Vec<regex::Match> = re.find_iter(&s).collect();
    if matches.len() >= 2 || (matches.len() == 1 && (s.contains("校区") || s.contains('\n'))) {
        let mut segs = Vec::new();
        for (i, m) in matches.iter().enumerate() {
            let end = matches.get(i + 1).map(|n| n.start()).unwrap_or(s.len());
            let chunk = s[m.start()..end].trim();
            let Some(weeks) = parse_weeks(chunk) else { continue };
            segs.push(WeekSeg {
                weeks,
                teacher: extract_teacher(chunk),
                place: extract_place(chunk),
            });
        }
        if !segs.is_empty() {
            return segs;
        }
    }
    if let Some(weeks) = parse_weeks(&s) {
        return vec![WeekSeg {
            weeks,
            teacher: extract_teacher(&s),
            place: extract_place(&s),
        }];
    }
    Vec::new()
}

fn tidy_place(p: &str) -> String {
    let orig = p.trim().replace(['\n', '\r', '\t'], " ");
    let orig = regex::Regex::new(r"\s+")
        .map(|re| re.replace_all(&orig, " ").into_owned())
        .unwrap_or(orig);
    let mut s = orig.clone();
    let mut campus_only: Option<&str> = None;
    for prefix in ["浑南校区", "南湖校区"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            campus_only = Some(prefix);
            s = rest.trim().to_string();
        }
    }
    for prefix in ["浑南 ", "南湖 "] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.trim().to_string();
        }
    }
    let s = s.trim_start_matches(['/', '-', ' ']).trim().to_string();
    let s = regex::Regex::new(r"号\s+")
        .map(|re| re.replace_all(&s, "号").into_owned())
        .unwrap_or(s);
    if s.is_empty() {
        campus_only.unwrap_or("").to_string()
    } else {
        s
    }
}

fn catalog_items(v: &Value) -> Vec<&Value> {
    let mut out = Vec::new();
    fn walk<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
        match v {
            Value::Object(m) => {
                if m.contains_key("courseName")
                    || m.contains_key("KCMC")
                    || m.contains_key("kcmc")
                    || m.contains_key("KCM")
                    || m.contains_key("kcm")
                {
                    out.push(v);
                    return;
                }
                for x in m.values() {
                    walk(x, out);
                }
            }
            Value::Array(a) => {
                for x in a {
                    walk(x, out);
                }
            }
            _ => {}
        }
    }
    walk(v, &mut out);
    out
}

fn apply_seg_to_course(c: &mut Course, seg: &WeekSeg) {
    if !seg.weeks.is_empty() {
        c.weeks = Some(seg.weeks.clone());
    }
    if let Some(t) = sanitize_teacher(seg.teacher.clone()) {
        c.teacher = Some(t);
    } else {
        c.teacher = sanitize_teacher(c.teacher.clone());
    }
}

pub fn catalog_has_courses(v: &Value) -> bool {
    !catalog_items(v).is_empty()
}

fn dedup_courses(courses: Vec<Course>) -> Vec<Course> {
    let mut out: Vec<Course> = Vec::new();
    for c in courses {
        if let Some(ex) = out.iter_mut().find(|x| {
            names_eq(&x.name, &c.name)
                && x.weekday == c.weekday
                && x.sections == c.sections
                && x.weeks == c.weeks
        }) {
            if ex.teacher.is_none() {
                ex.teacher = c.teacher;
            }
            if ex.place.is_empty() && !c.place.is_empty() {
                ex.place = c.place;
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn catalog_schedule_text(obj: &Value) -> String {
    json_str(
        obj,
        &[
            "classDateAndPlace",
            "SKSJ",
            "sksj",
            "weeksAndTeachers",
            "weeksAndTeacher",
        ],
    )
    .unwrap_or_default()
}

pub fn enrich_from_catalog(mut courses: Vec<Course>, catalog: &Value) -> Vec<Course> {
    #[derive(Eq, PartialEq, Hash, Clone)]
    struct Key {
        name: String,
        weekday: u8,
        begin: u32,
        end: u32,
    }
    let mut grouped: std::collections::HashMap<Key, Vec<SlashSlot>> =
        std::collections::HashMap::new();
    for obj in catalog_items(catalog) {
        let Some(name) = json_str(obj, &["courseName", "KCMC", "kcmc", "KCM", "kcm", "name"]) else {
            continue;
        };
        let folded = fold_name(&name);
        let text = catalog_schedule_text(obj);
        if text.is_empty() {
            continue;
        }
        for slot in parse_slash_schedule(&text) {
            let (Some(wd), Some((b, e))) = (slot.weekday, slot.sections) else {
                continue;
            };
            grouped
                .entry(Key {
                    name: folded.clone(),
                    weekday: wd,
                    begin: b,
                    end: e,
                })
                .or_default()
                .push(slot);
        }
    }

    let keys: Vec<Key> = grouped.keys().cloned().collect();
    for key in keys {
        let slots = grouped.remove(&key).unwrap_or_default();
        if slots.is_empty() {
            continue;
        }
        let mut template: Option<Course> = None;
        let mut kept = Vec::new();
        for c in courses.drain(..) {
            if fold_name(&c.name) == key.name
                && c.weekday == key.weekday
                && c.sections == Some((key.begin, key.end))
            {
                if template.is_none() {
                    template = Some(c);
                }
            } else {
                kept.push(c);
            }
        }
        let template = template.or_else(|| {
            kept.iter()
                .find(|c| fold_name(&c.name) == key.name)
                .cloned()
                .map(|mut t| {
                    t.weekday = key.weekday;
                    t.sections = Some((key.begin, key.end));
                    let camp = resolve_campus(t.campus.as_deref(), &t.place, None, Campus::Hunnan);
                    let (s, e) = times_for(camp, key.begin, key.end);
                    t.start = s;
                    t.end = e;
                    t
                })
        });
        if let Some(t) = template {
            let multi = slots.len() > 1;
            let sibling_has_room = slots.iter().any(|s| {
                s.place
                    .as_ref()
                    .map(|p| place_has_room(&tidy_place(p)))
                    .unwrap_or(false)
            });
            let cell_place = t.place.clone();
            for slot in slots {
                let mut c = t.clone();
                c.weekday = key.weekday;
                c.sections = Some((key.begin, key.end));
                let seg = slot_to_seg(&slot);
                apply_seg_to_course(&mut c, &seg);
                c.place = resolve_seg_place(&cell_place, &seg, multi, sibling_has_room);
                kept.push(c);
            }
        }
        courses = kept;
    }
    for c in courses.iter_mut() {
        c.teacher = sanitize_teacher(c.teacher.clone());
    }
    dedup_courses(courses)
}

fn week_segs_from_value(v: &Value) -> Vec<WeekSeg> {
    match v {
        Value::String(s) => split_week_segments(s),
        Value::Array(arr) => {
            let mut out = Vec::new();
            for item in arr {
                out.extend(week_segs_from_value(item));
            }
            out
        }
        Value::Object(map) => {
            let as_val = Value::Object(map.clone());
            let teacher = json_str(
                &as_val,
                &["teacherName", "teacher", "JSXM", "jsxm", "teacherNames"],
            );
            let place = json_str(&as_val, &["placeName", "JASMC", "jasmc", "SKDD", "place"]);
            let weeks = json_str(
                &as_val,
                &[
                    "SKZC",
                    "skzc",
                    "SKZC_DISPLAY",
                    "skzcDisplay",
                    "weeks",
                    "week",
                    "ZC",
                    "weeksAndTeachers",
                ],
            )
            .and_then(|s| parse_weeks(&s));
            if let Some(weeks) = weeks {
                vec![WeekSeg {
                    weeks,
                    teacher,
                    place,
                }]
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

pub struct CalendarHit {
    pub term_code: String,
    pub label: String,
    pub week1_monday: String,
}

pub fn parse_calendars(v: &Value, today: NaiveDate) -> CalendarHit {
    let mut best: Option<(i32, CalendarHit)> = None;
    if let Some(arr) = walk_array(v, &["schoolCalendars", "rows", "list", "data"]) {
        for item in arr {
            let code = json_str(
                item,
                &["itemCode", "XNXQDM", "termCode", "code", "id", "XNXQ"],
            );
            let Some(code) = code else { continue };
            let name = json_str(item, &["itemName", "XQMC", "name", "label", "XNXQMC"]);
            let start = json_str(
                item,
                &[
                    "XQKSRQ",
                    "KSRQ",
                    "startDate",
                    "week1Monday",
                    "XQKSSJ",
                    "firstDay",
                    "KS",
                    "XQKS",
                    "beginDate",
                    "firstWeekStart",
                ],
            );
            let current = json_str(item, &["SFYX", "DQBZ", "current", "isCurrent", "SFZJXNXQ"])
                .map(|s| matches!(s.as_str(), "1" | "true" | "Y" | "yes"))
                .or_else(|| item.get("current").and_then(|x| x.as_bool()))
                .unwrap_or(false);
            let week1 = start
                .and_then(|s| parse_ymd(&s))
                .map(monday_of)
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            let score = if current { 10 } else { 0 }
                + if code == guess_term(today) { 5 } else { 0 }
                + if !week1.is_empty() { 1 } else { 0 };
            let hit = CalendarHit {
                label: term_label(&code, name.as_deref()),
                term_code: code,
                week1_monday: week1,
            };
            if best.as_ref().map(|(s, _)| *s).unwrap_or(-1) < score {
                best = Some((score, hit));
            }
        }
    }
    if let Some((_, mut hit)) = best {
        if hit.week1_monday.is_empty() {
            hit.week1_monday = default_week1(&hit.term_code);
        }
        return hit;
    }
    // 校历可能是单对象而不是数组
    if let Some(obj) = walk_obj(v, &|m| {
        m.contains_key("itemCode") || m.contains_key("XNXQDM") || m.contains_key("termCode")
    }) {
        if let Some(code) = json_str(obj, &["itemCode", "XNXQDM", "termCode", "code", "XNXQ"]) {
            let start = json_str(obj, &["XQKSRQ", "KSRQ", "startDate", "XQKSSJ", "XQKS"]);
            let week1 = start
                .and_then(|s| parse_ymd(&s))
                .map(monday_of)
                .map(|d| d.format("%Y-%m-%d").to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| default_week1(&code));
            return CalendarHit {
                label: term_label(&code, json_str(obj, &["itemName", "XQMC", "name"]).as_deref()),
                term_code: code,
                week1_monday: week1,
            };
        }
    }
    let code = guess_term(today);
    CalendarHit {
        label: term_label(&code, None),
        term_code: code.clone(),
        week1_monday: default_week1(&code),
    }
}

pub fn parse_scheduled_campus(v: &Value) -> Option<Campus> {
    let obj = walk_obj(v, &|m| {
        m.contains_key("campusCode")
            || m.contains_key("XQDM")
            || m.contains_key("campusName")
            || m.contains_key("XQMC")
            || m.contains_key("scheduledCampus")
    })
    .unwrap_or(v);
    let code = json_str(obj, &["campusCode", "XQDM", "code", "scheduledCampus"]).unwrap_or_default();
    let name = json_str(obj, &["campusName", "XQMC", "name"]).unwrap_or_default();
    crate::periods::campus_from_code(&code, &name)
}

pub fn parse_arranged_list(v: &Value) -> Vec<Value> {
    let mut out = walk_array(v, &["arrangedList", "arranged", "scheduleList"])
        .cloned()
        .unwrap_or_default();
    if let Some(more) = walk_array(v, &["notArrangeList", "notArranged", "notArrange"]) {
        for item in more {
            if weekday_of_item(item).is_some() {
                out.push(item.clone());
            }
        }
    }
    out
}

fn weekday_of_item(item: &Value) -> Option<u8> {
    json_str(
        item,
        &["dayOfWeek", "SKXQ", "skxq", "weekday", "weekDay", "XQ"],
    )
    .and_then(|s| parse_weekday(&s))
    .or_else(|| json_u32(item, &["dayOfWeek", "SKXQ", "skxq", "weekday", "weekDay", "XQ"]).map(|n| n as u8))
    .filter(|n| (1..=7).contains(n))
}

fn collect_text_lines(item: &Value, key: &str, from_obj_text: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(arr) = item.get(key).and_then(|x| x.as_array()) {
        for x in arr {
            if let Some(s) = x.as_str() {
                lines.push(s.to_string());
            } else if from_obj_text {
                if let Some(s) = x.get("text").and_then(|t| t.as_str()) {
                    lines.push(s.to_string());
                }
            }
        }
    }
    lines
}

fn richer_week_segs(a: Vec<WeekSeg>, b: Vec<WeekSeg>) -> Vec<WeekSeg> {
    if a.is_empty() {
        return b;
    }
    if b.is_empty() {
        return a;
    }
    let score = |v: &[WeekSeg]| {
        let mut s = v.len() as i32 * 8;
        for seg in v {
            s += seg.weeks.len() as i32;
            if seg.place.as_ref().is_some_and(|p| place_has_room(p)) {
                s += 4;
            }
            if seg.teacher.is_some() {
                s += 1;
            }
        }
        s
    };
    if score(&b) > score(&a) {
        b
    } else {
        a
    }
}

fn title_detail_segs(item: &Value) -> Vec<WeekSeg> {
    let title = split_week_segments(&collect_text_lines(item, "titleDetail", false).join("\n"));
    let cell = split_week_segments(&collect_text_lines(item, "cellDetail", true).join("\n"));
    richer_week_segs(title, cell)
}

pub fn parse_courses(items: &[Value], scheduled: Option<Campus>) -> Vec<Course> {
    let mut draft: Vec<(Course, Option<String>)> = Vec::new();
    for item in items {
        let name = json_str(
            item,
            &["courseName", "KCMC", "kcmc", "name", "course_name"],
        );
        let Some(name) = name else { continue };
        let place = json_str(
            item,
            &[
                "placeName",
                "JASMC",
                "jasmc",
                "classroomName",
                "place",
                "SKDD",
            ],
        )
        .unwrap_or_default();
        let campus_name = json_str(item, &["campusName", "XQMC", "xqmc", "campus"]);
        let Some(weekday) = weekday_of_item(item) else {
            continue;
        };
        let begin = json_u32(
            item,
            &["beginSection", "KSJC", "ksjc", "startSection", "beginNode"],
        )
        .unwrap_or(1)
        .clamp(1, 12);
        let end = json_u32(
            item,
            &["endSection", "JSJC", "jsjc", "endSection", "endNode"],
        )
        .unwrap_or(begin)
        .clamp(begin, 12);
        let mut segs: Vec<WeekSeg> = json_str(item, &["weeksAndTeachers", "weeksAndTeacher"])
            .map(|s| {
                parse_slash_schedule(&s)
                    .into_iter()
                    .map(|slot| slot_to_seg(&slot))
                    .collect()
            })
            .unwrap_or_default();
        if segs.is_empty() {
            segs = item
                .get("weeksAndTeachers")
                .or_else(|| item.get("weeksAndTeacher"))
                .map(week_segs_from_value)
                .unwrap_or_default();
        }
        let title_segs = title_detail_segs(item);
        // 格子全文才是完整周段；weeksAndTeachers 经常只写第一段。
        if !title_segs.is_empty() {
            segs = title_segs;
        }
        if segs.is_empty() {
            if let Some(weeks) =
                json_str(item, &["SKZC", "skzc", "weeks", "SKZC_DISPLAY", "ZC"]).and_then(|s| parse_weeks(&s))
            {
                segs.push(WeekSeg {
                    weeks,
                    teacher: json_str(item, &["teacherName", "JSXM", "jsxm", "teacher"]),
                    place: None,
                });
            }
        }
        let teacher = sanitize_teacher(json_str(
            item,
            &["teacherName", "JSXM", "jsxm", "teacher", "SKJS", "skjs"],
        ));
        let json_start = json_str(item, &["beginTime", "KSSJ", "kssj", "startTime"])
            .and_then(|s| normalize_hhmm(&s));
        let json_end = json_str(item, &["endTime", "JSSJ", "jssj", "endTime"])
            .and_then(|s| normalize_hhmm(&s));
        let base = Course {
            name,
            place,
            teacher,
            start: json_start.unwrap_or_default(),
            end: json_end.unwrap_or_default(),
            weekday,
            weeks: None,
            sections: Some((begin, end)),
            campus: campus_name.clone(),
        };
        if segs.is_empty() {
            let mut c = base;
            c.teacher = sanitize_teacher(c.teacher);
            draft.push((c, campus_name));
        } else {
            let multi = segs.len() > 1;
            let sibling_has_room = segs.iter().any(|s| {
                s.place
                    .as_ref()
                    .map(|p| place_has_room(&tidy_place(p)))
                    .unwrap_or(false)
            });
            for seg in segs {
                let mut c = base.clone();
                c.weeks = Some(seg.weeks.clone());
                if let Some(t) = sanitize_teacher(seg.teacher.clone()) {
                    c.teacher = Some(t);
                }
                c.teacher = sanitize_teacher(c.teacher.clone());
                c.place = resolve_seg_place(&base.place, &seg, multi, sibling_has_room);
                draft.push((c, campus_name.clone()));
            }
        }
    }

    // 先用地名/条目名填校区，再算多数，再补钟点。
    for (c, cname) in draft.iter_mut() {
        let camp = resolve_campus(cname.as_deref(), &c.place, scheduled, Campus::Hunnan);
        c.campus = Some(camp.as_str().to_string());
    }
    let counted: Vec<Course> = draft.iter().map(|(c, _)| c.clone()).collect();
    let majority = majority_campus(&counted);
    for (c, cname) in &mut draft {
        let camp = resolve_campus(cname.as_deref(), &c.place, scheduled, majority);
        c.campus = Some(camp.as_str().to_string());
        if c.start.is_empty() || c.end.is_empty() {
            let (a, b) = c.sections.unwrap_or((1, 2));
            let (s, e) = times_for(camp, a, b);
            if c.start.is_empty() {
                c.start = s;
            }
            if c.end.is_empty() {
                c.end = e;
            }
        }
    }
    dedup_courses(draft.into_iter().map(|(c, _)| c).collect())
}

pub fn build_semester(
    term: CalendarHit,
    courses: Vec<Course>,
    fetched_at: String,
    term_weeks: Vec<TermWeek>,
) -> SemesterTable {
    let week1 = if term.week1_monday.trim().is_empty() {
        default_week1(&term.term_code)
    } else {
        term.week1_monday
    };
    SemesterTable {
        semester_id: term.term_code,
        label: term.label,
        week1_monday: week1,
        fetched_at,
        courses,
        term_weeks,
    }
}

pub fn looks_like_login_page(text: &str) -> bool {
    text.contains("tpass/login")
        || text.contains("统一身份认证")
        || text.contains("second_auth")
        || (text.contains("<html") && text.contains("login") && !text.contains("arrangedList"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn weeks_ranges_odd_bits() {
        assert_eq!(parse_weeks("1-16周"), Some((1..=16).collect()));
        assert_eq!(parse_weeks("1,3,5,7周"), Some(vec![1, 3, 5, 7]));
        assert_eq!(parse_weeks("1-8,10-12"), Some(vec![1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12]));
        assert_eq!(parse_weeks("1-4(单)"), Some(vec![1, 3]));
        assert_eq!(parse_weeks("单周").unwrap()[0], 1);
        assert_eq!(
            parse_weeks("0111111111111110"),
            Some((2..=15).collect())
        );
        assert_eq!(
            parse_weeks("1-16周[张三]"),
            Some((1..=16).collect())
        );
        // 教室号不能进周次
        assert_eq!(
            parse_weeks("14-15周 薛丽芳 浑南校区1号B409-微机室"),
            Some(vec![14, 15])
        );
        assert_eq!(parse_weeks("11周 杨晓翠 浑南校区1号B409-微机室"), Some(vec![11]));
        // 单/双只修饰紧挨着的那段：3-8 全上，11-13 只上单周
        assert_eq!(
            parse_weeks("3-8周,11-13周(单)"),
            Some(vec![3, 4, 5, 6, 7, 8, 11, 13])
        );
        assert_eq!(parse_weeks("10-14周(双)"), Some(vec![10, 12, 14]));
        assert_eq!(
            parse_weeks("3-8周(单),10-14周(双)"),
            Some(vec![3, 5, 7, 10, 12, 14])
        );
        assert_eq!(
            parse_weeks("3-8周,11-13周（单） 王爽 浑南校区 1号A204"),
            Some(vec![3, 4, 5, 6, 7, 8, 11, 13])
        );
    }

    #[test]
    fn programming_basics_odd_even_unassigned_room() {
        // 计算机类2610-12 程序设计基础：3-8 全上在 1号A204；
        // 11-13 单周同教室；10-14 双周上课但教室未定。合起来 3-8、10-14 都有课。
        let blob = "计算机类2610-12\n程序设计基础 A117062\n3-8周,11-13周(单) 王爽 浑南校区 1号\nA204\n10-14周(双) 王爽 浑南校区\n考试 / 百分制";
        let segs = split_week_segments(blob);
        assert_eq!(segs.len(), 2, "{segs:?}");
        assert_eq!(segs[0].weeks, vec![3, 4, 5, 6, 7, 8, 11, 13]);
        assert_eq!(segs[0].teacher.as_deref(), Some("王爽"));
        assert!(
            segs[0]
                .place
                .as_ref()
                .map(|p| p.contains("A204"))
                .unwrap_or(false),
            "room wrapped onto 1号, got {:?}",
            segs[0].place
        );
        assert_eq!(segs[1].weeks, vec![10, 12, 14]);
        assert_eq!(segs[1].teacher.as_deref(), Some("王爽"));
        assert!(
            segs[1]
                .place
                .as_ref()
                .map(|p| !p.contains("A204") && !p.contains("1号"))
                .unwrap_or(true),
            "even weeks must not inherit A204, got {:?}",
            segs[1].place
        );

        let items = json!([{
            "courseName": "程序设计基础",
            "placeName": "1号A204",
            "campusName": "浑南校区",
            "dayOfWeek": 1,
            "beginSection": 1,
            "endSection": 2,
            "beginTime": "08:30",
            "endTime": "10:10",
            "weeksAndTeachers": "3-8周,11-13周(单)[理论]/王爽[主讲]",
            "titleDetail": [
                "计算机类2610-12",
                "程序设计基础 A117062",
                "3-8周,11-13周(单) 王爽 浑南校区 1号",
                "A204",
                "10-14周(双) 王爽 浑南校区",
                "考试 / 百分制"
            ],
            "cellDetail": [
                {"text": "计算机类2610-12"},
                {"text": "程序设计基础 A117062"},
                {"text": "3-8周,11-13周(单) 王爽 浑南校区 1号"},
                {"text": "A204"},
                {"text": "10-14周(双) 王爽 浑南校区"},
                {"text": "考试 / 百分制"}
            ]
        }]);
        let mut cs = parse_courses(items.as_array().unwrap(), None);
        assert_eq!(cs.len(), 2, "{:?}", cs.iter().map(|c| (&c.weeks, &c.place)).collect::<Vec<_>>());
        let with_room = cs.iter().find(|c| c.place.contains("A204")).unwrap();
        assert_eq!(with_room.weeks, Some(vec![3, 4, 5, 6, 7, 8, 11, 13]));
        assert_eq!(with_room.teacher.as_deref(), Some("王爽"));
        let no_room = cs.iter().find(|c| !c.place.contains("A204")).unwrap();
        assert_eq!(no_room.weeks, Some(vec![10, 12, 14]));
        assert!(
            no_room.place.is_empty() || !place_has_room(&no_room.place),
            "week 10/12/14 must not show 1号A204, got {:?}",
            no_room.place
        );

        let cat = json!({
            "code": "0",
            "datas": [{
                "courseName": "程序设计基础",
                "classDateAndPlace": "3-8周,11-13周(单)[理论]/星期一/第一节-第二节/王爽[主讲]/1号A204，10-14周(双)[理论]/星期一/第一节-第二节/王爽[主讲]"
            }]
        });
        cs = enrich_from_catalog(cs, &cat);
        assert_eq!(cs.len(), 2, "{:?}", cs.iter().map(|c| (&c.weeks, &c.place)).collect::<Vec<_>>());
        let with_room = cs.iter().find(|c| c.place.contains("A204")).unwrap();
        assert_eq!(with_room.weeks, Some(vec![3, 4, 5, 6, 7, 8, 11, 13]));
        let no_room = cs.iter().find(|c| !c.place.contains("A204")).unwrap();
        assert_eq!(no_room.weeks, Some(vec![10, 12, 14]));
        assert!(
            no_room.place.is_empty() || !place_has_room(&no_room.place),
            "catalog must not reattach A204 to even weeks, got {:?}",
            no_room.place
        );

        let sem = SemesterTable {
            semester_id: "2026-2027-1".into(),
            label: "秋季".into(),
            week1_monday: "2026-08-31".into(),
            fetched_at: "".into(),
            courses: cs,
            term_weeks: vec![],
        };
        let w4 = courses_today(&sem, Some(4), 1);
        assert_eq!(w4.len(), 1);
        assert!(w4[0].place.contains("A204"));
        let w9 = courses_today(&sem, Some(9), 1);
        assert!(w9.is_empty(), "week 9 is a gap");
        let w10 = courses_today(&sem, Some(10), 1);
        assert_eq!(w10.len(), 1);
        assert!(w10[0].place.is_empty() || !place_has_room(&w10[0].place));
        let w11 = courses_today(&sem, Some(11), 1);
        assert_eq!(w11.len(), 1);
        assert!(w11[0].place.contains("A204"));
        let w14 = courses_today(&sem, Some(14), 1);
        assert_eq!(w14.len(), 1);
        assert!(w14[0].place.is_empty() || !place_has_room(&w14[0].place));
    }

    #[test]
    fn split_multi_teacher_weeks() {
        let text = "14-15周 薛丽芳 浑南校区1号B409-微机室\n16-17周 张昱 浑南校区1号B409-微机室\n12-13周 何强 浑南校区1号B409-微机室\n11周 杨晓翠 浑南校区1号B409-微机室";
        let segs = split_week_segments(text);
        assert_eq!(segs.len(), 4);
        assert_eq!(segs[0].weeks, vec![14, 15]);
        assert_eq!(segs[0].teacher.as_deref(), Some("薛丽芳"));
        assert_eq!(segs[3].weeks, vec![11]);
        assert_eq!(segs[3].teacher.as_deref(), Some("杨晓翠"));
        let items = json!([{
            "courseName": "生成式人工智能应用基础（一）",
            "placeName": "浑南校区1号B409-微机室",
            "campusName": "浑南校区",
            "dayOfWeek": "星期三",
            "beginSection": 5,
            "endSection": 6,
            "weeksAndTeachers": text
        }]);
        let cs = parse_courses(items.as_array().unwrap(), None);
        assert_eq!(cs.len(), 4);
        assert!(cs.iter().all(|c| c.weekday == 3));
        let w11 = cs.iter().find(|c| c.weeks.as_ref().map(|w| w == &vec![11]).unwrap_or(false)).unwrap();
        assert_eq!(w11.teacher.as_deref(), Some("杨晓翠"));
        let today = courses_today(
            &SemesterTable {
                semester_id: "2026-2027-1".into(),
                label: "秋季".into(),
                week1_monday: "2026-08-31".into(),
                fetched_at: "".into(),
                courses: cs.clone(),
                term_weeks: vec![],
            },
            Some(2),
            3,
        );
        assert!(today.is_empty(), "第2周不该出现11-17周的课");
        let w14 = courses_today(
            &SemesterTable {
                semester_id: "2026-2027-1".into(),
                label: "秋季".into(),
                week1_monday: "2026-08-31".into(),
                fetched_at: "".into(),
                courses: cs,
                term_weeks: vec![],
            },
            Some(14),
            3,
        );
        assert_eq!(w14.len(), 1);
        assert_eq!(w14[0].teacher.as_deref(), Some("薛丽芳"));
    }

    #[test]
    fn word_cell_teachers_not_kind() {
        let blob = "电信类2601-03\n生成式人工智能应用基础（一） A115658\n14-15周 薛丽芳 浑南校区 1号B409-微机室\n16-17周 张昱 浑南校区 1号B409-微机室\n12-13周 何强 浑南校区 1号B409-微机室\n11周 杨晓翠 浑南校区 1号B409-微机室\n考查 / 百分制";
        let segs = split_week_segments(blob);
        assert_eq!(segs.len(), 4);
        assert_eq!(segs[0].teacher.as_deref(), Some("薛丽芳"));
        assert_eq!(segs[1].teacher.as_deref(), Some("张昱"));
        assert_eq!(segs[2].teacher.as_deref(), Some("何强"));
        assert_eq!(segs[3].teacher.as_deref(), Some("杨晓翠"));
        let items = json!([{
            "courseName": "程序设计基础（C语言）",
            "placeName": "1号A304",
            "campusName": "浑南校区",
            "dayOfWeek": 1,
            "beginSection": 1,
            "endSection": 2,
            "weeksAndTeachers": "7-16周 理论",
            "teacherName": "理论"
        }]);
        let cs = parse_courses(items.as_array().unwrap(), None);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].weeks.as_ref().unwrap().first(), Some(&7));
        assert!(cs[0].teacher.is_none(), "理论不是老师, got {:?}", cs[0].teacher);
        let named = json!([{
            "courseName": "程序设计基础（C语言）",
            "placeName": "1号A304",
            "campusName": "浑南校区",
            "dayOfWeek": 1,
            "beginSection": 1,
            "endSection": 2,
            "weeksAndTeachers": "7-16周 张松伟 浑南校区 1号A304",
            "teacherName": "理论"
        }]);
        let cs = parse_courses(named.as_array().unwrap(), None);
        assert_eq!(cs[0].teacher.as_deref(), Some("张松伟"));
        let mil = "3-10周 金碧来,惠建庭,吕昫光,华蕾,刘选,芦书豪,杨楠 浑南校区 文管A129";
        let segs = split_week_segments(mil);
        assert_eq!(segs[0].weeks, (3..=10).collect::<Vec<_>>());
        assert!(segs[0].teacher.as_ref().unwrap().contains("金碧来"));
        let psy = "4周,7-8周 曾觉吾,王嫔 浑南校区 1号A105";
        let segs = split_week_segments(psy);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].weeks, vec![4, 7, 8]);
        assert!(segs[0].teacher.as_ref().unwrap().contains("曾觉吾"));
    }

    #[test]
    fn glued_course_code_pe_and_ai_cell() {
        assert_eq!(
            parse_weeks("A1187453-14周 刘美晨 浑南校区"),
            Some((3..=14).collect())
        );
        assert_eq!(parse_weeks("A11565818周 李金双"), Some(vec![18]));
        let pe = "人工智能（知行英才班）2601-03,物联网2601-02,电信类2601-05,计算机2601-06体育(一) A1187453-14周 刘美晨 浑南校区考查 / 百分制";
        let segs = split_week_segments(pe);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].weeks, (3..=14).collect::<Vec<_>>());
        assert_eq!(segs[0].teacher.as_deref(), Some("刘美晨"));
        assert!(
            segs[0].place.as_ref().unwrap().contains("浑南"),
            "{:?}",
            segs[0].place
        );
        let ai = "电信类2601-03生成式人工智能应用基础（一） A11565814-15周 薛丽芳 浑南校区 1号B409-微机室16-17周 张昱 浑南校区 1号B409-微机室12-13周 何强 浑南校区 1号B409-微机室11周 杨晓翠 浑南校区 1号B409-微机室考查 / 百分制";
        let segs = split_week_segments(ai);
        assert_eq!(segs.len(), 4, "{segs:?}");
        assert_eq!(segs[0].teacher.as_deref(), Some("薛丽芳"));
        assert_eq!(segs[1].teacher.as_deref(), Some("张昱"));
        assert_eq!(segs[2].teacher.as_deref(), Some("何强"));
        assert_eq!(segs[3].teacher.as_deref(), Some("杨晓翠"));
        assert!(segs[0].place.as_ref().unwrap().contains("B409"));
        let items = json!([{
            "courseName": "生成式人工智能应用基础（一）",
            "placeName": "1号B409-微机室",
            "campusName": "浑南校区",
            "dayOfWeek": 5,
            "beginSection": 5,
            "endSection": 6,
            "weeksAndTeachers": "A11565814-15周 薛丽芳 浑南校区 1号B409-微机室16-17周 张昱 浑南校区 1号B409-微机室12-13周 何强 浑南校区 1号B409-微机室11周 杨晓翠 浑南校区 1号B409-微机室"
        }]);
        let cs = parse_courses(items.as_array().unwrap(), None);
        assert_eq!(cs.len(), 4);
        assert!(cs.iter().any(|c| c.teacher.as_deref() == Some("杨晓翠") && c.weeks == Some(vec![11])));
    }

    #[test]
    fn weekday_not_from_course_name() {
        assert_eq!(parse_weekday("体育(一)"), None);
        assert_eq!(parse_weekday("生成式人工智能应用基础（一）"), None);
        assert_eq!(parse_weekday("星期四"), Some(4));
        assert_eq!(parse_weekday("周五"), Some(5));
        assert_eq!(parse_weekday("星期三"), Some(3));
        assert_eq!(parse_weekday("星期一"), Some(1));
        assert_eq!(parse_weekday("4"), Some(4));
        assert_eq!(parse_weekday("14-15周 薛丽芳"), None);
        assert_eq!(parse_weekday("11周 杨晓翠"), None);
        assert_eq!(parse_weekday("A1187453-14周 刘美晨 浑南校区"), None);
        assert_eq!(parse_weeks("18周 李金双"), Some(vec![18]));
        assert_eq!(parse_weeks("18周[理论]/李金双[主讲]"), Some(vec![18]));
    }

    #[test]
    fn real_jwxt_slash_format() {
        assert_eq!(parse_sections_cn("第三节-第四节"), Some((3, 4)));
        assert_eq!(parse_sections_cn("第九节-第十节"), Some((9, 10)));
        assert_eq!(parse_sections_cn("第五节-第六节"), Some((5, 6)));
        assert_eq!(parse_sections_cn("第七节-第八节"), Some((7, 8)));
        let pe = parse_slash_schedule(
            "3周[实验]/星期四/第三节-第四节/刘美晨[主讲]/1号B323，4-14周[实验]/星期四/第三节-第四节/刘美晨[主讲]",
        );
        assert_eq!(pe.len(), 2);
        assert_eq!(pe[0].weeks, vec![3]);
        assert_eq!(pe[0].weekday, Some(4));
        assert_eq!(pe[0].sections, Some((3, 4)));
        assert_eq!(pe[0].teacher.as_deref(), Some("刘美晨"));
        assert_eq!(pe[0].place.as_deref(), Some("1号B323"));
        assert_eq!(pe[1].weeks, (4..=14).collect::<Vec<_>>());
        assert!(pe[1].place.is_none());
        let ai = parse_slash_schedule(
            "11周[理论]/星期五/第五节-第六节/杨晓翠[主讲]/1号B409-微机室，12-13周[理论]/星期五/第五节-第六节/何强[主讲]/1号B409-微机室，14-15周[理论]/星期五/第五节-第六节/薛丽芳[主讲]/1号B409-微机室，16-17周[理论]/星期五/第五节-第六节/张昱[主讲]/1号B409-微机室，18周[理论]/星期二/第七节-第八节/李金双[主讲]/1号B409-微机室",
        );
        assert_eq!(ai.len(), 5);
        assert_eq!(ai[0].teacher.as_deref(), Some("杨晓翠"));
        assert_eq!(ai[4].weekday, Some(2));
        assert_eq!(ai[4].teacher.as_deref(), Some("李金双"));
        assert_eq!(ai[4].weeks, vec![18]);
        let psy = parse_slash_schedule("4周,7-8周[理论]/曾觉吾[主讲],王嫔[主讲]");
        assert_eq!(psy.len(), 1);
        assert_eq!(psy[0].weeks, vec![4, 7, 8]);
        assert_eq!(psy[0].teacher.as_deref(), Some("曾觉吾，王嫔"));
        assert_ne!(psy[0].teacher.as_deref(), Some("星期四"));
        let arranged_ai = parse_slash_schedule("14-15周[理论]/薛丽芳[主讲]");
        assert_eq!(arranged_ai[0].teacher.as_deref(), Some("薛丽芳"));
    }

    #[test]
    fn real_jwxt_pe_and_ai_no_dupes() {
        let arranged = json!([
            {
                "courseName": "生成式人工智能应用基础（一）",
                "placeName": "1号B409-微机室",
                "campusName": "浑南校区",
                "dayOfWeek": 2,
                "beginSection": 7,
                "endSection": 8,
                "beginTime": "16:00",
                "endTime": "17:40",
                "weeksAndTeachers": "18周[理论]/李金双[主讲]",
                "titleDetail": [
                    "电信类2601-03",
                    "生成式人工智能应用基础（一） A115658",
                    "18周 李金双 浑南校区 1号B409-微机室",
                    "考查 / 百分制"
                ]
            },
            {
                "courseName": "生成式人工智能应用基础（一）",
                "placeName": "1号B409-微机室",
                "campusName": "浑南校区",
                "dayOfWeek": 5,
                "beginSection": 5,
                "endSection": 6,
                "beginTime": "14:00",
                "endTime": "15:40",
                "weeksAndTeachers": "14-15周[理论]/薛丽芳[主讲]",
                "titleDetail": [
                    "电信类2601-03",
                    "生成式人工智能应用基础（一） A115658",
                    "14-15周 薛丽芳 浑南校区 1号B409-微机室",
                    "16-17周 张昱 浑南校区 1号B409-微机室",
                    "12-13周 何强 浑南校区 1号B409-微机室",
                    "11周 杨晓翠 浑南校区 1号B409-微机室",
                    "考查 / 百分制"
                ]
            },
            {
                "courseName": "体育(一)",
                "placeName": null,
                "campusName": "浑南校区",
                "dayOfWeek": 4,
                "beginSection": 3,
                "endSection": 4,
                "beginTime": "10:30",
                "endTime": "12:10",
                "weeksAndTeachers": "4-14周[实验]/刘美晨[主讲]",
                "titleDetail": [
                    "人工智能（知行英才班）2601-03,物联网2601-02,电信类2601-05,计算机2601-06",
                    "体育(一) A118745",
                    "4-14周 刘美晨 浑南校区",
                    "3周 刘美晨 浑南校区 1号B323",
                    "考查 / 百分制"
                ]
            },
            {
                "courseName": "大学生心理与健康教育㈡",
                "placeName": "1号A105",
                "campusName": "浑南校区",
                "dayOfWeek": 4,
                "beginSection": 9,
                "endSection": 10,
                "beginTime": "18:30",
                "endTime": "20:10",
                "weeksAndTeachers": "4周,7-8周[理论]/曾觉吾[主讲],王嫔[主讲]"
            }
        ]);
        let mut cs = parse_courses(arranged.as_array().unwrap(), None);
        let psy = cs.iter().find(|c| c.name.contains("㈡")).unwrap();
        assert_eq!(psy.teacher.as_deref(), Some("曾觉吾，王嫔"));
        let cat = json!({
            "code": "0",
            "datas": [
                {
                    "courseName": "体育(一)",
                    "classDateAndPlace": "3周[实验]/星期四/第三节-第四节/刘美晨[主讲]/1号B323，4-14周[实验]/星期四/第三节-第四节/刘美晨[主讲]"
                },
                {
                    "courseName": "生成式人工智能应用基础（一）",
                    "classDateAndPlace": "11周[理论]/星期五/第五节-第六节/杨晓翠[主讲]/1号B409-微机室，12-13周[理论]/星期五/第五节-第六节/何强[主讲]/1号B409-微机室，14-15周[理论]/星期五/第五节-第六节/薛丽芳[主讲]/1号B409-微机室，16-17周[理论]/星期五/第五节-第六节/张昱[主讲]/1号B409-微机室，18周[理论]/星期二/第七节-第八节/李金双[主讲]/1号B409-微机室"
                }
            ]
        });
        cs = enrich_from_catalog(cs, &cat);
        let names: Vec<_> = cs.iter().map(|c| {
            format!("{} wd{} {:?} {} {:?}", c.name, c.weekday, c.weeks, c.teacher.as_deref().unwrap_or("-"), c.place)
        }).collect();
        let ai_fri: Vec<_> = cs.iter().filter(|c| c.name.contains("生成式") && c.weekday == 5).collect();
        assert_eq!(ai_fri.len(), 4, "{names:?}");
        assert!(ai_fri.iter().all(|c| c.start == "14:00"));
        assert!(ai_fri.iter().any(|c| c.teacher.as_deref() == Some("薛丽芳") && c.weeks == Some(vec![14, 15])));
        assert!(ai_fri.iter().any(|c| c.teacher.as_deref() == Some("张昱") && c.weeks == Some(vec![16, 17])));
        assert!(ai_fri.iter().any(|c| c.teacher.as_deref() == Some("何强") && c.weeks == Some(vec![12, 13])));
        assert!(ai_fri.iter().any(|c| c.teacher.as_deref() == Some("杨晓翠") && c.weeks == Some(vec![11])));
        let ai_tue: Vec<_> = cs.iter().filter(|c| c.name.contains("生成式") && c.weekday == 2).collect();
        assert_eq!(ai_tue.len(), 1, "{names:?}");
        assert_eq!(ai_tue[0].teacher.as_deref(), Some("李金双"));
        assert_eq!(ai_tue[0].weeks, Some(vec![18]));
        assert_eq!(ai_tue[0].start, "16:00");
        assert!(cs.iter().all(|c| c.teacher.as_deref() != Some("主讲")));
        assert!(cs.iter().all(|c| c.teacher.as_deref() != Some("星期四")));
        let pe: Vec<_> = cs.iter().filter(|c| c.name.contains("体育")).collect();
        assert_eq!(pe.len(), 2, "{names:?}");
        assert!(pe.iter().all(|c| c.weekday == 4 && c.sections == Some((3, 4)) && c.start == "10:30"));
        let w3 = pe.iter().find(|c| c.weeks == Some(vec![3])).unwrap();
        assert_eq!(w3.teacher.as_deref(), Some("刘美晨"));
        assert!(w3.place.contains("B323"), "{:?}", w3.place);
        let w414 = pe.iter().find(|c| c.weeks.as_ref().map(|w| w.first() == Some(&4)).unwrap_or(false)).unwrap();
        assert_eq!(w414.teacher.as_deref(), Some("刘美晨"));
        assert_eq!(cs.iter().filter(|c| c.sections == Some((1, 2))).count(), 0, "ghost 1-2 {names:?}");
    }

    #[test]
    fn yyyymmdd_and_default_week1() {
        assert_eq!(
            parse_ymd("20260901"),
            NaiveDate::from_ymd_opt(2026, 9, 1)
        );
        assert_eq!(default_week1("2026-2027-1"), "2026-08-31");
    }

    #[test]
    fn missing_week_does_not_dump_whole_table() {
        let sem = SemesterTable {
            semester_id: "2026-2027-1".into(),
            label: "秋季".into(),
            week1_monday: "2026-08-31".into(),
            fetched_at: "".into(),
            courses: parse_courses(
                json!([{
                    "courseName": "高等数学A",
                    "placeName": "生命 B213",
                    "dayOfWeek": 3,
                    "beginSection": 1,
                    "endSection": 2,
                    "weeksAndTeachers": "1-16周"
                }])
                .as_array()
                .unwrap(),
                None,
            ),
            term_weeks: vec![],
        };
        assert!(courses_today(&sem, None, 3).is_empty());
    }

    #[test]
    fn teaching_week_from_monday() {
        let w1 = "2026-09-01";
        assert_eq!(
            teaching_week(w1, NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()),
            Some(1)
        );
        assert_eq!(
            teaching_week(w1, NaiveDate::from_ymd_opt(2026, 9, 9).unwrap()),
            Some(2)
        );
        // 2026-09-01 是周二，回拨到当周周一 08-31 仍算第 1 周
        assert_eq!(
            teaching_week(w1, NaiveDate::from_ymd_opt(2026, 8, 31).unwrap()),
            Some(1)
        );
        assert_eq!(
            teaching_week(w1, NaiveDate::from_ymd_opt(2026, 8, 30).unwrap()),
            None
        );
    }

    #[test]
    fn hunnan_json_times_kept_nanhu_fallback() {
        let items = json!([
            {
                "courseName": "高等数学A",
                "placeName": "生命 B213",
                "campusName": "浑南校区",
                "dayOfWeek": 1,
                "beginSection": 1,
                "endSection": 2,
                "beginTime": "08:30",
                "endTime": "10:10",
                "weeksAndTeachers": "1-16周[张三]"
            },
            {
                "courseName": "体育",
                "placeName": "南湖操场",
                "campusName": "南湖校区",
                "dayOfWeek": 2,
                "beginSection": 7,
                "endSection": 8,
                "weeksAndTeachers": "1-16周"
            },
            {
                "courseName": "线性代数",
                "placeName": "何世礼 101",
                "dayOfWeek": 3,
                "beginSection": 1,
                "endSection": 2
            }
        ]);
        let arr = items.as_array().unwrap();
        let cs = parse_courses(arr, Some(Campus::Hunnan));
        assert_eq!(cs.len(), 3);
        assert_eq!(cs[0].start, "08:30");
        assert_eq!(cs[0].end, "10:10");
        assert_eq!(cs[0].campus.as_deref(), Some("hunnan"));
        assert_eq!(cs[0].teacher.as_deref(), Some("张三"));
        assert_eq!(cs[1].campus.as_deref(), Some("nanhu"));
        assert_eq!(cs[2].campus.as_deref(), Some("nanhu"));
        assert_eq!(cs[2].start, "08:00");
        assert_eq!(cs[2].end, "09:40");
    }

    #[test]
    fn today_filters_week_and_weekday() {
        let sem = SemesterTable {
            semester_id: "2026-2027-1".into(),
            label: "2026-2027 秋季".into(),
            week1_monday: "2026-09-01".into(),
            fetched_at: "2026-09-01T00:00:00+08:00".into(),
            courses: parse_courses(
                json!([{
                    "courseName": "高等数学A",
                    "placeName": "生命 B213",
                    "campusName": "浑南校区",
                    "dayOfWeek": 3,
                    "beginSection": 1,
                    "endSection": 2,
                    "beginTime": "08:30",
                    "endTime": "10:10",
                    "weeksAndTeachers": "1-16周"
                }])
                .as_array()
                .unwrap(),
                None,
            ),
            term_weeks: vec![],
        };
        let wed = courses_today(&sem, Some(2), 3);
        assert_eq!(wed.len(), 1);
        assert_eq!(wed[0].name, "高等数学A");
        assert!(courses_today(&sem, Some(2), 1).is_empty());
    }

    #[test]
    fn calendars_pick_current_term() {
        let v = json!({
            "code": "0",
            "datas": {
                "schoolCalendars": [
                    {"itemCode": "2025-2026-2", "itemName": "2025-2026学年第二学期", "XQKSRQ": "2026-02-24", "SFYX": "0"},
                    {"itemCode": "2026-2027-1", "itemName": "2026-2027学年第一学期", "XQKSRQ": "2026-09-01", "SFYX": "1"}
                ]
            }
        });
        let hit = parse_calendars(&v, NaiveDate::from_ymd_opt(2026, 9, 9).unwrap());
        assert_eq!(hit.term_code, "2026-2027-1");
        assert_eq!(hit.week1_monday, "2026-08-31");
        assert!(hit.label.contains("秋季") || hit.label.contains("2026"));
    }

    #[test]
    fn guess_autumn_term() {
        assert_eq!(
            guess_term(NaiveDate::from_ymd_opt(2026, 9, 9).unwrap()),
            "2026-2027-1"
        );
        assert_eq!(
            guess_term(NaiveDate::from_ymd_opt(2027, 3, 1).unwrap()),
            "2026-2027-2"
        );
    }

    #[test]
    fn live_jwxt_fixtures_no_ghosts_or_dupes() {
        let detail: Value =
            serde_json::from_str(include_str!("fixtures/jwxt_detail.json")).unwrap();
        let catalog: Value =
            serde_json::from_str(include_str!("fixtures/jwxt_courses.json")).unwrap();
        let arranged = parse_arranged_list(&detail);
        assert_eq!(arranged.len(), 21);
        let mut cs = parse_courses(&arranged, None);
        cs = enrich_from_catalog(cs, &catalog);
        assert!(
            cs.iter().all(|c| c.teacher.as_deref() != Some("主讲")
                && c.teacher.as_deref() != Some("理论")
                && c.teacher.as_deref() != Some("实验")
                && c.teacher.as_deref() != Some("星期四")),
            "kind leaked {:?}",
            cs.iter().filter(|c| matches!(c.teacher.as_deref(), Some("主讲" | "理论" | "实验" | "星期四"))).map(|c| (&c.name, &c.teacher)).collect::<Vec<_>>()
        );
        let ghosts: Vec<_> = cs
            .iter()
            .filter(|c| c.sections == Some((1, 2)) && c.name.contains("生成式"))
            .collect();
        assert!(ghosts.is_empty(), "ghost AI 1-2 {ghosts:?}");
        let ai_fri: Vec<_> = cs
            .iter()
            .filter(|c| c.name.contains("生成式") && c.weekday == 5)
            .collect();
        assert_eq!(ai_fri.len(), 4, "{:?}", ai_fri.iter().map(|c| (&c.teacher, &c.weeks, c.sections)).collect::<Vec<_>>());
        assert!(ai_fri.iter().all(|c| c.sections == Some((5, 6)) && c.start == "14:00"));
        let ai_tue: Vec<_> = cs
            .iter()
            .filter(|c| c.name.contains("生成式") && c.weekday == 2)
            .collect();
        assert_eq!(ai_tue.len(), 1);
        assert_eq!(ai_tue[0].teacher.as_deref(), Some("李金双"));
        let pe: Vec<_> = cs.iter().filter(|c| c.name.contains("体育")).collect();
        assert_eq!(pe.len(), 2, "{:?}", pe.iter().map(|c| (&c.weeks, &c.place, &c.teacher)).collect::<Vec<_>>());
        assert!(pe.iter().all(|c| c.weekday == 4 && c.teacher.as_deref() == Some("刘美晨")));
        let xi: Vec<_> = cs
            .iter()
            .filter(|c| c.name.contains("习近平") && c.weekday == 1)
            .collect();
        assert_eq!(xi.len(), 1, "xi dup {:?}", xi.iter().map(|c| &c.weeks).collect::<Vec<_>>());
        assert_eq!(xi[0].teacher.as_deref(), Some("姜耀东"));
    }

    #[test]
    fn term_weeks_and_official_current() {
        let v = json!({
            "code": "0",
            "datas": [
                {"endDate":"2026-09-05 00:00:00","startDate":"2026-08-30 00:00:00","curWeek":false,"serialNumber":1,"name":"第1周"},
                {"endDate":"2026-09-26 00:00:00","startDate":"2026-09-20 00:00:00","curWeek":true,"serialNumber":4,"name":"第4周"}
            ]
        });
        let weeks = parse_term_weeks(&v);
        assert_eq!(weeks[0].n, 1);
        assert_eq!(weeks[0].start, "2026-08-30");
        assert_eq!(
            teaching_week_from_term(&weeks, NaiveDate::from_ymd_opt(2026, 9, 20).unwrap()),
            Some(4)
        );
        // 周日按官方周，不按「周一往前推」掉到第 3 周
        assert_eq!(
            teaching_week("2026-08-31", NaiveDate::from_ymd_opt(2026, 9, 20).unwrap()),
            Some(3)
        );
    }

    #[test]
    fn weekly_api_membership_splits_unassigned_room() {
        let item = |place: Option<&str>, wat: &str, title_place: &str| {
            json!({
                "courseName": "程序设计基础",
                "placeName": place,
                "campusName": "浑南校区",
                "dayOfWeek": 1,
                "beginSection": 1,
                "endSection": 2,
                "beginTime": "08:30",
                "endTime": "10:10",
                "weeksAndTeachers": wat,
                "titleDetail": [
                    "计算机类2610-12",
                    "程序设计基础 A117062",
                    title_place,
                    "考试 / 百分制"
                ]
            })
        };
        let w8 = parse_courses(
            json!([item(
                Some("1号A204"),
                "3-8周,11-13周(单)[理论]/王爽[主讲]",
                " 王爽 浑南校区 1号A204"
            )])
            .as_array()
            .unwrap(),
            None,
        );
        let w10 = parse_courses(
            json!([item(
                None,
                "10-14周(双)[理论]/王爽[主讲]",
                " 王爽 浑南校区"
            )])
            .as_array()
            .unwrap(),
            None,
        );
        let w11 = parse_courses(
            json!([item(
                Some("1号A204"),
                "3-8周,11-13周(单)[理论]/王爽[主讲]",
                " 王爽 浑南校区 1号A204"
            )])
            .as_array()
            .unwrap(),
            None,
        );
        let full = json!([
            item(Some("1号A204"), "3-8周,11-13周(单)[理论]/王爽[主讲]", "3-8周,11-13周(单) 王爽 浑南校区 1号A204"),
            item(None, "10-14周(双)[理论]/王爽[主讲]", "10-14周(双) 王爽 浑南校区")
        ]);
        let weekly8 = json!([item(Some("1号A204"), "3-8周,11-13周(单)[理论]/王爽[主讲]", " 王爽 浑南校区 1号A204")]);
        assert!(week_param_filters(
            full.as_array().unwrap(),
            weekly8.as_array().unwrap()
        ));
        let merged = merge_weekly_stamped(vec![
            stamp_courses_week(w8, 8),
            stamp_courses_week(w10, 10),
            stamp_courses_week(w11, 11),
        ]);
        assert_eq!(merged.len(), 2, "{:?}", merged.iter().map(|c| (&c.weeks, &c.place)).collect::<Vec<_>>());
        let room = merged.iter().find(|c| c.place.contains("A204")).unwrap();
        let none = merged.iter().find(|c| !c.place.contains("A204")).unwrap();
        assert_eq!(room.weeks, Some(vec![8, 11]));
        assert_eq!(none.weeks, Some(vec![10]));
        assert!(none.place.is_empty() || !place_has_room(&none.place));
    }
}
