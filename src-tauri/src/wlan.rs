//! WLAN 查询与连接：
//! - Windows 使用 wlanapi，避免无控制台进程拉起 netsh 失败；当前 SSID 用 WinRT GetConnectedSsid 避免位置图标。
//! - Linux 使用 NetworkManager (`nmcli`)，不可用时回退 `iwgetid` / `iw` 只读查询。

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WlanNow {
    pub connected: bool,
    pub ssid: String,
    pub has_adapter: bool,
}

#[cfg(windows)]
pub fn query() -> Result<WlanNow, String> {
    unsafe { query_inner() }
}

#[cfg(windows)]
pub fn connect_profile(profile: &str) -> Result<(), String> {
    let name = profile.trim();
    if name.is_empty() {
        return Err("校园网名称是空的".into());
    }
    unsafe { connect_inner(name) }
}

#[cfg(windows)]
pub fn disconnect() -> Result<(), String> {
    unsafe { disconnect_inner() }
}

#[cfg(target_os = "linux")]
pub fn query() -> Result<WlanNow, String> {
    linux::query()
}

#[cfg(target_os = "linux")]
pub fn connect_profile(profile: &str) -> Result<(), String> {
    let name = profile.trim();
    if name.is_empty() {
        return Err("校园网名称是空的".into());
    }
    linux::connect_profile(name)
}

#[cfg(target_os = "linux")]
pub fn disconnect() -> Result<(), String> {
    linux::disconnect()
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn query() -> Result<WlanNow, String> {
    Err("校园网自动连接当前仅支持 Windows 和 Linux".into())
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn connect_profile(_profile: &str) -> Result<(), String> {
    Err("校园网自动连接当前仅支持 Windows 和 Linux".into())
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn disconnect() -> Result<(), String> {
    Ok(())
}

/// 按 `nmcli -t` 的转义规则切分字段（`\:` 表示字面冒号，`\\` 表示反斜杠）。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn split_nmcli_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                cur.push(next);
            } else {
                cur.push('\\');
            }
        } else if c == ':' {
            fields.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    fields.push(cur);
    fields
}

/// 解析 `LC_ALL=C nmcli -t -f DEVICE,TYPE,STATE,CONNECTION dev status`。
/// 返回 `(has_wifi_adapter, Option<(device, connection_name)>)`。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_nmcli_dev_status(output: &str) -> (bool, Option<(String, String)>) {
    let mut has_adapter = false;
    let mut active = None;
    for raw in output.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let fields = split_nmcli_fields(line);
        if fields.len() < 3 {
            continue;
        }
        if !fields[1].eq_ignore_ascii_case("wifi") {
            continue;
        }
        has_adapter = true;
        let state = fields[2].to_ascii_lowercase();
        if state == "connected" || state.starts_with("connected ") {
            let conn = fields.get(3).cloned().unwrap_or_default().trim().to_string();
            active = Some((fields[0].trim().to_string(), conn));
            break;
        }
    }
    (has_adapter, active)
}

/// 解析 `LC_ALL=C nmcli -t -f NAME,TYPE connection show`，按名称匹配已保存无线配置。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn match_saved_wifi_connection(output: &str, wanted: &str) -> Option<String> {
    let wanted = wanted.trim();
    if wanted.is_empty() {
        return None;
    }
    let mut fallback = None;
    for raw in output.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let fields = split_nmcli_fields(line);
        if fields.len() < 2 {
            continue;
        }
        let kind = fields[1].trim();
        if !kind.eq_ignore_ascii_case("802-11-wireless") && !kind.eq_ignore_ascii_case("wifi") {
            continue;
        }
        let name = fields[0].trim();
        if name == wanted {
            return Some(name.to_string());
        }
        if fallback.is_none() && name.eq_ignore_ascii_case(wanted) {
            fallback = Some(name.to_string());
        }
    }
    fallback
}

