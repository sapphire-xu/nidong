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

pub fn detect_proxy() -> Option<String> {
    let mut candidates: Vec<String> = Vec::new();
    let mut push = |s: String| {
        if !s.is_empty() && !candidates.iter().any(|c| c == &s) {
            candidates.push(s);
        }
    };
    for k in [
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ] {
        if let Ok(v) = std::env::var(k) {
            if !v.trim().is_empty() {
                push(normalize_proxy(v.trim()));
            }
        }
    }
    #[cfg(windows)]
    {
        if let Some(p) = windows_ie_proxy() {
            push(p);
        }
    }
    push("http://127.0.0.1:7897".into());
    push("http://127.0.0.1:7890".into());
    for p in candidates {
        if proxy_reachable(&p) {
            return Some(p);
        }
        eprintln!("nidong: skip dead proxy {p}");
    }
    None
}

#[cfg(windows)]
#[allow(dead_code)]
fn windows_ie_proxy() -> Option<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            "/v",
            "ProxyServer",
        ])
        .creation_flags(0x08000000)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let server = text
        .lines()
        .find(|l| l.contains("ProxyServer"))?
        .split_whitespace()
        .last()?
        .trim()
        .to_string();
    if server.is_empty() {
        return None;
    }
    let host = if server.contains('=') {
        server
            .split(';')
            .find_map(|p| p.strip_prefix("https=").or_else(|| p.strip_prefix("http=")))
            .unwrap_or(server.split(';').next().unwrap_or(&server))
            .to_string()
    } else {
        server
    };
    Some(normalize_proxy(&host))
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

pub fn phone_mask(html: &str) -> String {
    Regex::new(r"\*{3,}\d{4}")
        .ok()
        .and_then(|re| re.find(html).map(|m| m.as_str().to_string()))
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
        attr_value, input_id_value, is_dead_proxy_err, is_offline_err, normalize_proxy, parse_money,
        proxy_reachable,
    };

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
}
