//! 全局快捷键（默认 Ctrl+Q）与悬停热区。
//! Windows 用低层键盘钩子：自定义组合可能含 Tab，RegisterHotKey 接不住。
//! 钩子里只记录按键，真正弹窗放到独立线程 / 主线程，避免在钩子回调里调窗口 API。

use crate::models::AppSettings;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

const MAX_KEYS: usize = 4;
/// 组合键必须在这个时间内全部按下，才算一次和弦，避免先按 Tab 过很久再按 Q。
const CHORD_MS: u64 = 400;

#[derive(Clone, Copy)]
enum HotkeyEvt {
    Down,
    Up,
}

struct KeyMap {
    down: [bool; 256],
    at_ms: [u64; 256],
}

static APP: OnceLock<AppHandle> = OnceLock::new();
static HOTKEY_TX: OnceLock<SyncSender<HotkeyEvt>> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(false);
static FIRED: AtomicBool = AtomicBool::new(false);
static NKEYS: AtomicUsize = AtomicUsize::new(0);
static KEYS: [AtomicU32; MAX_KEYS] = [
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
];
static KEY_MAP: Mutex<KeyMap> = Mutex::new(KeyMap {
    down: [false; 256],
    at_ms: [0; 256],
});

static HOVER_ON: AtomicBool = AtomicBool::new(false);
static HOVER_INSIDE: AtomicBool = AtomicBool::new(false);
static HOVER_X: AtomicI32 = AtomicI32::new(0);
static HOVER_Y: AtomicI32 = AtomicI32::new(0);
static HOVER_W: AtomicI32 = AtomicI32::new(0);
static HOVER_H: AtomicI32 = AtomicI32::new(0);

pub fn init(app: AppHandle) {
    let _ = APP.set(app);
    let (tx, rx) = mpsc::sync_channel::<HotkeyEvt>(8);
    let _ = HOTKEY_TX.set(tx);
    std::thread::Builder::new()
        .name("nidong-hotkey-fire".into())
        .spawn(move || {
            while let Ok(evt) = rx.recv() {
                match evt {
                    HotkeyEvt::Down => emit_reveal(),
                    HotkeyEvt::Up => emit_release(),
                }
            }
        })
        .ok();
    #[cfg(windows)]
    {
        win::start_hook_thread();
        win::start_hover_thread();
        win::start_fullscreen_thread();
    }
}

pub fn update(settings: &AppSettings) {
    let on = settings.auto_hide && settings.auto_hide_hotkey;
    let combo = parse_hotkey(&settings.reveal_hotkey);
    for (i, slot) in KEYS.iter().enumerate() {
        slot.store(*combo.get(i).unwrap_or(&0), Ordering::Relaxed);
    }
    NKEYS.store(combo.len().min(MAX_KEYS), Ordering::Relaxed);
    ENABLED.store(on && combo.len() >= 2, Ordering::Relaxed);
    FIRED.store(false, Ordering::Relaxed);
}

pub fn set_hover_watch(enabled: bool, x: i32, y: i32, width: i32, height: i32) {
    HOVER_X.store(x, Ordering::Relaxed);
    HOVER_Y.store(y, Ordering::Relaxed);
    HOVER_W.store(width.max(0), Ordering::Relaxed);
    HOVER_H.store(height.max(0), Ordering::Relaxed);
    HOVER_ON.store(enabled && width > 0 && height > 0, Ordering::Relaxed);
    if !enabled {
        HOVER_INSIDE.store(false, Ordering::Relaxed);
    }
}

pub fn raise_noactivate(win: &WebviewWindow) {
    #[cfg(windows)]
    win::raise_noactivate(win);
    let _ = win;
}

/// Clip the HWND to the island's CSS radius so Windows doesn't draw a square frame.
pub fn clip_round(win: &WebviewWindow) {
    #[cfg(windows)]
    win::clip_round(win);
    let _ = win;
}

