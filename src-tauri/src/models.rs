use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub fetched_at: String,
    pub network: String,
    pub logged_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card: Option<CardInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card_bills: Option<Vec<Bill>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card_spend: Option<Vec<DaySpend>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card_month: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bills_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<NetInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elec: Option<ElecInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub courses_today: Option<Vec<Course>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semester: Option<SemesterTable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub teaching_week: Option<u32>,
    pub errors: SnapshotErrors,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Course {
    pub name: String,
    pub place: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub teacher: Option<String>,
    pub start: String,
    pub end: String,
    pub weekday: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weeks: Option<Vec<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sections: Option<(u32, u32)>,
    /// `nanhu` / `hunnan`，按课自动，不选手动校区。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub campus: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemesterTable {
    pub semester_id: String,
    pub label: String,
    pub week1_monday: String,
    pub fetched_at: String,
    pub courses: Vec<Course>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SnapshotErrors {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub course: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardInfo {
    pub balance: f64,
    pub subsidy: f64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bill {
    pub time: String,
    pub subject: String,
    pub amount: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee: Option<f64>,
    pub balance_after: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub station: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaySpend {
    pub date: String,
    pub spend: f64,
    pub recharge: f64,
    pub count: u32,
    pub bills: Vec<Bill>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetInfo {
    pub balance: f64,
    pub month_bytes: u64,
    pub month_seconds: u64,
    pub month_times: u64,
    /// 门户 getWlzzList 是否解析到了流量字段。false 时 UI 应沿用上次或显示 —，不要当成真的 0B。
    #[serde(default)]
    pub flow_from_portal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElecInfo {
    pub unbound: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remain_kwh: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_no: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_read_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub room_no: String,
    #[serde(default = "default_card_warn")]
    pub card_warn: f64,
    #[serde(default = "default_net_warn")]
    pub net_warn: f64,
    #[serde(default = "default_elec_warn")]
    pub elec_warn: f64,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_true")]
    pub autostart: bool,
    #[serde(default = "default_true")]
    pub remember_password: bool,
    #[serde(default = "default_true")]
    pub auto_login: bool,
    #[serde(default)]
    pub last_username: String,
    #[serde(default)]
    pub island_x: Option<f64>,
    #[serde(default)]
    pub island_y: Option<f64>,
    /// 本学期第一周周一 YYYY-MM-DD；空则用教务校历。
    #[serde(default)]
    pub week1_monday: String,
    /// 折叠后自动藏进屏幕上沿。
    #[serde(default = "default_true")]
    pub auto_hide: bool,
    /// 自动隐藏后，用全局快捷键唤出并展开。
    #[serde(default = "default_true")]
    pub auto_hide_hotkey: bool,
    /// 自动隐藏后，在隐藏处悬停 1 秒唤出并展开。
    #[serde(default = "default_true")]
    pub auto_hide_hover: bool,
    #[serde(default = "default_reveal_hotkey")]
    pub reveal_hotkey: String,
    #[serde(default = "default_true")]
    pub class_remind: bool,
    #[serde(default = "default_remind_minutes")]
    pub class_remind_minutes: u32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            room_no: String::new(),
            card_warn: 100.0,
            net_warn: 10.0,
            elec_warn: 20.0,
            theme: "system".into(),
            autostart: true,
            remember_password: true,
            auto_login: true,
            last_username: String::new(),
            island_x: None,
            island_y: None,
            week1_monday: String::new(),
            auto_hide: true,
            auto_hide_hotkey: true,
            auto_hide_hover: true,
            reveal_hotkey: default_reveal_hotkey(),
            class_remind: true,
            class_remind_minutes: default_remind_minutes(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_theme() -> String {
    "system".into()
}

fn default_card_warn() -> f64 {
    100.0
}

fn default_net_warn() -> f64 {
    10.0
}

fn default_elec_warn() -> f64 {
    20.0
}

fn default_reveal_hotkey() -> String {
    "Tab+Q".into()
}

fn default_remind_minutes() -> u32 {
    15
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LoginOutcome {
    Ok,
    NeedSms {
        #[serde(rename = "phoneMask")]
        phone_mask: String,
        #[serde(rename = "captchaPng")]
        captcha_png: String,
        #[serde(rename = "ocrHint")]
        ocr_hint: Option<String>,
    },
}
