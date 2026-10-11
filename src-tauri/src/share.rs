//! 校园网文件分享。发送方在本机开网页，接收方用浏览器输入口令后下载。
//!
//! 每一路下载单独打开文件，按 64KB 送出，不把整个文件读进内存。
//! 同时下载最多 128 路，一百人同时下不会把进程撑满；速度由这台电脑的无线上传决定。

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_stream::stream;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Form, Router};
use base64::Engine;
use rand::rngs::OsRng;
use rand::{Rng, RngCore};
use serde::Serialize;
use subtle::ConstantTimeEq;
use tokio::io::AsyncReadExt;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use tauri::{AppHandle, Emitter};

use crate::error::{AppError, Result};

const PORT: u16 = 38721;
const MAX_FILES: usize = 100;
const MAX_DOWNLOADS: usize = 128;
const CODE_LEN: usize = 8;
const FAIL_LIMIT: u8 = 5;
const LOCK: Duration = Duration::from_secs(60);
const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const GITHUB_REPO: &str = "https://github.com/sapphire-xu/nidong";
const GITEE_REPO: &str = "https://gitee.com/sapphire_fic/nidong";
/// 和 tauri.conf.json 里 plugins.updater.endpoints 是同一个清单。安装包地址从清单读，不按版本拼。
const UPDATE_MANIFEST: &str = "https://gitee.com/sapphire_fic/nidong/raw/master/updates/latest.json";
const COOKIE: &str = "nidong_share";

#[derive(Clone)]
struct Promo {
    version: String,
    href: String,
}

impl Promo {
    fn fallback() -> Self {
        Self {
            version: String::new(),
            href: GITEE_REPO.into(),
        }
    }
}

fn trusted_gitee(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    parsed.scheme() == "https"
        && parsed.host_str() == Some("gitee.com")
        && parsed.path().starts_with("/sapphire_fic/nidong/")
}

fn promo_from_manifest(body: &str) -> Promo {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return Promo::fallback();
    };
    let version = value
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let href = value
        .pointer("/platforms/windows-x86_64/url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if version.is_empty() || !trusted_gitee(&href) {
        return Promo::fallback();
    }
    Promo { version, href }
}

async fn fetch_promo() -> Promo {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    else {
        return Promo::fallback();
    };
    let Ok(res) = client.get(UPDATE_MANIFEST).send().await else {
        return Promo::fallback();
    };
    let Ok(text) = res.text().await else {
        return Promo::fallback();
    };
    promo_from_manifest(&text)
}

