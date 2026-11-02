//! WARP、Clash 之类虚拟网卡的本地地址。
//! 这些网卡上的连接是隧道内层，外层已经按物理网卡记过一次；学校网关也只看得到外层。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{OnceLock, RwLock};

use windows::Win32::Foundation::ERROR_BUFFER_OVERFLOW;
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH, GAA_FLAG_INCLUDE_PREFIX, GAA_FLAG_SKIP_ANYCAST,
    GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
};
use windows::Win32::Networking::WinSock::{SOCKET_ADDRESS, AF_UNSPEC};

const VIRTUAL_NAMES: &[&str] = &[
    "warp",
    "cloudflare",
    "clash",
    "mihomo",
    "wintun",
    "wireguard",
    "sing-box",
];

#[derive(Clone, Debug, Default)]
pub struct VirtualAddrs {
    v4: Vec<[u8; 4]>,
    v6: Vec<[u8; 16]>,
}

impl VirtualAddrs {
    pub fn is_v4(&self, addr: &[u8; 4]) -> bool {
        self.v4.iter().any(|a| a == addr)
    }

    pub fn is_v6(&self, addr: &[u8; 16]) -> bool {
        self.v6.iter().any(|a| a == addr)
    }

    #[cfg(test)]
    pub(crate) fn from_parts(v4: Vec<[u8; 4]>, v6: Vec<[u8; 16]>) -> Self {
        Self { v4, v6 }
    }
}

static ADDRS: OnceLock<RwLock<VirtualAddrs>> = OnceLock::new();

fn store() -> &'static RwLock<VirtualAddrs> {
    ADDRS.get_or_init(|| RwLock::new(VirtualAddrs::default()))
}

pub fn with<R>(f: impl FnOnce(&VirtualAddrs) -> R) -> R {
    let guard = store().read().unwrap_or_else(|err| err.into_inner());
    f(&guard)
}

pub fn snapshot() -> VirtualAddrs {
    let guard = store().read().unwrap_or_else(|err| err.into_inner());
    guard.clone()
}

pub fn refresh() {
    let next = scan();
    let mut guard = store().write().unwrap_or_else(|err| err.into_inner());
    *guard = next;
}

fn scan() -> VirtualAddrs {
    unsafe { scan_unsafe().unwrap_or_default() }
}

unsafe fn scan_unsafe() -> Option<VirtualAddrs> {
    let flags = GAA_FLAG_INCLUDE_PREFIX
        | GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_DNS_SERVER
        | GAA_FLAG_SKIP_MULTICAST;
    let family = AF_UNSPEC.0 as u32;
    let mut size = 0u32;
    let rc = unsafe { GetAdaptersAddresses(family, flags, None, None, &mut size) };
    if rc != ERROR_BUFFER_OVERFLOW.0 {
        return None;
    }
    for _ in 0..4 {
        let mut buf = vec![0u64; (size as usize + 7) / 8];
        let rc = unsafe {
            GetAdaptersAddresses(
                family,
                flags,
                None,
                Some(buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut size,
            )
        };
        if rc == ERROR_BUFFER_OVERFLOW.0 {
            continue;
        }
        if rc != 0 {
            return None;
        }
        return Some(unsafe { collect(buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH) });
    }
    None
}

unsafe fn collect(mut current: *const IP_ADAPTER_ADDRESSES_LH) -> VirtualAddrs {
    let mut out = VirtualAddrs::default();
    let mut guard = 0;
    while !current.is_null() && guard < 256 {
        let adapter = unsafe { &*current };
        let name = unsafe { wide(adapter.FriendlyName) };
        let desc = unsafe { wide(adapter.Description) };
        if is_virtual_name(&name) || is_virtual_name(&desc) {
            let mut unicast = adapter.FirstUnicastAddress;
            let mut n = 0;
            while !unicast.is_null() && n < 64 {
                let entry = unsafe { &*unicast };
                if let Some(addr) = parse_sockaddr(&entry.Address) {
                    match addr {
                        IpAddr::V4(v) => out.v4.push(v.octets()),
                        IpAddr::V6(v) => out.v6.push(v.octets()),
                    }
                }
                unicast = entry.Next;
                n += 1;
            }
        }
        current = adapter.Next;
        guard += 1;
    }
    out.v4.sort_unstable();
    out.v4.dedup();
    out.v6.sort_unstable();
    out.v6.dedup();
    out
}

unsafe fn wide(ptr: windows::core::PWSTR) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { ptr.to_string() }.unwrap_or_default()
}

fn parse_sockaddr(addr: &SOCKET_ADDRESS) -> Option<IpAddr> {
    if addr.lpSockaddr.is_null() || addr.iSockaddrLength < 2 {
        return None;
    }
    let raw = unsafe {
        std::slice::from_raw_parts(addr.lpSockaddr as *const u8, addr.iSockaddrLength as usize)
    };
    let family = u16::from_ne_bytes([raw[0], raw[1]]);
    match family {
        2 if raw.len() >= 8 => {
            let mut octets = [0u8; 4];
            octets.copy_from_slice(&raw[4..8]);
            Some(IpAddr::V4(Ipv4Addr::from(octets)))
        }
        23 if raw.len() >= 24 => {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&raw[8..24]);
            Some(IpAddr::V6(Ipv6Addr::from(octets)))
        }
        _ => None,
    }
}

fn is_virtual_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    VIRTUAL_NAMES.iter().any(|key| lower.contains(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_names_cover_warp_and_clash() {
        for name in [
            "Cloudflare WARP",
            "WARP",
            "Clash",
            "Mihomo",
            "Wintun Userspace Tunnel",
            "WireGuard Tunnel",
        ] {
            assert!(is_virtual_name(name), "{name}");
        }
        for name in [
            "MediaTek Wi-Fi 7 MT7925 Wireless LAN Card",
            "Realtek Gaming 2.5GbE Family Controller",
            "以太网",
            "",
        ] {
            assert!(!is_virtual_name(name), "{name}");
        }
    }

    #[test]
    fn addresses_match_exactly() {
        let addrs = VirtualAddrs::from_parts(vec![[172, 16, 0, 2]], vec![]);
        assert!(addrs.is_v4(&[172, 16, 0, 2]));
        assert!(!addrs.is_v4(&[172, 16, 0, 3]));
        assert!(!addrs.is_v6(&[0u8; 16]));
    }
}
