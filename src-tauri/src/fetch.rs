//! 工具箱 · 定时下载。
//!
//! 到点用 http(s) 把文件拉到用户选的文件夹。进度写在 `.partial` 里，中断后按已有字节续传。
//! 预定在 0:00–6:00 的任务，如果当时没在跑，只在同一个免费时段里补下；过了 6 点不自动开始。

use crate::clock;
use crate::error::Result;
use chrono::{Days, NaiveDateTime, TimeDelta, Timelike};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;
use url::Url;

const MAX_URLS: usize = 20;
const ATTEMPTS: u32 = 3;
const SYNC_EVERY: u64 = 4 * 1024 * 1024;
const INTERNET_SETTINGS: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WhenDue {
    Wait,
    Start,
    Missed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Judge {
    Wait,
    Run,
    Missed,
    Interrupted,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchStatus {
    pub phase: String,
    pub when: String,
    pub dir: String,
    pub shutdown: bool,
    pub use_proxy: bool,
    pub proxy: String,
    pub proxy_kind: String,
    pub note: String,
    pub links: String,
    pub files: Vec<JobFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobFile {
    pub url: String,
    pub name: String,
    pub phase: String,
    pub received: u64,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(default)]
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Job {
    phase: String,
    when: String,
    dir: String,
    shutdown: bool,
    use_proxy: bool,
    #[serde(default)]
    touched: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    links: String,
    #[serde(default)]
    files: Vec<JobFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default)]
    job: Option<Job>,
    #[serde(default)]
    dir: String,
    #[serde(default)]
    shutdown: bool,
    #[serde(default)]
    use_proxy: bool,
    #[serde(default)]
    note: String,
    #[serde(default)]
    links: String,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            job: None,
            dir: String::new(),
            shutdown: false,
            use_proxy: false,
            note: String::new(),
            links: String::new(),
        }
    }
}

struct Inner {
    data_dir: PathBuf,
    store: Store,
    owns_run: bool,
    cancel: bool,
}

pub struct FetchHub {
    inner: Mutex<Inner>,
    wake: tokio::sync::Notify,
}

enum Attempt {
    Done,
    Retry(String),
    Fatal(String),
    Cancel,
}

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let Some(hub) = app.try_state::<FetchHub>() else {
                break;
            };
            if hub.poll() {
                run_downloads(&hub).await;
                let _ = app.emit("fetch-changed", ());
            }
            let wake = hub.wake.notified();
            tokio::pin!(wake);
            tokio::select! {
                _ = wake => {}
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
        }
    });
}

impl FetchHub {
    pub fn open(data_dir: PathBuf) -> Self {
        let mut store = read_store(&data_dir);
        let now = clock::now().naive_local();
        if let Some(job) = store.job.as_mut() {
            if job.phase == "shutting-down" {
                job.phase = "done".into();
                job.note = "上次准备关机，但电脑还在运行。".into();
            }
            if !known_phase(&job.phase) {
                store.job = None;
            }
        }
        let changed = settle_store(&mut store, now);
        if changed {
            persist(&data_dir, &store);
        }
        Self {
            inner: Mutex::new(Inner {
                data_dir,
                store,
                owns_run: false,
                cancel: false,
            }),
            wake: tokio::sync::Notify::new(),
        }
    }

    fn poll(&self) -> bool {
        let mut guard = self.inner.lock().unwrap();
        let now = clock::now().naive_local();
        let Some(job) = guard.store.job.clone() else {
            return false;
        };
        let Some(when) = parse_stamp(&job.when) else {
            guard.store.job = None;
            persist(&guard.data_dir, &guard.store);
            return false;
        };
        let touched = parse_stamp(&job.touched).unwrap_or(when);
        match judge(&job.phase, when, touched, now, guard.owns_run) {
            Judge::Run => {
                if let Some(job) = guard.store.job.as_mut() {
                    job.phase = "running".into();
                    job.touched = stamp(now);
                    job.note.clear();
                }
                guard.owns_run = true;
                guard.cancel = false;
                persist(&guard.data_dir, &guard.store);
                true
            }
            Judge::Missed => {
                if let Some(job) = guard.store.job.as_mut() {
                    job.phase = "missed".into();
                    job.note = missed_note(when, now);
                }
                persist(&guard.data_dir, &guard.store);
                false
            }
            Judge::Interrupted => {
                if let Some(job) = guard.store.job.as_mut() {
                    job.phase = "interrupted".into();
                    job.note = interrupted_note(when, now);
                }
                persist(&guard.data_dir, &guard.store);
                false
            }
            Judge::Wait => false,
        }
    }

    fn status(&self) -> FetchStatus {
        let guard = self.inner.lock().unwrap();
        let (proxy_kind, proxy) = proxy_status();
        if let Some(job) = &guard.store.job {
            FetchStatus {
                phase: job.phase.clone(),
                when: job.when.clone(),
                dir: job.dir.clone(),
                shutdown: job.shutdown,
                use_proxy: job.use_proxy,
                proxy,
                proxy_kind,
                note: job.note.clone(),
                links: job.links.clone(),
                files: job.files.clone(),
            }
        } else {
            let dir = if guard.store.dir.trim().is_empty() {
                downloads_dir_string()
            } else {
                guard.store.dir.clone()
            };
            FetchStatus {
                phase: "idle".into(),
                when: String::new(),
                dir,
                shutdown: guard.store.shutdown,
                use_proxy: guard.store.use_proxy,
                proxy,
                proxy_kind,
                note: guard.store.note.clone(),
                links: guard.store.links.clone(),
                files: Vec::new(),
            }
        }
    }