/// 解析 `iw dev <iface> link` 输出里的 `SSID:` 行。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_iw_link_ssid(output: &str) -> Option<String> {
    for raw in output.lines() {
        let trimmed = raw.trim();
        if let Some(rest) = trimmed.strip_prefix("SSID:") {
            let ssid = rest.trim();
            if !ssid.is_empty() {
                return Some(ssid.to_string());
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    fn run_cmd(prog: &str, args: &[&str]) -> Option<std::process::Output> {
        Command::new(prog)
            .args(args)
            .env("LC_ALL", "C")
            .output()
            .ok()
    }

    fn nmcli_connection_ssid(conn: &str) -> Option<String> {
        if conn.trim().is_empty() {
            return None;
        }
        let out = run_cmd("nmcli", &["-g", "802-11-wireless.ssid", "connection", "show", conn])?;
        if !out.status.success() {
            return None;
        }
        let ssid = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if ssid.is_empty() { None } else { Some(ssid) }
    }

    fn iwgetid_ssid() -> Option<String> {
        let out = run_cmd("iwgetid", &["-r"])?;
        if !out.status.success() {
            return None;
        }
        let ssid = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if ssid.is_empty() { None } else { Some(ssid) }
    }

    fn sysfs_wifi_ifaces() -> Vec<String> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir("/sys/class/net") else {
            return out;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.join("wireless").is_dir() || p.join("phy80211").exists() {
                if let Some(name) = entry.file_name().to_str() {
                    out.push(name.to_string());
                }
            }
        }
        out
    }

    fn iw_ssid(iface: &str) -> Option<String> {
        let out = run_cmd("iw", &["dev", iface, "link"])?;
        if !out.status.success() {
            return None;
        }
        parse_iw_link_ssid(&String::from_utf8_lossy(&out.stdout))
    }

    pub fn query() -> Result<WlanNow, String> {
        if let Some(out) = run_cmd("nmcli", &["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "dev", "status"]) {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                let (has_adapter, active) = parse_nmcli_dev_status(&text);
                if let Some((dev, conn)) = active {
                    let ssid = nmcli_connection_ssid(&conn)
                        .or_else(iwgetid_ssid)
                        .or_else(|| iw_ssid(&dev))
                        .unwrap_or(conn);
                    return Ok(WlanNow {
                        connected: true,
                        ssid,
                        has_adapter: true,
                    });
                }
                return Ok(WlanNow {
                    connected: false,
                    ssid: String::new(),
                    has_adapter,
                });
            }
        }

        let ifaces = sysfs_wifi_ifaces();
        if ifaces.is_empty() {
            return Ok(WlanNow {
                connected: false,
                ssid: String::new(),
                has_adapter: false,
            });
        }
        let ssid = iwgetid_ssid()
            .or_else(|| ifaces.iter().find_map(|i| iw_ssid(i)))
            .unwrap_or_default();
        Ok(WlanNow {
            connected: !ssid.is_empty(),
            ssid,
            has_adapter: true,
        })
    }

    fn find_saved_profile(wanted: &str) -> Option<String> {
        let out = run_cmd("nmcli", &["-t", "-f", "NAME,TYPE", "connection", "show"])?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        if let Some(name) = match_saved_wifi_connection(&text, wanted) {
            return Some(name);
        }
        // 连接名与 SSID 不同时，再按 802-11-wireless.ssid 匹配已保存的无线配置。
        for raw in text.lines() {
            let fields = split_nmcli_fields(raw.trim());
            if fields.len() < 2 {
                continue;
            }
            let kind = fields[1].trim();
            if !kind.eq_ignore_ascii_case("802-11-wireless") && !kind.eq_ignore_ascii_case("wifi") {
                continue;
            }
            let name = fields[0].trim();
            if let Some(ssid) = nmcli_connection_ssid(name) {
                if ssid.eq_ignore_ascii_case(wanted) {
                    return Some(name.to_string());
                }
            }
        }
        None
    }

    pub fn connect_profile(name: &str) -> Result<(), String> {
        let current = query()?;
        if !current.has_adapter {
            return Err("没有无线网卡".into());
        }
        let Some(profile) = find_saved_profile(name) else {
            if !Path::new("/usr/bin/nmcli").exists() && run_cmd("nmcli", &["--version"]).is_none() {
                return Err("系统缺少 NetworkManager (nmcli)，无法自动切换无线".into());
            }
            return Err(format!("系统里没有名为「{name}」的无线配置"));
        };
        let out = run_cmd("nmcli", &["connection", "up", "id", &profile])
            .ok_or_else(|| "无法调用 nmcli 连接无线".to_string())?;
        if out.status.success() {
            return Ok(());
        }
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if err.to_ascii_lowercase().contains("unknown connection") {
            Err(format!("系统里没有名为「{name}」的无线配置"))
        } else if err.is_empty() {
            Err("校园网没连上".into())
        } else {
            Err(format!("校园网没连上（{err}）"))
        }
    }

    pub fn disconnect() -> Result<(), String> {
        let Some(out) = run_cmd("nmcli", &["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "dev", "status"]) else {
            return Ok(());
        };
        if !out.status.success() {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let (_, active) = parse_nmcli_dev_status(&text);
        let Some((dev, _)) = active else {
            return Ok(());
        };
        let res = run_cmd("nmcli", &["device", "disconnect", &dev])
            .ok_or_else(|| "断开无线失败".to_string())?;
        if res.status.success() {
            Ok(())
        } else {
            Err(format!(
                "断开失败（{}）",
                String::from_utf8_lossy(&res.stderr).trim()
            ))
        }
    }
}

#[cfg(windows)]
unsafe fn query_inner() -> Result<WlanNow, String> {
    use windows::Win32::NetworkManagement::WiFi::{
        WlanCloseHandle, WlanFreeMemory, wlan_interface_state_connected,
    };

    let handle = open_handle()?;
    let list = enum_interfaces(handle)?;
    let n = unsafe { (*list).dwNumberOfItems as usize };
    if n == 0 {
        unsafe { WlanFreeMemory(list.cast()); }
        let _ = unsafe { WlanCloseHandle(handle, None) };
        return Ok(WlanNow { connected: false, ssid: String::new(), has_adapter: false });
    }
    let base = unsafe { std::ptr::addr_of!((*list).InterfaceInfo[0]) };
    let mut connected = false;
    for i in 0..n {
        let info = unsafe { &*base.add(i) };
        if info.isState == wlan_interface_state_connected {
            connected = true;
            break;
        }
    }
    unsafe { WlanFreeMemory(list.cast()); }
    let _ = unsafe { WlanCloseHandle(handle, None) };
    let ssid = if connected { nidong_meter::connected_ssid() } else { String::new() };
    Ok(WlanNow { connected, ssid, has_adapter: true })
}

#[cfg(windows)]
unsafe fn connect_inner(profile: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use std::ffi::OsStr;
    use windows::core::PCWSTR;
    use windows::Win32::NetworkManagement::WiFi::{
        WlanCloseHandle, WlanConnect, WlanFreeMemory, WLAN_CONNECTION_PARAMETERS,
        WLAN_INTERFACE_INFO, dot11_BSS_type_infrastructure, wlan_connection_mode_profile,
        wlan_interface_state_connected,
    };

    let handle = open_handle()?;
    let list = enum_interfaces(handle)?;
    let n = unsafe { (*list).dwNumberOfItems as usize };
    if n == 0 {
        unsafe { WlanFreeMemory(list.cast()); }
        let _ = unsafe { WlanCloseHandle(handle, None) };
        return Err("没有无线网卡".into());
    }
    let base = unsafe { std::ptr::addr_of!((*list).InterfaceInfo[0]) };
    let mut chosen: *const WLAN_INTERFACE_INFO = base;
    for i in 0..n {
        let info = unsafe { &*base.add(i) };
        if info.isState == wlan_interface_state_connected {
            chosen = info;
            break;
        }
    }
    let guid = unsafe { (*chosen).InterfaceGuid };
    let wide: Vec<u16> = OsStr::new(profile).encode_wide().chain(std::iter::once(0)).collect();
    let params = WLAN_CONNECTION_PARAMETERS {
        wlanConnectionMode: wlan_connection_mode_profile,
        strProfile: PCWSTR(wide.as_ptr()),
        pDot11Ssid: std::ptr::null_mut(),
        pDesiredBssidList: std::ptr::null_mut(),
        dot11BssType: dot11_BSS_type_infrastructure,
        dwFlags: 0,
    };
    let rc = unsafe { WlanConnect(handle, &guid, &params, None) };
    unsafe { WlanFreeMemory(list.cast()); }
    let _ = unsafe { WlanCloseHandle(handle, None) };
    if rc == 0 {
        Ok(())
    } else if rc == 1168 || rc == 2 {
        Err(format!("系统里没有名为「{profile}」的无线配置"))
    } else {
        Err(format!("校园网没连上（{rc}）"))
    }
}

#[cfg(windows)]
unsafe fn disconnect_inner() -> Result<(), String> {
    use windows::Win32::NetworkManagement::WiFi::{
        WlanCloseHandle, WlanDisconnect, WlanFreeMemory, wlan_interface_state_connected,
    };

    let handle = open_handle()?;
    let list = enum_interfaces(handle)?;
    let n = unsafe { (*list).dwNumberOfItems as usize };
    let base = unsafe { std::ptr::addr_of!((*list).InterfaceInfo[0]) };
    let mut rc_out = 0u32;
    for i in 0..n {
        let info = unsafe { &*base.add(i) };
        if info.isState == wlan_interface_state_connected {
            rc_out = unsafe { WlanDisconnect(handle, &info.InterfaceGuid, None) };
        }
    }
    unsafe { WlanFreeMemory(list.cast()); }
    let _ = unsafe { WlanCloseHandle(handle, None) };
    if rc_out == 0 {
        Ok(())
    } else {
        Err(format!("断开失败（{rc_out}）"))
    }
}

#[cfg(windows)]
unsafe fn open_handle() -> Result<windows::Win32::Foundation::HANDLE, String> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::NetworkManagement::WiFi::WlanOpenHandle;
    let mut negotiated = 0u32;
    let mut handle = HANDLE::default();
    let rc = unsafe { WlanOpenHandle(2, None, &mut negotiated, &mut handle) };
    if rc != 0 {
        return Err("没有无线网卡".into());
    }
    Ok(handle)
}

#[cfg(windows)]
unsafe fn enum_interfaces(handle: windows::Win32::Foundation::HANDLE) -> Result<*mut windows::Win32::NetworkManagement::WiFi::WLAN_INTERFACE_INFO_LIST, String> {
    use windows::Win32::NetworkManagement::WiFi::{WlanCloseHandle, WlanEnumInterfaces, WLAN_INTERFACE_INFO_LIST};
    let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
    let rc = unsafe { WlanEnumInterfaces(handle, None, &mut list) };
    if rc != 0 || list.is_null() {
        let _ = unsafe { WlanCloseHandle(handle, None) };
        return Err("没有无线网卡".into());
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nmcli_dev_status_and_escaped_colons() {
        let out = "enp3s0:ethernet:unavailable:--\nwlp2s0:wifi:connected:My\\:Campus\\:WiFi\nlo:loopback:unmanaged:--\n";
        let (has_adapter, active) = parse_nmcli_dev_status(out);
        assert!(has_adapter);
        assert_eq!(active, Some(("wlp2s0".into(), "My:Campus:WiFi".into())));

        let disconnected = "wlp2s0:wifi:disconnected:--\n";
        let (has_adapter2, active2) = parse_nmcli_dev_status(disconnected);
        assert!(has_adapter2);
        assert_eq!(active2, None);

        let no_wifi = "enp3s0:ethernet:connected:Wired\n";
        assert_eq!(parse_nmcli_dev_status(no_wifi), (false, None));
    }

    #[test]
    fn matches_saved_wifi_connection_case_insensitively() {
        let out = "Wired:802-3-ethernet\nNEU-Mobile:802-11-wireless\nNEU:802-11-wireless\n";
        assert_eq!(match_saved_wifi_connection(out, "NEU").as_deref(), Some("NEU"));
        assert_eq!(match_saved_wifi_connection(out, "neu-mobile").as_deref(), Some("NEU-Mobile"));
        assert_eq!(match_saved_wifi_connection(out, "Wired"), None);
    }

    #[test]
    fn parses_iw_link_ssid() {
        let out = "Connected to aa:bb:cc:dd:ee:ff (on wlp2s0)\n\tSSID: NEU-Mobile\n\tfreq: 5180\n";
        assert_eq!(parse_iw_link_ssid(out).as_deref(), Some("NEU-Mobile"));
        assert_eq!(parse_iw_link_ssid("Not connected.\n"), None);
    }
}
