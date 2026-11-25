use encoding_rs::GBK;
use regex::Regex;
use scraper::{Html, Selector};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

pub fn normalize_proxy(raw: &str) -> String {
    let t = raw.trim();
    let with_scheme = if t.contains("://") {
        t.to_string()
    } else {
        format!("http://{t}")
    };
    with_scheme.replace("://localhost", "://127.0.0.1")
}

static CAMPUS_CACHE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
static ONLINE_CACHE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

/// 校园网内才能很快连上的主机。校外这两个端口通常不通。
/// 同一轮刷新里结果缓存 30 秒，避免卡/课各探一次结论打架。
pub fn on_campus_net() -> bool {
    if let Ok(g) = CAMPUS_CACHE.lock() {
        if let Some((t, v)) = *g {
            if t.elapsed() < Duration::from_secs(30) {
                return v;
            }
        }
    }
    probe_campus_net()
}

pub fn probe_campus_net() -> bool {
    let v = tcp_ok("ecard.neu.edu.cn", 80, 1200) || tcp_ok("jwxt.neu.edu.cn", 443, 1200);
    if let Ok(mut g) = CAMPUS_CACHE.lock() {
        *g = Some((Instant::now(), v));
    }
    v
}

/// 本机是否还能连上外网或校园网。没网时不要把会话当成过期去催重新登录。
pub fn online() -> bool {
    if let Ok(g) = ONLINE_CACHE.lock() {
        if let Some((t, v)) = *g {
            if t.elapsed() < Duration::from_secs(8) {
                return v;
            }
        }
    }
    let v = probe_online();
    if let Ok(mut g) = ONLINE_CACHE.lock() {
        *g = Some((Instant::now(), v));
    }
    v
}

fn probe_online() -> bool {
    // IP 先探，断网时不必等 DNS。国内 DNS 的 53/tcp 偶尔被拦，再补学校站和校园网。
    tcp_ok("223.5.5.5", 53, 450)
        || tcp_ok("114.114.114.114", 53, 450)
        || tcp_ok("pass.neu.edu.cn", 443, 900)
        || probe_campus_net()
}

pub fn is_offline_err(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains("10050")
        || t.contains("10051")
        || t.contains("10065")
        || t.contains("11001")
        || t.contains("11004")
        || t.contains("network is unreachable")
        || t.contains("no route to host")
        || t.contains("name or service not known")
        || t.contains("temporary failure in name resolution")
        || t.contains("could not resolve")
        || t.contains("failed to lookup")
        || t.contains("dns error")
        || t.contains("connectex")
        || s.contains("无法访问网络")
        || s.contains("找不到主机")
}

pub fn tcp_ok(host: &str, port: u16, ms: u64) -> bool {
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    let mut addrs: Vec<_> = addrs.collect();
    addrs.sort_by_key(|a| a.is_ipv6());
    addrs
        .iter()
        .any(|addr| TcpStream::connect_timeout(addr, Duration::from_millis(ms)).is_ok())
}

pub fn proxy_reachable(proxy: &str) -> bool {
    let Ok(u) = url::Url::parse(&normalize_proxy(proxy)) else {
        return false;
    };
    let Some(host) = u.host_str() else {
        return false;
    };
    let port = u.port_or_known_default().unwrap_or(80);
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    let mut addrs: Vec<_> = addrs.collect();
    addrs.sort_by_key(|a| a.is_ipv6());
    addrs
        .iter()
        .any(|addr| TcpStream::connect_timeout(addr, Duration::from_millis(350)).is_ok())
}

#[allow(dead_code)]
pub fn is_dead_proxy_err(e: &impl std::fmt::Display) -> bool {
    let s = e.to_string();
    s.contains("10061")
        || s.contains("积极拒绝")
        || s.contains("tunnel error")
        || s.contains("failed to create underlying connection")
        || s.contains("Connection refused")
        || s.contains("connection refused")
}

