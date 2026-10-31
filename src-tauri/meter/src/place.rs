//! 在不在校园网。无线名和校园网自动登录用同一组；有线宿舍靠 172.22 地址。
//! 不拿「能连上教务」判断，避免人在校外时把家里的流量记进来。

use std::fs;
use std::net::IpAddr;

use serde::Deserialize;

const CAMPUS_SSIDS: &[&str] = &["NEU", "NEU-2.4G", "NEU-Mobile"];

#[derive(Deserialize)]
struct SsidSetting {
    #[serde(default, rename = "campusWifiSsid")]
    campus_wifi_ssid: String,
}

pub fn currently_on_campus() -> (bool, String, String) {
    let addrs = local_addrs();
    let (v4, v6) = pick_addrs(&addrs);
    let ssid = connected_ssid();
    let configured = configured_ssid();
    let on = on_campus_net(&ssid, &configured, &addrs);
    (on, v4, v6)
}

pub fn on_campus_net(ssid: &str, configured: &str, addrs: &[IpAddr]) -> bool {
    addrs.iter().any(is_campus_addr) || is_campus_ssid(ssid, configured)
}

fn is_campus_ssid(name: &str, configured: &str) -> bool {
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    if !configured.trim().is_empty() && name.eq_ignore_ascii_case(configured.trim()) {
        return true;
    }
    CAMPUS_SSIDS.iter().any(|s| name.eq_ignore_ascii_case(s))
}

fn is_campus_addr(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(v) => v4_is_campus(&v.octets()),
        IpAddr::V6(v) => v6_is_campus(&v.octets()),
    }
}

pub(crate) fn v4_is_campus(o: &[u8; 4]) -> bool {
    o[0] == 172 && o[1] == 22
}

pub(crate) fn v6_is_campus(o: &[u8; 16]) -> bool {
    o[0] == 0x20 && o[1] == 0x01 && o[2] == 0x0d && o[3] == 0xa8 && o[4] == 0x90 && o[5] == 0x00
}

fn local_addrs() -> Vec<IpAddr> {
    if_addrs::get_if_addrs()
        .map(|list| list.into_iter().map(|iface| iface.ip()).collect())
        .unwrap_or_default()
}

fn pick_addrs(addrs: &[IpAddr]) -> (String, String) {
    let v4 = addrs
        .iter()
        .find_map(|a| match a {
            IpAddr::V4(v) if is_campus_addr(&IpAddr::V4(*v)) => Some(v.to_string()),
            _ => None,
        })
        .or_else(|| {
            addrs.iter().find_map(|a| match a {
                IpAddr::V4(v) if !v.is_loopback() && !v.is_link_local() && !v.is_unspecified() => Some(v.to_string()),
                _ => None,
            })
        })
        .unwrap_or_default();
    let v6 = addrs
        .iter()
        .find_map(|a| match a {
            IpAddr::V6(v) if is_campus_addr(&IpAddr::V6(*v)) => Some(v.to_string()),
            _ => None,
        })
        .or_else(|| {
            addrs.iter().find_map(|a| match a {
                IpAddr::V6(v) if is_global_v6(v) => Some(v.to_string()),
                _ => None,
            })
        })
        .unwrap_or_default();
    (v4, v6)
}

fn is_global_v6(v: &std::net::Ipv6Addr) -> bool {
    let o = v.octets();
    !(o == [0u8; 16] || o[0] == 0xff || (o[0] == 0xfe && (o[1] & 0xc0) == 0x80) || v.is_loopback())
}

fn configured_ssid() -> String {
    let path = crate::store::app_dir().join("settings.json");
    let Ok(text) = fs::read_to_string(path) else {
        return String::new();
    };
    serde_json::from_str::<SsidSetting>(&text)
        .map(|s| s.campus_wifi_ssid)
        .unwrap_or_default()
}

pub fn connected_ssid() -> String {
    current_ssid().unwrap_or_default()
}

fn current_ssid() -> Result<String, String> {
    // GetConnectedSsid 只返回无线名。WlanQueryInterface(current_connection) 带接入点 BSSID，
    // Windows 会把它记成正在使用位置。
    ensure_winrt();
    read_connected_ssid().map_err(|err| err.to_string())
}

fn ensure_winrt() {
    unsafe {
        let _ = windows::Win32::System::WinRT::RoInitialize(
            windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
        );
    }
}

fn read_connected_ssid() -> windows::core::Result<String> {
    use windows::Networking::Connectivity::{NetworkConnectivityLevel, NetworkInformation};

    let profiles = NetworkInformation::GetConnectionProfiles()?;
    let count = profiles.Size()?;
    for i in 0..count {
        let profile = profiles.GetAt(i)?;
        let level = profile
            .GetNetworkConnectivityLevel()
            .unwrap_or(NetworkConnectivityLevel::None);
        if level == NetworkConnectivityLevel::None {
            continue;
        }
        if !profile.IsWlanConnectionProfile().unwrap_or(false) {
            continue;
        }
        let Ok(details) = profile.WlanConnectionProfileDetails() else {
            continue;
        };
        let text = details.GetConnectedSsid()?.to_string();
        let text = text.trim().trim_matches('\u{0}').to_string();
        if !text.is_empty() {
            return Ok(text);
        }
    }
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn campus_address_or_the_same_wifi_names() {
        let v4 = IpAddr::V4(Ipv4Addr::new(172, 22, 1, 8));
        let outside = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 9));
        let v6 = IpAddr::V6(Ipv6Addr::new(0x2001, 0x0da8, 0x9000, 0xa835, 0, 0, 0, 1));
        assert!(on_campus_net("", "", &[v4]));
        assert!(on_campus_net("", "", &[v6]));
        assert!(on_campus_net("NEU", "", &[outside]));
        assert!(on_campus_net("neu-mobile", "", &[outside]));
        assert!(on_campus_net("NEU-2.4G", "", &[]));
        assert!(on_campus_net("宿舍热点", "宿舍热点", &[outside]));
        assert!(!on_campus_net("NEUQ", "", &[outside]));
        assert!(!on_campus_net("CMCC", "", &[outside]));
        assert!(!on_campus_net("", "", &[outside]));
    }

    #[test]
    fn connected_ssid_is_a_name_not_a_scan() {
        let name = connected_ssid();
        assert!(name.len() <= 32);
    }
}
