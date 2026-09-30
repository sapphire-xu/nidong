//! 校园网自动连接。五种模式都在这一条循环里。
//! 网关登录只用系统凭据里的一号通密码；没记住密码时仍可连接无线，但不能登录。
//! 提示发到岛上，不弹窗，也不打开浏览器。

use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Off,
    Offline,
    Boot,
    Hotspot,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Idle,
    ConnectAndLogin,
    LoginOnly,
}

#[derive(Debug, Default)]
pub struct Watch {
    last_mode: Mode,
    boot_done: bool,
    seen_hotspot: bool,
    hotspot_miss: u32,
    offline_streak: u32,
    offline_tries: u32,
    saw_sample: bool,
    was_campus: bool,
    /// 证书对不上之后暂停自动连接，避免对着同名热点反复试。
    suspect_ticks: u32,
}

pub fn parse_mode(s: &str) -> Mode {
    match s {
        "offline" => Mode::Offline,
        "boot" => Mode::Boot,
        "hotspot" => Mode::Hotspot,
        "manual" => Mode::Manual,
        _ => Mode::Off,
    }
}

pub fn ssid_eq(a: &str, b: &str) -> bool {
    let a = a.trim();
    let b = b.trim();
    !a.is_empty() && a.eq_ignore_ascii_case(b)
}

/// 南湖、浑南对外是同一套无线名。秦皇岛分校是 NEUQ，本程序不覆盖。
const CAMPUS_SSIDS: &[&str] = &["NEU", "NEU-2.4G", "NEU-Mobile"];

pub fn is_campus_net(name: &str, configured: &str) -> bool {
    ssid_eq(name, configured) || CAMPUS_SSIDS.iter().any(|s| ssid_eq(name, s))
}

pub fn decide(w: &mut Watch, mode: Mode, ssid: &str, online: bool, campus: &str, hotspot: &str) -> Action {
    if w.last_mode != mode {
        w.hotspot_miss = 0;
        w.seen_hotspot = false;
        w.offline_streak = 0;
        w.offline_tries = 0;
        w.saw_sample = false;
        w.was_campus = false;
        w.last_mode = mode;
    }
    let on_campus = is_campus_net(ssid, campus);
    match mode {
        Mode::Off => Action::Idle,
        Mode::Boot => {
            if w.boot_done {
                return Action::Idle;
            }
            w.boot_done = true;
            if on_campus {
                Action::LoginOnly
            } else {
                Action::ConnectAndLogin
            }
        }
        Mode::Manual => {
            let ready = w.saw_sample;
            let prev = w.was_campus;
            w.saw_sample = true;
            w.was_campus = on_campus;
            if ready && on_campus && !prev {
                Action::LoginOnly
            } else {
                Action::Idle
            }
        }
        Mode::Hotspot => {
            let hot = hotspot.trim();
            // 指定网络是任一校园网时，离开它再去连校园网，会把人从别的网络拉回来。
            if hot.is_empty() || is_campus_net(hot, campus) {
                w.seen_hotspot = false;
                w.hotspot_miss = 0;
                return Action::Idle;
            }
            if ssid_eq(ssid, hot) {
                w.seen_hotspot = true;
                w.hotspot_miss = 0;
                return Action::Idle;
            }
            if w.seen_hotspot {
                w.hotspot_miss += 1;
                if w.hotspot_miss >= 2 {
                    w.seen_hotspot = false;
                    w.hotspot_miss = 0;
                    return if on_campus { Action::LoginOnly } else { Action::ConnectAndLogin };
                }
            }
            Action::Idle
        }
        Mode::Offline => {
            if online {
                w.offline_streak = 0;
                w.offline_tries = 0;
                return Action::Idle;
            }
            w.offline_streak += 1;
            if w.offline_streak < 2 || w.offline_tries >= 3 {
                return Action::Idle;
            }
            w.offline_tries += 1;
            w.offline_streak = 0;
            if on_campus { Action::LoginOnly } else { Action::ConnectAndLogin }
        }
    }
}

pub fn short_fail(msg: &str) -> &'static str {
    if msg.contains("不是校园网") {
        "这不是校园网，密码没提交"
    } else if msg.contains("密码") {
        "校园网密码不对"
    } else if msg.contains("短信") {
        "校园网要短信验证"
    } else if msg.contains("验证码") {
        "校园网要验证码"
    } else if msg.contains("欠费") {
        "校园网已欠费"
    } else if msg.contains("无线配置") {
        "先手动连一次校园网"
    } else if msg.contains("无线") || msg.contains("网卡") {
        "没有无线网卡"
    } else if msg.contains("没连上") || msg.contains("范围内") || msg.contains("网络不通") {
        "校园网没连上"
    } else if msg.contains("记住") {
        "先记住密码"
    } else {
        "校园网登录失败"
    }
}

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut watch = Watch::default();
        let mut last_hint = String::new();
        loop {
            tick(&app, &mut watch, &mut last_hint).await;
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
    });
}