    fn arm(&self, urls: String, time: String, dir: String, shutdown: bool, use_proxy: bool) -> Result<FetchStatus> {
        let parsed = split_urls(&urls).map_err(crate::error::AppError::Msg)?;
        let (hour, minute) = parse_hhmm(&time).map_err(crate::error::AppError::Msg)?;
        let folder = clean_dir(Path::new(dir.trim())).map_err(crate::error::AppError::Msg)?;
        let now = clock::now().naive_local();
        let when = next_clock(now, hour, minute).ok_or_else(|| crate::error::AppError::Msg("这个时间排不了。".into()))?;
        let mut files = Vec::new();
        for url in &parsed {
            let desired = name_from_url(url);
            let (name, _) = allocate_paths(&folder, &desired, url.as_str());
            files.push(JobFile {
                url: url.to_string(),
                name,
                phase: "pending".into(),
                received: 0,
                total: None,
                error: String::new(),
            });
        }
        let mut guard = self.inner.lock().unwrap();
        if let Some(job) = &guard.store.job {
            if matches!(job.phase.as_str(), "armed" | "running" | "shutting-down") {
                return Err("上一次还没结束。先取消，再约下一次。".into());
            }
        }
        let dir_text = folder.to_string_lossy().to_string();
        guard.store.dir = dir_text.clone();
        guard.store.shutdown = shutdown;
        guard.store.use_proxy = use_proxy;
        guard.store.links = urls;
        guard.store.note.clear();
        guard.cancel = false;
        guard.owns_run = false;
        guard.store.job = Some(Job {
            phase: "armed".into(),
            when: stamp(when),
            dir: dir_text,
            shutdown,
            use_proxy,
            touched: stamp(now),
            note: String::new(),
            links: guard.store.links.clone(),
            files,
        });
        persist(&guard.data_dir, &guard.store);
        drop(guard);
        self.wake.notify_one();
        Ok(self.status())
    }

    fn cancel(&self) -> FetchStatus {
        let mut guard = self.inner.lock().unwrap();
        let phase = guard
            .store
            .job
            .as_ref()
            .map(|job| job.phase.clone())
            .unwrap_or_default();
        match phase.as_str() {
            "shutting-down" => {
                abort_shutdown();
                if let Some(job) = guard.store.job.as_mut() {
                    job.phase = "done".into();
                    job.note = "已取消关机。".into();
                }
                persist(&guard.data_dir, &guard.store);
            }
            "armed" => {
                clear_job(&mut guard, "已取消。");
            }
            "running" => {
                guard.cancel = true;
                if let Some(job) = guard.store.job.as_mut() {
                    job.note = "正在取消…".into();
                }
                persist(&guard.data_dir, &guard.store);
            }
            "missed" | "interrupted" => {
                let started = guard.store.job.as_ref().is_some_and(|job| {
                    job.files.iter().any(|file| file.received > 0 || file.phase == "done")
                });
                let note = if started {
                    "已放弃。没下完的文件还在文件夹里。"
                } else {
                    "已放弃。"
                };
                clear_job(&mut guard, note);
            }
            _ => {}
        }
        drop(guard);
        self.wake.notify_one();
        self.status()
    }

    fn resume(&self) -> Result<FetchStatus> {
        let mut guard = self.inner.lock().unwrap();
        let Some(job) = guard.store.job.as_mut() else {
            return Err("现在没有要继续的下载。".into());
        };
        if job.phase != "missed" && job.phase != "interrupted" {
            return Err("现在没有要继续的下载。".into());
        }
        let now = clock::now().naive_local();
        job.phase = "running".into();
        job.touched = stamp(now);
        job.note.clear();
        guard.owns_run = false;
        guard.cancel = false;
        persist(&guard.data_dir, &guard.store);
        drop(guard);
        self.wake.notify_one();
        Ok(self.status())
    }

    fn cancelled(&self) -> bool {
        self.inner.lock().unwrap().cancel
    }

    fn file_count(&self) -> usize {
        self.inner
            .lock()
            .unwrap()
            .store
            .job
            .as_ref()
            .map(|job| job.files.len())
            .unwrap_or(0)
    }

    fn all_done(&self) -> bool {
        self.inner
            .lock()
            .unwrap()
            .store
            .job
            .as_ref()
            .is_some_and(|job| !job.files.is_empty() && job.files.iter().all(|file| file.phase == "done"))
    }

    fn file_phase(&self, index: usize) -> String {
        self.inner
            .lock()
            .unwrap()
            .store
            .job
            .as_ref()
            .and_then(|job| job.files.get(index))
            .map(|file| file.phase.clone())
            .unwrap_or_default()
    }

    fn mark_running(&self, index: usize) {
        self.touch_file(index, |file| {
            file.phase = "running".into();
            file.error.clear();
        });
    }

    fn mark_done(&self, index: usize) {
        self.touch_file(index, |file| {
            file.phase = "done".into();
            file.error.clear();
        });
    }

    fn mark_failed(&self, index: usize, message: &str) {
        self.touch_file(index, |file| {
            file.phase = "failed".into();
            file.error = message.to_string();
        });
    }

    fn touch_file(&self, index: usize, edit: impl FnOnce(&mut JobFile)) {
        let mut guard = self.inner.lock().unwrap();
        let Some(job) = guard.store.job.as_mut() else {
            return;
        };
        if job.phase != "running" {
            return;
        }
        if let Some(file) = job.files.get_mut(index) {
            edit(file);
        }
        job.touched = stamp(clock::now().naive_local());
        persist(&guard.data_dir, &guard.store);
    }

    /// 返回 false 表示任务已经取消，调用方应停下。
    fn set_received(&self, index: usize, received: u64, total: Option<u64>, persist_now: bool) -> bool {
        let mut guard = self.inner.lock().unwrap();
        if guard.cancel {
            return false;
        }
        let Some(job) = guard.store.job.as_mut() else {
            return false;
        };
        if job.phase != "running" {
            return false;
        }
        if let Some(file) = job.files.get_mut(index) {
            file.phase = "running".into();
            file.received = received;
            if total.is_some() {
                file.total = total;
            }
        }
        if persist_now {
            job.touched = stamp(clock::now().naive_local());
            persist(&guard.data_dir, &guard.store);
        }
        true
    }

    fn file_spec(&self, index: usize) -> Option<FileSpec> {
        let guard = self.inner.lock().unwrap();
        let job = guard.store.job.as_ref()?;
        let file = job.files.get(index)?;
        Some(FileSpec {
            url: file.url.clone(),
            name: file.name.clone(),
            dir: PathBuf::from(&job.dir),
            use_proxy: job.use_proxy,
            total: file.total,
        })
    }

