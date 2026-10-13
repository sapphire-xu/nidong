//! 读 Windows 每条 TCP 连接的收发字节。UDP 没有按连接的字节计数，不记。

use std::collections::HashMap;

use crate::store::Totals;

const AF_INET: u32 = 2;
const AF_INET6: u32 = 23;
const TCP_TABLE_OWNER_PID_ALL: i32 = 5;
const MIB_TCP_STATE_LISTEN: u32 = 2;
const NO_ERROR: u32 = 0;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
const TCP_CONNECTION_ESTATS_DATA: i32 = 1;
const ESTATS_LIMIT: u64 = 16 << 40;
const MAX_STEP: u64 = 512 * 1024 * 1024;

#[repr(C)]
struct TcpRowOwnerPid {
    state: u32,
    local_addr: u32,
    local_port: u32,
    remote_addr: u32,
    remote_port: u32,
    pid: u32,
}

#[repr(C)]
struct TcpRow {
    state: u32,
    local_addr: u32,
    local_port: u32,
    remote_addr: u32,
    remote_port: u32,
}

#[repr(C)]
struct Tcp6RowOwnerPid {
    local: [u8; 16],
    local_scope: u32,
    local_port: u32,
    remote: [u8; 16],
    remote_scope: u32,
    remote_port: u32,
    state: u32,
    pid: u32,
}

#[repr(C)]
struct Tcp6Row {
    state: u32,
    local: [u8; 16],
    local_scope: u32,
    local_port: u32,
    remote: [u8; 16],
    remote_scope: u32,
    remote_port: u32,
}

#[repr(C)]
struct DataRw {
    enable: u8,
}

#[repr(C)]
struct DataRod {
    data_bytes_out: u64,
    data_segs_out: u64,
    data_bytes_in: u64,
    data_segs_in: u64,
    segs_out: u64,
    segs_in: u64,
    soft_errors: u32,
    soft_error_reason: u32,
    snd_una: u32,
    snd_nxt: u32,
    snd_max: u32,
    thru_bytes_acked: u64,
    rcv_nxt: u32,
    thru_bytes_received: u64,
}

#[link(name = "iphlpapi")]
extern "system" {
    fn GetExtendedTcpTable(
        table: *mut core::ffi::c_void,
        size: *mut u32,
        order: i32,
        family: u32,
        class: i32,
        reserved: u32,
    ) -> u32;
    fn SetPerTcpConnectionEStats(
        row: *const TcpRow,
        kind: i32,
        rw: *const u8,
        rw_version: u32,
        rw_size: u32,
        offset: u32,
    ) -> u32;
    fn GetPerTcpConnectionEStats(
        row: *const TcpRow,
        kind: i32,
        rw: *mut u8,
        rw_version: u32,
        rw_size: u32,
        ros: *mut u8,
        ros_version: u32,
        ros_size: u32,
        rod: *mut u8,
        rod_version: u32,
        rod_size: u32,
    ) -> u32;
    fn SetPerTcp6ConnectionEStats(
        row: *const Tcp6Row,
        kind: i32,
        rw: *const u8,
        rw_version: u32,
        rw_size: u32,
        offset: u32,
    ) -> u32;
    fn GetPerTcp6ConnectionEStats(
        row: *const Tcp6Row,
        kind: i32,
        rw: *mut u8,
        rw_version: u32,
        rw_size: u32,
        ros: *mut u8,
        ros_version: u32,
        ros_size: u32,
        rod: *mut u8,
        rod_version: u32,
        rod_size: u32,
    ) -> u32;
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    v6: bool,
    local: [u8; 16],
    local_port: u32,
    remote: [u8; 16],
    remote_port: u32,
}

#[derive(Clone, Copy)]
struct Base {
    bytes_in: u64,
    bytes_out: u64,
}

pub struct Sampler {
    seen: HashMap<Key, Base>,
}

#[derive(Clone, Debug, Default)]
pub struct Tick {
    pub totals: Totals,
    pub tcp: u32,
    pub bad: u32,
}

impl Sampler {
    pub fn new() -> Self {
        Self { seen: HashMap::new() }
    }

    pub fn poll(&mut self, night: bool) -> Tick {
        let mut tick = Tick::default();
        let mut still = HashMap::new();
        self.collect4(&mut tick, &mut still, night);
        self.collect6(&mut tick, &mut still);
        self.seen = still;
        tick
    }