async fn tick(app: &AppHandle, watch: &mut Watch, last_hint: &mut String) {
    let Some(state) = app.try_state::<crate::AppState>() else {
        return;
    };
    let settings = state.settings.lock().unwrap().clone();
    let creds = crate::stored_login(&state);
    drop(state);
    let mode = parse_mode(&settings.campus_wifi_mode);
    let campus = {
        let t = settings.campus_wifi_ssid.trim();
        if t.is_empty() { "NEU".to_string() } else { t.to_string() }
    };
    if mode == Mode::Off {
        let _ = decide(watch, mode, "", false, &campus, "");
        return;
    }
    let wlan = match tokio::task::spawn_blocking(crate::wlan::query).await {
        Ok(Ok(v)) => v,
        _ => {
            hint(app, last_hint, "没有无线网卡");
            return;
        }
    };
    if !wlan.has_adapter {
        hint(app, last_hint, "没有无线网卡");
        return;
    }
    let online = if mode == Mode::Offline {
        tokio::task::spawn_blocking(has_internet).await.unwrap_or(false)
    } else {
        false
    };
    let ssid = if wlan.connected { wlan.ssid } else { String::new() };
    if wlan.connected && ssid.is_empty() {
        return;
    }
    if watch.last_mode != mode {
        watch.suspect_ticks = 0;
    } else if skip_suspect(&mut watch.suspect_ticks) {
        return;
    }
    let action = decide(watch, mode, &ssid, online, &campus, &settings.campus_hotspot_ssid);
    let blocked = match action {
        Action::Idle => false,
        Action::LoginOnly => login_phase(app, last_hint, creds.as_ref()).await,
        Action::ConnectAndLogin => connect_phase(app, last_hint, &campus, &ssid, creds.as_ref()).await,
    };
    if blocked {
        watch.suspect_ticks = SUSPECT_TICKS;
    }
}

const SUSPECT_TICKS: u32 = 40;

fn skip_suspect(ticks: &mut u32) -> bool {
    if *ticks == 0 {
        return false;
    }
    *ticks -= 1;
    true
}

fn has_internet() -> bool {
    crate::util::tcp_ok("223.5.5.5", 53, 450) || crate::util::tcp_ok("114.114.114.114", 53, 450)
}

