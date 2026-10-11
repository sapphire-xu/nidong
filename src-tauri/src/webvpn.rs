//! 东北大学 WebVPN（Wengine）URL 编码。
//!
//! 校外访问 `ecard` / `jwxt` 这类校园网主机时，把原 URL 编成
//! `https://webvpn.neu.edu.cn/http(s)/<IV+密文主机>/<路径>`，由网关代访校内。
//! 密钥 `wrdvpnisthebest!` 来自学校 WebVPN 前端，不是登录口令。

use aes::Aes128;
use aes::cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray};

const WEBVPN_HOST: &str = "https://webvpn.neu.edu.cn";
/// 学校当前网关用的密钥（auth-debug 里 live URL 前缀 62304135…）。
const WRD_KEY: &[u8; 16] = b"b0A58a69394ce73@";
pub const WEBVPN_CAS_SERVICE: &str = "https://webvpn.neu.edu.cn/login?cas_login=true";

pub fn encrypt_url_to_webvpn(raw: &str) -> String {
    let raw = raw.trim();
    if raw.contains("webvpn.neu.edu.cn") {
        return raw.to_string();
    }
    let (protocol, rest) = if let Some(r) = raw.strip_prefix("https://") {
        ("https", r)
    } else if let Some(r) = raw.strip_prefix("http://") {
        ("http", r)
    } else if let Some(r) = raw.strip_prefix("//") {
        ("http", r)
    } else {
        ("http", raw)
    };

    let (hostport_path, query) = match rest.split_once('?') {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };
    let (hostport, path) = match hostport_path.split_once('/') {
        Some((h, p)) => (h, format!("/{p}")),
        None => (hostport_path, String::new()),
    };

    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => (h, Some(p)),
        _ => (hostport, None),
    };

    let enc_host = encrypt_host(host);
    let proto = if let Some(port) = port {
        format!("{protocol}-{port}")
    } else {
        protocol.to_string()
    };
    let mut out = format!("{WEBVPN_HOST}/{proto}/{enc_host}{path}");
    if let Some(q) = query {
        out.push('?');
        out.push_str(q);
    }
    out
}

pub fn is_intranet_url(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.contains("ecard.neu.edu.cn")
        || u.contains("jwxt.neu.edu.cn")
        || u.contains("219.216.96.4")
}

fn encrypt_host(host: &str) -> String {
    let cipher = Aes128::new(GenericArray::from_slice(WRD_KEY));
    let data = host.as_bytes();
    let mut prev = *WRD_KEY;
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let mut block = GenericArray::clone_from_slice(&prev);
        cipher.encrypt_block(&mut block);
        let mut c = vec![0u8; chunk.len()];
        for (i, b) in chunk.iter().enumerate() {
            c[i] = b ^ block[i];
        }
        if chunk.len() == 16 {
            prev.copy_from_slice(&c);
        }
        out.extend_from_slice(&c);
    }
    format!("{}{}", hex::encode(WRD_KEY), hex::encode(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypts_with_live_school_key() {
        let got = encrypt_url_to_webvpn("http://ecard.neu.edu.cn/selfsearch/User/baseinfo.aspx");
        assert!(
            got.starts_with("https://webvpn.neu.edu.cn/http/62304135386136393339346365373340"),
            "{got}"
        );
        assert!(got.ends_with("/selfsearch/User/baseinfo.aspx"));
    }

    #[test]
    fn encrypts_ecard_keeps_path_and_ticket() {
        let got = encrypt_url_to_webvpn(
            "http://ecard.neu.edu.cn/selfsearch/User/baseinfo.aspx?ticket=ST-1",
        );
        assert!(got.starts_with("https://webvpn.neu.edu.cn/http/"));
        assert!(got.contains("/selfsearch/User/baseinfo.aspx?ticket=ST-1"));
        assert!(!got.contains("ecard.neu.edu.cn"));
    }

    #[test]
    fn wraps_personal_portal_host() {
        let got = encrypt_url_to_webvpn(
            "https://personal.neu.edu.cn/portal/schedule/frontend/calendar/calendar",
        );
        assert!(
            got.contains(
                "62304135386136393339346365373340a0e4b62c85cb47d1bc166e66c800d19283ef83"
            ),
            "{got}"
        );
        assert!(got.contains("/portal/schedule/frontend/calendar/calendar"));
        assert!(!got.contains("personal.neu.edu.cn"));
    }

    #[test]
    fn leaves_already_wrapped() {
        let u = "https://webvpn.neu.edu.cn/http/abc/foo";
        assert_eq!(encrypt_url_to_webvpn(u), u);
    }
}