    fn rename_file(&self, index: usize, name: String) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(file) = guard.store.job.as_mut().and_then(|job| job.files.get_mut(index)) {
            file.name = name;
        }
        persist(&guard.data_dir, &guard.store);
    }

    fn finish_cancel(&self) {
        let mut guard = self.inner.lock().unwrap();
        let started = guard.store.job.as_ref().is_some_and(|job| {
            job.files
                .iter()
                .any(|file| file.received > 0 || file.phase == "done")
        });
        let note = if started {
            "已取消。没下完的部分还在文件夹里，同一条链接下次会接着下。"
        } else {
            "已取消。"
        };
        clear_job(&mut guard, note);
    }

    fn finish_success(&self) {
        let mut guard = self.inner.lock().unwrap();
        if guard.cancel {
            drop(guard);
            self.finish_cancel();
            return;
        }
        let Some(job) = guard.store.job.as_mut() else {
            guard.owns_run = false;
            return;
        };
        let failed = job.files.iter().any(|file| file.phase != "done");
        if failed {
            job.phase = "done".into();
            job.note = "有文件没下完，没有关机。".into();
            guard.owns_run = false;
            persist(&guard.data_dir, &guard.store);
            return;
        }
        if job.shutdown {
            match schedule_shutdown() {
                Ok(()) => {
                    job.phase = "shutting-down".into();
                    job.note = "下载完成，大约一分钟后关机。退出泥洞也不会取消。".into();
                }
                Err(message) => {
                    job.phase = "done".into();
                    job.note = message;
                }
            }
        } else {
            job.phase = "done".into();
            job.note = "下载完成。".into();
        }
        guard.owns_run = false;
        persist(&guard.data_dir, &guard.store);
    }
}

struct FileSpec {
    url: String,
    name: String,
    dir: PathBuf,
    use_proxy: bool,
    total: Option<u64>,
}

fn clear_job(guard: &mut Inner, note: &str) {
    if let Some(job) = guard.store.job.take() {
        guard.store.dir = job.dir;
        guard.store.shutdown = job.shutdown;
        guard.store.use_proxy = job.use_proxy;
        if !job.links.is_empty() {
            guard.store.links = job.links;
        }
    }
    guard.store.note = note.to_string();
    guard.owns_run = false;
    guard.cancel = false;
    persist(&guard.data_dir, &guard.store);
}

fn known_phase(phase: &str) -> bool {
    matches!(
        phase,
        "armed" | "running" | "done" | "missed" | "interrupted" | "shutting-down"
    )
}

fn settle_store(store: &mut Store, now: NaiveDateTime) -> bool {
    let Some(job) = store.job.as_ref() else {
        return false;
    };
    let Some(when) = parse_stamp(&job.when) else {
        store.job = None;
        return true;
    };
    let touched = parse_stamp(&job.touched).unwrap_or(when);
    match judge(&job.phase, when, touched, now, false) {
        Judge::Missed => {
            let note = missed_note(when, now);
            if let Some(job) = store.job.as_mut() {
                job.phase = "missed".into();
                job.note = note;
            }
            true
        }
        Judge::Interrupted => {
            let note = interrupted_note(when, now);
            if let Some(job) = store.job.as_mut() {
                job.phase = "interrupted".into();
                job.note = note;
            }
            true
        }
        _ => false,
    }
}

fn in_free(time: NaiveDateTime) -> bool {
    time.hour() < 6
}

fn within(start: NaiveDateTime, now: NaiveDateTime, limit: TimeDelta) -> bool {
    if now < start {
        return false;
    }
    now.signed_duration_since(start) <= limit
}

fn same_free_window(when: NaiveDateTime, now: NaiveDateTime) -> bool {
    in_free(when) && in_free(now) && when.date() == now.date() && now >= when
}

fn should_start(when: NaiveDateTime, now: NaiveDateTime) -> WhenDue {
    if now < when {
        return WhenDue::Wait;
    }
    if in_free(when) {
        if same_free_window(when, now) {
            WhenDue::Start
        } else {
            WhenDue::Missed
        }
    } else if within(when, now, TimeDelta::minutes(20)) {
        WhenDue::Start
    } else {
        WhenDue::Missed
    }
}

fn resume_after_break(when: NaiveDateTime, touched: NaiveDateTime, now: NaiveDateTime) -> bool {
    within(touched, now, TimeDelta::minutes(20)) || same_free_window(when, now)
}

fn judge(
    phase: &str,
    when: NaiveDateTime,
    touched: NaiveDateTime,
    now: NaiveDateTime,
    owns_run: bool,
) -> Judge {
    match phase {
        "armed" => match should_start(when, now) {
            WhenDue::Wait => Judge::Wait,
            WhenDue::Start => Judge::Run,
            WhenDue::Missed => Judge::Missed,
        },
        "running" if owns_run => Judge::Wait,
        "running" => {
            if resume_after_break(when, touched, now) {
                Judge::Run
            } else {
                Judge::Interrupted
            }
        }
        _ => Judge::Wait,
    }
}

fn missed_note(when: NaiveDateTime, now: NaiveDateTime) -> String {
    if in_free(when) && !in_free(now) {
        "到点时泥洞没在运行。免费时段已经过了，没有自动开始。".into()
    } else {
        "到点时泥洞没在运行，已经过了预定时间，没有自动开始。".into()
    }
}

fn interrupted_note(when: NaiveDateTime, now: NaiveDateTime) -> String {
    if in_free(when) && !in_free(now) {
        "下载被打断了。现在不在 0:00–6:00，没有自动继续。".into()
    } else {
        "下载被打断了，没有自动继续。".into()
    }
}

fn next_clock(now: NaiveDateTime, hour: u32, minute: u32) -> Option<NaiveDateTime> {
    let today = now.date().and_hms_opt(hour, minute, 0)?;
    if today > now {
        Some(today)
    } else {
        now.date()
            .checked_add_days(Days::new(1))?
            .and_hms_opt(hour, minute, 0)
    }
}

fn parse_hhmm(raw: &str) -> std::result::Result<(u32, u32), String> {
    let raw = raw.trim();
    let raw = if raw.is_empty() { "00:00" } else { raw };
    let Some((hour_text, minute_text)) = raw.split_once(':') else {
        return Err("时间写成 00:00 这样。".into());
    };
    if hour_text.is_empty()
        || minute_text.is_empty()
        || hour_text.len() > 2
        || minute_text.len() > 2
        || !hour_text.chars().all(|ch| ch.is_ascii_digit())
        || !minute_text.chars().all(|ch| ch.is_ascii_digit())
    {
        return Err("时间写成 00:00 这样。".into());
    }
    let hour: u32 = hour_text.parse().map_err(|_| "时间写成 00:00 这样。".to_string())?;
    let minute: u32 = minute_text
        .parse()
        .map_err(|_| "时间写成 00:00 这样。".to_string())?;
    if hour > 23 || minute > 59 {
        return Err("时间要在 00:00 和 23:59 之间。".into());
    }
    Ok((hour, minute))
}

fn stamp(time: NaiveDateTime) -> String {
    time.format("%Y-%m-%dT%H:%M:%S").to_string()
}