async fn connect_phase(app: &AppHandle, last: &mut String, campus: &str, ssid: &str, creds: Option<&(String, String)>) -> bool {
    if !ssid_eq(ssid, campus) {
        hint(app, last, "正在连接校园网");
        let profile = campus.to_string();
        let connected = tokio::task::spawn_blocking(move || crate::wlan::connect_profile(&profile)).await;
        match connected {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                hint(app, last, short_fail(&e));
                return false;
            }
            Err(_) => {
                hint(app, last, "校园网没连上");
                return false;
            }
        }
        if !wait_ssid(campus, 25).await {
            hint(app, last, "校园网没连上");
            return false;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    login_phase(app, last, creds).await
}

async fn wait_ssid(campus: &str, secs: u64) -> bool {
    for _ in 0..secs {
        let now = tokio::task::spawn_blocking(crate::wlan::query).await;
        if let Ok(Ok(v)) = now {
            if v.connected && ssid_eq(&v.ssid, campus) {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    false
}

async fn login_phase(app: &AppHandle, last: &mut String, creds: Option<&(String, String)>) -> bool {
    let Some((user, pass)) = creds else {
        hint(app, last, "先记住密码");
        return false;
    };
    hint(app, last, "正在登录校园网");
    let mut err = String::from("校园网登录失败");
    let mut blocked = false;
    for attempt in 1..=3 {
        match crate::ipgw::login(user, pass).await {
            Ok(_) => {
                hint(app, last, "校园网已登录");
                return false;
            }
            Err(e) => {
                err = e;
                if err.contains("不是校园网") {
                    blocked = true;
                    let _ = tokio::task::spawn_blocking(crate::wlan::disconnect).await;
                    break;
                }
            }
        }
        if attempt < 3 {
            tokio::time::sleep(Duration::from_secs(2 * attempt)).await;
        }
    }
    hint(app, last, short_fail(&err));
    blocked
}

fn hint(app: &AppHandle, last: &mut String, text: &str) {
    if last == text {
        return;
    }
    *last = text.to_string();
    let _ = app.emit("campus-hint", text);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w() -> Watch {
        Watch::default()
    }

    #[test]
    fn hotspot_only_after_it_was_seen() {
        let mut w = w();
        assert_eq!(decide(&mut w, Mode::Hotspot, "NEU", false, "NEU", "Phone"), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Hotspot, "", false, "NEU", "Phone"), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Hotspot, "Phone", false, "NEU", "Phone"), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Hotspot, "", false, "NEU", "Phone"), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Hotspot, "Home", false, "NEU", "Phone"), Action::ConnectAndLogin);
    }

    #[test]
    fn hotspot_name_required() {
        let mut w = w();
        assert_eq!(decide(&mut w, Mode::Hotspot, "Phone", false, "NEU", "  "), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Hotspot, "", false, "NEU", ""), Action::Idle);
    }

    #[test]
    fn hotspot_same_as_campus_does_not_reconnect() {
        let mut w = w();
        assert_eq!(decide(&mut w, Mode::Hotspot, "NEU", false, "NEU", "NEU"), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Hotspot, "", false, "NEU", "NEU"), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Hotspot, "Home", false, "NEU", "neu"), Action::Idle);
        decide(&mut w, Mode::Hotspot, "Phone", false, "NEU", "Phone");
        assert_eq!(decide(&mut w, Mode::Hotspot, "Home", false, "NEU", "NEU"), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Hotspot, "", false, "NEU", "NEU"), Action::Idle);
    }

    #[test]
    fn leaving_any_campus_ssid_does_not_reconnect() {
        for name in ["NEU-Mobile", "NEU-2.4G", "neu-mobile", "neu-2.4g"] {
            let mut w = w();
            assert_eq!(decide(&mut w, Mode::Hotspot, name, false, "NEU", name), Action::Idle);
            assert_eq!(decide(&mut w, Mode::Hotspot, "", false, "NEU", name), Action::Idle);
            assert_eq!(decide(&mut w, Mode::Hotspot, "Home", false, "NEU", name), Action::Idle);
        }
    }

    #[test]
    fn leaving_phone_onto_another_campus_ssid_only_logs_in() {
        let mut w = w();
        decide(&mut w, Mode::Hotspot, "Phone", false, "NEU", "Phone");
        decide(&mut w, Mode::Hotspot, "NEU-Mobile", false, "NEU", "Phone");
        assert_eq!(decide(&mut w, Mode::Hotspot, "NEU-2.4G", false, "NEU", "Phone"), Action::LoginOnly);
    }

    #[test]
    fn leaving_hotspot_onto_campus_only_logs_in() {
        let mut w = w();
        decide(&mut w, Mode::Hotspot, "Phone", false, "NEU", "Phone");
        decide(&mut w, Mode::Hotspot, "NEU", false, "NEU", "Phone");
        assert_eq!(decide(&mut w, Mode::Hotspot, "NEU", false, "NEU", "Phone"), Action::LoginOnly);
    }

    #[test]
    fn manual_ignores_the_network_already_up() {
        let mut w = w();
        assert_eq!(decide(&mut w, Mode::Manual, "NEU", false, "NEU", ""), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Manual, "NEU", false, "NEU", ""), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Manual, "Home", false, "NEU", ""), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Manual, "neu", false, "NEU", ""), Action::LoginOnly);
    }

    #[test]
    fn boot_runs_once() {
        let mut w = w();
        assert_eq!(decide(&mut w, Mode::Boot, "", false, "NEU", ""), Action::ConnectAndLogin);
        assert_eq!(decide(&mut w, Mode::Boot, "", false, "NEU", ""), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Boot, "NEU", false, "NEU", ""), Action::Idle);
    }

    #[test]
    fn boot_on_campus_only_logs_in() {
        let mut w = w();
        assert_eq!(decide(&mut w, Mode::Boot, "NEU", false, "NEU", ""), Action::LoginOnly);
    }

    #[test]
    fn offline_debounces_and_stops_after_three() {
        let mut w = w();
        assert_eq!(decide(&mut w, Mode::Offline, "", false, "NEU", ""), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Offline, "", false, "NEU", ""), Action::ConnectAndLogin);
        assert_eq!(decide(&mut w, Mode::Offline, "NEU", false, "NEU", ""), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Offline, "NEU", false, "NEU", ""), Action::LoginOnly);
        assert_eq!(decide(&mut w, Mode::Offline, "", false, "NEU", ""), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Offline, "", false, "NEU", ""), Action::ConnectAndLogin);
        assert_eq!(decide(&mut w, Mode::Offline, "", false, "NEU", ""), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Offline, "", false, "NEU", ""), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Offline, "", true, "NEU", ""), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Offline, "", false, "NEU", ""), Action::Idle);
        assert_eq!(decide(&mut w, Mode::Offline, "", false, "NEU", ""), Action::ConnectAndLogin);
    }

    #[test]
    fn off_does_nothing() {
        let mut w = w();
        assert_eq!(decide(&mut w, Mode::Off, "", false, "NEU", "Phone"), Action::Idle);
    }

    #[test]
    fn fail_copy_stays_short() {
        assert_eq!(short_fail("系统里没有名为「NEU」的无线配置"), "先手动连一次校园网");
        assert_eq!(short_fail("校园网密码不对"), "校园网密码不对");
        assert_eq!(short_fail("这不是校园网"), "这不是校园网，密码没提交");
        assert_eq!(short_fail("别的问题"), "校园网登录失败");
    }

    #[test]
    fn suspect_cooldown_skips_then_allows() {
        let mut ticks = 2u32;
        assert!(skip_suspect(&mut ticks));
        assert!(skip_suspect(&mut ticks));
        assert!(!skip_suspect(&mut ticks));
    }
}
