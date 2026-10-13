//! 对外 IPv4。校园网出口做了地址转换，本机网卡上看不到。
//! 只在主进程里偶尔问一次，成功后缓存几分钟。不要放进流量记录的两秒循环。

use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const OK_TTL: Duration = Duration::from_secs(3 * 60);
const RETRY: Duration = Duration::from_secs(30);
const KEEP: Duration = Duration::from_secs(30 * 60);
const URLS: [&str; 3] = [
    "https://ipv4.icanhazip.com",
    "https://4.ident.me",
    "https://api4.ipify.org",
];

struct Slot {
    ip: String,
    ok_at: Option<Instant>,
    tried_at: Instant,
}

static CACHE: Mutex<Option<Slot>> = Mutex::new(None);
static GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub fn parse_public_v4(body: &str) -> Option<String> {
    let line = body.lines().map(str::trim).find(|line| !line.is_empty())?;
    let token = line.split_whitespace().next()?;
    let ip: Ipv4Addr = token.parse().ok()?;
    if accept_public_v4(ip) {
        Some(ip.to_string())
    } else {
        None
    }
}

fn accept_public_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    if ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
    {
        return false;
    }
    if o[0] == 0 || o[0] >= 240 {
        return false;
    }
    // 100.64.0.0/10 是运营商共享地址，不是出口公网地址。
    if o[0] == 100 && (o[1] & 0xc0) == 64 {
        return false;
    }
    if o[0] == 192 && o[1] == 0 && o[2] == 0 {
        return false;
    }
    if o[0] == 198 && (o[1] == 18 || o[1] == 19) {
        return false;
    }
    true
}

fn v4_client() -> Option<reqwest::Client> {
    static CELL: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = CELL.get() {
        return Some(client.clone());
    }
    let built = reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .connect_timeout(Duration::from_secs(3))
        .local_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::limited(2))
        .user_agent("nidong")
        .build()
        .ok()?;
    let _ = CELL.set(built.clone());
    Some(built)
}

fn plain_client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .connect_timeout(Duration::from_secs(3))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::limited(2))
        .user_agent("nidong")
        .build()
        .ok()
}

async fn ask(client: &reqwest::Client) -> Option<String> {
    for url in URLS {
        let Ok(response) = client.get(url).send().await else {
            continue;
        };
        let Ok(text) = response.text().await else {
            continue;
        };
        let head: String = text.chars().take(64).collect();
        if let Some(ip) = parse_public_v4(&head) {
            return Some(ip);
        }
    }
    None
}

async fn query() -> Option<String> {
    if let Some(client) = v4_client() {
        if let Some(ip) = ask(&client).await {
            return Some(ip);
        }
    }
    // 个别网卡绑 0.0.0.0 会失败。这几个域名只有 IPv4，再问一次不绑定的。
    let client = plain_client()?;
    ask(&client).await
}

fn cached() -> Option<String> {
    let guard = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    let slot = guard.as_ref()?;
    if slot.ok_at.is_some_and(|t| t.elapsed() < OK_TTL) {
        return Some(slot.ip.clone());
    }
    if slot.tried_at.elapsed() < RETRY {
        let kept = slot.ok_at.is_some_and(|t| t.elapsed() < KEEP);
        return Some(if kept { slot.ip.clone() } else { String::new() });
    }
    None
}

fn remember(found: Option<String>) -> String {
    let mut guard = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    let now = Instant::now();
    if let Some(ip) = found {
        *guard = Some(Slot {
            ip: ip.clone(),
            ok_at: Some(now),
            tried_at: now,
        });
        return ip;
    }
    let previous = guard.as_ref();
    let keep = previous.is_some_and(|slot| slot.ok_at.is_some_and(|t| t.elapsed() < KEEP));
    let ip = if keep {
        previous.map(|slot| slot.ip.clone()).unwrap_or_default()
    } else {
        String::new()
    };
    let ok_at = if keep {
        previous.and_then(|slot| slot.ok_at)
    } else {
        None
    };
    *guard = Some(Slot {
        ip: ip.clone(),
        ok_at,
        tried_at: now,
    });
    ip
}

pub async fn lookup() -> String {
    if let Some(ip) = cached() {
        return ip;
    }
    let _gate = GATE.lock().await;
    if let Some(ip) = cached() {
        return ip;
    }
    let found = query().await;
    remember(found)
}

#[tauri::command]
pub async fn public_ipv4() -> String {
    lookup().await
}

#[cfg(test)]
mod tests {
    use super::parse_public_v4;

    #[test]
    fn accepts_a_public_address() {
        assert_eq!(parse_public_v4("  8.8.8.8\n").as_deref(), Some("8.8.8.8"));
        assert_eq!(parse_public_v4("202.118.1.47").as_deref(), Some("202.118.1.47"));
        assert_eq!(parse_public_v4("172.32.1.1").as_deref(), Some("172.32.1.1"));
        assert_eq!(parse_public_v4("100.128.0.1").as_deref(), Some("100.128.0.1"));
    }

    #[test]
    fn rejects_private_and_noise() {
        for sample in [
            "",
            "\n",
            "<html>8.8.8.8</html>",
            "2001:da8:9000::1",
            "0.0.0.0",
            "10.1.2.3",
            "127.0.0.1",
            "169.254.1.1",
            "172.16.0.1",
            "172.22.8.8",
            "172.31.255.255",
            "192.168.1.1",
            "100.64.0.1",
            "100.127.255.255",
            "192.0.0.1",
            "192.0.2.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "240.1.2.3",
            "255.255.255.255",
        ] {
            assert_eq!(parse_public_v4(sample), None, "{sample}");
        }
    }
}