fn parse_stamp(value: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S").ok()
}

fn split_urls(raw: &str) -> std::result::Result<Vec<Url>, String> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if out.len() >= MAX_URLS {
            return Err("一次最多 20 条。".into());
        }
        if line.len() > 2000 {
            return Err("有一条链接太长了。".into());
        }
        let url = Url::parse(line).map_err(|_| format!("这不是链接：{line}"))?;
        if url.scheme() != "http" && url.scheme() != "https" {
            return Err(format!("只支持 http 和 https：{line}"));
        }
        if url.host_str().is_none() {
            return Err(format!("这不是链接：{line}"));
        }
        if out.iter().any(|have: &Url| have.as_str() == url.as_str()) {
            continue;
        }
        out.push(url);
    }
    if out.is_empty() {
        return Err("先贴上下载链接，一行一个。".into());
    }
    Ok(out)
}

fn name_from_url(url: &Url) -> String {
    let segment = url
        .path_segments()
        .and_then(|mut parts| parts.next_back())
        .unwrap_or("");
    let decoded = urlencoding::decode(segment).unwrap_or(std::borrow::Cow::Borrowed(segment));
    sanitize_filename(&decoded)
}

fn sanitize_filename(raw: &str) -> String {
    let segment = raw.split(['/', '\\']).next_back().unwrap_or(raw);
    let mut out = String::new();
    for ch in segment.chars() {
        if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || (ch as u32) < 32 {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    let mut out = out.trim().trim_end_matches(['.', ' ']).to_string();
    out = out.chars().take(80).collect();
    while out.ends_with(['.', ' ']) {
        out.pop();
    }
    if out.is_empty() || out == "." || out == ".." {
        return "download.bin".into();
    }
    let stem = out.split('.').next().unwrap_or(&out);
    if is_reserved(stem) {
        out.insert(0, '_');
    }
    out
}

fn is_reserved(stem: &str) -> bool {
    matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn filename_from_disposition(header: &str) -> Option<String> {
    for part in header.split(';') {
        let part = part.trim();
        let lower = part.to_ascii_lowercase();
        let Some(rest) = lower.strip_prefix("filename*=") else {
            continue;
        };
        let raw = &part[part.len() - rest.len()..];
        let encoded = raw.split_once("''").map(|(_, value)| value).unwrap_or(raw);
        let encoded = encoded.trim().trim_matches('"');
        let Ok(decoded) = urlencoding::decode(encoded) else {
            continue;
        };
        let name = sanitize_filename(&decoded);
        if name != "download.bin" || !decoded.trim().is_empty() {
            return Some(name);
        }
    }
    for part in header.split(';') {
        let part = part.trim();
        let lower = part.to_ascii_lowercase();
        if lower.starts_with("filename*=") {
            continue;
        }
        let Some(rest) = lower.strip_prefix("filename=") else {
            continue;
        };
        let raw = &part[part.len() - rest.len()..];
        let raw = raw.trim().trim_matches('"');
        let name = sanitize_filename(raw);
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

fn numbered(name: &str, index: u32) -> String {
    if let Some((stem, ext)) = name.rsplit_once('.') {
        if !stem.is_empty() && !ext.is_empty() && ext.len() <= 8 && !ext.contains(' ') {
            return format!("{stem} ({index}).{ext}");
        }
    }
    format!("{name} ({index})")
}

fn sidecar_path(partial: &Path) -> PathBuf {
    let mut text = partial.as_os_str().to_owned();
    text.push(".url");
    PathBuf::from(text)
}

fn sidecar_matches(partial: &Path, url: &str) -> bool {
    std::fs::read_to_string(sidecar_path(partial))
        .ok()
        .is_some_and(|text| text == url)
}

fn allocate_paths(dir: &Path, desired: &str, url: &str) -> (String, PathBuf) {
    let mut name = desired.to_string();
    for index in 1..100 {
        let partial = dir.join(format!("{name}.partial"));
        let final_path = dir.join(&name);
        if sidecar_matches(&partial, url) {
            return (name, partial);
        }
        if !partial.exists() && !final_path.exists() && !sidecar_path(&partial).exists() {
            return (name, partial);
        }
        name = numbered(desired, index + 1);
    }
    let partial = dir.join(format!("{name}.partial"));
    (name, partial)
}

fn downloads_dir() -> PathBuf {
    if let Ok(home) = std::env::var("USERPROFILE") {
        let dir = PathBuf::from(home).join("Downloads");
        if dir.is_dir() {
            return dir;
        }
        return dir;
    }
    PathBuf::from(".")
}

fn downloads_dir_string() -> String {
    downloads_dir().to_string_lossy().to_string()
}

fn clean_dir(path: &Path) -> std::result::Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return clean_dir(&downloads_dir());
    }
    if !path.is_absolute() {
        return Err("请选一个文件夹。".into());
    }
    if path.is_file() {
        return Err("这不是文件夹。".into());
    }
    if !path.is_dir() {
        std::fs::create_dir_all(path).map_err(|_| "这个文件夹建不出来。".to_string())?;
    }
    Ok(path.to_path_buf())
}

fn read_store(data_dir: &Path) -> Store {
    let bytes = std::fs::read(data_dir.join("fetch.json")).unwrap_or_default();
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn persist(data_dir: &Path, store: &Store) {
    let _ = std::fs::create_dir_all(data_dir);
    let path = data_dir.join("fetch.json");
    let tmp = data_dir.join("fetch.json.tmp");
    let Ok(bytes) = serde_json::to_vec_pretty(store) else {
        return;
    };
    if std::fs::write(&tmp, &bytes).is_ok() {
        if std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::write(&path, bytes);
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

#[derive(Clone)]
struct ProxySnap {
    http: Option<String>,
    https: Option<String>,
    bypass: Vec<String>,
    kind: &'static str,
    label: String,
}

fn proxy_status() -> (String, String) {
    let snap = current_proxy();
    if snap.kind == "http" {
        return ("http".into(), snap.label);
    }
    if let Some(endpoint) = env_proxy("https").or_else(|| env_proxy("http")) {
        return ("http".into(), endpoint_label(&endpoint));
    }
    if snap.kind == "socks" {
        return ("socks".into(), snap.label);
    }
    ("off".into(), String::new())
}

fn choose_proxy(url: &Url) -> Option<String> {
    let host = url.host_str().unwrap_or("");
    let snap = current_proxy();
    if host_bypassed(host, &snap.bypass) {
        return None;
    }
    let endpoint = match url.scheme() {
        "https" => snap.https.clone().or(snap.http.clone()),
        _ => snap.http.clone().or(snap.https.clone()),
    };
    if endpoint.is_some() {
        return endpoint;
    }
    if host_bypassed(host, &[]) {
        return None;
    }
    env_proxy(url.scheme())
}

fn env_proxy(scheme: &str) -> Option<String> {
    let keys: &[&str] = if scheme == "https" {
        &["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
    } else {
        &["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]
    };
    for key in keys {
        let Ok(value) = std::env::var(key) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() || value.to_ascii_lowercase().starts_with("socks") {
            continue;
        }
        return normalize_proxy_url(value);
    }
    None
}

fn endpoint_label(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_string()
}

struct ParsedProxy {
    http: Option<String>,
    https: Option<String>,
    socks: bool,
}

fn parse_proxy_server(raw: &str) -> ParsedProxy {
    let raw = raw.trim();
    if raw.is_empty() {
        return ParsedProxy {
            http: None,
            https: None,
            socks: false,
        };
    }
    if !raw.contains('=') {
        let lower = raw.to_ascii_lowercase();
        if lower.starts_with("socks") {
            return ParsedProxy {
                http: None,
                https: None,
                socks: true,
            };
        }
        let url = normalize_proxy_url(raw);
        return ParsedProxy {
            http: url.clone(),
            https: url,
            socks: false,
        };
    }
    let mut http = None;
    let mut https = None;
    let mut socks = false;
    for part in raw.split(';') {
        let Some((key, value)) = part.trim().split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        if value.to_ascii_lowercase().starts_with("socks") || key.starts_with("socks") {
            socks = true;
            continue;
        }
        let url = normalize_proxy_url(value);
        match key.as_str() {
            "http" => http = url,
            "https" => https = url,
            _ => {}
        }
    }
    ParsedProxy { http, https, socks }
}

fn normalize_proxy_url(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("socks") {
        return None;
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some(value.to_string());
    }
    Some(format!("http://{value}"))
}

fn parse_bypass(raw: &str) -> Vec<String> {
    raw.split(';')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn host_bypassed(host: &str, rules: &[String]) -> bool {
    let host = host
        .trim_matches(['[', ']'])
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host == "localhost" || host == "127.0.0.1" || host == "::1" {
        return true;
    }
    for rule in rules {
        let rule = rule.trim().trim_matches('"').to_ascii_lowercase();
        if rule.is_empty() {
            continue;
        }
        if rule == "<local>" {
            if !host.contains('.') {
                return true;
            }
            continue;
        }
        if proxy_rule_match(&host, &rule) {
            return true;
        }
    }
    false
}

fn proxy_rule_match(host: &str, rule: &str) -> bool {
    if let Some(prefix) = rule.strip_suffix(".*") {
        return host == prefix || host.starts_with(&format!("{prefix}."));
    }
    if let Some(suffix) = rule.strip_prefix("*.") {
        return host == suffix || host.ends_with(&format!(".{suffix}"));
    }
    if let Some(suffix) = rule.strip_prefix('*') {
        return host.ends_with(suffix);
    }
    host == rule
}

#[cfg(windows)]
fn current_proxy() -> ProxySnap {
    let enabled = reg_dword(INTERNET_SETTINGS, "ProxyEnable").unwrap_or(0);
    if enabled == 0 {
        return ProxySnap {
            http: None,
            https: None,
            bypass: Vec::new(),
            kind: "off",
            label: String::new(),
        };
    }
    let server = reg_sz(INTERNET_SETTINGS, "ProxyServer").unwrap_or_default();
    let bypass = parse_bypass(&reg_sz(INTERNET_SETTINGS, "ProxyOverride").unwrap_or_default());
    let parsed = parse_proxy_server(&server);
    if parsed.http.is_some() || parsed.https.is_some() {
        let label = parsed
            .https
            .clone()
            .or(parsed.http.clone())
            .map(|value| endpoint_label(&value))
            .unwrap_or_default();
        return ProxySnap {
            http: parsed.http,
            https: parsed.https,
            bypass,
            kind: "http",
            label,
        };
    }
    if parsed.socks {
        return ProxySnap {
            http: None,
            https: None,
            bypass,
            kind: "socks",
            label: endpoint_label(&server),
        };
    }
    ProxySnap {
        http: None,
        https: None,
        bypass,
        kind: "off",
        label: String::new(),
    }
}

#[cfg(not(windows))]
fn current_proxy() -> ProxySnap {
    ProxySnap {
        http: None,
        https: None,
        bypass: Vec::new(),
        kind: "off",
        label: String::new(),
    }
}

#[cfg(windows)]
fn reg_sz(subkey: &str, value: &str) -> Option<String> {
    use std::os::windows::ffi::OsStringExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};

    let sub = wide(subkey);
    let name = wide(value);
    let mut bytes = 0u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        )
    };
    if rc != ERROR_SUCCESS || bytes < 4 {
        return None;
    }
    let mut buf = vec![0u16; bytes as usize / 2 + 1];
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut bytes),
        )
    };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let len = (bytes as usize / 2).saturating_sub(1);
    let text = std::ffi::OsString::from_wide(&buf[..len])
        .to_string_lossy()
        .trim()
        .trim_matches('"')
        .to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(windows)]
fn reg_dword(subkey: &str, value: &str) -> Option<u32> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};

    let sub = wide(subkey);
    let name = wide(value);
    let mut data = 0u32;
    let mut bytes = 4u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut _),
            Some(&mut bytes),
        )
    };
    if rc == ERROR_SUCCESS {
        Some(data)
    } else {
        None
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn schedule_shutdown() -> std::result::Result<(), String> {
    #[cfg(windows)]
    {
        let status = std::process::Command::new("shutdown")
            .args(["/s", "/t", "60", "/c", "泥洞的下载已经完成，一分钟后关机"])
            .status()
            .map_err(|_| "没有发出关机。".to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err("没有发出关机。".into())
        }
    }
    #[cfg(not(windows))]
    {
        Err("只有 Windows 会在下载完成后关机。".into())
    }
}

fn abort_shutdown() {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("shutdown").args(["/a"]).status();
    }
}

