mod agenda;
mod campus;
mod client;
mod clock;
mod course;
mod crypto;
mod error;
mod hotkey;
mod ipgw;
mod models;
mod periods;
mod util;
mod webvpn;
mod wlan;

use client::{
    aggregate_month, load_month_cache, month_cache_fresh, save_month_cache, NeuClient, SessionState,
};
use course::{
    apply_semester, fetched_local_date, load_semester, save_semester, semester_cache_stale,
    should_fetch_courses,
};

fn week1_opt(s: &str) -> Option<&str> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}
use error::Result;
use models::*;
use chrono::{Datelike, NaiveDate, Timelike};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, WebviewWindow};
use tokio::sync::Mutex as AsyncMutex;

pub struct AppState {
    client: AsyncMutex<Option<Arc<NeuClient>>>,
    epoch: AtomicU64,
    settings: Mutex<AppSettings>,
    snapshot: Mutex<Snapshot>,
    pending_login: Mutex<Option<PendingLogin>>,
    pause_bills: AtomicBool,
    desk_open_lock: Mutex<()>,
    /// 刷新串行。避免定时任务和手动刷新同时写 snapshot。
    refresh_gate: AsyncMutex<()>,
    /// 自动课表同步今天已经打过教务（不论成败）。手动刷新不受这一天限制。
    course_attempt_day: Mutex<Option<NaiveDate>>,
    data_dir: PathBuf,
    tray_balances: Mutex<Option<TrayBalances>>,
}

struct TrayBalances {
    card: MenuItem<tauri::Wry>,
    net: MenuItem<tauri::Wry>,
    elec: MenuItem<tauri::Wry>,
}

/// 23:00–06:00 电费系统经常连不上。这段时间失败就留缓存，不标同步失败。
fn elec_night(hour: u32) -> bool {
    hour >= 23 || hour < 6
}

async fn ensure_client(state: &AppState) -> Result<Arc<NeuClient>> {
    let mut g = state.client.lock().await;
    if g.is_none() {
        *g = Some(Arc::new(NeuClient::new(state.data_dir.clone())?));
    }
    Ok(g.as_ref().unwrap().clone())
}

async fn live_client(state: &AppState) -> Result<Arc<NeuClient>> {
    state
        .client
        .lock()
        .await
        .clone()
        .ok_or_else(|| "未登录".into())
}

async fn with_timeout<T>(secs: u64, fut: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(Duration::from_secs(secs), fut)
        .await
        .unwrap_or_else(|_| Err("同步超时，请再刷新一次。".into()))
}

struct PendingLogin {
    username: String,
    password: String,
    remember: bool,
    auto: bool,
}

fn load_settings(dir: &PathBuf) -> AppSettings {
    let p = dir.join("settings.json");
    let mut s: AppSettings = fs::read(&p)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    if s.reveal_hotkey.eq_ignore_ascii_case("tab+q") {
        s.reveal_hotkey = "Ctrl+Q".into();
        save_settings_file(dir, &s);
    }
    s
}

fn save_settings_file(dir: &PathBuf, s: &AppSettings) {
    let _ = fs::write(dir.join("settings.json"), serde_json::to_vec_pretty(s).unwrap_or_default());
}

fn save_snapshot_file(dir: &PathBuf, s: &Snapshot) {
    let mut disk = s.clone();
    disk.offline = false;
    disk.agenda = Vec::new();
    let _ = fs::write(
        dir.join("snapshot.json"),
        serde_json::to_vec_pretty(&disk).unwrap_or_default(),
    );
}

fn keyring_set(user: &str, pass: &str) -> Result<()> {
    let e = keyring::Entry::new("edu.neu.nidong", user).map_err(|e| e.to_string())?;
    e.set_password(pass).map_err(|e| e.to_string())?;
    Ok(())
}

fn keyring_get(user: &str) -> Option<String> {
    keyring::Entry::new("edu.neu.nidong", user)
        .ok()
        .and_then(|e| e.get_password().ok())
        .filter(|s| !s.is_empty())
}

fn keyring_delete(user: &str) {
    if let Ok(e) = keyring::Entry::new("edu.neu.nidong", user) {
        let _ = e.delete_credential();
    }
}

pub(crate) fn stored_login(state: &AppState) -> Option<(String, String)> {
    let s = state.settings.lock().unwrap();
    if !s.remember_password || s.last_username.trim().is_empty() {
        return None;
    }
    let user = s.last_username.clone();
    drop(s);
    let pass = keyring_get(&user)?;
    Some((user, pass))
}

fn tray_labels(snap: &Snapshot) -> (String, String, String) {
    let card = match snap.card.as_ref() {
        Some(c) => format!("校园卡  {:.2} 元", c.balance),
        None => "校园卡  —".into(),
    };
    let net = match snap.net.as_ref() {
        Some(n) => format!("网费  {:.2} 元", n.balance),
        None => "网费  —".into(),
    };
    let elec = match snap.elec.as_ref() {
        Some(e) if e.unbound => "电费  未绑定".into(),
        Some(e) => match e.remain_kwh {
            Some(k) => format!("电费  {} 度", k.round() as i64),
            None => "电费  —".into(),
        },
        None => "电费  —".into(),
    };
    (card, net, elec)
}

fn sync_tray(app: &AppHandle, snap: &Snapshot) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let guard = state.tray_balances.lock().unwrap();
    let Some(t) = guard.as_ref() else {
        return;
    };
    let (card, net, elec) = tray_labels(snap);
    let _ = t.card.set_text(card);
    let _ = t.net.set_text(net);
    let _ = t.elec.set_text(elec);
}

fn emit_snapshot(app: &AppHandle, snap: &Snapshot) {
    sync_tray(app, snap);
    let _ = app.emit("snapshot-updated", snap);
}