    fn collect4(&mut self, tick: &mut Tick, still: &mut HashMap<Key, Base>, night: bool) {
        let Some(buf) = table(AF_INET) else {
            return;
        };
        if buf.len() < 4 {
            return;
        }
        let count = u32::from_ne_bytes(buf[0..4].try_into().unwrap()) as usize;
        let row_size = std::mem::size_of::<TcpRowOwnerPid>();
        for i in 0..count {
            let start = 4 + i * row_size;
            if start + row_size > buf.len() {
                break;
            }
            let row: TcpRowOwnerPid = unsafe { std::ptr::read_unaligned(buf[start..].as_ptr() as *const _) };
            if row.state == MIB_TCP_STATE_LISTEN {
                continue;
            }
            tick.tcp += 1;
            let query = TcpRow {
                state: row.state,
                local_addr: row.local_addr,
                local_port: row.local_port,
                remote_addr: row.remote_addr,
                remote_port: row.remote_port,
            };
            let Ok((inn, out)) = read4(&query) else {
                tick.bad += 1;
                continue;
            };
            let mut local = [0u8; 16];
            let mut remote = [0u8; 16];
            local[..4].copy_from_slice(&row.local_addr.to_ne_bytes());
            remote[..4].copy_from_slice(&row.remote_addr.to_ne_bytes());
            let key = Key {
                v6: false,
                local,
                local_port: row.local_port,
                remote,
                remote_port: row.remote_port,
            };
            let (din, dout) = delta_of(&self.seen, still, key, inn, out);
            let octets = row.remote_addr.to_ne_bytes();
            if !ipv4_is_local(octets) {
                if night {
                    tick.totals.v4_night += din;
                } else {
                    tick.totals.v4_day += din;
                }
            }
            tick.totals.v4_up += dout;
        }
    }

    fn collect6(&mut self, tick: &mut Tick, still: &mut HashMap<Key, Base>) {
        let Some(buf) = table(AF_INET6) else {
            return;
        };
        if buf.len() < 4 {
            return;
        }
        let count = u32::from_ne_bytes(buf[0..4].try_into().unwrap()) as usize;
        let row_size = std::mem::size_of::<Tcp6RowOwnerPid>();
        for i in 0..count {
            let start = 4 + i * row_size;
            if start + row_size > buf.len() {
                break;
            }
            let row: Tcp6RowOwnerPid = unsafe { std::ptr::read_unaligned(buf[start..].as_ptr() as *const _) };
            if row.state == MIB_TCP_STATE_LISTEN {
                continue;
            }
            if !v6_is_global(&row.remote) {
                continue;
            }
            tick.tcp += 1;
            let query = Tcp6Row {
                state: row.state,
                local: row.local,
                local_scope: row.local_scope,
                local_port: row.local_port,
                remote: row.remote,
                remote_scope: row.remote_scope,
                remote_port: row.remote_port,
            };
            let Ok((inn, out)) = read6(&query) else {
                tick.bad += 1;
                continue;
            };
            let key = Key {
                v6: true,
                local: row.local,
                local_port: row.local_port,
                remote: row.remote,
                remote_port: row.remote_port,
            };
            let (din, dout) = delta_of(&self.seen, still, key, inn, out);
            tick.totals.v6_down += din;
            tick.totals.v6_up += dout;
        }
    }
}

pub fn ipv4_is_local(o: [u8; 4]) -> bool {
    let a = o[0];
    let b = o[1];
    a == 0
        || a == 10
        || a == 127
        || (a == 100 && (64..128).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..32).contains(&b))
        || (a == 192 && b == 168)
        || a >= 224
}

fn v6_is_global(addr: &[u8; 16]) -> bool {
    if *addr == [0u8; 16] || addr[0] == 0xff {
        return false;
    }
    if addr[0] == 0xfe && (addr[1] & 0xc0) == 0x80 {
        return false;
    }
    if addr[..15] == [0u8; 15] && addr[15] == 1 {
        return false;
    }
    true
}