async fn run_downloads(hub: &FetchHub) {
    if hub.all_done() {
        hub.finish_success();
        return;
    }
    let count = hub.file_count();
    for index in 0..count {
        if hub.cancelled() {
            hub.finish_cancel();
            return;
        }
        if hub.file_phase(index) == "done" {
            continue;
        }
        hub.mark_running(index);
        match download_index(hub, index).await {
            Ok(()) => hub.mark_done(index),
            Err(true) => {
                hub.finish_cancel();
                return;
            }
            Err(false) => {}
        }
    }
    if hub.cancelled() {
        hub.finish_cancel();
        return;
    }
    hub.finish_success();
}

/// `Err(true)` 是取消，`Err(false)` 是这一条失败、已经记在文件上。
async fn download_index(hub: &FetchHub, index: usize) -> std::result::Result<(), bool> {
    let mut attempt = 0u32;
    let mut marked = 0u64;
    loop {
        if hub.cancelled() {
            return Err(true);
        }
        match try_download(hub, index).await {
            Attempt::Done => return Ok(()),
            Attempt::Cancel => return Err(true),
            Attempt::Fatal(message) => {
                hub.mark_failed(index, &message);
                return Err(false);
            }
            Attempt::Retry(message) => {
                let received = current_received(hub, index);
                if received > marked {
                    marked = received;
                    attempt = 0;
                }
                attempt += 1;
                if attempt >= ATTEMPTS {
                    hub.mark_failed(index, &message);
                    return Err(false);
                }
                if !sleep_cancel(hub, 2).await {
                    return Err(true);
                }
            }
        }
    }
}