fn apply_autostart(enable: bool) -> Result<()> {
    if cfg!(debug_assertions) {
        let _ = enable;
        return Ok(());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let exe = std::env::current_exe()?;
        let exe_s = exe.to_string_lossy().replace('/', "\\");
        if enable {
            let quoted = format!("\"{exe_s}\"");
            let st = std::process::Command::new("reg")
                .args([
                    "add",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    "Nidong",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &quoted,
                    "/f",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .status()
                .map_err(|e| e.to_string())?;
            if !st.success() {
                return Err("无法写入开机自启".into());
            }
        } else {
            let _ = std::process::Command::new("reg")
                .args([
                    "delete",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    "Nidong",
                    "/f",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .status();
        }
    }
    let _ = enable;
    Ok(())
}

fn island_logical_width(win: &WebviewWindow) -> f64 {
    let scale = win.scale_factor().unwrap_or(1.0);
    win.outer_size()
        .ok()
        .map(|s| s.width as f64 / scale)
        .filter(|w| *w > 1.0)
        .unwrap_or(420.0)
}

fn clamp_pos(win: &WebviewWindow, x: f64, y: f64) -> (f64, f64) {
    let mut x = x;
    // 旧默认距顶 8px；贴顶时上沿不应留空。
    let mut y = if y <= 8.0 { 0.0 } else { y };
    let ww = island_logical_width(win);
    if let Ok(Some(m)) = win.current_monitor() {
        let scale = m.scale_factor();
        let origin = m.position();
        let size = m.size();
        let mx = origin.x as f64 / scale;
        let my = origin.y as f64 / scale;
        let sw = size.width as f64 / scale;
        let sh = size.height as f64 / scale;
        x = x.clamp(mx, (mx + sw - ww).max(mx));
        y = y.clamp(my, (my + sh - 40.0).max(my));
        if y <= my + 8.0 {
            y = my;
        }
    }
    (x, y)
}

fn place_window(win: &WebviewWindow, settings: &AppSettings) {
    if let (Some(x), Some(y)) = (settings.island_x, settings.island_y) {
        let (x, y) = clamp_pos(win, x, y);
        let _ = win.set_position(LogicalPosition::new(x, y));
        return;
    }
    if let Ok(Some(m)) = win.current_monitor() {
        let size = m.size();
        let scale = m.scale_factor();
        let sw = size.width as f64 / scale;
        let x = ((sw - 420.0) / 2.0).max(0.0);
        let _ = win.set_position(LogicalPosition::new(x, 0.0));
    }
}

fn wipe_local_data(dir: &PathBuf) {
    let _ = fs::remove_file(dir.join("cookies.json"));
    let _ = fs::remove_file(dir.join("snapshot.json"));
    let _ = fs::remove_file(dir.join("agenda.json"));
    clear_session_open(dir);
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name();
            let s = name.to_string_lossy();
            if s.starts_with("bills-") && s.ends_with(".json") {
                let _ = fs::remove_file(e.path());
            }
            if s == "semester.json" {
                let _ = fs::remove_file(e.path());
            }
        }
    }
}

const SESSION_FLAG: &str = "session.open";

fn session_is_open(dir: &PathBuf) -> bool {
    dir.join(SESSION_FLAG).is_file()
}

fn mark_session_open(dir: &PathBuf) {
    let _ = fs::write(dir.join(SESSION_FLAG), b"1");
}

fn clear_session_open(dir: &PathBuf) {
    let _ = fs::remove_file(dir.join(SESSION_FLAG));
}

/// 旧版没有 session.open：有学号缓存且还留着 cookie，视为没点过退出登录。
fn migrate_session_flag(dir: &PathBuf, snap: &Snapshot) {
    if session_is_open(dir) {
        return;
    }
    let has_user = snap
        .username
        .as_ref()
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if has_user && dir.join("cookies.json").is_file() {
        mark_session_open(dir);
    }
}

fn attach_local(dir: &PathBuf, snap: &mut Snapshot) {
    snap.agenda = agenda::load(dir);
}

fn apply_offline(state: &AppState) -> Snapshot {
    let mut snap = state.snapshot.lock().unwrap().clone();
    snap.offline = true;
    snap.errors.auth = None;
    if session_is_open(&state.data_dir) {
        snap.logged_in = true;
    }
    attach_local(&state.data_dir, &mut snap);
    *state.snapshot.lock().unwrap() = snap.clone();
    snap
}

#[tauri::command]
fn bootstrap(state: State<AppState>) -> Snapshot {
    let mut snap = state.snapshot.lock().unwrap().clone();
    attach_local(&state.data_dir, &mut snap);
    *state.snapshot.lock().unwrap() = snap.clone();
    snap
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ClockNow {
    iso: String,
    overridden: bool,
}

/// 返回泥洞认定的“当前时间”。无 dev-now.txt / NIDONG_NOW 时即系统时间。
#[tauri::command]
fn clock_now() -> ClockNow {
    ClockNow {
        iso: crate::clock::now().to_rfc3339(),
        overridden: crate::clock::is_overridden(),
    }
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> AppSettings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(app: AppHandle, state: State<AppState>, mut settings: AppSettings) -> Result<AppSettings> {
    {
        let mut g = state.settings.lock().unwrap();
        settings.island_x = g.island_x;
        settings.island_y = g.island_y;
        if settings.last_username.is_empty() {
            settings.last_username = g.last_username.clone();
        }
        if !settings.remember_password {
            settings.auto_login = false;
            let extra = state.snapshot.lock().unwrap().username.clone();
            if !settings.last_username.is_empty() {
                keyring_delete(&settings.last_username);
            }
            if let Some(u) = extra {
                if !u.is_empty() && u != settings.last_username {
                    keyring_delete(&u);
                }
            }
        }
        crate::hotkey::update(&settings);
        apply_autostart(settings.autostart)?;
        *g = settings.clone();
    }
    save_settings_file(&state.data_dir, &settings);
    if let Some(sem) = load_semester(&state.data_dir) {
        let mut snap = state.snapshot.lock().unwrap();
        apply_semester(&mut snap, sem, week1_opt(&settings.week1_monday));
    }
    let _ = app.emit("settings-updated", &settings);
    Ok(settings)
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedCreds {
    username: String,
    remember_password: bool,
    auto_login: bool,
}

#[tauri::command]
fn saved_creds(state: State<AppState>) -> SavedCreds {
    let s = state.settings.lock().unwrap().clone();
    let username = if !s.last_username.is_empty() {
        s.last_username.clone()
    } else {
        state
            .snapshot
            .lock()
            .unwrap()
            .username
            .clone()
            .unwrap_or_default()
    };
    SavedCreds {
        username,
        remember_password: s.remember_password,
        auto_login: s.auto_login,
    }
}

#[tauri::command]
fn saved_password(state: State<AppState>) -> Option<String> {
    let s = state.settings.lock().unwrap().clone();
    if !s.remember_password || s.last_username.is_empty() {
        return None;
    }
    keyring_get(&s.last_username)
}

fn remember_login(state: &AppState, username: &str, password: &str, remember: bool, auto: bool) {
    let mut s = state.settings.lock().unwrap();
    s.last_username = username.to_string();
    s.remember_password = remember;
    s.auto_login = remember && auto;
    if remember {
        let _ = keyring_set(username, password);
    } else {
        keyring_delete(username);
        s.auto_login = false;
    }
    save_settings_file(&state.data_dir, &s);
}

fn mark_logged_in(state: &AppState, username: &str) {
    mark_session_open(&state.data_dir);
    let mut snap = state.snapshot.lock().unwrap();
    snap.logged_in = true;
    snap.offline = false;
    snap.username = Some(username.to_string());
    snap.errors.auth = None;
    save_snapshot_file(&state.data_dir, &snap);
}

fn reveal_running_instance(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_decorations(false);
        let _ = w.set_shadow(false);
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_always_on_top(true);
        crate::hotkey::raise_noactivate(&w);
    }
    let _ = app.emit("island-reveal", ());
    let _ = open_desk_window(app);
}

fn spawn_refresh(app: AppHandle) {
    let _ = app.emit("auth-status", "syncing");
    tauri::async_runtime::spawn(async move {
        if let Some(state) = app.try_state::<AppState>() {
            match refresh_inner(&state, true).await {
                Ok(snap) => {
                    let status = if snap.offline {
                        "idle"
                    } else if snap.logged_in {
                        "ok"
                    } else {
                        "idle"
                    };
                    emit_snapshot(&app, &snap);
                    let _ = app.emit("auth-status", status);
                }
                Err(e) => {
                    let _ = app.emit("auth-status", format!("fail:{e}"));
                }
            }
        }
    });
}

#[tauri::command]
async fn login(
    app: AppHandle,
    state: State<'_, AppState>,
    username: String,
    password: String,
    remember_password: Option<bool>,
    auto_login: Option<bool>,
) -> Result<LoginOutcome> {
    let remember = remember_password.unwrap_or(true);
    let auto = auto_login.unwrap_or(true);
    if !crate::util::online() {
        return Err("当前离线，连上网络后再登录".into());
    }
    let client = ensure_client(&state).await?;
    let out = client.login(&username, &password).await?;
    match &out {
        LoginOutcome::Ok => {
            *state.pending_login.lock().unwrap() = None;
            remember_login(&state, &username, &password, remember, auto);
            mark_logged_in(&state, &username);
            spawn_refresh(app);
        }
        LoginOutcome::NeedSms { .. } => {
            *state.pending_login.lock().unwrap() = Some(PendingLogin {
                username,
                password,
                remember,
                auto,
            });
        }
    }
    Ok(out)
}

#[tauri::command]
async fn refresh_captcha(state: State<'_, AppState>) -> Result<String> {
    live_client(&state).await?.fetch_captcha().await
}

#[tauri::command]
async fn send_sms(state: State<'_, AppState>, captcha: String) -> Result<()> {
    live_client(&state).await?.send_sms(&captcha).await
}

#[tauri::command]
async fn cancel_sms(state: State<'_, AppState>) -> Result<()> {
    if let Ok(c) = live_client(&state).await {
        c.abort_second_auth();
    }
    *state.pending_login.lock().unwrap() = None;
    Ok(())
}

#[tauri::command]
async fn submit_sms(
    app: AppHandle,
    state: State<'_, AppState>,
    sms: String,
    trust: bool,
) -> Result<LoginOutcome> {
    let out = live_client(&state).await?.submit_sms(&sms, trust).await?;
    if matches!(out, LoginOutcome::Ok) {
        if let Some(p) = state.pending_login.lock().unwrap().take() {
            remember_login(&state, &p.username, &p.password, p.remember, p.auto);
            mark_logged_in(&state, &p.username);
        }
        spawn_refresh(app);
    }
    Ok(out)
}

#[tauri::command]
fn set_bills_open(state: State<AppState>, open: bool) {
    state.pause_bills.store(open, Ordering::SeqCst);
}

#[tauri::command]
async fn refresh(app: AppHandle, state: State<'_, AppState>) -> Result<Snapshot> {
    let snap = if !crate::util::online() {
        apply_offline(&state)
    } else {
        refresh_inner(&state, true).await?
    };
    emit_snapshot(&app, &snap);
    Ok(snap)
}

#[tauri::command]
async fn load_month(state: State<'_, AppState>, year: i32, month: u32) -> Result<Snapshot> {
    let user = live_client(&state).await.ok().and_then(|c| c.username());
    let data_dir = state.data_dir.clone();
    let now = crate::clock::now();
    let current_month = year == now.year() && month == now.month();
    let cached = if current_month {
        month_cache_fresh(&data_dir, year, month, 20 * 60)
    } else {
        load_month_cache(&data_dir, year, month)
    };
    if let Some(cached) = cached {
        let mut snap = state.snapshot.lock().unwrap();
        snap.card_spend = Some(cached);
        snap.card_month = Some(format!("{year:04}-{month:02}"));
        attach_local(&data_dir, &mut snap);
        return Ok(snap.clone());
    }
    let bills = {
        let c = live_client(&state).await?;
        with_timeout(15, c.fetch_bills_month(year, month)).await?
    };
    let (list, truncated) = bills;
    let spend = aggregate_month(year, month, &list);
    let _ = save_month_cache(&data_dir, year, month, &spend);
    let mut snap = state.snapshot.lock().unwrap();
    snap.card_bills = Some(list);
    snap.card_spend = Some(spend);
    snap.card_month = Some(format!("{year:04}-{month:02}"));
    snap.bills_truncated = Some(truncated);
    snap.username = user;
    attach_local(&data_dir, &mut snap);
    save_snapshot_file(&data_dir, &snap);
    Ok(snap.clone())
}

async fn refresh_inner(state: &AppState, retry_courses: bool) -> Result<Snapshot> {
    let _refresh_gate = state.refresh_gate.lock().await;
    let settings = state.settings.lock().unwrap().clone();
    let data_dir = state.data_dir.clone();
    let now = crate::clock::now();
    let year = now.year();
    let month = now.month();

    if !crate::util::online() {
        return Ok(apply_offline(state));
    }

    let epoch = state.epoch.load(Ordering::SeqCst);
    let c = match live_client(state).await {
        Ok(c) => c,
        Err(_) => {
            if session_is_open(&data_dir) {
                return Ok(apply_offline(state));
            }
            return Err("未登录".into());
        }
    };
    let username = c.username();

    let session = if c.has_tgt() {
        SessionState::Valid
    } else {
        match tokio::time::timeout(Duration::from_secs(10), c.session_state()).await {
            Ok(s) => s,
            Err(_) => {
                if !crate::util::online() {
                    return Ok(apply_offline(state));
                }
                SessionState::Expired
            }
        }
    };
    if session == SessionState::Expired && !c.has_tgt() {
        if !crate::util::online() {
            return Ok(apply_offline(state));
        }
        let mut snap = state.snapshot.lock().unwrap().clone();
        snap.logged_in = false;
        snap.offline = false;
        snap.errors.auth = Some("会话已失效，请重新登录".into());
        attach_local(&data_dir, &mut snap);
        save_snapshot_file(&data_dir, &snap);
        *state.snapshot.lock().unwrap() = snap.clone();
        return Ok(snap);
    }

    let (cached_sem, prev) = {
        let snap = state.snapshot.lock().unwrap();
        (
            load_semester(&data_dir).or_else(|| snap.semester.clone()),
            snap.clone(),
        )
    };
    let today = now.date_naive();
    let has_course_cache = cached_sem.is_some();
    let semester_stale = cached_sem.as_ref().map(semester_cache_stale).unwrap_or(false);
    let fetched_today = cached_sem
        .as_ref()
        .and_then(|s| fetched_local_date(&s.fetched_at))
        .map(|d| d == today)
        .unwrap_or(false);
    let attempted_today = state
        .course_attempt_day
        .lock()
        .unwrap()
        .is_some_and(|d| d == today);
    let fetch_courses = should_fetch_courses(
        has_course_cache,
        semester_stale,
        fetched_today,
        attempted_today,
        retry_courses,
    );
    // 有缓存时，自动任务今天只打一次。没缓存或学期过期则下次刷新继续试。
    if fetch_courses && has_course_cache && !semester_stale && !fetched_today {
        *state.course_attempt_day.lock().unwrap() = Some(today);
    }

    let campus = c.prepare_access().await;
    let u = username.clone().unwrap_or_default();
    let bills_fresh = month_cache_fresh(&data_dir, year, month, 20 * 60);
    let pause_bills = state.pause_bills.load(Ordering::SeqCst);
    let elec_quiet = elec_night(now.hour());
    // 校外 WebVPN 已在上面种好。卡/网/电/课并行；账单依赖一卡通 SSO，等卡回来再拉。
    // 夜里仍试电费：系统未必整晚宕着。超时短一些，失败就留缓存。
    let (card, net, elec, sem_res) = tokio::join!(
        with_timeout(22, c.fetch_card()),
        with_timeout(18, c.fetch_net(&u)),
        with_timeout(if elec_quiet { 5 } else { 12 }, c.fetch_elec(&settings.room_no)),
        async {
            if fetch_courses {
                Some(with_timeout(90, c.fetch_semester()).await)
            } else {
                None
            }
        },
    );
    let bills = if pause_bills || bills_fresh.is_some() {
        None
    } else {
        Some(with_timeout(12, c.fetch_bills_month_ex(year, month, card.is_err())).await)
    };

    let prev_net = prev.net.clone();
    let mut snap = prev;
    snap.fetched_at = now.to_rfc3339();
    snap.logged_in = true;
    snap.offline = false;
    snap.username = username;
    snap.errors.auth = None;
    snap.network = if campus {
        "campus".into()
    } else {
        "offcampus".into()
    };

    match card {
        Ok(c) => {
            snap.card = Some(c);
            snap.errors.card = None;
        }
        Err(e) => {
            let es = e.to_string();
            snap.errors.card = Some(es);
        }
    }
    match net {
        Ok(mut n) => {
            if !n.flow_from_portal {
                if let Some(prev_n) = &prev_net {
                    n.month_bytes = prev_n.month_bytes;
                    n.month_seconds = prev_n.month_seconds;
                    n.month_times = prev_n.month_times;
                    n.flow_from_portal = prev_n.flow_from_portal;
                }
            }
            snap.net = Some(n);
            snap.errors.net = None;
        }
        Err(e) => snap.errors.net = Some(e.to_string()),
    }
    match elec {
        Ok(e) => {
            snap.elec = Some(e);
            snap.errors.elec = None;
        }
        Err(e) => {
            let want = settings.room_no.trim().replace(['-', ' '], "");
            let keep = snap.elec.as_ref().is_some_and(|old| {
                !old.unbound
                    && old.remain_kwh.is_some()
                    && old.room_no.as_deref() == Some(want.as_str())
            });
            if !keep {
                if snap
                    .elec
                    .as_ref()
                    .is_some_and(|old| old.room_no.as_deref() != Some(want.as_str()))
                {
                    snap.elec = None;
                }
            }
            // 23:00–06:00 连不上就显示上次度数，不写成同步失败。
            if elec_quiet && keep {
                if let Some(old) = snap.elec.as_mut() {
                    old.stale = true;
                }
                snap.errors.elec = None;
            } else {
                if let Some(old) = snap.elec.as_mut() {
                    old.stale = false;
                }
                snap.errors.elec = Some(e.to_string());
            }
        }
    }
    match bills {
        None => {
            if snap.card_spend.is_none() {
                if let Some(cached) = load_month_cache(&data_dir, year, month) {
                    snap.card_spend = Some(cached);
                    snap.card_month = Some(format!("{year:04}-{month:02}"));
                }
            }
        }
        Some(Ok((list, truncated))) => {
            let spend = aggregate_month(year, month, &list);
            let _ = save_month_cache(&data_dir, year, month, &spend);
            snap.card_bills = Some(list);
            snap.card_spend = Some(spend);
            snap.card_month = Some(format!("{year:04}-{month:02}"));
            snap.bills_truncated = Some(truncated);
            if snap.errors.card.as_deref().is_some_and(|s| s.contains("账单") || s.contains("流水")) {
                snap.errors.card = None;
            }
        }
        Some(Err(e)) => {
            if snap.card_spend.is_none() && snap.errors.card.is_none() {
                snap.errors.card = Some(e.to_string());
            }
        }
    }

    if state.epoch.load(Ordering::SeqCst) != epoch {
        return Ok(state.snapshot.lock().unwrap().clone());
    }

    let mut sem = cached_sem;
    match sem_res {
        Some(Ok(s)) => {
            let _ = save_semester(&data_dir, &s);
            snap.errors.course = None;
            sem = Some(s);
        }
        Some(Err(e)) => {
            snap.network = if e.to_string().contains("校外") {
                "offcampus".into()
            } else {
                "unknown".into()
            };
            if sem.is_none() {
                snap.errors.course = Some(e.to_string());
            }
        }
        None => {}
    }
    if state.epoch.load(Ordering::SeqCst) != epoch {
        return Ok(state.snapshot.lock().unwrap().clone());
    }
    if let Some(s) = sem {
        apply_semester(&mut snap, s, week1_opt(&settings.week1_monday));
    }
    attach_local(&data_dir, &mut snap);

    save_snapshot_file(&data_dir, &snap);
    *state.snapshot.lock().unwrap() = snap.clone();
    Ok(snap)
}

#[tauri::command]
async fn sync_courses(app: AppHandle, state: State<'_, AppState>) -> Result<Snapshot> {
    if !crate::util::online() {
        return Err("当前离线，连上网络后再同步".into());
    }
    let settings = state.settings.lock().unwrap().clone();
    let data_dir = state.data_dir.clone();
    let sem = {
        let c = live_client(&state).await?;
        with_timeout(90, c.fetch_semester()).await?
    };
    let _ = save_semester(&data_dir, &sem);
    let mut snap = state.snapshot.lock().unwrap().clone();
    snap.errors.course = None;
    snap.offline = false;
    snap.network = if crate::util::on_campus_net() {
        "campus".into()
    } else {
        "offcampus".into()
    };
    apply_semester(&mut snap, sem, week1_opt(&settings.week1_monday));
    attach_local(&data_dir, &mut snap);
    save_snapshot_file(&data_dir, &snap);
    *state.snapshot.lock().unwrap() = snap.clone();
    emit_snapshot(&app, &snap);
    Ok(snap)
}

static DESK_PLACED: AtomicBool = AtomicBool::new(false);

fn center_desk_on_island(app: &AppHandle, desk: &WebviewWindow) {
    let mon = app
        .get_webview_window("main")
        .and_then(|w| w.current_monitor().ok().flatten())
        .or_else(|| desk.current_monitor().ok().flatten());
    let Some(m) = mon else {
        let _ = desk.center();
        return;
    };
    let area = m.work_area();
    let scale = m.scale_factor();
    let sw = area.size.width as f64 / scale;
    let sh = area.size.height as f64 / scale;
    let mx = area.position.x as f64 / scale;
    let my = area.position.y as f64 / scale;
    let (ww, wh) = desk
        .outer_size()
        .ok()
        .map(|s| (s.width as f64 / scale, s.height as f64 / scale))
        .unwrap_or((1100.0, 720.0));
    let x = mx + ((sw - ww) / 2.0).max(0.0);
    let y = my + ((sh - wh) / 2.0).max(0.0);
    let _ = desk.set_position(LogicalPosition::new(x, y));
}

fn open_desk_on_ui(app: &AppHandle) -> Result<()> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "应用状态尚未初始化".to_string())?;
    // SetForegroundWindow 会泵消息。连点若在这里阻塞等同一把锁，主线程就死锁，窗口显示未响应。
    let _open_guard = match state.desk_open_lock.try_lock() {
        Ok(g) => g,
        Err(std::sync::TryLockError::WouldBlock) => return Ok(()),
        Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
    };
    let Some(desk) = app.get_webview_window("desk") else {
        return Err("找不到工作区窗口".into());
    };
    if !DESK_PLACED.swap(true, Ordering::SeqCst) {
        center_desk_on_island(app, &desk);
    }
    let _ = desk.unminimize();
    desk.show().map_err(|e| e.to_string())?;
    // 在 UI 线程里只调用 SetForegroundWindow。set_focus 失败时会注入 Alt，
    // 岛还是前台时这个 Alt 会打开系统标题栏，连点会把消息循环卡死。
    crate::hotkey::focus_without_menu(&desk);
    Ok(())
}

fn open_desk_window(app: &AppHandle) -> Result<()> {
    let app2 = app.clone();
    app.run_on_main_thread(move || {
        let _ = open_desk_on_ui(&app2);
    })
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn open_desk(app: AppHandle) -> Result<()> {
    open_desk_window(&app)
}

/// 任意门。`policy`: direct 原样打开；intranet 校园网直连、校外走 WebVPN；campus-only 不在校园网就拒绝。
#[tauri::command]
async fn open_door(app: AppHandle, url: String, policy: String) -> Result<()> {
    let url = url.trim().to_string();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("链接不正确".into());
    }
    let campus = tokio::task::spawn_blocking(crate::util::on_campus_net)
        .await
        .unwrap_or(false);
    let target = match policy.as_str() {
        "campus-only" => {
            if !campus {
                return Err("校园网网关只能在校园网里打开".into());
            }
            url
        }
        "intranet" => {
            if campus {
                url
            } else {
                crate::webvpn::encrypt_url_to_webvpn(&url)
            }
        }
        _ => url,
    };
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(target, None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn current_wifi() -> std::result::Result<String, String> {
    let now = tokio::task::spawn_blocking(crate::wlan::query)
        .await
        .map_err(|_| "没有无线网卡".to_string())??;
    if !now.has_adapter {
        return Err("没有无线网卡".into());
    }
    let name = now.ssid.trim();
    if !now.connected || name.is_empty() {
        return Err("现在没有连上无线".into());
    }
    Ok(name.to_string())
}

#[tauri::command]
fn open_feedback(app: AppHandle) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url("mailto:xufeiyang@mails.neu.edu.cn", None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn preview_remind(app: AppHandle) {
    let _ = app.emit("preview-remind", ());
}

#[tauri::command]
fn persist_island(state: State<AppState>, win: WebviewWindow) -> Result<()> {
    if let Ok(pos) = win.outer_position() {
        let scale = win.scale_factor().unwrap_or(1.0);
        let (x, y) = clamp_pos(&win, pos.x as f64 / scale, pos.y as f64 / scale);
        let _ = win.set_position(LogicalPosition::new(x, y));
        crate::hotkey::clip_round(&win);
        let mut s = state.settings.lock().unwrap();
        s.island_x = Some(x);
        s.island_y = Some(y);
        save_settings_file(&state.data_dir, &s);
    }
    Ok(())
}

#[tauri::command]
fn reset_island(app: AppHandle, state: State<AppState>, win: WebviewWindow) -> Result<()> {
    let _ = win.show();
    win.set_size(LogicalSize::new(420.0, 40.0))
        .map_err(|e| e.to_string())?;
    {
        let mut s = state.settings.lock().unwrap();
        s.island_x = None;
        s.island_y = None;
        save_settings_file(&state.data_dir, &s);
        place_window(&win, &s);
    }
    let _ = win.set_always_on_top(true);
    crate::hotkey::raise_noactivate(&win);
    let _ = app.emit("island-reset", ());
    Ok(())
}

#[tauri::command]
fn raise_topmost(win: WebviewWindow) -> Result<()> {
    let _ = win.set_decorations(false);
    let _ = win.set_shadow(false);
    let _ = win.show();
    let _ = win.set_always_on_top(true);
    crate::hotkey::raise_noactivate(&win);
    Ok(())
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct HoverWatch {
    enabled: bool,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[tauri::command]
fn set_hover_watch(watch: HoverWatch) {
    crate::hotkey::set_hover_watch(watch.enabled, watch.x, watch.y, watch.width, watch.height);
}

#[tauri::command]
fn save_agenda(app: AppHandle, state: State<AppState>, item: AgendaItem) -> Result<Snapshot> {
    let items = agenda::upsert(&state.data_dir, item)?;
    let mut snap = state.snapshot.lock().unwrap();
    snap.agenda = items;
    let out = snap.clone();
    drop(snap);
    emit_snapshot(&app, &out);
    Ok(out)
}

#[tauri::command]
fn delete_agenda(app: AppHandle, state: State<AppState>, id: String) -> Result<Snapshot> {
    let items = agenda::delete(&state.data_dir, &id)?;
    let mut snap = state.snapshot.lock().unwrap();
    snap.agenda = items;
    let out = snap.clone();
    drop(snap);
    emit_snapshot(&app, &out);
    Ok(out)
}

#[tauri::command]
async fn logout(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    state.epoch.fetch_add(1, Ordering::SeqCst);
    let remember = state.settings.lock().unwrap().remember_password;
    let user = {
        let g = state.client.lock().await;
        g.as_ref()
            .and_then(|c| c.username())
            .or_else(|| state.snapshot.lock().unwrap().username.clone())
    };
    *state.client.lock().await = None;
    *state.snapshot.lock().unwrap() = Snapshot::default();
    *state.pending_login.lock().unwrap() = None;
    wipe_local_data(&state.data_dir);
    if !remember {
        if let Some(u) = user {
            keyring_delete(&u);
        }
    }
    let snap = state.snapshot.lock().unwrap().clone();
    emit_snapshot(&app, &snap);
    Ok(())
}

fn emit_logged_out_keep_cache(app: &AppHandle, state: &AppState, reason: &str) {
    let mut snap = state.snapshot.lock().unwrap().clone();
    snap.logged_in = false;
    snap.offline = false;
    if snap.errors.auth.is_none() {
        snap.errors.auth = Some(reason.into());
    }
    attach_local(&state.data_dir, &mut snap);
    save_snapshot_file(&state.data_dir, &snap);
    *state.snapshot.lock().unwrap() = snap.clone();
    emit_snapshot(&app, &snap);
    let _ = app.emit("auth-status", "idle");
}

fn emit_offline(app: &AppHandle, state: &AppState) {
    let snap = apply_offline(state);
    emit_snapshot(&app, &snap);
    let _ = app.emit("auth-status", "idle");
}

async fn resume_session(app: AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let settings = state.settings.lock().unwrap().clone();
    let username = {
        let snap_u = state.snapshot.lock().unwrap().username.clone();
        if snap_u.as_ref().map(|s| !s.is_empty()).unwrap_or(false) {
            snap_u
        } else if !settings.last_username.is_empty() {
            Some(settings.last_username.clone())
        } else {
            None
        }
    };

    if !crate::util::online() {
        if session_is_open(&state.data_dir) {
            emit_offline(&app, &state);
        }
        return;
    }

    let session = {
        let c = state.client.lock().await.clone();
        match c {
            Some(c) => Some(c.session_state().await),
            None => None,
        }
    };

    match session {
        Some(SessionState::Valid) => {
            let _ = app.emit("auth-status", "syncing");
            match refresh_inner(&state, true).await {
                Ok(snap) => {
                    let status = if snap.offline {
                        "idle"
                    } else if snap.logged_in {
                        "ok"
                    } else {
                        "idle"
                    };
                    emit_snapshot(&app, &snap);
                    let _ = app.emit("auth-status", status);
                }
                Err(e) => {
                    let es = e.to_string();
                    if session_is_open(&state.data_dir)
                        && (!crate::util::online() || crate::util::is_offline_err(&es))
                    {
                        emit_offline(&app, &state);
                    } else {
                        emit_logged_out_keep_cache(&app, &state, "会话已失效，请重新登录");
                    }
                }
            }
            return;
        }
        Some(SessionState::NeedSms) => {
            if settings.auto_login && settings.remember_password {
                // fall through to password login, which may resume 2FA
            } else if session_is_open(&state.data_dir) && !crate::util::online() {
                emit_offline(&app, &state);
                return;
            } else {
                emit_logged_out_keep_cache(&app, &state, "换了网络，这台电脑需要再验证一次");
                return;
            }
        }
        Some(SessionState::Expired) | None => {
            if !(settings.auto_login && settings.remember_password) {
                if session_is_open(&state.data_dir) && !crate::util::online() {
                    emit_offline(&app, &state);
                } else {
                    emit_logged_out_keep_cache(&app, &state, "会话已失效，请重新登录");
                }
                return;
            }
        }
    }

    let Some(user) = username else {
        if session_is_open(&state.data_dir) && !crate::util::online() {
            emit_offline(&app, &state);
        } else {
            emit_logged_out_keep_cache(&app, &state, "未登录");
        }
        return;
    };
    let Some(pass) = keyring_get(&user) else {
        if session_is_open(&state.data_dir) && !crate::util::online() {
            emit_offline(&app, &state);
        } else {
            emit_logged_out_keep_cache(&app, &state, "未登录");
        }
        return;
    };
    let _ = app.emit("auth-status", "logging");
    let client = match ensure_client(&state).await {
        Ok(c) => c,
        Err(_) => {
            let _ = app.emit("auth-status", "idle");
            return;
        }
    };
    let out = match client.login_ex(&user, &pass, false).await {
        Ok(o) => o,
        Err(e) => {
            let es = e.to_string();
            if session_is_open(&state.data_dir)
                && (!crate::util::online() || crate::util::is_offline_err(&es))
            {
                emit_offline(&app, &state);
            } else {
                let _ = app.emit("auth-status", format!("fail:{e}"));
            }
            return;
        }
    };
    match out {
        LoginOutcome::Ok => {
            mark_logged_in(&state, &user);
            spawn_refresh(app);
        }
        out @ LoginOutcome::NeedSms { .. } => {
            *state.pending_login.lock().unwrap() = Some(PendingLogin {
                username: user,
                password: pass,
                remember: settings.remember_password,
                auto: settings.auto_login,
            });
            let _ = app.emit("need-sms", out);
            let _ = app.emit("auth-status", "need_sms");
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();
    builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        reveal_running_instance(app);
    }));
    builder = builder.plugin(tauri_plugin_opener::init());
    builder = builder.plugin(tauri_plugin_process::init());
    builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    builder
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            let _ = fs::create_dir_all(&data_dir);
            crate::clock::init(data_dir.clone());
            let settings = load_settings(&data_dir);
            let _ = apply_autostart(settings.autostart);
            crate::hotkey::init(app.handle().clone());
            crate::hotkey::update(&settings);
            let mut snapshot: Snapshot = fs::read(data_dir.join("snapshot.json"))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default();
            if let Some(sem) = load_semester(&data_dir) {
                apply_semester(&mut snapshot, sem, week1_opt(&settings.week1_monday));
            }
            let client = NeuClient::new(data_dir.clone()).ok().map(Arc::new);
            if let Some(c) = &client {
                if let Some(u) = snapshot.username.clone() {
                    c.set_username(u);
                }
            }
            migrate_session_flag(&data_dir, &snapshot);
            snapshot.agenda = agenda::load(&data_dir);
            if session_is_open(&data_dir) {
                snapshot.logged_in = true;
                snapshot.errors.auth = None;
            } else {
                snapshot.logged_in = false;
            }
            app.manage(AppState {
                client: AsyncMutex::new(client),
                epoch: AtomicU64::new(0),
                settings: Mutex::new(settings.clone()),
                snapshot: Mutex::new(snapshot),
                pending_login: Mutex::new(None),
                pause_bills: AtomicBool::new(false),
                desk_open_lock: Mutex::new(()),
                refresh_gate: AsyncMutex::new(()),
                course_attempt_day: Mutex::new(None),
                data_dir: data_dir.clone(),
                tray_balances: Mutex::new(None),
            });

            if let Some(win) = app.get_webview_window("main") {
                place_window(&win, &settings);
                crate::hotkey::clip_round(&win);
                crate::hotkey::block_island_caption(&win);
                let handle = app.handle().clone();
                let move_gen = Arc::new(AtomicU64::new(0));
                let move_echo = Arc::new(AtomicBool::new(false));
                let _ = win.on_window_event(move |ev| {
                    let Some(w) = handle.get_webview_window("main") else {
                        return;
                    };
                    if matches!(ev, tauri::WindowEvent::Resized(_)) {
                        crate::hotkey::clip_round(&w);
                    }
                    if let tauri::WindowEvent::Moved(p) = ev {
                        // set_position 会再发一条 Moved。若再纠正，DPI 取整会把消息循环打满，窗口未响应。
                        if move_echo.swap(false, Ordering::SeqCst) {
                            crate::hotkey::clip_round(&w);
                            return;
                        }
                        let scale = w.scale_factor().unwrap_or(1.0);
                        let raw_x = p.x as f64 / scale;
                        let raw_y = p.y as f64 / scale;
                        let (x, y) = clamp_pos(&w, raw_x, raw_y);
                        if (x - raw_x).abs() > 2.0 || (y - raw_y).abs() > 2.0 {
                            move_echo.store(true, Ordering::SeqCst);
                            let _ = w.set_position(LogicalPosition::new(x, y));
                        }
                        crate::hotkey::clip_round(&w);
                        if let Some(state) = handle.try_state::<AppState>() {
                            let mut s = state.settings.lock().unwrap();
                            s.island_x = Some(x);
                            s.island_y = Some(y);
                        }
                        let n = move_gen.fetch_add(1, Ordering::Relaxed) + 1;
                        let handle2 = handle.clone();
                        let gen = move_gen.clone();
                        tauri::async_runtime::spawn(async move {
                            tokio::time::sleep(Duration::from_millis(350)).await;
                            if gen.load(Ordering::Relaxed) != n {
                                return;
                            }
                            if let Some(state) = handle2.try_state::<AppState>() {
                                let s = state.settings.lock().unwrap().clone();
                                save_settings_file(&state.data_dir, &s);
                            }
                        });
                    }
                });
            }

            let boot = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                resume_session(boot).await;
            });
            // 卡/网/电约 20 分钟。课表不跟这次一起打，除非今天还没同步过。
            let ticker = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(20 * 60)).await;
                    let Some(state) = ticker.try_state::<AppState>() else {
                        continue;
                    };
                    if !crate::util::online() {
                        continue;
                    }
                    if let Ok(snap) = refresh_inner(&state, false).await {
                        emit_snapshot(&ticker, &snap);
                    }
                }
            });

            if let Some(desk) = app.get_webview_window("desk") {
                let win = desk.clone();
                desk.on_window_event(move |ev| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = ev {
                        api.prevent_close();
                        let _ = win.hide();
                    }
                });
            }

            let snap_now = app.state::<AppState>().snapshot.lock().unwrap().clone();
            let (card_t, net_t, elec_t) = tray_labels(&snap_now);
            let card = MenuItem::with_id(app, "bal-card", card_t, true, None::<&str>)?;
            let net = MenuItem::with_id(app, "bal-net", net_t, true, None::<&str>)?;
            let elec = MenuItem::with_id(app, "bal-elec", elec_t, true, None::<&str>)?;
            let open = MenuItem::with_id(app, "open", "打开窗口", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let reset = MenuItem::with_id(app, "reset", "复位岛", true, None::<&str>)?;
            let refresh = MenuItem::with_id(app, "refresh", "刷新", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let menu = Menu::new(app)?;
            menu.append(&card)?;
            menu.append(&net)?;
            menu.append(&elec)?;
            menu.append(&sep)?;
            menu.append(&open)?;
            menu.append(&refresh)?;
            menu.append(&reset)?;
            menu.append(&quit)?;
            *app.state::<AppState>().tray_balances.lock().unwrap() = Some(TrayBalances {
                card: card.clone(),
                net: net.clone(),
                elec: elec.clone(),
            });
            let mut tray = TrayIconBuilder::new()
                .menu(&menu)
                .tooltip("泥洞")
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|icon, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let _ = open_desk_window(icon.app_handle());
                    }
                });
            // Use the native 32px mark (not a downscaled 128/ICO frame) so the
            // notification-area icon stays sharp at 100–150% DPI.
            tray = tray.icon(Image::new_owned(
                include_bytes!("../icons/tray-32.rgba").to_vec(),
                32,
                32,
            ));
            let _tray = tray
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "open" => {
                        let _ = open_desk_window(app);
                    }
                    "reset" => {
                        if let Some(w) = app.get_webview_window("main") {
                            if let Some(state) = app.try_state::<AppState>() {
                                let _ = w.show();
                                let _ = w.set_size(LogicalSize::new(420.0, 40.0));
                                let mut s = state.settings.lock().unwrap();
                                s.island_x = None;
                                s.island_y = None;
                                save_settings_file(&state.data_dir, &s);
                                place_window(&w, &s);
                                let _ = w.set_always_on_top(true);
                                crate::hotkey::raise_noactivate(&w);
                            }
                        }
                        let _ = app.emit("island-reset", ());
                    }
                    "refresh" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Some(state) = app.try_state::<AppState>() {
                                if let Ok(snap) = refresh_inner(&state, true).await {
                                    emit_snapshot(&app, &snap);
                                }
                            }
                        });
                    }
                    _ => {}
                })
                .build(app)?;
            campus::spawn(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            clock_now,
            get_settings,
            save_settings,
            saved_creds,
            saved_password,
            persist_island,
            login,
            refresh_captcha,
            send_sms,
            cancel_sms,
            submit_sms,
            refresh,
            load_month,
            sync_courses,
            open_desk,
            open_door,
            preview_remind,
            current_wifi,
            open_feedback,
            reset_island,
            raise_topmost,
            set_hover_watch,
            set_bills_open,
            save_agenda,
            delete_agenda,
            logout
        ])
        .run(tauri::generate_context!())
        .expect("泥洞启动失败");
}

#[cfg(test)]
mod tests {
    use super::elec_night;

    #[test]
    fn elec_quiet_hours_are_23_to_6() {
        assert!(elec_night(23));
        assert!(elec_night(0));
        assert!(elec_night(5));
        assert!(!elec_night(6));
        assert!(!elec_night(11));
        assert!(!elec_night(22));
    }
}