pub fn parse_hotkey(s: &str) -> Vec<u32> {
    s.split('+')
        .filter_map(|p| vk_of(p.trim()))
        .collect::<Vec<_>>()
}

fn vk_of(p: &str) -> Option<u32> {
    let u = p.to_ascii_uppercase();
    Some(match u.as_str() {
        "TAB" => 0x09,
        "CTRL" | "CONTROL" => 0x11,
        "ALT" => 0x12,
        "SHIFT" => 0x10,
        "WIN" | "META" | "SUPER" => 0x5B,
        "ESC" | "ESCAPE" => 0x1B,
        "SPACE" | " " => 0x20,
        "ENTER" | "RETURN" => 0x0D,
        "BACKSPACE" => 0x08,
        x if x.len() == 1 => {
            let c = x.chars().next()?.to_ascii_uppercase();
            if c.is_ascii_alphanumeric() {
                c as u32
            } else {
                return None;
            }
        }
        x if x.starts_with('F') => {
            let n: u32 = x[1..].parse().ok()?;
            if (1..=12).contains(&n) {
                0x70 + n - 1
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn combo_down() -> bool {
    let n = NKEYS.load(Ordering::Relaxed);
    if n < 2 {
        return false;
    }
    let Ok(map) = KEY_MAP.lock() else {
        return false;
    };
    (0..n).all(|i| {
        let vk = KEYS[i].load(Ordering::Relaxed) as usize;
        vk < 256 && map.down[vk]
    })
}

/// 所有组合键当前都按下，且最早/最晚按下不超过 CHORD_MS。忽略长按自动重复。
fn combo_chord() -> bool {
    let n = NKEYS.load(Ordering::Relaxed);
    if n < 2 {
        return false;
    }
    let Ok(map) = KEY_MAP.lock() else {
        return false;
    };
    let mut min_at = u64::MAX;
    let mut max_at = 0u64;
    for i in 0..n {
        let vk = KEYS[i].load(Ordering::Relaxed) as usize;
        if vk >= 256 || !map.down[vk] || map.at_ms[vk] == 0 {
            return false;
        }
        min_at = min_at.min(map.at_ms[vk]);
        max_at = max_at.max(map.at_ms[vk]);
    }
    max_at.saturating_sub(min_at) <= CHORD_MS
}

fn normalize_vk(vk: u32) -> u32 {
    match vk {
        0xA0 | 0xA1 => 0x10, // L/R Shift
        0xA2 | 0xA3 => 0x11, // L/R Ctrl
        0xA4 | 0xA5 => 0x12, // L/R Alt
        _ => vk,
    }
}

fn note_key(vk: u32, is_down: bool) {
    let vk = normalize_vk(vk);
    if vk >= 256 {
        return;
    }
    let Ok(mut map) = KEY_MAP.lock() else {
        return;
    };
    let i = vk as usize;
    if is_down {
        if !map.down[i] {
            map.at_ms[i] = now_ms();
        }
        map.down[i] = true;
    } else {
        map.down[i] = false;
        map.at_ms[i] = 0;
    }
}

fn emit_reveal() {
    let Some(app) = APP.get().cloned() else {
        return;
    };
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = app2.get_webview_window("main") {
            let _ = w.show();
            let _ = w.set_always_on_top(true);
            raise_noactivate(&w);
        }
        let _ = app2.emit("hotkey-reveal", ());
    });
}

fn emit_release() {
    let Some(app) = APP.get().cloned() else {
        return;
    };
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        let _ = app2.emit("hotkey-release", ());
    });
}

fn emit_hover(inside: bool) {
    let Some(app) = APP.get().cloned() else {
        return;
    };
    let _ = app.emit("hover-zone", inside);
}

fn emit_exclusive_fullscreen(on: bool) {
    let Some(app) = APP.get().cloned() else {
        return;
    };
    let _ = app.emit("exclusive-fullscreen", on);
}

fn fire_hotkey(evt: HotkeyEvt) {
    if let Some(tx) = HOTKEY_TX.get() {
        let _ = tx.try_send(evt);
    } else {
        match evt {
            HotkeyEvt::Down => emit_reveal(),
            HotkeyEvt::Up => emit_release(),
        }
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::ffi::c_void;

    const WH_KEYBOARD_LL: i32 = 13;
    const WM_KEYDOWN: usize = 0x0100;
    const WM_SYSKEYDOWN: usize = 0x0104;
    const WM_KEYUP: usize = 0x0101;
    const WM_SYSKEYUP: usize = 0x0105;
    const LLKHF_UP: u32 = 0x80;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_SHOWWINDOW: u32 = 0x0040;

    #[repr(C)]
    struct Msg {
        hwnd: *mut c_void,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        pt_x: i32,
        pt_y: i32,
        lprivate: u32,
    }

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    struct KbdLl {
        vk_code: u32,
        scan_code: u32,
        flags: u32,
        time: u32,
        extra: usize,
    }

    #[link(name = "user32")]
    extern "system" {
        fn SetWindowPos(
            hwnd: *mut c_void,
            after: *mut c_void,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> i32;
        fn SetWindowsHookExW(
            id: i32,
            lpfn: Option<unsafe extern "system" fn(i32, usize, isize) -> isize>,
            hmod: *mut c_void,
            tid: u32,
        ) -> *mut c_void;
        fn UnhookWindowsHookEx(hhk: *mut c_void) -> i32;
        fn CallNextHookEx(hhk: *mut c_void, code: i32, w: usize, l: isize) -> isize;
        fn GetMessageW(msg: *mut Msg, hwnd: *mut c_void, min: u32, max: u32) -> i32;
        fn TranslateMessage(msg: *const Msg) -> i32;
        fn DispatchMessageW(msg: *const Msg) -> isize;
        fn GetCursorPos(pt: *mut Point) -> i32;
        fn SetWindowRgn(hwnd: *mut c_void, rgn: *mut c_void, redraw: i32) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    }

    #[link(name = "shell32")]
    extern "system" {
        fn SHQueryUserNotificationState(state: *mut i32) -> i32;
    }

    #[link(name = "gdi32")]
    extern "system" {
        fn CreateRoundRectRgn(x1: i32, y1: i32, x2: i32, y2: i32, w: i32, h: i32) -> *mut c_void;
        fn CreateRectRgn(x1: i32, y1: i32, x2: i32, y2: i32) -> *mut c_void;
        fn CombineRgn(dst: *mut c_void, a: *mut c_void, b: *mut c_void, mode: i32) -> i32;
        fn DeleteObject(obj: *mut c_void) -> i32;
    }

    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(hwnd: *mut c_void, attr: u32, val: *const c_void, size: u32) -> i32;
    }

    const RGN_OR: i32 = 2;
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWCP_DONOTROUND: u32 = 1;

    pub fn clip_round(win: &WebviewWindow) {
        let Ok(hwnd) = win.hwnd() else {
            return;
        };
        let Ok(size) = win.outer_size() else {
            return;
        };
        let scale = win.scale_factor().unwrap_or(1.0);
        let w = size.width as i32;
        let h = size.height as i32;
        if w <= 0 || h <= 0 {
            return;
        }
        let logical_y = win
            .outer_position()
            .ok()
            .map(|p| p.y as f64 / scale)
            .unwrap_or(0.0);
        let flush = logical_y <= 8.0;
        let expanded = (h as f64 / scale) > 50.0;
        let square_top = flush || expanded;
        let dia = ((20.0 * scale).round() as i32 * 2).max(2);
        unsafe {
            let prefer = DWMWCP_DONOTROUND;
            DwmSetWindowAttribute(
                hwnd.0,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &prefer as *const u32 as *const c_void,
                4,
            );
            let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, dia, dia);
            if rgn.is_null() {
                return;
            }
            if square_top {
                let top = CreateRectRgn(0, 0, w + 1, dia / 2 + 1);
                if !top.is_null() {
                    CombineRgn(rgn, rgn, top, RGN_OR);
                    DeleteObject(top);
                }
            }
            SetWindowRgn(hwnd.0, rgn, 1);
        }
    }

    const QUNS_RUNNING_D3D_FULL_SCREEN: i32 = 3;

    pub fn raise_noactivate(win: &WebviewWindow) {
        if let Ok(hwnd) = win.hwnd() {
            unsafe {
                SetWindowPos(
                    hwnd.0,
                    -1isize as *mut c_void,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
    }

    pub fn start_hook_thread() {
        std::thread::Builder::new()
            .name("nidong-hotkey".into())
            .spawn(|| unsafe {
                let hmod = GetModuleHandleW(std::ptr::null());
                let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_keyboard), hmod, 0);
                if hook.is_null() {
                    return;
                }
                let mut msg = std::mem::zeroed::<Msg>();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                let _ = UnhookWindowsHookEx(hook);
            })
            .ok();
    }

    pub fn start_fullscreen_thread() {
        std::thread::Builder::new()
            .name("nidong-fs".into())
            .spawn(|| {
                let mut last = false;
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(800));
                    let mut state: i32 = 0;
                    let ok = unsafe { SHQueryUserNotificationState(&mut state) } >= 0;
                    let on = ok && state == QUNS_RUNNING_D3D_FULL_SCREEN;
                    if on != last {
                        last = on;
                        emit_exclusive_fullscreen(on);
                    }
                }
            })
            .ok();
    }

    pub fn start_hover_thread() {
        std::thread::Builder::new()
            .name("nidong-hover".into())
            .spawn(|| loop {
                std::thread::sleep(std::time::Duration::from_millis(40));
                if !HOVER_ON.load(Ordering::Relaxed) {
                    if HOVER_INSIDE.swap(false, Ordering::SeqCst) {
                        emit_hover(false);
                    }
                    continue;
                }
                let mut pt = Point { x: 0, y: 0 };
                let ok = unsafe { GetCursorPos(&mut pt) } != 0;
                let x = HOVER_X.load(Ordering::Relaxed);
                let y = HOVER_Y.load(Ordering::Relaxed);
                let w = HOVER_W.load(Ordering::Relaxed);
                let h = HOVER_H.load(Ordering::Relaxed);
                let inside = ok && w > 0 && h > 0 && pt.x >= x && pt.x < x + w && pt.y >= y && pt.y < y + h;
                let was = HOVER_INSIDE.swap(inside, Ordering::SeqCst);
                if inside != was {
                    emit_hover(inside);
                }
            })
            .ok();
    }

    unsafe extern "system" fn ll_keyboard(code: i32, wparam: usize, lparam: isize) -> isize {
        if code >= 0 && lparam != 0 && ENABLED.load(Ordering::Relaxed) {
            let info = &*(lparam as *const KbdLl);
            let vk = info.vk_code;
            let is_up = wparam == WM_KEYUP
                || wparam == WM_SYSKEYUP
                || info.flags & LLKHF_UP != 0;
            let is_down = !is_up
                && (wparam == WM_KEYDOWN || wparam == WM_SYSKEYDOWN);
            if is_down || is_up {
                note_key(vk, is_down);
                if is_down && combo_chord() {
                    if !FIRED.swap(true, Ordering::SeqCst) {
                        fire_hotkey(super::HotkeyEvt::Down);
                    }
                } else if is_up && FIRED.load(Ordering::SeqCst) && !combo_down() {
                    FIRED.store(false, Ordering::SeqCst);
                    fire_hotkey(super::HotkeyEvt::Up);
                }
            }
        }
        CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
    }
}
