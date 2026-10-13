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
    let ssid = current_ssid().unwrap_or_default();
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
        IpAddr::V4(v) => {
            let o = v.octets();
            o[0] == 172 && o[1] == 22
        }
        IpAddr::V6(v) => {
            let o = v.octets();
            o[0] == 0x20 && o[1] == 0x01 && o[2] == 0x0d && o[3] == 0xa8 && o[4] == 0x90 && o[5] == 0x00
        }
    }
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

fn current_ssid() -> Result<String, String> {
    unsafe { query_ssid() }
}

unsafe fn query_ssid() -> Result<String, String> {
    use windows::Win32::NetworkManagement::WiFi::{
        WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory, WlanOpenHandle, WlanQueryInterface,
        WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST, wlan_interface_state_connected,
        wlan_intf_opcode_current_connection,
    };

    let mut negotiated = 0u32;
    let mut handle = windows::Win32::Foundation::HANDLE::default();
    if unsafe { WlanOpenHandle(2, None, &mut negotiated, &mut handle) } != 0 {
        return Ok(String::new());
    }
    let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
    if unsafe { WlanEnumInterfaces(handle, None, &mut list) } != 0 || list.is_null() {
        let _ = unsafe { WlanCloseHandle(handle, None) };
        return Ok(String::new());
    }
    let n = unsafe { (*list).dwNumberOfItems as usize };
    let base = unsafe { std::ptr::addr_of!((*list).InterfaceInfo[0]) };
    let mut ssid = String::new();
    for i in 0..n {
        let info = unsafe { &*base.add(i) };
        if info.isState != wlan_interface_state_connected {
            continue;
        }
        let mut size = 0u32;
        let mut data = std::ptr::null_mut();
        let rc = unsafe {
            WlanQueryInterface(
                handle,
                &info.InterfaceGuid,
                wlan_intf_opcode_current_connection,
                None,
                &mut size,
                &mut data,
                None,
            )
        };
        if rc == 0 && !data.is_null() {
            let conn = unsafe { &*(data as *const WLAN_CONNECTION_ATTRIBUTES) };
            if conn.isState == wlan_interface_state_connected {
                let raw = &conn.wlanAssociationAttributes.dot11Ssid;
                let len = (raw.uSSIDLength as usize).min(raw.ucSSID.len());
                ssid = String::from_utf8_lossy(&raw.ucSSID[..len])
                    .trim_matches('\u{0}')
                    .trim()
                    .to_string();
            }
        }
        if !data.is_null() {
            unsafe { WlanFreeMemory(data.cast()) };
        }
        if !ssid.is_empty() {
            break;
        }
    }
    unsafe { WlanFreeMemory(list.cast()) };
    let _ = unsafe { WlanCloseHandle(handle, None) };
    Ok(ssid)
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
}