fn current_received(hub: &FetchHub, index: usize) -> u64 {
    hub.inner
        .lock()
        .unwrap()
        .store
        .job
        .as_ref()
        .and_then(|job| job.files.get(index))
        .map(|file| file.received)
        .unwrap_or(0)
}

async fn sleep_cancel(hub: &FetchHub, seconds: u64) -> bool {
    for _ in 0..(seconds * 5) {
        if hub.cancelled() {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    !hub.cancelled()
}

async fn try_download(hub: &FetchHub, index: usize) -> Attempt {
    let Some(spec) = hub.file_spec(index) else {
        return Attempt::Cancel;
    };
    let url = match Url::parse(&spec.url) {
        Ok(url) => url,
        Err(_) => return Attempt::Fatal("这条链接没法用。".into()),
    };
    let finished = spec.dir.join(&spec.name);
    let partial_now = spec.dir.join(format!("{}.partial", spec.name));
    if finished.is_file() && !partial_now.exists() {
        let len = file_len(&finished).await;
        if !hub.set_received(index, len, Some(len), true) {
            return Attempt::Cancel;
        }
        return Attempt::Done;
    }
    let mut force_full = false;
    for _pass in 0..2 {
        if hub.cancelled() {
            return Attempt::Cancel;
        }
        let partial = spec.dir.join(format!("{}.partial", spec.name));
        let dest = spec.dir.join(&spec.name);
        let offset = if force_full { 0 } else { file_len(&partial).await };
        if offset > 0 && !hub.set_received(index, offset, spec.total, true) {
            return Attempt::Cancel;
        }
        let (client, used_proxy) = match build_client(spec.use_proxy, &url) {
            Ok(client) => client,
            Err(message) => return Attempt::Fatal(message),
        };
        let mut request = client.get(url.clone());
        if offset > 0 && !force_full {
            request = request.header("Range", format!("bytes={offset}-"));
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => return Attempt::Retry(net_error(&error, used_proxy)),
        };
        let status = response.status().as_u16();
        if status == 416 {
            if let Some(total) = spec.total {
                if offset >= total && offset > 0 {
                    return finalize_partial(&partial, &dest).await;
                }
            }
            return Attempt::Fatal("服务器不能从中断的位置继续。".into());
        }
        if !(200..300).contains(&status) {
            let message = http_failure(status);
            if status == 408 || status == 429 || status >= 500 {
                return Attempt::Retry(message);
            }
            return Attempt::Fatal(message);
        }
        if status == 206 {
            if let Some(start) = response
                .headers()
                .get("content-range")
                .and_then(|value| value.to_str().ok())
                .and_then(range_start)
            {
                if start != offset {
                    drop(response);
                    let _ = tokio::fs::remove_file(&partial).await;
                    let _ = std::fs::remove_file(sidecar_path(&partial));
                    force_full = true;
                    continue;
                }
            }
        }
        let total = total_of(&response, status, offset).or(spec.total);
        let mode = if status == 200 && offset > 0 {
            WriteMode::Overwrite
        } else if offset > 0 {
            WriteMode::Append
        } else {
            WriteMode::Overwrite
        };
        return write_body(hub, index, response, spec, partial, dest, total, offset, mode, used_proxy).await;
    }
    Attempt::Fatal("续传没有对上，已停止。".into())
}

enum WriteMode {
    Append,
    Overwrite,
}

async fn write_body(
    hub: &FetchHub,
    index: usize,
    mut response: reqwest::Response,
    spec: FileSpec,
    mut partial: PathBuf,
    mut dest: PathBuf,
    total: Option<u64>,
    offset: u64,
    mode: WriteMode,
    used_proxy: bool,
) -> Attempt {
    let mut name = spec.name.clone();
    let start_at = match mode {
        WriteMode::Append => offset,
        WriteMode::Overwrite => 0,
    };
    if start_at == 0 {
        if let Some(better) = response
            .headers()
            .get("content-disposition")
            .and_then(|value| value.to_str().ok())
            .and_then(filename_from_disposition)
        {
            if better != name {
                let (adopted, adopted_partial) = allocate_paths(&spec.dir, &better, &spec.url);
                name = adopted;
                partial = adopted_partial;
                dest = spec.dir.join(&name);
                hub.rename_file(index, name);
            }
        }
    }
    if let Err(message) = std::fs::write(sidecar_path(&partial), &spec.url) {
        if disk_full(&message) {
            return Attempt::Fatal("磁盘空间不够。".into());
        }
    }
    let opened = match mode {
        WriteMode::Append => tokio::fs::OpenOptions::new().create(true).append(true).open(&partial).await,
        WriteMode::Overwrite => {
            tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&partial)
                .await
        }
    };
    let mut file = match opened {
        Ok(file) => file,
        Err(error) => return io_attempt(error),
    };
    let mut received = start_at;
    let mut since_sync = 0u64;
    if !hub.set_received(index, received, total, true) {
        return Attempt::Cancel;
    }
    loop {
        if hub.cancelled() {
            let _ = file.sync_all().await;
            return Attempt::Cancel;
        }
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if let Err(error) = file.write_all(&chunk).await {
                    let _ = file.sync_all().await;
                    return io_attempt(error);
                }
                received += chunk.len() as u64;
                since_sync += chunk.len() as u64;
                let persist_now = since_sync >= SYNC_EVERY;
                if persist_now {
                    if let Err(error) = file.sync_all().await {
                        return io_attempt(error);
                    }
                    since_sync = 0;
                }
                if !hub.set_received(index, received, total, persist_now) {
                    let _ = file.sync_all().await;
                    return Attempt::Cancel;
                }
            }
            Ok(None) => break,
            Err(error) => {
                let _ = file.sync_all().await;
                let _ = hub.set_received(index, received, total, true);
                return Attempt::Retry(net_error(&error, used_proxy));
            }
        }
    }
    if let Err(error) = file.sync_all().await {
        return io_attempt(error);
    }
    drop(file);
    if let Some(expected) = total {
        if received < expected {
            let _ = hub.set_received(index, received, total, true);
            return Attempt::Retry("连接在文件下完前断了。".into());
        }
    }
    finalize_partial(&partial, &dest).await
}

