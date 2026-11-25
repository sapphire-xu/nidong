//! 泥洞本机流量记录。界面在泥洞里，计数在这个提权进程里。

#[cfg(windows)]
mod netif;
#[cfg(windows)]
mod place;
#[cfg(windows)]
mod run;
#[cfg(windows)]
mod sample;
mod store;
#[cfg(windows)]
mod task;
#[cfg(windows)]
mod udp;

use serde::Serialize;

use store::Totals;

#[cfg(windows)]
pub use place::connected_ssid;
#[cfg(not(windows))]
pub fn connected_ssid() -> String {
    String::new()
}
pub use store::Day;

pub fn is_supported() -> bool {
    cfg!(windows)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub supported: bool,
    pub enabled: bool,
    pub running: bool,
    pub on_campus: bool,
    pub ipv4: String,
    pub ipv6: String,
    pub v4_down_bps: u64,
    pub v6_down_bps: u64,
    pub v4_down_tcp_bps: u64,
    pub v4_down_udp_bps: u64,
    pub v4_down_campus_bps: u64,
    pub v6_down_tcp_bps: u64,
    pub v6_down_udp_bps: u64,
    pub v6_down_campus_bps: u64,
    pub v4_up_bps: u64,
    pub v6_up_bps: u64,
    pub night: bool,
    pub elevated: bool,
    pub udp_ready: bool,
    pub udp_gap: bool,
    pub today: Option<Totals>,
    pub month: Option<Totals>,
    pub error: String,
}

#[cfg(windows)]
pub fn entry() {
    run::entry();
}

#[cfg(not(windows))]
pub fn entry() {}

#[cfg(windows)]
pub fn status() -> Status {
    let enabled = run::enabled_file();
    let live = run::read_live();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let running = live.as_ref().map(|l| now.saturating_sub(l.updated) <= 15).unwrap_or(false);
    task::kick_if_idle(enabled, running);
    let (month, today, _) = store::now_parts();
    let (today_tot, month_tot) = store::summary(&month, &today);
    let error = if enabled {
        String::new()
    } else {
        run::read_error()
    };
    match live {
        Some(live) if running => Status {
            supported: true,
            enabled,
            running,
            on_campus: live.on_campus,
            ipv4: live.ipv4,
            ipv6: live.ipv6,
            v4_down_bps: live.v4_down_bps,
            v6_down_bps: live.v6_down_bps,
            v4_down_tcp_bps: live.v4_down_tcp_bps,
            v4_down_udp_bps: live.v4_down_udp_bps,
            v4_down_campus_bps: live.v4_down_campus_bps,
            v6_down_tcp_bps: live.v6_down_tcp_bps,
            v6_down_udp_bps: live.v6_down_udp_bps,
            v6_down_campus_bps: live.v6_down_campus_bps,
            v4_up_bps: live.v4_up_bps,
            v6_up_bps: live.v6_up_bps,
            night: live.night,
            elevated: live.elevated,
            udp_ready: live.udp_ready,
            udp_gap: live.udp_gap,
            today: today_tot,
            month: month_tot,
            error,
        },
        _ => Status {
            supported: true,
            enabled,
            running: false,
            on_campus: false,
            ipv4: String::new(),
            ipv6: String::new(),
            v4_down_bps: 0,
            v6_down_bps: 0,
            v4_down_tcp_bps: 0,
            v4_down_udp_bps: 0,
            v4_down_campus_bps: 0,
            v6_down_tcp_bps: 0,
            v6_down_udp_bps: 0,
            v6_down_campus_bps: 0,
            v4_up_bps: 0,
            v6_up_bps: 0,
            night: false,
            elevated: false,
            udp_ready: false,
            udp_gap: false,
            today: today_tot,
            month: month_tot,
            error,
        },
    }
}

#[cfg(not(windows))]
pub fn status() -> Status {
    let (month, today, night) = store::now_parts();
    let (today_tot, month_tot) = store::summary(&month, &today);
    Status {
        supported: false,
        enabled: false,
        running: false,
        on_campus: false,
        ipv4: String::new(),
        ipv6: String::new(),
        v4_down_bps: 0,
        v6_down_bps: 0,
        v4_down_tcp_bps: 0,
        v4_down_udp_bps: 0,
        v4_down_campus_bps: 0,
        v6_down_tcp_bps: 0,
        v6_down_udp_bps: 0,
        v6_down_campus_bps: 0,
        v4_up_bps: 0,
        v6_up_bps: 0,
        night,
        elevated: false,
        udp_ready: false,
        udp_gap: false,
        today: today_tot,
        month: month_tot,
        error: String::new(),
    }
}

pub fn log_days() -> Vec<Day> {
    let mut days = store::all_days();
    days.reverse();
    days
}

pub fn meter_dir() -> std::path::PathBuf {
    store::meter_dir()
}

pub fn ensure_log_file() -> std::path::PathBuf {
    let path = store::usage_path();
    if !path.exists() {
        let mut ledger = store::Ledger::default();
        let (month, _, _) = store::now_parts();
        ledger.month = month;
        let _ = store::save_ledger(&path, &ledger);
    }
    path
}