fn delta_of(
    prev: &HashMap<Key, Base>,
    still: &mut HashMap<Key, Base>,
    key: Key,
    inn: u64,
    out: u64,
) -> (u64, u64) {
    let (mut din, mut dout) = if let Some(old) = prev.get(&key) {
        (
            inn.saturating_sub(old.bytes_in),
            out.saturating_sub(old.bytes_out),
        )
    } else {
        (0, 0)
    };
    if din > MAX_STEP {
        din = 0;
    }
    if dout > MAX_STEP {
        dout = 0;
    }
    still.insert(key, Base { bytes_in: inn, bytes_out: out });
    (din, dout)
}

fn table(family: u32) -> Option<Vec<u8>> {
    unsafe {
        let mut size = 0u32;
        let rc = GetExtendedTcpTable(std::ptr::null_mut(), &mut size, 0, family, TCP_TABLE_OWNER_PID_ALL, 0);
        if rc != ERROR_INSUFFICIENT_BUFFER || size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize + 64];
        let rc = GetExtendedTcpTable(buf.as_mut_ptr() as *mut _, &mut size, 0, family, TCP_TABLE_OWNER_PID_ALL, 0);
        if rc != NO_ERROR {
            return None;
        }
        buf.truncate(size as usize);
        Some(buf)
    }
}

fn read4(row: &TcpRow) -> Result<(u64, u64), u32> {
    unsafe {
        let rw = DataRw { enable: 1 };
        let set = SetPerTcpConnectionEStats(
            row,
            TCP_CONNECTION_ESTATS_DATA,
            &rw as *const _ as *const u8,
            0,
            std::mem::size_of::<DataRw>() as u32,
            0,
        );
        if set != NO_ERROR {
            return Err(set);
        }
        let mut rod = std::mem::zeroed::<DataRod>();
        let rc = GetPerTcpConnectionEStats(
            row,
            TCP_CONNECTION_ESTATS_DATA,
            std::ptr::null_mut(),
            0,
            0,
            std::ptr::null_mut(),
            0,
            0,
            &mut rod as *mut _ as *mut u8,
            0,
            std::mem::size_of::<DataRod>() as u32,
        );
        accept(rc, rod)
    }
}

fn read6(row: &Tcp6Row) -> Result<(u64, u64), u32> {
    unsafe {
        let rw = DataRw { enable: 1 };
        let set = SetPerTcp6ConnectionEStats(
            row,
            TCP_CONNECTION_ESTATS_DATA,
            &rw as *const _ as *const u8,
            0,
            std::mem::size_of::<DataRw>() as u32,
            0,
        );
        if set != NO_ERROR {
            return Err(set);
        }
        let mut rod = std::mem::zeroed::<DataRod>();
        let rc = GetPerTcp6ConnectionEStats(
            row,
            TCP_CONNECTION_ESTATS_DATA,
            std::ptr::null_mut(),
            0,
            0,
            std::ptr::null_mut(),
            0,
            0,
            &mut rod as *mut _ as *mut u8,
            0,
            std::mem::size_of::<DataRod>() as u32,
        );
        accept(rc, rod)
    }
}

fn accept(rc: u32, rod: DataRod) -> Result<(u64, u64), u32> {
    if rc != NO_ERROR {
        return Err(rc);
    }
    if rod.data_bytes_in > ESTATS_LIMIT || rod.data_bytes_out > ESTATS_LIMIT {
        return Err(u32::MAX);
    }
    Ok((rod.data_bytes_in, rod.data_bytes_out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_rod_matches_the_windows_layout() {
        assert_eq!(std::mem::size_of::<DataRod>(), 96);
        assert_eq!(std::mem::size_of::<Tcp6RowOwnerPid>(), 56);
        assert_eq!(std::mem::size_of::<TcpRowOwnerPid>(), 24);
        assert_eq!(std::mem::size_of::<Tcp6Row>(), 52);
        assert_eq!(std::mem::size_of::<TcpRow>(), 20);
    }

    #[test]
    fn private_addresses_stay_out_of_the_billed_column() {
        assert!(ipv4_is_local([172, 22, 12, 148]));
        assert!(ipv4_is_local([10, 1, 2, 3]));
        assert!(ipv4_is_local([192, 168, 1, 1]));
        assert!(!ipv4_is_local([210, 30, 192, 148]));
        assert!(!ipv4_is_local([1, 1, 1, 1]));
    }
}