async fn finalize_partial(partial: &Path, dest: &Path) -> Attempt {
    if dest.exists() {
        return Attempt::Fatal("目标文件已经存在。".into());
    }
    match std::fs::rename(partial, dest) {
        Ok(()) => {
            let _ = std::fs::remove_file(sidecar_path(partial));
            Attempt::Done
        }
        Err(_) => Attempt::Fatal("保存文件失败。".into()),
    }
}

fn total_of(response: &reqwest::Response, status: u16, offset: u64) -> Option<u64> {
    if status == 206 {
        if let Some(total) = response
            .headers()
            .get("content-range")
            .and_then(|value| value.to_str().ok())
            .and_then(total_from_content_range)
        {
            return Some(total);
        }
        return header_u64(response, "content-length").map(|len| offset + len);
    }
    header_u64(response, "content-length")
}

fn header_u64(response: &reqwest::Response, name: &str) -> Option<u64> {
    response.headers().get(name)?.to_str().ok()?.parse().ok()
}

fn total_from_content_range(value: &str) -> Option<u64> {
    let (_, total) = value.trim().rsplit_once('/')?;
    let total = total.trim();
    if total == "*" {
        return None;
    }
    total.parse().ok()
}

fn range_start(value: &str) -> Option<u64> {
    let rest = value.trim().get("bytes ".len()..)?;
    if !value.trim().to_ascii_lowercase().starts_with("bytes ") {
        return None;
    }
    let (range, _) = rest.split_once('/')?;
    let (start, _) = range.split_once('-')?;
    start.trim().parse().ok()
}

async fn file_len(path: &Path) -> u64 {
    tokio::fs::metadata(path).await.map(|meta| meta.len()).unwrap_or(0)
}

fn build_client(use_proxy: bool, url: &Url) -> std::result::Result<(reqwest::Client, bool), String> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(45))
        .redirect(reqwest::redirect::Policy::limited(10))
        .user_agent(format!(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 nidong/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .no_gzip();
    let mut used = false;
    if use_proxy {
        if let Some(proxy) = choose_proxy(url) {
            let proxy = reqwest::Proxy::all(&proxy).map_err(|_| "系统代理地址读不懂。".to_string())?;
            builder = builder.proxy(proxy);
            used = true;
        } else {
            builder = builder.no_proxy();
        }
    } else {
        builder = builder.no_proxy();
    }
    match builder.build() {
        Ok(client) => Ok((client, used)),
        Err(_) => Err("下载客户端没有建起来。".into()),
    }
}

fn net_error(error: &reqwest::Error, used_proxy: bool) -> String {
    let raw = error.to_string();
    let refused = raw.contains("10061")
        || raw.contains("积极拒绝")
        || raw.to_ascii_lowercase().contains("connection refused");
    if used_proxy && (refused || raw.to_ascii_lowercase().contains("proxy") || error.is_connect()) {
        return "系统代理连不上。请确认代理软件已经打开，或关掉「使用系统代理」。".into();
    }
    if error.is_timeout() || raw.to_ascii_lowercase().contains("timed out") {
        return "连接超时。可以再试一次；校外网站可能要先开代理。".into();
    }
    if error.is_connect() {
        return if used_proxy {
            "连不上。请确认代理软件开着。".into()
        } else {
            "连不上。校外链接可能要打开「使用系统代理」，并让代理软件接管系统代理。".into()
        };
    }
    format!("下载失败：{raw}")
}

fn http_failure(status: u16) -> String {
    match status {
        401 | 403 => format!("服务器拒绝了（{status}）。链接可能要登录，或需要换网络。"),
        404 => "链接上没有这个文件（404）。".into(),
        _ => format!("服务器返回 {status}。"),
    }
}

fn io_attempt(error: std::io::Error) -> Attempt {
    if disk_full_io(&error) {
        Attempt::Fatal("磁盘空间不够。".into())
    } else {
        Attempt::Retry("写入文件失败。".into())
    }
}

fn disk_full(error: &std::io::Error) -> bool {
    disk_full_io(error)
}

fn disk_full_io(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(112) | Some(39))
}

#[tauri::command]
pub fn fetch_status(hub: tauri::State<'_, FetchHub>) -> FetchStatus {
    hub.status()
}

#[tauri::command]
pub fn fetch_arm(
    hub: tauri::State<'_, FetchHub>,
    urls: String,
    time: String,
    dir: String,
    shutdown: bool,
    use_proxy: bool,
) -> Result<FetchStatus> {
    hub.arm(urls, time, dir, shutdown, use_proxy)
}

#[tauri::command]
pub fn fetch_cancel(hub: tauri::State<'_, FetchHub>) -> FetchStatus {
    hub.cancel()
}

