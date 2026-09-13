//! 南湖 / 浑南官方作息。按课表校区自动选，设置里不选手动校区。

use crate::models::Course;

pub const BLOCKS: [(&str, u32, u32); 6] = [
    ("1-2", 1, 2),
    ("3-4", 3, 4),
    ("5-6", 5, 6),
    ("7-8", 7, 8),
    ("9-10", 9, 10),
    ("11-12", 11, 12),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Campus {
    Nanhu,
    Hunnan,
}

impl Campus {
    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Campus::Nanhu => "南湖",
            Campus::Hunnan => "浑南",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Campus::Nanhu => "nanhu",
            Campus::Hunnan => "hunnan",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "nanhu" | "南湖" => Some(Campus::Nanhu),
            "hunnan" | "浑南" => Some(Campus::Hunnan),
            _ => None,
        }
    }
}

/// 小节起止。南湖上午比浑南非 30 分钟；下午晚上相同。
pub fn section_span(campus: Campus, section: u32) -> Option<(&'static str, &'static str)> {
    let table: [(&str, &str); 12] = match campus {
        Campus::Nanhu => [
            ("08:00", "08:45"),
            ("08:55", "09:40"),
            ("10:00", "10:45"),
            ("10:55", "11:40"),
            ("14:00", "14:45"),
            ("14:55", "15:40"),
            ("16:00", "16:45"),
            ("16:55", "17:40"),
            ("18:30", "19:15"),
            ("19:25", "20:10"),
            ("20:20", "21:05"),
            ("21:15", "22:00"),
        ],
        Campus::Hunnan => [
            ("08:30", "09:15"),
            ("09:25", "10:10"),
            ("10:30", "11:15"),
            ("11:25", "12:10"),
            ("14:00", "14:45"),
            ("14:55", "15:40"),
            ("16:00", "16:45"),
            ("16:55", "17:40"),
            ("18:30", "19:15"),
            ("19:25", "20:10"),
            ("20:20", "21:05"),
            ("21:15", "22:00"),
        ],
    };
    let i = section.checked_sub(1)? as usize;
    table.get(i).copied()
}

pub fn times_for(campus: Campus, begin: u32, end: u32) -> (String, String) {
    let start = section_span(campus, begin)
        .map(|x| x.0)
        .unwrap_or("08:00");
    let stop = section_span(campus, end.max(begin))
        .map(|x| x.1)
        .unwrap_or("09:40");
    (start.to_string(), stop.to_string())
}

#[allow(dead_code)]
pub fn block_time(campus: Campus, id: &str) -> Option<(String, String)> {
    BLOCKS
        .iter()
        .find(|(bid, _, _)| *bid == id)
        .map(|(_, a, b)| times_for(campus, *a, *b))
}

#[allow(dead_code)]
pub fn block_id_for(begin: u32, end: u32) -> String {
    for (id, a, b) in BLOCKS {
        if begin <= b && end >= a {
            return id.to_string();
        }
    }
    format!("{begin}-{end}")
}

#[allow(dead_code)]
pub fn course_overlaps_block(c: &Course, begin: u32, end: u32) -> bool {
    match c.sections {
        Some((a, b)) => a <= end && b >= begin,
        None => false,
    }
}

/// 条目 campusName → 校区。
pub fn campus_from_name(name: &str) -> Option<Campus> {
    if name.contains("浑南") {
        Some(Campus::Hunnan)
    } else if name.contains("南湖") {
        Some(Campus::Nanhu)
    } else {
        None
    }
}

/// 地点启发式（开发文档 §7.5）。
pub fn campus_from_place(place: &str) -> Option<Campus> {
    const NANHU: &[&str] = &["大成", "何世礼", "逸夫", "机电", "采矿", "冶金", "南湖"];
    const HUNNAN: &[&str] = &["文管", "生命", "文科", "信息科学", "浑南"];
    if NANHU.iter().any(|k| place.contains(k)) {
        return Some(Campus::Nanhu);
    }
    if HUNNAN.iter().any(|k| place.contains(k)) {
        return Some(Campus::Hunnan);
    }
    None
}

/// getMyScheduledCampus：实测浑南 `01`。
pub fn campus_from_code(code: &str, name: &str) -> Option<Campus> {
    if let Some(c) = campus_from_name(name) {
        return Some(c);
    }
    match code.trim() {
        "01" | "1" => Some(Campus::Hunnan),
        "00" | "0" | "02" | "2" => Some(Campus::Nanhu),
        _ => None,
    }
}

pub fn resolve_campus(
    campus_name: Option<&str>,
    place: &str,
    scheduled: Option<Campus>,
    majority: Campus,
) -> Campus {
    if let Some(n) = campus_name {
        if let Some(c) = campus_from_name(n) {
            return c;
        }
    }
    // 地点比学生默认校区更具体（跨校区：南湖操场 / 何世礼）。
    if let Some(c) = campus_from_place(place) {
        return c;
    }
    if let Some(c) = scheduled {
        return c;
    }
    majority
}

pub fn majority_campus(courses: &[Course]) -> Campus {
    majority_campus_week(courses, None)
}

pub fn majority_campus_week(courses: &[Course], week: Option<u32>) -> Campus {
    let mut hunnan = 0u32;
    let mut nanhu = 0u32;
    for c in courses {
        if let Some(w) = week {
            if let Some(ws) = c.weeks.as_ref() {
                if !ws.contains(&w) {
                    continue;
                }
            }
        }
        match c.campus.as_deref() {
            Some("hunnan") => hunnan += 1,
            Some("nanhu") => nanhu += 1,
            _ => {}
        }
    }
    if hunnan >= nanhu {
        Campus::Hunnan
    } else {
        Campus::Nanhu
    }
}

#[allow(dead_code)]
pub fn course_campus(c: &Course) -> Campus {
    c.campus
        .as_deref()
        .and_then(Campus::parse)
        .unwrap_or(Campus::Hunnan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hunnan_morning_is_half_hour_later() {
        assert_eq!(section_span(Campus::Nanhu, 1), Some(("08:00", "08:45")));
        assert_eq!(section_span(Campus::Hunnan, 1), Some(("08:30", "09:15")));
        assert_eq!(times_for(Campus::Hunnan, 1, 2), ("08:30".into(), "10:10".into()));
        assert_eq!(times_for(Campus::Nanhu, 1, 2), ("08:00".into(), "09:40".into()));
        assert_eq!(times_for(Campus::Hunnan, 5, 6), times_for(Campus::Nanhu, 5, 6));
        assert_eq!(times_for(Campus::Hunnan, 11, 12), ("20:20".into(), "22:00".into()));
    }

    #[test]
    fn place_and_name_detect_campus() {
        assert_eq!(campus_from_name("浑南校区"), Some(Campus::Hunnan));
        assert_eq!(campus_from_name("南湖校区"), Some(Campus::Nanhu));
        assert_eq!(campus_from_place("何世礼 101"), Some(Campus::Nanhu));
        assert_eq!(campus_from_place("生命 B213"), Some(Campus::Hunnan));
        assert_eq!(campus_from_place("南湖操场"), Some(Campus::Nanhu));
        assert_eq!(campus_from_code("01", ""), Some(Campus::Hunnan));
    }
}
