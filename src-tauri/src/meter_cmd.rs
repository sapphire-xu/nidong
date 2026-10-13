//! 网费页开关本机流量记录。计数在提权的 nidong-meter.exe 里，泥洞本身不提权。

use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

fn find_exe() -> Result<PathBuf, String> {
    if let Ok(cur) = std::env::current_exe() {
        if let Some(dir) = cur.parent() {
            let beside = dir.join("nidong-meter.exe");
            if beside.is_file() {
                return Ok(beside);
            }
        }
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for rel in ["target/debug/nidong-meter.exe", "target/release/nidong-meter.exe"] {
        let path = manifest.join(rel);
        if path.is_file() {
            return Ok(path);
        }
    }
    Err("流量记录程序不在安装目录里。".into())
}

fn launch(exe: &std::path::Path, args: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::{SEE_MASK_NO_CONSOLE, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let exe_w: Vec<u16> = exe.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let args_w: Vec<u16> = args.encode_utf16().chain(std::iter::once(0)).collect();
    let verb: Vec<u16> = "runas".encode_utf16().chain(std::iter::once(0)).collect();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NO_CONSOLE,
        hwnd: HWND::default(),
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(exe_w.as_ptr()),
        lpParameters: PCWSTR(args_w.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    let ok = unsafe { ShellExecuteExW(&mut info) }.is_ok();
    if ok {
        Ok(())
    } else {
        Err("没有允许管理员权限，流量记录没有打开。".into())
    }
}

fn enabled_now() -> bool {
    fs::read_to_string(nidong_meter::meter_dir().join("enabled.txt"))
        .map(|text| text.trim() == "on")
        .unwrap_or(false)
}

fn error_now() -> String {
    fs::read_to_string(nidong_meter::meter_dir().join("error.txt"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn wait_enabled(want: bool) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if enabled_now() == want {
            return Ok(());
        }
        let err = error_now();
        if !err.is_empty() {
            return Err(err);
        }
        thread::sleep(Duration::from_millis(200));
    }
    if want {
        Err("已经请求管理员权限，但记录没有开始。可以再试一次。".into())
    } else {
        Err("没有关掉流量记录。可以再试一次。".into())
    }
}

#[tauri::command]
pub fn meter_status() -> nidong_meter::Status {
    nidong_meter::status()
}

#[tauri::command]
pub fn meter_log() -> Vec<nidong_meter::Day> {
    nidong_meter::log_days()
}

#[tauri::command]
pub fn meter_enable() -> Result<(), String> {
    let exe = find_exe()?;
    let _ = fs::remove_file(nidong_meter::meter_dir().join("error.txt"));
    launch(&exe, "--enable")?;
    wait_enabled(true)
}

#[tauri::command]
pub fn meter_disable() -> Result<(), String> {
    let exe = find_exe()?;
    let _ = fs::remove_file(nidong_meter::meter_dir().join("error.txt"));
    launch(&exe, "--disable")?;
    wait_enabled(false)
}

#[tauri::command]
pub fn meter_open_log() -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let path = nidong_meter::ensure_log_file();
    let path_w: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let rc = unsafe {
        ShellExecuteW(
            None,
            PCWSTR::null(),
            PCWSTR(path_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if (rc.0 as isize) > 32 {
        Ok(())
    } else {
        Err("没有打开日志文件。".into())
    }
}
