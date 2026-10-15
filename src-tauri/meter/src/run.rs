//! 提权后的记录循环。不在校园网时仍采样，只为把基线往前挪，不写入账本。

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::sample::Sampler;
use crate::store;
use crate::task;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Live {
    on_campus: bool,
    ipv4: String,
    ipv6: String,
    v4_down_bps: u64,
    v6_down_bps: u64,
    night: bool,
    elevated: bool,
    udp_ready: bool,
    udp_gap: bool,
    updated: u64,
}

pub fn entry() {
    hide_console();
    let cmd = std::env::args().nth(1).unwrap_or_default();
    match cmd.as_str() {
        "--enable" => enable(),
        "--disable" => disable(),
        "--run" => run(),
        _ => {}
    }
}

fn enable() {
    clear_error();
    if let Err(err) = require_admin() {
        write_error(&err);
        return;
    }
    if let Err(err) = task::install(&task::exe_path()) {
        write_error(&err);
        return;
    }
    if let Err(err) = set_enabled(true) {
        write_error(&err.to_string());
        return;
    }
    let _ = fs::remove_file(stop_path());
    if !acquire_mutex() {
        return;
    }
    loop_sample();
}

fn disable() {
    clear_error();
    if let Err(err) = require_admin() {
        write_error(&err);
        return;
    }
    let _ = fs::create_dir_all(store::meter_dir());
    let _ = fs::write(stop_path(), b"stop\n");
    if let Err(err) = task::remove() {
        write_error(&err);
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline && mutex_held_by_other() {
        thread::sleep(Duration::from_millis(200));
    }
    let _ = set_enabled(false);
    if !mutex_held_by_other() {
        let _ = fs::remove_file(stop_path());
    }
}

fn run() {
    if !enabled_file() {
        return;
    }
    if require_admin().is_err() {
        return;
    }
    clear_error();
    if !acquire_mutex() {
        return;
    }
    loop_sample();
}

fn loop_sample() {
    let path = store::usage_path();
    let mut ledger = store::load_ledger(&path);
    let (month, _, _) = store::now_parts();
    ledger = store::archive_if_needed(&path, ledger, &month);
    if ledger.month.is_empty() {
        ledger.month = month;
    }
    let mut sampler = Sampler::new();
    let udp_watch = crate::udp::Watch::start();
    let _ = udp_watch.take(false);
    let mut last_save = Instant::now();
    let mut last_poll = Instant::now();
    loop {
        if stop_requested() {
            break;
        }
        let (month, day, night) = store::now_parts();
        ledger = store::archive_if_needed(&path, ledger, &month);
        let (on_campus, ipv4, ipv6) = crate::place::currently_on_campus();
        let tick = sampler.poll(night);
        let udp = udp_watch.take(night);
        // 会话正常时 TCP 已在事件里。再叠连接计数会把长连接算两次。
        let totals = if udp.ready && !udp.gap { udp.totals } else { tick.totals };
        let elapsed = last_poll.elapsed().as_secs_f64().max(0.2);
        last_poll = Instant::now();
        if on_campus {
            ledger.add(&day, totals);
        }
        let v4_down = totals.v4_day + totals.v4_night;
        let (v4_down_bps, v6_down_bps) = if on_campus {
            (
                (v4_down as f64 / elapsed) as u64,
                (totals.v6_down as f64 / elapsed) as u64,
            )
        } else {
            (0, 0)
        };
        write_live(&Live {
            on_campus,
            ipv4,
            ipv6,
            v4_down_bps,
            v6_down_bps,
            night,
            elevated: true,
            udp_ready: udp.ready,
            udp_gap: udp.gap,
            updated: unix_now(),
        });
        if last_save.elapsed() >= Duration::from_secs(30) {
            let _ = store::save_ledger(&path, &ledger);
            last_save = Instant::now();
        }
        thread::sleep(Duration::from_secs(2));
    }
    let _ = store::save_ledger(&path, &ledger);
    drop(udp_watch);
    let _ = fs::remove_file(stop_path());
    release_mutex();
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn write_live(live: &Live) {
    let dir = store::meter_dir();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("live.json");
    let tmp = dir.join("live.json.tmp");
    let Ok(text) = serde_json::to_string(live) else {
        return;
    };
    if fs::File::create(&tmp).and_then(|mut f| f.write_all(text.as_bytes())).is_ok() {
        let _ = fs::rename(&tmp, &path);
    }
}

pub fn read_live() -> Option<LiveOwned> {
    let text = fs::read_to_string(store::meter_dir().join("live.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(LiveOwned {
        on_campus: v.get("onCampus").and_then(|x| x.as_bool()).unwrap_or(false),
        ipv4: v.get("ipv4").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        ipv6: v.get("ipv6").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        v4_down_bps: v.get("v4DownBps").and_then(|x| x.as_u64()).unwrap_or(0),
        v6_down_bps: v.get("v6DownBps").and_then(|x| x.as_u64()).unwrap_or(0),
        night: v.get("night").and_then(|x| x.as_bool()).unwrap_or(false),
        elevated: v.get("elevated").and_then(|x| x.as_bool()).unwrap_or(true),
        udp_ready: v.get("udpReady").and_then(|x| x.as_bool()).unwrap_or(false),
        udp_gap: v.get("udpGap").and_then(|x| x.as_bool()).unwrap_or(false),
        updated: v.get("updated").and_then(|x| x.as_u64()).unwrap_or(0),
    })
}

pub struct LiveOwned {
    pub on_campus: bool,
    pub ipv4: String,
    pub ipv6: String,
    pub v4_down_bps: u64,
    pub v6_down_bps: u64,
    pub night: bool,
    pub elevated: bool,
    pub udp_ready: bool,
    pub udp_gap: bool,
    pub updated: u64,
}

fn require_admin() -> Result<(), String> {
    let ok = unsafe { windows::Win32::UI::Shell::IsUserAnAdmin().as_bool() };
    if ok {
        Ok(())
    } else {
        Err("没有管理员权限，流量记录没有打开。".into())
    }
}

fn enabled_path() -> PathBuf {
    store::meter_dir().join("enabled.txt")
}

fn stop_path() -> PathBuf {
    store::meter_dir().join("stop")
}

fn error_path() -> PathBuf {
    store::meter_dir().join("error.txt")
}

pub fn enabled_file() -> bool {
    fs::read_to_string(enabled_path()).map(|t| t.trim() == "on").unwrap_or(false)
}

fn set_enabled(on: bool) -> std::io::Result<()> {
    let path = enabled_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    if on {
        fs::write(path, "on\n")
    } else {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }
}

pub fn read_error() -> String {
    fs::read_to_string(error_path()).unwrap_or_default().trim().to_string()
}

fn write_error(msg: &str) {
    let path = error_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, msg);
}

fn clear_error() {
    let _ = fs::remove_file(error_path());
}

fn stop_requested() -> bool {
    stop_path().is_file()
}

fn hide_console() {
    unsafe {
        let hwnd = GetConsoleWindow();
        if !hwnd.is_null() {
            ShowWindow(hwnd, 0);
        }
        FreeConsole();
    }
}

const MUTEX_NAME: &str = "Local\\NidongMeter";

struct MutexHold {
    handle: *mut core::ffi::c_void,
}

unsafe impl Send for MutexHold {}

static HOLD: std::sync::Mutex<Option<MutexHold>> = std::sync::Mutex::new(None);

fn acquire_mutex() -> bool {
    let name = wide(MUTEX_NAME);
    unsafe { SetLastError(0) };
    let handle = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
    if handle.is_null() {
        return false;
    }
    let already = unsafe { GetLastError() } == 183;
    if already {
        unsafe { CloseHandle(handle) };
        return false;
    }
    if let Ok(mut slot) = HOLD.lock() {
        *slot = Some(MutexHold { handle });
    }
    true
}

fn release_mutex() {
    if let Ok(mut slot) = HOLD.lock() {
        if let Some(hold) = slot.take() {
            unsafe { CloseHandle(hold.handle) };
        }
    }
}

fn mutex_held_by_other() -> bool {
    let name = wide(MUTEX_NAME);
    let handle = unsafe { OpenMutexW(0x00100000, 0, name.as_ptr()) };
    if handle.is_null() {
        return false;
    }
    unsafe { CloseHandle(handle) };
    true
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateMutexW(attr: *const u8, initial: i32, name: *const u16) -> *mut core::ffi::c_void;
    fn OpenMutexW(access: u32, inherit: i32, name: *const u16) -> *mut core::ffi::c_void;
    fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
    fn GetLastError() -> u32;
    fn SetLastError(code: u32);
    fn GetConsoleWindow() -> *mut core::ffi::c_void;
    fn FreeConsole() -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn ShowWindow(hwnd: *mut core::ffi::c_void, cmd: i32) -> i32;
}
