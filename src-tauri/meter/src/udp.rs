//! TCP 和 UDP 字节来自 Microsoft-Windows-Kernel-Network 的收发事件。
//! 只累计字节和地址，不保存数据包。会话随本进程结束。

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::ERROR_ALREADY_EXISTS;
use windows::Win32::System::Diagnostics::Etw::{
    CloseTrace, ControlTraceW, EnableTraceEx2, OpenTraceW, ProcessTrace, StartTraceW,
    CONTROLTRACE_HANDLE, ENABLE_TRACE_PARAMETERS, EVENT_CONTROL_CODE_ENABLE_PROVIDER,
    EVENT_FILTER_DESCRIPTOR, EVENT_FILTER_TYPE_EVENT_ID, EVENT_RECORD, EVENT_TRACE_CONTROL_STOP,
    EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE,
    PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME, TRACE_LEVEL_INFORMATION,
    WNODE_FLAG_TRACED_GUID,
};

use crate::sample::{ipv4_is_local, v6_is_global};
use crate::store::Totals;

const SESSION: &str = "NidongMeterUdp";
const PROVIDER: windows::core::GUID = windows::core::GUID::from_u128(0x7dd42a49_5329_4832_8dfd_43d979153a88);
const KEYWORD_V4_V6: u64 = 0x30;
const SEND_TCP_V4: u16 = 10;
const RECV_TCP_V4: u16 = 11;
const RETX_TCP_V4: u16 = 14;
const SEND_TCP_V6: u16 = 26;
const RECV_TCP_V6: u16 = 27;
const RETX_TCP_V6: u16 = 30;
const SEND_V4: u16 = 42;
const RECV_V4: u16 = 43;
const SEND_V6: u16 = 58;
const RECV_V6: u16 = 59;
const EVENT_IDS: [u16; 10] = [
    SEND_TCP_V4, RECV_TCP_V4, RETX_TCP_V4, SEND_TCP_V6, RECV_TCP_V6, RETX_TCP_V6, SEND_V4, RECV_V4,
    SEND_V6, RECV_V6,
];
const MAX_DATAGRAM: u32 = 1024 * 1024;

static V4_DOWN: AtomicU64 = AtomicU64::new(0);
static V4_UP: AtomicU64 = AtomicU64::new(0);
static V6_DOWN: AtomicU64 = AtomicU64::new(0);
static V6_UP: AtomicU64 = AtomicU64::new(0);
static LOST_SEEN: AtomicU32 = AtomicU32::new(0);
static GAP: AtomicBool = AtomicBool::new(false);
static READY: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flow {
    pub v4_down: u64,
    pub v4_up: u64,
    pub v6_down: u64,
    pub v6_up: u64,
}

pub struct Sample {
    pub totals: Totals,
    pub ready: bool,
    pub gap: bool,
}

#[repr(C)]
struct TraceProps {
    props: EVENT_TRACE_PROPERTIES,
    name: [u16; 32],
}

pub struct Watch {
    stop: Arc<AtomicBool>,
    control: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

impl Watch {
    pub fn start() -> Self {
        READY.store(false, Ordering::SeqCst);
        GAP.store(false, Ordering::SeqCst);
        LOST_SEEN.store(0, Ordering::SeqCst);
        V4_DOWN.store(0, Ordering::SeqCst);
        V4_UP.store(0, Ordering::SeqCst);
        V6_DOWN.store(0, Ordering::SeqCst);
        V6_UP.store(0, Ordering::SeqCst);
        let stop = Arc::new(AtomicBool::new(false));
        let control = Arc::new(AtomicU64::new(0));
        let stop_t = stop.clone();
        let control_t = control.clone();
        let thread = thread::spawn(move || session(&stop_t, &control_t));
        let deadline = Instant::now() + Duration::from_millis(800);
        while Instant::now() < deadline && !READY.load(Ordering::SeqCst) && !thread.is_finished() {
            thread::sleep(Duration::from_millis(20));
        }
        Self { stop, control, thread: Some(thread) }
    }