fn icon_uri() -> String {
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(include_bytes!("../icons/256x256.png"))
    )
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Picked {
    pub id: u64,
    pub path: String,
    pub name: String,
    pub bytes: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeOut {
    pub files: Vec<Picked>,
    pub note: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareOffer {
    pub url: String,
    pub code: String,
    pub host: String,
    pub port: u16,
    pub files: Vec<ShareFileInfo>,
    pub unlocks: u64,
    pub active_downloads: u32,
    pub last_peer: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareFileInfo {
    pub id: u64,
    pub path: String,
    pub name: String,
    pub bytes: u64,
}

#[derive(Default)]
pub struct ShareHub {
    inner: Mutex<Option<Live>>,
}

struct Live {
    state: Arc<ShareState>,
    bound: SocketAddr,
    display_code: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Live {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct ShareState {
    files: Mutex<Vec<Picked>>,
    next_id: AtomicU64,
    code_norm: String,
    sessions: Mutex<HashSet<String>>,
    fails: Mutex<HashMap<IpAddr, FailSlot>>,
    unlocks: AtomicU64,
    active: Arc<AtomicU32>,
    last_peer: Mutex<Option<String>>,
    slots: Arc<Semaphore>,
    promo: Promo,
}

struct FailSlot {
    count: u8,
    until: Option<Instant>,
}

enum Attempt {
    Mismatch,
    Wait,
}

struct DownloadGuard {
    active: Arc<AtomicU32>,
    _permit: OwnedSemaphorePermit,
}

impl DownloadGuard {
    fn new(active: Arc<AtomicU32>, permit: OwnedSemaphorePermit) -> Self {
        active.fetch_add(1, Ordering::Relaxed);
        Self { active, _permit: permit }
    }
}

impl Drop for DownloadGuard {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::Relaxed);
    }
}

#[derive(serde::Deserialize)]
struct UnlockForm {
    code: String,
}

pub(crate) fn choose_ip(ips: &[Ipv4Addr]) -> Option<Ipv4Addr> {
    ips.iter()
        .copied()
        .filter(|ip| usable(*ip))
        .min_by_key(|ip| (rank(*ip), *ip))
}

fn usable(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    if ip.is_loopback() || ip.is_unspecified() || ip.is_broadcast() {
        return false;
    }
    if a == 169 && b == 254 {
        return false;
    }
    rank(ip) < 9
}

fn rank(ip: Ipv4Addr) -> u8 {
    let [a, b, _, _] = ip.octets();
    if a == 172 && b == 22 {
        0
    } else if a == 10 {
        1
    } else if a == 192 && b == 168 {
        2
    } else if a == 172 && (16..32).contains(&b) {
        3
    } else {
        9
    }
}

fn host_ips() -> Vec<Ipv4Addr> {
    let Ok(list) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    list.into_iter()
        .filter_map(|iface| match iface.addr {
            if_addrs::IfAddr::V4(v4) => Some(v4.ip),
            _ => None,
        })
        .collect()
}

fn pick_ip() -> Result<Ipv4Addr> {
    choose_ip(&host_ips()).ok_or_else(|| "没有可用的局域网地址".into())
}

fn new_code() -> (String, String) {
    let mut rng = OsRng;
    let mut raw = String::with_capacity(CODE_LEN);
    for _ in 0..CODE_LEN {
        let i = rng.gen_range(0..ALPHABET.len());
        raw.push(ALPHABET[i] as char);
    }
    let display = format!("{}-{}", &raw[..4], &raw[4..]);
    (display, raw)
}

fn new_token() -> String {
    let mut buf = [0u8; 32];
    OsRng.fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

fn normalize_code(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| match c.to_ascii_uppercase() {
            'I' | 'L' => '1',
            'O' => '0',
            other => other,
        })
        .collect()
}

fn codes_match(given: &str, expect: &str) -> bool {
    let given = normalize_code(given);
    if given.len() != expect.len() {
        return false;
    }
    bool::from(given.as_bytes().ct_eq(expect.as_bytes()))
}

fn record_failure(slot: &mut FailSlot, now: Instant) -> Attempt {
    if let Some(until) = slot.until {
        if now < until {
            return Attempt::Wait;
        }
        slot.until = None;
        slot.count = 0;
    }
    slot.count = slot.count.saturating_add(1);
    if slot.count >= FAIL_LIMIT {
        slot.count = 0;
        slot.until = Some(now + LOCK);
        return Attempt::Wait;
    }
    Attempt::Mismatch
}

struct Classified {
    files: Vec<Picked>,
    saw_dir: bool,
    missing: bool,
}

fn classify(paths: &[String]) -> Classified {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let mut saw_dir = false;
    let mut missing = false;
    for raw in paths {
        let text = raw.trim();
        if text.is_empty() {
            continue;
        }
        let path = PathBuf::from(text);
        let meta = match std::fs::metadata(&path) {
            Ok(meta) => meta,
            Err(_) => {
                missing = true;
                continue;
            }
        };
        if meta.is_dir() {
            saw_dir = true;
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if !seen.insert(key) {
            continue;
        }
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "文件".into());
        files.push(Picked {
            id: 0,
            path: text.to_string(),
            name,
            bytes: meta.len(),
        });
    }
    Classified { files, saw_dir, missing }
}

fn describe_out(found: Classified) -> DescribeOut {
    let mut note = None;
    let mut files = found.files;
    if found.saw_dir {
        note = Some("请拖入文件".into());
    } else if found.missing && files.is_empty() {
        note = Some("找不到这个文件".into());
    }
    if files.len() > MAX_FILES {
        files.truncate(MAX_FILES);
        note = Some("一次最多分享 100 个文件".into());
    }
    DescribeOut { files, note }
}

/// 同名只留最后一份。已有的名字沿用编号，新名字领新编号。没出现的名字删掉。
fn merge_named(current: &[Picked], incoming: Vec<Picked>, next_id: &AtomicU64) -> Vec<Picked> {
    let mut folded: Vec<Picked> = Vec::new();
    for file in incoming {
        if let Some(slot) = folded.iter_mut().find(|item| item.name == file.name) {
            *slot = file;
        } else {
            folded.push(file);
        }
    }
    folded
        .into_iter()
        .map(|file| {
            if let Some(old) = current.iter().find(|item| item.name == file.name && item.id != 0) {
                Picked {
                    id: old.id,
                    path: file.path,
                    name: file.name,
                    bytes: file.bytes,
                }
            } else {
                Picked {
                    id: next_id.fetch_add(1, Ordering::Relaxed),
                    path: file.path,
                    name: file.name,
                    bytes: file.bytes,
                }
            }
        })
        .collect()
}

fn files_for_start(found: Classified) -> Result<Vec<Picked>> {
    if found.files.is_empty() && found.saw_dir {
        return Err("请拖入文件".into());
    }
    if found.files.is_empty() && found.missing {
        return Err("找不到这个文件".into());
    }
    if found.files.is_empty() {
        return Err("先选择文件".into());
    }
    if found.files.len() > MAX_FILES {
        return Err("一次最多分享 100 个文件".into());
    }
    Ok(found.files)
}

fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn size_text(n: u64) -> String {
    const K: f64 = 1024.0;
    let n = n as f64;
    if n < K {
        return format!("{n:.0} B");
    }
    if n < K * K {
        return format!("{:.1} KB", n / K);
    }
    if n < K * K * K {
        return format!("{:.1} MB", n / K / K);
    }
    format!("{:.2} GB", n / K / K / K)
}

fn footer(promo: &Promo) -> String {
    let version = if promo.version.is_empty() {
        String::new()
    } else {
        format!(" · {}", esc(&promo.version))
    };
    format!(
        r#"<footer>
  <p>技术支持：泥洞工具箱 · sapphire_fic</p>
  <p><a href="{href}">Gitee 下载泥洞</a>{version}</p>
  <p>GitHub 仓库<br><a href="{github}">{github}</a></p>
</footer>"#,
        href = esc(&promo.href),
        version = version,
        github = esc(GITHUB_REPO),
    )
}

fn document(main: &str, promo: &Promo) -> String {
    format!(
        r#"<!doctype html>
<html lang="zh-CN">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="referrer" content="no-referrer">
<title>泥洞文件</title>
<style>
  :root {{ color-scheme: light dark; }}
  body {{
    margin: 0;
    min-height: 100vh;
    display: grid;
    place-items: safe center;
    font: 16px/1.5 "Segoe UI", system-ui, sans-serif;
    background: #f4f1ea;
    color: #1c1915;
  }}
  main {{
    width: min(36rem, calc(100% - 40px));
    margin: 32px 0;
    text-align: center;
  }}
  .mark {{ width: 64px; height: 64px; display: block; margin: 0 auto 12px; }}
  h1 {{ font-size: 22px; font-weight: 650; margin: 0 0 8px; }}
  p {{ margin: 0 0 12px; }}
  form {{ display: flex; justify-content: center; gap: 8px; margin: 16px 0 8px; }}
  input {{ flex: 1; min-width: 0; max-width: 16rem; font: inherit; padding: 8px 10px; border: 1px solid #c8c0b4; border-radius: 8px; background: #fff; color: inherit; }}
  button, .dl {{ font: inherit; background: #1c1915; color: #fff; border: 0; border-radius: 8px; padding: 8px 14px; text-decoration: none; white-space: nowrap; }}
  .err {{ color: #9a3b12; }}
  ul {{ list-style: none; padding: 0; margin: 16px auto; }}
  li {{ display: flex; justify-content: center; align-items: center; flex-wrap: wrap; gap: 12px; padding: 12px 0; border-top: 1px solid #e4ddd2; }}
  .name {{ min-width: 0; overflow-wrap: anywhere; }}
  .size {{ color: #6b645c; font-size: 13px; white-space: nowrap; }}
  .note {{ color: #6b645c; font-size: 13px; }}
  footer {{ margin-top: 36px; padding-top: 16px; border-top: 1px solid #e4ddd2; color: #6b645c; font-size: 13px; overflow-wrap: anywhere; }}
  footer a {{ color: #1c1915; }}
  @media (max-width: 520px) {{
    form {{ flex-direction: column; align-items: center; }}
    input {{ width: 100%; max-width: none; }}
  }}
  @media (prefers-color-scheme: dark) {{
    body {{ background: #161412; color: #f3efe8; }}
    input {{ background: #221f1b; border-color: #3a342c; }}
    button, .dl {{ background: #f3efe8; color: #161412; }}
    .size, .note, footer {{ color: #b7aea3; }}
    li, footer {{ border-color: #3a342c; }}
    footer a {{ color: #f3efe8; }}
  }}
</style>
<main>
<img class="mark" alt="泥洞" width="64" height="64" src="{icon}">
{main}
{footer}
</main>
"#,
        icon = icon_uri(),
        main = main,
        footer = footer(promo),
    )
}

fn page_gate(message: Option<&str>, promo: &Promo) -> String {
    let err = match message {
        Some(text) => format!(r#"<p class="err">{}</p>"#, esc(text)),
        None => String::new(),
    };
    document(
        &format!(
            r#"<h1>泥洞文件</h1>
<p>输入口令后才能下载。口令不区分大小写。</p>
{err}
<form method="post" action="/">
  <input name="code" autocomplete="off" autocapitalize="characters" spellcheck="false" required>
  <button type="submit">进入</button>
</form>"#,
            err = err,
        ),
        promo,
    )
}

fn page_files(files: &[Picked], promo: &Promo) -> String {
    let mut list = String::new();
    if files.is_empty() {
        list.push_str("<p>这次没有文件</p>");
    } else {
        list.push_str("<ul>");
        for file in files {
            list.push_str(&format!(
                r#"<li><span class="name">{}</span><span class="size">{}</span><a class="dl" href="/f/{}">下载</a></li>"#,
                esc(&file.name),
                esc(&size_text(file.bytes)),
                file.id
            ));
        }
        list.push_str("</ul>");
    }
    document(
        &format!(
            r#"<h1>泥洞文件</h1>
<p>选择要下载的文件</p>
<p class="note">若发现文件不存在，请刷新本页面</p>
{list}"#,
            list = list,
        ),
        promo,
    )
}

fn html_page(status: StatusCode, body: String) -> Response {
    let mut res = (status, body).into_response();
    let headers = res.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

fn not_found() -> Response {
    let mut res = StatusCode::NOT_FOUND.into_response();
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in raw.split(';') {
        let mut kv = part.trim().splitn(2, '=');
        if kv.next() == Some(name) {
            return Some(kv.next().unwrap_or("").to_string());
        }
    }
    None
}

fn session_ok(state: &ShareState, headers: &HeaderMap) -> bool {
    let Some(token) = read_cookie(headers, COOKIE) else {
        return false;
    };
    state.sessions.lock().unwrap().contains(&token)
}

fn content_disposition(name: &str) -> HeaderValue {
    let ascii_ok = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii() && c != '"' && c != '\\' && !c.is_control());
    let fallback = if ascii_ok { name } else { "download" };
    let value = format!(
        "attachment; filename=\"{}\"; filename*=UTF-8''{}",
        fallback,
        urlencoding::encode(name)
    );
    HeaderValue::from_str(&value).unwrap_or(HeaderValue::from_static("attachment"))
}

fn router(state: Arc<ShareState>) -> Router {
    Router::new()
        .route("/", get(index).post(unlock))
        .route("/f/{id}", get(download))
        .with_state(state)
}

async fn index(State(state): State<Arc<ShareState>>, headers: HeaderMap) -> Response {
    if session_ok(&state, &headers) {
        let files = state.files.lock().unwrap().clone();
        html_page(StatusCode::OK, page_files(&files, &state.promo))
    } else {
        html_page(StatusCode::OK, page_gate(None, &state.promo))
    }
}

async fn unlock(
    State(state): State<Arc<ShareState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Form(form): Form<UnlockForm>,
) -> Response {
    let ip = peer.ip();
    let mut fails = state.fails.lock().unwrap();
    if fails.len() > 1024 {
        let now = Instant::now();
        fails.retain(|_, slot| slot.until.map(|t| t > now).unwrap_or(false) || slot.count > 0);
    }
    let slot = fails.entry(ip).or_insert(FailSlot { count: 0, until: None });
    if let Some(until) = slot.until {
        if Instant::now() < until {
            drop(fails);
            return html_page(
                StatusCode::OK,
                page_gate(Some("试太多次了，请等一分钟再试"), &state.promo),
            );
        }
    }
    if !codes_match(&form.code, &state.code_norm) {
        let attempt = record_failure(slot, Instant::now());
        drop(fails);
        let text = match attempt {
            Attempt::Wait => "试太多次了，请等一分钟再试",
            Attempt::Mismatch => "口令不对",
        };
        return html_page(StatusCode::OK, page_gate(Some(text), &state.promo));
    }
    fails.remove(&ip);
    drop(fails);
    let token = new_token();
    state.sessions.lock().unwrap().insert(token.clone());
    state.unlocks.fetch_add(1, Ordering::Relaxed);
    *state.last_peer.lock().unwrap() = Some(ip.to_string());
    let mut res = StatusCode::SEE_OTHER.into_response();
    let headers = res.headers_mut();
    headers.insert(header::LOCATION, HeaderValue::from_static("/"));
    headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!("{COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/"))
            .unwrap_or(HeaderValue::from_static("nidong_share=; HttpOnly; Path=/")),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

async fn download(
    State(state): State<Arc<ShareState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> Response {
    if !session_ok(&state, &headers) {
        return not_found();
    }
    let Some(file) = state.files.lock().unwrap().iter().find(|file| file.id == id).cloned() else {
        return not_found();
    };
    let permit = match Arc::clone(&state.slots).try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return html_page(
                StatusCode::SERVICE_UNAVAILABLE,
                document(
                    "<h1>泥洞文件</h1><p>现在下载的人太多，请稍后再试。</p>",
                    &state.promo,
                ),
            );
        }
    };
    let guard = DownloadGuard::new(Arc::clone(&state.active), permit);
    let mut opened = match tokio::fs::File::open(&file.path).await {
        Ok(file) => file,
        Err(_) => return not_found(),
    };
    *state.last_peer.lock().unwrap() = Some(peer.ip().to_string());
    let len = file.bytes;
    let disposition = content_disposition(&file.name);
    let body = stream! {
        let _guard = guard;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match opened.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => yield Ok::<Bytes, std::io::Error>(Bytes::copy_from_slice(&buf[..n])),
                Err(err) => {
                    yield Err(err);
                    break;
                }
            }
        }
    };
    let mut res = axum::body::Body::from_stream(body).into_response();
    let headers = res.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::CONTENT_DISPOSITION, disposition);
    if let Ok(value) = HeaderValue::from_str(&len.to_string()) {
        headers.insert(header::CONTENT_LENGTH, value);
    }
    res
}

fn open_socket(addr: SocketAddr) -> std::io::Result<tokio::net::TcpListener> {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    socket.set_reuse_address(true)?;
    socket.bind(&socket2::SockAddr::from(addr))?;
    socket.listen(256)?;
    socket.set_nonblocking(true)?;
    let std_listener: std::net::TcpListener = socket.into();
    tokio::net::TcpListener::from_std(std_listener)
}

async fn bind_listener(addr: SocketAddr) -> Result<tokio::net::TcpListener> {
    let mut last_in_use = false;
    let mut last_msg = String::new();
    for attempt in 0..20 {
        match open_socket(addr) {
            Ok(listener) => return Ok(listener),
            Err(err) => {
                last_in_use = err.kind() == std::io::ErrorKind::AddrInUse;
                last_msg = err.to_string();
                if !last_in_use || attempt + 1 == 20 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
    if last_in_use && addr.port() == PORT {
        Err("38721 已被占用".into())
    } else if last_in_use {
        Err(format!("端口 {} 已被占用", addr.port()).into())
    } else {
        Err(format!("开不了分享：{last_msg}").into())
    }
}

async fn launch(
    addr: SocketAddr,
    files: Vec<Picked>,
    display_code: String,
    code_norm: String,
    promo: Promo,
) -> Result<Live> {
    let listener = bind_listener(addr).await?;
    let bound = listener
        .local_addr()
        .map_err(|err| AppError::Msg(format!("开不了分享：{err}")))?;
    let next_id = AtomicU64::new(1);
    let files = merge_named(&[], files, &next_id);
    let state = Arc::new(ShareState {
        files: Mutex::new(files),
        next_id,
        code_norm,
        sessions: Mutex::new(HashSet::new()),
        fails: Mutex::new(HashMap::new()),
        unlocks: AtomicU64::new(0),
        active: Arc::new(AtomicU32::new(0)),
        last_peer: Mutex::new(None),
        slots: Arc::new(Semaphore::new(MAX_DOWNLOADS)),
        promo,
    });
    let app = router(Arc::clone(&state)).into_make_service_with_connect_info::<SocketAddr>();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok(Live {
        state,
        bound,
        display_code,
        task,
    })
}

impl Live {
    fn offer(&self) -> ShareOffer {
        ShareOffer {
            url: format!("http://{}/", self.bound),
            code: self.display_code.clone(),
            host: self.bound.ip().to_string(),
            port: self.bound.port(),
            files: self
                .state
                .files
                .lock()
                .unwrap()
                .iter()
                .map(|file| ShareFileInfo {
                    id: file.id,
                    path: file.path.clone(),
                    name: file.name.clone(),
                    bytes: file.bytes,
                })
                .collect(),
            unlocks: self.state.unlocks.load(Ordering::Relaxed),
            active_downloads: self.state.active.load(Ordering::Relaxed),
            last_peer: self.state.last_peer.lock().unwrap().clone(),
        }
    }
}

impl ShareHub {
    fn stop(&self) {
        self.inner.lock().unwrap().take();
    }

    fn offer(&self) -> Option<ShareOffer> {
        self.inner.lock().unwrap().as_ref().map(Live::offer)
    }

    async fn start(&self, paths: Vec<String>) -> Result<ShareOffer> {
        let files = files_for_start(classify(&paths))?;
        let ip = pick_ip()?;
        let (display, norm) = new_code();
        self.stop();
        let promo = fetch_promo().await;
        let live = launch(SocketAddr::from((ip, PORT)), files, display, norm, promo).await?;
        let offer = live.offer();
        *self.inner.lock().unwrap() = Some(live);
        Ok(offer)
    }

    fn revise(&self, paths: Vec<String>) -> Result<ShareOffer> {
        let found = classify(&paths);
        if found.saw_dir && found.files.is_empty() {
            return Err("请拖入文件".into());
        }
        if found.files.len() > MAX_FILES {
            return Err("一次最多分享 100 个文件".into());
        }
        let guard = self.inner.lock().unwrap();
        let live = guard.as_ref().ok_or("还没有开始分享")?;
        {
            let mut files = live.state.files.lock().unwrap();
            let merged = merge_named(&files, found.files, &live.state.next_id);
            *files = merged;
        }
        Ok(live.offer())
    }
}

#[tauri::command]
pub fn share_describe(paths: Vec<String>) -> DescribeOut {
    describe_out(classify(&paths))
}

#[tauri::command]
pub async fn share_start(
    app: AppHandle,
    hub: tauri::State<'_, ShareHub>,
    paths: Vec<String>,
) -> Result<ShareOffer> {
    let offer = hub.start(paths).await?;
    let _ = app.emit("share-changed", true);
    Ok(offer)
}

#[tauri::command]
pub fn share_stop(app: AppHandle, hub: tauri::State<'_, ShareHub>) -> Result<()> {
    hub.stop();
    let _ = app.emit("share-changed", false);
    Ok(())
}

#[tauri::command]
pub fn share_offer(hub: tauri::State<'_, ShareHub>) -> Option<ShareOffer> {
    hub.offer()
}

#[tauri::command]
pub fn share_revise(hub: tauri::State<'_, ShareHub>, paths: Vec<String>) -> Result<ShareOffer> {
    hub.revise(paths)
}

/// 和校园网自动登录同一套：看当前无线名，不探测教务或一卡通。热点也能打开教务。
#[tauri::command]
pub async fn share_on_campus(state: tauri::State<'_, crate::AppState>) -> Result<bool> {
    let configured = {
        let settings = state.settings.lock().unwrap();
        let name = settings.campus_wifi_ssid.trim();
        if name.is_empty() {
            "NEU".to_string()
        } else {
            name.to_string()
        }
    };
    let now = tokio::task::spawn_blocking(crate::wlan::query)
        .await
        .ok()
        .and_then(|result| result.ok());
    Ok(match now {
        Some(wlan) if wlan.connected => crate::campus::is_campus_net(&wlan.ssid, &configured),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process;

    fn sample_promo() -> Promo {
        Promo {
            version: "9.9.9".into(),
            href: "https://gitee.com/sapphire_fic/nidong/raw/master/updates/nidong_9.9.9_x64-setup.exe"
                .into(),
        }
    }

    #[test]
    fn chooses_campus_address_before_other_private_nets() {
        let ips = [
            Ipv4Addr::new(192, 168, 1, 8),
            Ipv4Addr::new(10, 0, 0, 4),
            Ipv4Addr::new(172, 22, 12, 148),
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(8, 8, 8, 8),
        ];
        assert_eq!(choose_ip(&ips), Some(Ipv4Addr::new(172, 22, 12, 148)));
        assert_eq!(
            choose_ip(&[Ipv4Addr::new(192, 168, 0, 2), Ipv4Addr::new(10, 1, 1, 1)]),
            Some(Ipv4Addr::new(10, 1, 1, 1))
        );
        assert_eq!(choose_ip(&[Ipv4Addr::new(1, 1, 1, 1)]), None);
    }

    #[test]
    fn spoken_code_ignores_case_dash_and_lookalikes() {
        assert_eq!(normalize_code("k7m9-q2px"), "K7M9Q2PX");
        assert_eq!(normalize_code("k7m9 q2po"), "K7M9Q2P0");
        assert!(codes_match("k7m9-q2px", "K7M9Q2PX"));
        assert!(!codes_match("k7m9-q2py", "K7M9Q2PX"));
        assert!(!codes_match("short", "K7M9Q2PX"));
    }

    #[test]
    fn fifth_wrong_code_makes_the_address_wait() {
        let mut slot = FailSlot { count: 0, until: None };
        let start = Instant::now();
        for _ in 0..4 {
            assert!(matches!(record_failure(&mut slot, start), Attempt::Mismatch));
        }
        assert!(matches!(record_failure(&mut slot, start), Attempt::Wait));
        assert!(matches!(
            record_failure(&mut slot, start + Duration::from_secs(10)),
            Attempt::Wait
        ));
        assert!(matches!(
            record_failure(&mut slot, start + Duration::from_secs(61)),
            Attempt::Mismatch
        ));
    }

    #[test]
    fn filenames_are_escaped() {
        let page = page_files(
            &[Picked {
                id: 5,
                path: "x".into(),
                name: "报告<script>.txt".into(),
                bytes: 3,
            }],
            &sample_promo(),
        );
        assert!(page.contains("报告&lt;script&gt;.txt"));
        assert!(!page.contains("<script>"));
        assert!(page.contains("技术支持：泥洞工具箱"));
        assert!(page.contains("sapphire_fic"));
        assert!(page.contains(GITHUB_REPO));
        assert!(page.contains("Gitee 下载泥洞"));
        assert!(page.contains("9.9.9"));
        assert!(page.contains("nidong_9.9.9_x64-setup.exe"));
        assert!(!page.contains(&format!(
            "nidong_{}_x64-setup.exe",
            env!("CARGO_PKG_VERSION")
        )));
        assert!(page.contains("href=\"/f/5\""));
        assert!(!page.contains("href=\"/f/0\""));
        assert!(page.contains("class=\"mark\""));
        let icon = icon_uri();
        let sharp = base64::engine::general_purpose::STANDARD
            .encode(include_bytes!("../icons/256x256.png"));
        let soft = base64::engine::general_purpose::STANDARD
            .encode(include_bytes!("../icons/64x64.png"));
        assert!(icon.contains(&sharp));
        assert!(!icon.contains(&soft));
        assert!(page.contains("place-items: safe center"));
        assert!(page.contains("data:image/png;base64,"));
        assert!(page.contains("选择要下载的文件"));
        assert!(!page.contains("选择要下载的文件。"));
        assert!(page.contains("若发现文件不存在，请刷新本页面"));
        assert!(!page.contains("若发现文件不存在，请刷新本页面。"));
        assert!(!page.contains("这次没有文件"));
        let empty = page_files(&[], &sample_promo());
        assert!(empty.contains(">这次没有文件</p>"));
        assert!(!empty.contains("这次没有文件。"));
        assert!(empty.contains("若发现文件不存在，请刷新本页面"));
    }

    #[test]
    fn manifest_version_is_not_baked_from_the_crate() {
        let promo = promo_from_manifest(
            r#"{"version":"9.9.9","platforms":{"windows-x86_64":{"url":"https://gitee.com/sapphire_fic/nidong/raw/master/updates/nidong_9.9.9_x64-setup.exe"}}}"#,
        );
        assert_eq!(promo.version, "9.9.9");
        let page = page_gate(None, &promo);
        assert!(page.contains(">Gitee 下载泥洞</a> · 9.9.9"));
        let broken = promo_from_manifest("not json");
        assert!(broken.version.is_empty());
        let plain = page_gate(None, &broken);
        assert!(plain.contains("Gitee 下载泥洞"));
        assert!(plain.contains(GITEE_REPO));
        assert!(!plain.contains(".exe"));
        let foreign = promo_from_manifest(
            r#"{"version":"1.0.0","platforms":{"windows-x86_64":{"url":"https://evil.example/nidong.exe"}}}"#,
        );
        assert!(foreign.version.is_empty());
        assert_eq!(foreign.href, GITEE_REPO);
    }

    #[test]
    fn same_name_keeps_its_number_and_the_newer_file_wins() {
        let next = AtomicU64::new(3);
        let current = vec![
            Picked {
                id: 1,
                path: "old-a".into(),
                name: "a.txt".into(),
                bytes: 1,
            },
            Picked {
                id: 2,
                path: "old-b".into(),
                name: "b.txt".into(),
                bytes: 2,
            },
        ];
        let incoming = vec![
            Picked {
                id: 0,
                path: "new-b".into(),
                name: "b.txt".into(),
                bytes: 8,
            },
            Picked {
                id: 0,
                path: "first-a".into(),
                name: "a.txt".into(),
                bytes: 3,
            },
            Picked {
                id: 0,
                path: "second-a".into(),
                name: "a.txt".into(),
                bytes: 9,
            },
            Picked {
                id: 0,
                path: "c".into(),
                name: "c.txt".into(),
                bytes: 4,
            },
        ];
        let merged = merge_named(&current, incoming, &next);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].id, 2);
        assert_eq!(merged[0].path, "new-b");
        assert_eq!(merged[1].id, 1);
        assert_eq!(merged[1].path, "second-a");
        assert_eq!(merged[1].bytes, 9);
        assert_eq!(merged[2].id, 3);
        assert_eq!(merged[2].name, "c.txt");
        let kept = merge_named(&merged, vec![merged[0].clone()], &next);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, 2);
    }

    struct TestShare {
        live: Live,
        dir: PathBuf,
    }

    impl Drop for TestShare {
        fn drop(&mut self) {
            self.live.task.abort();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    async fn start_fixture(files: &[(&str, &[u8])], code: &str) -> TestShare {
        let dir = std::env::temp_dir().join(format!(
            "nidong-share-{}-{}",
            process::id(),
            OsRng.gen::<u32>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut picked = Vec::new();
        for (name, bytes) in files {
            let path = dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            picked.push(Picked {
                id: 0,
                path: path.to_string_lossy().into_owned(),
                name: (*name).to_string(),
                bytes: bytes.len() as u64,
            });
        }
        let live = launch(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            picked,
            format!("{}-{}", &code[..4], &code[4..]),
            code.to_string(),
            sample_promo(),
        )
        .await
        .unwrap();
        TestShare { live, dir }
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .cookie_store(true)
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn browser_needs_the_code_and_then_gets_the_file() {
        let payload = b"nidong-share-bytes";
        let server = start_fixture(&[("note.txt", payload), ("b.bin", &[0, 1, 2])], "K7M9Q2PX").await;
        let base = server.live.offer().url;
        assert!(!base.contains("K7M9"));
        let http = client();
        let home = http.get(&base).send().await.unwrap().text().await.unwrap();
        assert!(home.contains("输入口令后才能下载"));
        assert!(home.contains("技术支持：泥洞工具箱"));
        assert!(home.contains("sapphire_fic"));
        assert!(home.contains(GITHUB_REPO));
        assert!(home.contains("gitee.com/sapphire_fic/nidong"));
        assert!(!home.contains("note.txt"));
        let wrong = http
            .post(&base)
            .form(&[("code", "ZZZZZZZZ")])
            .send()
            .await
            .unwrap();
        assert!(wrong.status().is_success());
        assert!(wrong.headers().get("set-cookie").is_none());
        let wrong_body = wrong.text().await.unwrap();
        assert!(wrong_body.contains("口令不对"));
        assert!(!wrong_body.contains("note.txt"));
        let note_id = server.live.offer().files[0].id;
        let bin_id = server.live.offer().files[1].id;
        let blocked = http.get(format!("{base}f/{note_id}")).send().await.unwrap();
        assert_eq!(blocked.status(), reqwest::StatusCode::NOT_FOUND);

        let opened = http
            .post(&base)
            .form(&[("code", "k7m9-q2px")])
            .send()
            .await
            .unwrap();
        assert!(opened.status().is_success());
        let list = opened.text().await.unwrap();
        assert!(list.contains("note.txt"));
        assert!(list.contains("sapphire_fic"));
        assert!(list.contains("Gitee 下载泥洞"));
        assert!(list.contains("9.9.9"));
        assert!(list.contains("class=\"mark\""));
        assert!(list.contains(&format!("/f/{note_id}")));
        let file = http.get(format!("{base}f/{note_id}")).send().await.unwrap();
        assert_eq!(file.status(), reqwest::StatusCode::OK);
        assert_eq!(file.bytes().await.unwrap().as_ref(), payload);
        let second = http.get(format!("{base}f/{bin_id}")).send().await.unwrap();
        assert_eq!(second.bytes().await.unwrap().as_ref(), &[0, 1, 2]);
        assert_eq!(server.live.offer().unlocks, 1);
    }

    #[tokio::test]
    async fn one_hundred_downloads_at_once_all_finish() {
        let payload = vec![7u8; 256 * 1024];
        let server = start_fixture(&[("course.bin", &payload)], "ABCDEFGH").await;
        let id = server.live.offer().files[0].id;
        let base = server.live.offer().url;
        let http = client();
        http.post(&base)
            .form(&[("code", "ABCDEFGH")])
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let mut jobs = Vec::new();
        for _ in 0..100 {
            let http = http.clone();
            let url = format!("{base}f/{id}");
            jobs.push(tokio::spawn(async move {
                http.get(url).send().await.unwrap().bytes().await.unwrap()
            }));
        }
        for job in jobs {
            assert_eq!(job.await.unwrap().as_ref(), payload.as_slice());
        }
    }
}