/// 直连失败时才用的代理候选。系统代理关着时返回空：不读环境变量，不读 ProxyServer，也不因为 7897/7890 有进程就改走 Clash。
pub fn assemble_proxy_candidates(
    env_values: impl IntoIterator<Item = impl AsRef<str>>,
    system_enabled: bool,
    system_server: Option<&str>,
) -> Vec<String> {
    if !system_enabled {
        return Vec::new();
    }
    let mut candidates: Vec<String> = Vec::new();
    let mut push = |s: String| {
        if !s.is_empty() && !candidates.iter().any(|c| c == &s) {
            candidates.push(s);
        }
    };
    for v in env_values {
        let t = v.as_ref().trim();
        if !t.is_empty() {
            push(normalize_proxy(t));
        }
    }
    if let Some(url) = system_server.and_then(system_proxy_url) {
        push(url);
    }
    candidates
}

fn system_proxy_url(server: &str) -> Option<String> {
    let server = server.trim();
    if server.is_empty() {
        return None;
    }
    let host = if server.contains('=') {
        server
            .split(';')
            .find_map(|p| p.strip_prefix("https=").or_else(|| p.strip_prefix("http=")))
            .unwrap_or_else(|| server.split(';').next().unwrap_or(server))
            .to_string()
    } else {
        server.to_string()
    };
    if host.is_empty() {
        None
    } else {
        Some(normalize_proxy(&host))
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesktopProxy {
    pub enabled: bool,
    pub http: Option<String>,
    pub https: Option<String>,
    pub socks: Option<String>,
    pub bypass: Vec<String>,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parse_gnome_string(raw: &str) -> Option<String> {
    let t = raw.trim();
    let inner = t
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .unwrap_or(t)
        .trim();
    if inner.is_empty() {
        None
    } else {
        Some(inner.to_string())
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parse_gnome_ignore_hosts(raw: &str) -> Vec<String> {
    let t = raw.trim();
    let inner = t
        .strip_prefix("@as")
        .map(str::trim)
        .unwrap_or(t)
        .trim_start_matches('[')
        .trim_end_matches(']');
    inner
        .split(',')
        .filter_map(|item| parse_gnome_string(item))
        .collect()
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn format_host_port(host: Option<String>, port: Option<u16>, scheme: &str) -> Option<String> {
    let host = host?;
    let port = port.filter(|p| *p > 0)?;
    if host.contains("://") {
        Some(host)
    } else {
        Some(format!("{scheme}://{host}:{port}"))
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parse_kde_kioslaverc(content: &str) -> DesktopProxy {
    let mut in_section = false;
    let mut proxy_type = 0u8;
    let mut http = None;
    let mut https = None;
    let mut socks = None;
    let mut bypass = Vec::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_section = line.eq_ignore_ascii_case("[Proxy Settings]");
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim();
        match k {
            "ProxyType" => proxy_type = v.parse().unwrap_or(0),
            "httpProxy" if !v.is_empty() => http = Some(normalize_kde_proxy(v, "http")),
            "httpsProxy" if !v.is_empty() => https = Some(normalize_kde_proxy(v, "http")),
            "socksProxy" if !v.is_empty() => socks = Some(normalize_kde_proxy(v, "socks5")),
            "NoProxyFor" => {
                bypass = v
                    .split([',', ';'])
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            _ => {}
        }
    }
    if proxy_type != 1 {
        return DesktopProxy::default();
    }
    DesktopProxy {
        enabled: http.is_some() || https.is_some() || socks.is_some(),
        http,
        https,
        socks,
        bypass,
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn normalize_kde_proxy(raw: &str, default_scheme: &str) -> String {
    let mut parts = raw.split_whitespace();
    let first = parts.next().unwrap_or("").trim();
    if let Some(port) = parts.next().map(str::trim).filter(|p| !p.is_empty()) {
        if first.contains("://") {
            format!("{}:{port}", first.trim_end_matches('/'))
        } else {
            format!("{default_scheme}://{first}:{port}")
        }
    } else if first.contains("://") {
        first.to_string()
    } else {
        format!("{default_scheme}://{first}")
    }
}

#[cfg(target_os = "linux")]
fn gsettings_get(schema: &str, key: &str) -> Option<String> {
    let out = std::process::Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(target_os = "linux")]
pub fn linux_desktop_proxy() -> DesktopProxy {
    if let Some(mode) = gsettings_get("org.gnome.system.proxy", "mode") {
        if parse_gnome_string(&mode).as_deref() == Some("manual") {
            let read_ep = |sub: &str, scheme: &str| {
                let schema = format!("org.gnome.system.proxy.{sub}");
                let host = gsettings_get(&schema, "host").and_then(|s| parse_gnome_string(&s));
                let port = gsettings_get(&schema, "port").and_then(|s| s.trim().parse::<u16>().ok());
                format_host_port(host, port, scheme)
            };
            let http = read_ep("http", "http");
            let https = read_ep("https", "http");
            let socks = read_ep("socks", "socks5");
            let bypass = gsettings_get("org.gnome.system.proxy", "ignore-hosts")
                .map(|s| parse_gnome_ignore_hosts(&s))
                .unwrap_or_default();
            if http.is_some() || https.is_some() || socks.is_some() {
                return DesktopProxy {
                    enabled: true,
                    http,
                    https,
                    socks,
                    bypass,
                };
            }
        }
    }
    let kde_path = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(|h| std::path::PathBuf::from(h).join(".config"))
        })
        .map(|d| d.join("kioslaverc"));
    if let Some(path) = kde_path {
        if let Ok(text) = std::fs::read_to_string(path) {
            let kde = parse_kde_kioslaverc(&text);
            if kde.enabled {
                return kde;
            }
        }
    }
    DesktopProxy::default()
}

pub fn detect_proxy() -> Option<String> {
    let mut env_values = Vec::new();
    for k in [
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ] {
        if let Ok(v) = std::env::var(k) {
            env_values.push(v);
        }
    }
    #[cfg(windows)]
    let (system_on, server) = windows_proxy_setting();
    #[cfg(target_os = "linux")]
    let (system_on, server) = {
        let dp = linux_desktop_proxy();
        let srv = dp.https.or(dp.http);
        let has_env = env_values.iter().any(|v| !v.trim().is_empty());
        (dp.enabled || has_env, srv)
    };
    #[cfg(not(any(windows, target_os = "linux")))]
    let (system_on, server): (bool, Option<String>) = (true, None);
    let candidates = assemble_proxy_candidates(env_values, allow_proxy_fallback(system_on), server.as_deref());
    for p in candidates {
        if proxy_reachable(&p) {
            return Some(p);
        }
        eprintln!("nidong: skip dead proxy {p}");
    }
    None
}

fn allow_proxy_fallback(system_on: bool) -> bool {
    #[cfg(any(windows, target_os = "linux"))]
    {
        system_on
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = system_on;
        true
    }
}

#[cfg(windows)]
fn windows_proxy_setting() -> (bool, Option<String>) {
    if !windows_proxy_enabled() {
        return (false, None);
    }
    (true, windows_proxy_server())
}

#[cfg(windows)]
fn windows_proxy_enabled() -> bool {
    reg_dword_nonzero(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings", "ProxyEnable")
}

#[cfg(windows)]
fn windows_proxy_server() -> Option<String> {
    reg_sz(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings", "ProxyServer")
}

#[cfg(windows)]
fn reg_sz(key: &str, name: &str) -> Option<String> {
    let text = reg_query(key, name)?;
    let value = text
        .lines()
        .find(|l| l.contains(name))?
        .split_whitespace()
        .last()?
        .trim()
        .to_string();
    if value.is_empty() { None } else { Some(value) }
}

#[cfg(windows)]
fn reg_dword_nonzero(key: &str, name: &str) -> bool {
    let Some(text) = reg_query(key, name) else {
        return false;
    };
    let Some(token) = text
        .lines()
        .find(|l| l.contains(name))
        .and_then(|l| l.split_whitespace().last())
    else {
        return false;
    };
    let hex = token.trim_start_matches("0x").trim_start_matches("0X");
    u32::from_str_radix(hex, 16).unwrap_or(0) != 0
}

#[cfg(windows)]
fn reg_query(key: &str, name: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("reg")
        .args(["query", key, "/v", name])
        .creation_flags(0x08000000)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn decode_bytes(bytes: &[u8], content_type: Option<&str>) -> String {
    let ct = content_type.unwrap_or("").to_ascii_lowercase();
    let looks_gbk = ct.contains("gbk") || ct.contains("gb2312") || ct.contains("gb18030");
    if looks_gbk {
        return GBK.decode(bytes).0.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => GBK.decode(bytes).0.into_owned(),
    }
}

pub fn attr_value(html: &str, name: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    if let Ok(sel) = Selector::parse(&format!("input[name=\"{name}\"]")) {
        if let Some(v) = doc
            .select(&sel)
            .next()
            .and_then(|el| el.value().attr("value").map(|s| s.to_string()))
        {
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    let escaped = regex::escape(name);
    let re = Regex::new(&format!(
        r#"(?is)name\s*=\s*['"]{escaped}['"][^>]*value\s*=\s*['"]([^'"]*)['"]|value\s*=\s*['"]([^'"]*)['"][^>]*name\s*=\s*['"]{escaped}['"]"#
    ))
    .ok()?;
    re.captures(html).and_then(|c| {
        c.get(1)
            .or_else(|| c.get(2))
            .map(|m| m.as_str().to_string())
    })
}

pub fn input_id_value(html: &str, id: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    if let Ok(sel) = Selector::parse(&format!("#{id}")) {
        if let Some(el) = doc.select(&sel).next() {
            if let Some(v) = el.value().attr("value") {
                if !v.trim().is_empty() {
                    return Some(v.to_string());
                }
            }
            let text: String = el.text().collect::<String>().trim().to_string();
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    let escaped = regex::escape(id);
    let re = Regex::new(&format!(
        r#"(?is)id\s*=\s*['"]{escaped}['"][^>]*>([^<]*)<"#
    ))
    .ok()?;
    re.captures(html)
        .map(|c| c[1].trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn parse_money(raw: &str) -> f64 {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+')
        .collect();
    cleaned.parse().unwrap_or(0.0)
}

pub fn hidden_fields(html: &str) -> Vec<(String, String)> {
    let doc = Html::parse_document(html);
    let Ok(sel) = Selector::parse("input[type=hidden]") else {
        return vec![];
    };
    doc.select(&sel)
        .filter_map(|el| {
            let name = el.value().attr("name")?.to_string();
            let value = el.value().attr("value").unwrap_or("").to_string();
            Some((name, value))
        })
        .collect()
}

/// 登录页把没启用的面板放在 `<script type="text/html">` 里。那不是当前步骤。
pub fn strip_html_templates(html: &str) -> String {
    let Ok(re) = Regex::new(
        r#"(?is)<script\b[^>]*\btype\s*=\s*['"]text/html['"][^>]*>.*?</script>"#,
    ) else {
        return html.to_string();
    };
    re.replace_all(html, "").into_owned()
}

/// 二次认证只看模板以外的正文。2026-11 的密码页把说明和占位号 `186****1233` 写在模板里。
pub fn cas_needs_sms(html: &str) -> bool {
    let live = strip_html_templates(html);
    live.contains("当前设备需进行身份验证")
        || live.contains("second_auth_form")
        || live.contains("scendAuthCode")
        || live.contains("getScendAuthCode")
}

pub fn phone_mask(html: &str) -> String {
    let live = strip_html_templates(html);
    Regex::new(r"\*{3,}\d{4}")
        .ok()
        .and_then(|re| re.find(&live).map(|m| m.as_str().to_string()))
        .unwrap_or_else(|| "绑定手机".into())
}

pub fn extract_rsa_key(html_or_js: &str) -> Option<String> {
    let re = Regex::new(r#"MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA[A-Za-z0-9+/=]{200,}"#).ok()?;
    re.find(html_or_js).map(|m| m.as_str().to_string())
}

pub fn yyyy_mm(year: i32, month: u32) -> String {
    format!("{year:04}-{month:02}")
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    chrono::NaiveDate::from_ymd_opt(
        if month == 12 { year + 1 } else { year },
        if month == 12 { 1 } else { month + 1 },
        1,
    )
    .and_then(|d| d.pred_opt())
    .map(|d| d.day())
    .unwrap_or(30)
}

use chrono::Datelike;

#[cfg(test)]
mod tests {
    use super::{
        assemble_proxy_candidates, attr_value, cas_needs_sms, input_id_value, is_dead_proxy_err,
        is_offline_err, normalize_proxy, parse_money, phone_mask, proxy_reachable,
    };

    #[test]
    fn template_placeholder_is_not_a_bound_phone() {
        let html = r#"<div class="phone">139****5678</div><script id="second_auth_template" type="text/html"><div class="phone-number">186****1233</div>当前设备需进行身份验证</script>"#;
        assert_eq!(phone_mask(html), "****5678");
        assert!(!cas_needs_sms(html));
        assert!(cas_needs_sms(
            r#"<form id="second_auth_form"><p>当前设备需进行身份验证</p></form>"#
        ));
    }

    #[test]
    fn sso_hidden_single_quotes() {
        let html = "<form id='form' action='http://ecard.neu.edu.cn/selfsearch/SSOLogin.aspx' METHOD=POST><input type=hidden name='username' id='username' value='20200000'/><input type=hidden name='timestamp' id='timestamp' value='1788890696'/><input type=hidden name='auid' id='auid' value='76F9B2B95810A748E9B77212474489C2'/></form>";
        assert_eq!(attr_value(html, "username").as_deref(), Some("20200000"));
        assert_eq!(attr_value(html, "timestamp").as_deref(), Some("1788890696"));
        assert_eq!(
            attr_value(html, "auid").as_deref(),
            Some("76F9B2B95810A748E9B77212474489C2")
        );
    }

    #[test]
    fn system_proxy_off_does_not_use_an_open_clash_port() {
        let none: [&str; 0] = [];
        let off = assemble_proxy_candidates(none, false, Some("127.0.0.1:7897"));
        assert!(off.is_empty());
        let server_left_behind = assemble_proxy_candidates(none, false, Some("127.0.0.1:7890"));
        assert!(server_left_behind.is_empty());
        assert_eq!(
            assemble_proxy_candidates(none, true, Some("127.0.0.1:7897")),
            vec!["http://127.0.0.1:7897".to_string()]
        );
        assert!(
            assemble_proxy_candidates(["http://127.0.0.1:7897"], false, Some("127.0.0.1:7897"))
                .is_empty()
        );
    }

    #[test]
    fn proxy_url_localhost_to_ipv4() {
        assert_eq!(
            normalize_proxy("http://localhost:7897"),
            "http://127.0.0.1:7897"
        );
        assert_eq!(normalize_proxy("127.0.0.1:7897"), "http://127.0.0.1:7897");
    }

    #[test]
    fn dead_port_not_reachable() {
        assert!(!proxy_reachable("http://127.0.0.1:1"));
    }

    #[test]
    fn detects_clash_refused() {
        assert!(is_dead_proxy_err(
            &"tunnel error: failed to create underlying connection → 积极拒绝 (os error 10061)"
        ));
    }

    #[test]
    fn offline_err_matches_windows_codes() {
        assert!(is_offline_err("os error 10051"));
        assert!(is_offline_err("Name or service not known"));
        assert!(!is_offline_err("会话已失效，请重新登录"));
    }

    #[test]
    fn span_balance_text() {
        let html = r#"<span id="ContentPlaceHolder1_txtOddFare" class="red">376.07 元</span>"#;
        let v = input_id_value(html, "ContentPlaceHolder1_txtOddFare").unwrap();
        assert_eq!(parse_money(&v), 376.07);
    }

    #[test]
    fn parses_gnome_and_kde_proxy_configs() {
        assert_eq!(super::parse_gnome_string("'manual'").as_deref(), Some("manual"));
        assert_eq!(super::parse_gnome_string("''"), None);
        assert_eq!(
            super::parse_gnome_ignore_hosts("['localhost', '127.0.0.0/8', '::1']"),
            vec!["localhost", "127.0.0.0/8", "::1"]
        );
        assert!(super::parse_gnome_ignore_hosts("@as []").is_empty());
        assert_eq!(
            super::format_host_port(Some("127.0.0.1".into()), Some(7890), "http").as_deref(),
            Some("http://127.0.0.1:7890")
        );
        let kde = super::parse_kde_kioslaverc(
            "[Proxy Settings]\nProxyType=1\nhttpProxy=http://127.0.0.1 7897\nhttpsProxy=127.0.0.1:7897\nNoProxyFor=localhost,127.0.0.1\n",
        );
        assert!(kde.enabled);
        assert_eq!(kde.http.as_deref(), Some("http://127.0.0.1:7897"));
        assert_eq!(kde.https.as_deref(), Some("http://127.0.0.1:7897"));
        assert_eq!(kde.bypass, vec!["localhost", "127.0.0.1"]);
        let kde_off = super::parse_kde_kioslaverc("[Proxy Settings]\nProxyType=0\nhttpProxy=127.0.0.1:7897\n");
        assert!(!kde_off.enabled);
    }
}