    pub fn take(&self, night: bool) -> Sample {
        let ready = READY.load(Ordering::SeqCst);
        let gap = GAP.swap(false, Ordering::SeqCst);
        let flow = Flow {
            v4_down: V4_DOWN.swap(0, Ordering::SeqCst),
            v4_up: V4_UP.swap(0, Ordering::SeqCst),
            v6_down: V6_DOWN.swap(0, Ordering::SeqCst),
            v6_up: V6_UP.swap(0, Ordering::SeqCst),
        };
        if !ready || gap {
            return Sample { totals: Totals::default(), ready, gap: ready && gap };
        }
        Sample { totals: fold(flow, night), ready: true, gap: false }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let handle = self.control.load(Ordering::SeqCst);
        if handle != 0 {
            stop_handle(handle);
        } else {
            stop_named();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        READY.store(false, Ordering::SeqCst);
    }
}

pub fn classify(id: u16, data: &[u8]) -> Flow {
    match id {
        SEND_V4 | SEND_TCP_V4 | RETX_TCP_V4 => v4_flow(data, true),
        RECV_V4 | RECV_TCP_V4 => v4_flow(data, false),
        SEND_V6 | SEND_TCP_V6 | RETX_TCP_V6 => v6_flow(data, true),
        RECV_V6 | RECV_TCP_V6 => v6_flow(data, false),
        _ => Flow::default(),
    }
}

pub fn fold(flow: Flow, night: bool) -> Totals {
    let mut totals = Totals::default();
    if night {
        totals.v4_night = flow.v4_down;
    } else {
        totals.v4_day = flow.v4_down;
    }
    totals.v4_up = flow.v4_up;
    totals.v6_down = flow.v6_down;
    totals.v6_up = flow.v6_up;
    totals
}

fn v4_flow(data: &[u8], send: bool) -> Flow {
    let Some(size) = datagram_size(data) else {
        return Flow::default();
    };
    let Some(daddr) = read_v4(data, 8) else {
        return Flow::default();
    };
    let Some(saddr) = read_v4(data, 12) else {
        return Flow::default();
    };
    let mut flow = Flow::default();
    if send {
        flow.v4_up = size;
    } else if !ipv4_is_local(daddr) || !ipv4_is_local(saddr) {
        flow.v4_down = size;
    }
    flow
}

fn v6_flow(data: &[u8], send: bool) -> Flow {
    let Some(size) = datagram_size(data) else {
        return Flow::default();
    };
    let Some((daddr, saddr)) = read_v6(data) else {
        return Flow::default();
    };
    // ::ffff:a.b.c.d 是双栈套接字上的 IPv4。同一份数据报还会有 UDPv4 事件，这里再记就会两边各算一次。
    if !v6_counts(&daddr) && !v6_counts(&saddr) {
        return Flow::default();
    }
    let mut flow = Flow::default();
    if send {
        flow.v6_up = size;
    } else {
        flow.v6_down = size;
    }
    flow
}

fn datagram_size(data: &[u8]) -> Option<u64> {
    let size = u32::from_le_bytes(data.get(4..8)?.try_into().ok()?);
    if size == 0 || size > MAX_DATAGRAM {
        None
    } else {
        Some(u64::from(size))
    }
}

fn v6_counts(addr: &[u8; 16]) -> bool {
    !is_v4_mapped(addr) && v6_is_global(addr)
}

fn is_v4_mapped(addr: &[u8; 16]) -> bool {
    addr[..10] == [0u8; 10] && addr[10] == 0xff && addr[11] == 0xff
}

fn read_v4(data: &[u8], at: usize) -> Option<[u8; 4]> {
    data.get(at..at + 4)?.try_into().ok()
}

fn read_v6(data: &[u8]) -> Option<([u8; 16], [u8; 16])> {
    let rest = data.get(8..)?;
    if rest.len() >= 36 && rest[0] == 16 && rest[1] == 0 && rest[18] == 16 && rest[19] == 0 {
        let daddr = rest.get(2..18)?.try_into().ok()?;
        let saddr = rest.get(20..36)?.try_into().ok()?;
        return Some((daddr, saddr));
    }
    let daddr = rest.get(0..16)?.try_into().ok()?;
    let saddr = rest.get(16..32)?.try_into().ok()?;
    Some((daddr, saddr))
}

fn add_flow(flow: Flow) {
    if flow.v4_down > 0 {
        V4_DOWN.fetch_add(flow.v4_down, Ordering::Relaxed);
    }
    if flow.v4_up > 0 {
        V4_UP.fetch_add(flow.v4_up, Ordering::Relaxed);
    }
    if flow.v6_down > 0 {
        V6_DOWN.fetch_add(flow.v6_down, Ordering::Relaxed);
    }
    if flow.v6_up > 0 {
        V6_UP.fetch_add(flow.v6_up, Ordering::Relaxed);
    }
}

fn blank_props() -> TraceProps {
    let mut props: TraceProps = unsafe { std::mem::zeroed() };
    let mut n = 0;
    for unit in SESSION.encode_utf16() {
        props.name[n] = unit;
        n += 1;
    }
    props.props.Wnode.BufferSize = std::mem::size_of::<TraceProps>() as u32;
    props.props.Wnode.Flags = WNODE_FLAG_TRACED_GUID;
    props.props.Wnode.ClientContext = 1;
    props.props.BufferSize = 256;
    props.props.MinimumBuffers = 64;
    props.props.MaximumBuffers = 256;
    props.props.LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
    props.props.FlushTimer = 1;
    props.props.LoggerNameOffset = std::mem::offset_of!(TraceProps, name) as u32;
    props
}

fn stop_named() {
    let mut props = blank_props();
    unsafe {
        let _ = ControlTraceW(
            CONTROLTRACE_HANDLE::default(),
            PCWSTR(props.name.as_ptr()),
            &mut props.props,
            EVENT_TRACE_CONTROL_STOP,
        );
    }
}

fn stop_handle(handle: u64) {
    let mut props = blank_props();
    unsafe {
        let _ = ControlTraceW(
            CONTROLTRACE_HANDLE { Value: handle },
            PCWSTR(props.name.as_ptr()),
            &mut props.props,
            EVENT_TRACE_CONTROL_STOP,
        );
    }
}

fn session(stop: &AtomicBool, control: &AtomicU64) {
    let mut props = blank_props();
    let mut handle = CONTROLTRACE_HANDLE::default();
    let mut rc = unsafe { StartTraceW(&mut handle, PCWSTR(props.name.as_ptr()), &mut props.props) };
    if rc == ERROR_ALREADY_EXISTS {
        stop_named();
        props = blank_props();
        rc = unsafe { StartTraceW(&mut handle, PCWSTR(props.name.as_ptr()), &mut props.props) };
    }
    if rc.0 != 0 {
        return;
    }
    control.store(handle.Value, Ordering::SeqCst);
    if stop.load(Ordering::SeqCst) {
        stop_handle(handle.Value);
        return;
    }
    let mut logger: Vec<u16> = SESSION.encode_utf16().chain(std::iter::once(0)).collect();
    let mut logfile: EVENT_TRACE_LOGFILEW = unsafe { std::mem::zeroed() };
    logfile.LoggerName = PWSTR(logger.as_mut_ptr());
    logfile.Anonymous1.ProcessTraceMode = PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
    logfile.Anonymous2.EventRecordCallback = Some(on_record);
    logfile.BufferCallback = Some(on_buffer);
    let opened = unsafe { OpenTraceW(&mut logfile) };
    if opened.Value == u64::MAX {
        stop_handle(handle.Value);
        return;
    }
    if stop.load(Ordering::SeqCst) || !enable_udp(handle) {
        let _ = unsafe { CloseTrace(opened) };
        stop_handle(handle.Value);
        return;
    }
    READY.store(true, Ordering::SeqCst);
    let _ = unsafe { ProcessTrace(&[opened], None, None) };
    READY.store(false, Ordering::SeqCst);
    let _ = unsafe { CloseTrace(opened) };
    stop_handle(handle.Value);
}

fn enable_udp(handle: CONTROLTRACE_HANDLE) -> bool {
    let mut ids = [0u8; 4 + EVENT_IDS.len() * 2];
    ids[0] = 1;
    ids[2..4].copy_from_slice(&(EVENT_IDS.len() as u16).to_le_bytes());
    for (i, id) in EVENT_IDS.iter().enumerate() {
        ids[4 + i * 2..6 + i * 2].copy_from_slice(&id.to_le_bytes());
    }
    let mut filter = EVENT_FILTER_DESCRIPTOR {
        Ptr: ids.as_ptr() as u64,
        Size: ids.len() as u32,
        Type: EVENT_FILTER_TYPE_EVENT_ID,
    };
    let params = ENABLE_TRACE_PARAMETERS {
        Version: 2,
        EnableFilterDesc: &mut filter,
        FilterDescCount: 1,
        ..Default::default()
    };
    let filtered = unsafe {
        EnableTraceEx2(
            handle,
            &PROVIDER,
            EVENT_CONTROL_CODE_ENABLE_PROVIDER.0,
            TRACE_LEVEL_INFORMATION as u8,
            KEYWORD_V4_V6,
            0,
            0,
            Some(std::ptr::addr_of!(params)),
        )
    };
    if filtered.0 == 0 {
        return true;
    }
    let plain = unsafe {
        EnableTraceEx2(
            handle,
            &PROVIDER,
            EVENT_CONTROL_CODE_ENABLE_PROVIDER.0,
            TRACE_LEVEL_INFORMATION as u8,
            KEYWORD_V4_V6,
            0,
            0,
            None,
        )
    };
    plain.0 == 0
}

unsafe extern "system" fn on_record(event: *mut EVENT_RECORD) {
    if event.is_null() {
        return;
    }
    let event = &*event;
    let id = event.EventHeader.EventDescriptor.Id;
    let len = event.UserDataLength as usize;
    if event.UserData.is_null() || len < 8 {
        return;
    }
    let data = std::slice::from_raw_parts(event.UserData as *const u8, len);
    add_flow(classify(id, data));
}

unsafe extern "system" fn on_buffer(logfile: *mut EVENT_TRACE_LOGFILEW) -> u32 {
    if !logfile.is_null() {
        let lost = (*logfile).EventsLost;
        let seen = LOST_SEEN.load(Ordering::Relaxed);
        if lost > seen {
            LOST_SEEN.store(lost, Ordering::Relaxed);
            GAP.store(true, Ordering::SeqCst);
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::{classify, fold};

    fn v4(send: bool, size: u32, daddr: [u8; 4], saddr: [u8; 4]) -> Vec<u8> {
        let mut data = vec![0u8; 28];
        data[4..8].copy_from_slice(&size.to_le_bytes());
        data[8..12].copy_from_slice(&daddr);
        data[12..16].copy_from_slice(&saddr);
        let _ = send;
        data
    }

    #[test]
    fn off_campus_download_follows_either_address() {
        let direct = v4(false, 1500, [172, 22, 1, 8], [8, 8, 8, 8]);
        let swapped = v4(false, 1500, [8, 8, 8, 8], [172, 22, 1, 8]);
        let day = fold(classify(43, &direct), false);
        let night = fold(classify(43, &swapped), true);
        assert_eq!(day.v4_day, 1500);
        assert_eq!(night.v4_night, 1500);
        assert_eq!(day.v4_up, 0);
    }

    #[test]
    fn campus_only_download_is_ignored_and_upload_is_kept() {
        let local = v4(false, 400, [172, 22, 0, 2], [10, 0, 0, 1]);
        assert!(fold(classify(43, &local), false).v4_day == 0);
        let sent = v4(true, 80, [1, 2, 3, 4], [172, 22, 8, 8]);
        let up = fold(classify(42, &sent), false);
        assert_eq!(up.v4_up, 80);
        assert_eq!(up.v4_day, 0);
    }

    #[test]
    fn ipv6_global_counts_and_link_local_does_not() {
        let mut data = vec![0u8; 52];
        data[4..8].copy_from_slice(&900u32.to_le_bytes());
        data[8] = 0x20;
        data[9] = 0x01;
        let down = fold(classify(59, &data), false);
        assert_eq!(down.v6_down, 900);
        let mut local = vec![0u8; 52];
        local[4..8].copy_from_slice(&900u32.to_le_bytes());
        local[8] = 0xfe;
        local[9] = 0x80;
        local[24] = 0xfe;
        local[25] = 0x80;
        assert_eq!(classify(59, &local), super::Flow::default());
        let mut prefixed = vec![0u8; 44];
        prefixed[4..8].copy_from_slice(&700u32.to_le_bytes());
        prefixed[8] = 16;
        prefixed[10] = 0xfe;
        prefixed[11] = 0x80;
        prefixed[26] = 16;
        prefixed[28] = 0xfe;
        prefixed[29] = 0x80;
        assert_eq!(classify(59, &prefixed), super::Flow::default());
        prefixed[10] = 0x20;
        prefixed[11] = 0x01;
        assert_eq!(fold(classify(59, &prefixed), false).v6_down, 700);
    }

    #[test]
    fn ipv4_mapped_event_stays_out_of_ipv6() {
        let data = v6_event(1200, mapped([8, 8, 8, 8]), mapped([172, 22, 1, 8]));
        assert_eq!(classify(59, &data), super::Flow::default());
        assert_eq!(classify(58, &data), super::Flow::default());
        let mut real = [0u8; 16];
        real[0] = 0x20;
        real[1] = 0x01;
        let mixed = fold(classify(59, &v6_event(500, mapped([8, 8, 8, 8]), real)), false);
        assert_eq!(mixed.v6_down, 500);
        assert_eq!(mixed.v4_day, 0);
        let native = fold(classify(43, &v4(false, 1200, [8, 8, 8, 8], [172, 22, 1, 8])), false);
        assert_eq!(native.v4_day, 1200);
        assert_eq!(native.v6_down, 0);
    }

    fn mapped(ipv4: [u8; 4]) -> [u8; 16] {
        let mut addr = [0u8; 16];
        addr[10] = 0xff;
        addr[11] = 0xff;
        addr[12..16].copy_from_slice(&ipv4);
        addr
    }

    fn v6_event(size: u32, daddr: [u8; 16], saddr: [u8; 16]) -> Vec<u8> {
        let mut data = vec![0u8; 52];
        data[4..8].copy_from_slice(&size.to_le_bytes());
        data[8..24].copy_from_slice(&daddr);
        data[24..40].copy_from_slice(&saddr);
        data
    }

    #[test]
    fn tcp_events_follow_the_udp_columns() {
        let down = fold(classify(11, &v4(false, 2000, [1, 1, 1, 1], [172, 22, 1, 8])), false);
        assert_eq!(down.v4_day, 2000);
        assert_eq!(down.v6_down, 0);
        let up = fold(classify(10, &v4(true, 300, [8, 8, 8, 8], [172, 22, 1, 8])), false);
        assert_eq!(up.v4_up, 300);
        assert_eq!(up.v4_day, 0);
        let retx = fold(classify(14, &v4(true, 40, [8, 8, 8, 8], [172, 22, 1, 8])), false);
        assert_eq!(retx.v4_up, 40);
        let mut remote = [0u8; 16];
        remote[0] = 0x20;
        remote[1] = 0x01;
        let recv = fold(classify(27, &v6_event(800, remote, mapped([172, 22, 1, 8]))), false);
        assert_eq!(recv.v6_down, 800);
        assert_eq!(recv.v4_day, 0);
        assert_eq!(fold(classify(26, &v6_event(800, remote, mapped([172, 22, 1, 8]))), false).v6_up, 800);
        assert_eq!(fold(classify(30, &v6_event(15, remote, mapped([172, 22, 1, 8]))), false).v6_up, 15);
        let mapped_only = v6_event(1200, mapped([8, 8, 8, 8]), mapped([172, 22, 1, 8]));
        assert_eq!(classify(27, &mapped_only), super::Flow::default());
        assert_eq!(classify(18, &v4(true, 100, [1, 1, 1, 1], [172, 22, 1, 8])), super::Flow::default());
    }

    #[test]
    fn ignores_empty_or_huge_sizes() {
        assert_eq!(classify(43, &v4(false, 0, [8, 8, 8, 8], [172, 22, 1, 1])), super::Flow::default());
        assert_eq!(
            classify(11, &v4(false, MAX_OVER, [8, 8, 8, 8], [172, 22, 1, 1])),
            super::Flow::default()
        );
    }

    const MAX_OVER: u32 = 1024 * 1024 + 1;
}