#[tauri::command]
pub fn fetch_resume(hub: tauri::State<'_, FetchHub>) -> Result<FetchStatus> {
    hub.resume()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    #[test]
    fn next_midnight_and_later_today() {
        assert_eq!(
            next_clock(at("2026-11-08 22:00:00"), 0, 0),
            Some(at("2026-11-09 00:00:00"))
        );
        assert_eq!(
            next_clock(at("2026-11-09 00:00:00"), 0, 0),
            Some(at("2026-11-10 00:00:00"))
        );
        assert_eq!(
            next_clock(at("2026-11-09 03:00:00"), 4, 30),
            Some(at("2026-11-09 04:30:00"))
        );
        assert_eq!(
            next_clock(at("2026-11-09 05:00:00"), 3, 0),
            Some(at("2026-11-10 03:00:00"))
        );
        assert_eq!(
            next_clock(at("2026-12-31 23:00:00"), 0, 0),
            Some(at("2027-01-01 00:00:00"))
        );
    }

    #[test]
    fn free_window_catch_up_stops_at_six() {
        let when = at("2026-11-09 00:00:00");
        assert_eq!(should_start(when, at("2026-11-08 23:00:00")), WhenDue::Wait);
        assert_eq!(should_start(when, at("2026-11-09 00:00:01")), WhenDue::Start);
        assert_eq!(should_start(when, at("2026-11-09 05:59:00")), WhenDue::Start);
        assert_eq!(should_start(when, at("2026-11-09 06:00:00")), WhenDue::Missed);
        assert_eq!(should_start(when, at("2026-11-10 02:00:00")), WhenDue::Missed);
        assert_eq!(
            should_start(at("2026-11-09 05:50:00"), at("2026-11-09 05:59:00")),
            WhenDue::Start
        );
        assert_eq!(
            should_start(at("2026-11-09 05:50:00"), at("2026-11-09 06:00:00")),
            WhenDue::Missed
        );
    }

    #[test]
    fn billed_hour_only_waits_twenty_minutes() {
        let when = at("2026-11-09 15:00:00");
        assert_eq!(should_start(when, at("2026-11-09 14:59:00")), WhenDue::Wait);
        assert_eq!(should_start(when, at("2026-11-09 15:20:00")), WhenDue::Start);
        assert_eq!(should_start(when, at("2026-11-09 15:20:01")), WhenDue::Missed);
    }

    #[test]
    fn break_resume_stays_inside_the_same_free_night() {
        let when = at("2026-11-09 00:00:00");
        let touched = at("2026-11-09 01:00:00");
        assert_eq!(
            judge("running", when, touched, at("2026-11-09 04:30:00"), false),
            Judge::Run
        );
        assert_eq!(
            judge("running", when, touched, at("2026-11-09 08:00:00"), false),
            Judge::Interrupted
        );
        assert_eq!(
            judge("running", when, touched, at("2026-11-10 02:00:00"), false),
            Judge::Interrupted
        );
        assert_eq!(
            judge("armed", when, touched, at("2026-11-10 02:00:00"), false),
            Judge::Missed
        );
        assert_eq!(
            judge(
                "running",
                at("2026-11-09 15:00:00"),
                at("2026-11-09 15:05:00"),
                at("2026-11-09 15:10:00"),
                false
            ),
            Judge::Run
        );
        assert_eq!(
            judge(
                "running",
                at("2026-11-09 15:00:00"),
                at("2026-11-09 15:05:00"),
                at("2026-11-09 18:00:00"),
                false
            ),
            Judge::Interrupted
        );
        assert_eq!(
            judge("running", when, touched, at("2026-11-09 01:10:00"), true),
            Judge::Wait
        );
        assert_eq!(judge("done", when, touched, at("2026-11-09 04:00:00"), false), Judge::Wait);
    }

    #[test]
    fn filenames_drop_path_and_reserved_words() {
        assert_eq!(sanitize_filename("a/b.zip"), "b.zip");
        assert_eq!(sanitize_filename("foo:bar"), "foo_bar");
        assert_eq!(sanitize_filename("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_filename(".."), "download.bin");
        assert_eq!(sanitize_filename("你好.zip"), "你好.zip");
        assert_eq!(sanitize_filename("foo."), "foo");
    }

    #[test]
    fn disposition_prefers_the_utf8_name() {
        assert_eq!(
            filename_from_disposition("attachment; filename=\"a.zip\"").as_deref(),
            Some("a.zip")
        );
        assert_eq!(
            filename_from_disposition("attachment; filename*=UTF-8''%E4%BD%A0%E5%A5%BD.zip").as_deref(),
            Some("你好.zip")
        );
    }

    #[test]
    fn proxy_server_and_bypass() {
        let both = parse_proxy_server("127.0.0.1:7890");
        assert_eq!(both.http.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(both.https.as_deref(), Some("http://127.0.0.1:7890"));
        let split = parse_proxy_server("http=127.0.0.1:7890;https=127.0.0.1:7891");
        assert_eq!(split.http.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(split.https.as_deref(), Some("http://127.0.0.1:7891"));
        let socks = parse_proxy_server("socks=127.0.0.1:7890");
        assert!(socks.http.is_none());
        assert!(socks.socks);
        assert!(host_bypassed("localhost", &[]));
        assert!(host_bypassed("printer", &parse_bypass("<local>")));
        assert!(!host_bypassed("github.com", &parse_bypass("<local>")));
        assert!(host_bypassed("10.1.2.3", &parse_bypass("10.*")));
        assert!(!host_bypassed("11.0.0.1", &parse_bypass("10.*")));
        assert!(host_bypassed("a.neu.edu.cn", &parse_bypass("*.neu.edu.cn")));
    }

    #[test]
    fn partial_reuses_the_same_url_only() {
        let dir = std::env::temp_dir().join(format!("nidong-fetch-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let partial = dir.join("a.zip.partial");
        std::fs::write(&partial, b"x").unwrap();
        std::fs::write(sidecar_path(&partial), "https://example.com/a.zip").unwrap();
        let (name, path) = allocate_paths(&dir, "a.zip", "https://example.com/a.zip");
        assert_eq!(name, "a.zip");
        assert_eq!(path, partial);
        let (other, _) = allocate_paths(&dir, "a.zip", "https://example.com/b.zip");
        assert_eq!(other, "a (2).zip");
        std::fs::write(dir.join("c.zip"), b"done").unwrap();
        let (third, _) = allocate_paths(&dir, "c.zip", "https://example.com/c.zip");
        assert_eq!(third, "c (2).zip");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn links_reject_other_schemes_and_blanks() {
        assert!(split_urls("   \n").is_err());
        assert!(split_urls("ftp://example.com/a").is_err());
        let urls = split_urls("https://example.com/a.zip\n\nhttps://example.com/a.zip\nhttp://10.0.0.8/b").unwrap();
        assert_eq!(urls.len(), 2);
    }

    #[test]
    fn content_range_total() {
        assert_eq!(total_from_content_range("bytes 0-1/123"), Some(123));
        assert_eq!(total_from_content_range("bytes 0-1/*"), None);
        assert_eq!(range_start("bytes 40-99/123"), Some(40));
    }
}
