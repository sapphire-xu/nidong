//! Windows WLAN via wlanapi. 校园网连接不走 netsh，避免无控制台进程拉起 netsh 失败。

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

#[cfg(not(windows))]
pub fn query() -> Result<WlanNow, String> {
    Err("校园网自动连接只在 Windows 上可用".into())
}

#[cfg(not(windows))]
pub fn connect_profile(_profile: &str) -> Result<(), String> {
    Err("校园网自动连接只在 Windows 上可用".into())
}

#[cfg(windows)]
pub fn disconnect() -> Result<(), String> {
    unsafe { disconnect_inner() }
}

#[cfg(not(windows))]
pub fn disconnect() -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
unsafe fn query_inner() -> Result<WlanNow, String> {
    use windows::Win32::NetworkManagement::WiFi::{
        WlanCloseHandle, WlanFreeMemory, WlanQueryInterface, WLAN_CONNECTION_ATTRIBUTES,
        WLAN_INTERFACE_INFO, wlan_interface_state_connected, wlan_intf_opcode_current_connection,
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
    let mut chosen: *const WLAN_INTERFACE_INFO = base;
    for i in 0..n {
        let info = unsafe { &*base.add(i) };
        if info.isState == wlan_interface_state_connected {
            chosen = info;
            break;
        }
    }
    let info = unsafe { &*chosen };
    let connected = info.isState == wlan_interface_state_connected;
    let guid = info.InterfaceGuid;
    let mut ssid = String::new();
    if connected {
        let mut size = 0u32;
        let mut data = std::ptr::null_mut();
        let rc = unsafe {
            WlanQueryInterface(
                handle,
                &guid,
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
                ssid = ssid_text(&conn.wlanAssociationAttributes.dot11Ssid);
            }
        }
        if !data.is_null() {
            unsafe { WlanFreeMemory(data.cast()); }
        }
    }
    unsafe { WlanFreeMemory(list.cast()); }
    let _ = unsafe { WlanCloseHandle(handle, None) };
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

#[cfg(windows)]
fn ssid_text(ssid: &windows::Win32::NetworkManagement::WiFi::DOT11_SSID) -> String {
    let n = (ssid.uSSIDLength as usize).min(ssid.ucSSID.len());
    String::from_utf8_lossy(&ssid.ucSSID[..n]).trim_matches('\u{0}').trim().to_string()
}
