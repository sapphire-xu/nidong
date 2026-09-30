//! 校园网网关登录。单独的 HTTP 客户端，不碰泥洞门户的 cookie，也不走系统代理。
//! 失败只返回一句说明，不打开浏览器。

use reqwest::redirect::Policy;
use scraper::{Html, Selector};
use std::time::Duration;

const SSO: &str = "https://pass.neu.edu.cn/tpass/login";
const IPGW: &str = "http://ipgw.neu.edu.cn";

pub async fn login(username: &str, password: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(Policy::limited(10))
        .timeout(Duration::from_secs(12))
        .user_agent(crate::util::UA)
        .no_proxy()
        .danger_accept_invalid_certs(false)
        .build()
        .map_err(|e| e.to_string())?;

    // 无线名可以仿冒。先跟学校登录主机做一次证书校验，失败就停，后面不再使用密码。
    confirm_school(&client).await?;

    let (landed, _) = get(&client, IPGW).await?;
    let ac_id = query_param(&landed, "ac_id").unwrap_or_else(|| "1".into());

    if let Ok((_, body)) = get(&client, &format!("{IPGW}/cgi-bin/rad_user_info?callback=nidong&_={}", ts())).await {
        if let Ok(info) = unwrap_json(&body) {
            if already_online(&info) {
                return Ok("已经在线".into());
            }
        }
    }

    let service_raw = format!("{IPGW}/srun_portal_sso?ac_id={ac_id}");
    let service = urlencoding::encode(&service_raw);
    let login_url = format!("{SSO}?service={service}");
    let (landed_login, page) = get(&client, &login_url).await?;
    if let Some(ticket) = ticket_from_school(&landed_login) {
        return complete_gateway(&client, &ac_id, &ticket).await;
    }
    require_school_https(&landed_login)?;
    if page_needs_sms(&page) {
        return Err("校园网登录要短信验证".into());
    }
    let lt = crate::util::input_id_value(&page, "lt")
        .or_else(|| crate::util::attr_value(&page, "lt"))
        .ok_or("登录页没有 lt")?;
    let execution = crate::util::attr_value(&page, "execution").unwrap_or_else(|| "e1s1".into());
    let key = rsa_key(&client, &page).await?;
    let action = form_action(&page).unwrap_or_else(|| login_url.clone());
    let post_url = password_target(&action)?;
    let rsa = crate::crypto::encrypt_rsa(username, password, &key).map_err(|e| e.to_string())?;
    let fields = [
        ("rsa", rsa),
        ("ul", username.len().to_string()),
        ("pl", password.len().to_string()),
        ("lt", lt),
        ("execution", execution),
        ("_eventId", "submit".into()),
    ];
    let resp = client
        .post(&post_url)
        .form(&fields)
        .send()
        .await
        .map_err(|e| post_err(&e.to_string()))?;
    let final_url = resp.url().to_string();
    let html = resp.text().await.map_err(|e| e.to_string())?;
    if page_needs_sms(&html) {
        return Err("校园网登录要短信验证".into());
    }
    let title = html_title(&html);
    let ticket = ticket_from_school(&final_url);
    let ticket = if let Some(t) = ticket {
        t
    } else if title.contains("统一身份认证") {
        if page_needs_captcha(&html) {
            return Err("校园网登录要验证码".into());
        }
        return Err("校园网密码不对".into());
    } else if title == "智慧东大" {
        let again = format!("{SSO}?service={service}");
        let (url, _) = get(&client, &again).await?;
        ticket_from_school(&url).ok_or("校园网登录失败")?
    } else if title == "IP控制网关" {
        return Err("校园网登录失败".into());
    } else {
        return Err("校园网登录失败".into());
    };

    complete_gateway(&client, &ac_id, &ticket).await
}

async fn complete_gateway(client: &reqwest::Client, ac_id: &str, ticket: &str) -> Result<String, String> {
    let sso = format!("{IPGW}/v1/srun_portal_sso?ac_id={ac_id}&ticket={ticket}");
    let resp = client
        .get(&sso)
        .header(reqwest::header::REFERER, &sso)
        .send()
        .await
        .map_err(|e| format!("网关登录失败：{e}"))?;
    let body = resp.text().await.map_err(|e| e.to_string())?;
    let v = serde_json::from_str::<serde_json::Value>(&body).or_else(|_| unwrap_json(&body))?;
    let code = v.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    let message = v.get("message").and_then(|m| m.as_str()).unwrap_or("");
    if code == 0 && message == "success" {
        return Ok("网关登录成功".into());
    }
    if message.contains("E2616") {
        return Err("校园网已欠费".into());
    }
    if message == "no_response_data_error" {
        return Err("网关没有回应".into());
    }
    Err("校园网登录失败".into())
}

fn ts() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

async fn confirm_school(client: &reqwest::Client) -> Result<(), String> {
    let resp = client
        .get(SSO)
        .send()
        .await
        .map_err(|e| transport_err(&e.to_string()))?;
    let final_url = resp.url().clone();
    let _ = resp.text().await;
    if final_url.scheme() == "https" && final_url.host_str() == Some(SCHOOL_LOGIN_HOST) {
        return Ok(());
    }
    // 证书已经核对过。跟着学校自己的跳转离开登录主机，不算仿冒。
    if is_school_redirect(&final_url) {
        return Ok(());
    }
    Err("这不是校园网".into())
}

fn is_school_redirect(url: &url::Url) -> bool {
    matches!(url.host_str(), Some("pass.neu.edu.cn" | "ipgw.neu.edu.cn" | "portal.neu.edu.cn"))
}

const SCHOOL_LOGIN_HOST: &str = "pass.neu.edu.cn";

/// 密码只允许发到这个主机的 https。相对路径补到该主机，其他主机一律拒绝。
pub(crate) fn password_target(action: &str) -> Result<String, String> {
    let absolute = if action.starts_with("https://") || action.starts_with("http://") {
        action.to_string()
    } else if action.starts_with("//") {
        return Err("这不是校园网".into());
    } else if action.starts_with('/') {
        format!("https://{SCHOOL_LOGIN_HOST}{action}")
    } else {
        format!("https://{SCHOOL_LOGIN_HOST}/tpass/{action}")
    };
    require_school_https(&absolute)
}

pub(crate) fn require_school_https(url: &str) -> Result<String, String> {
    let u = url::Url::parse(url).map_err(|_| "这不是校园网".to_string())?;
    let host_ok = u.host_str() == Some(SCHOOL_LOGIN_HOST);
    let https = u.scheme() == "https";
    let port_ok = u.port_or_known_default() == Some(443);
    let no_user = u.username().is_empty() && u.password().is_none();
    if host_ok && https && port_ok && no_user {
        Ok(u.to_string())
    } else {
        Err("这不是校园网".into())
    }
}

pub(crate) fn transport_err(err: &str) -> String {
    if cert_failure(err) {
        "这不是校园网".into()
    } else {
        format!("网络不通：{err}")
    }
}

fn post_err(err: &str) -> String {
    if cert_failure(err) {
        "这不是校园网".into()
    } else {
        format!("登录请求失败：{err}")
    }
}

fn cert_failure(err: &str) -> bool {
    let low = err.to_ascii_lowercase();
    low.contains("certificate")
        || low.contains("unknownissuer")
        || low.contains("notvalidforname")
        || low.contains("invalid peer")
}

fn ticket_from_school(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    if !matches!(u.host_str(), Some("pass.neu.edu.cn" | "ipgw.neu.edu.cn")) {
        return None;
    }
    query_param(url, "ticket").filter(|s| !s.is_empty())
}

async fn get(client: &reqwest::Client, url: &str) -> Result<(String, String), String> {
    let resp = client.get(url).send().await.map_err(|e| transport_err(&e.to_string()))?;
    let url = resp.url().to_string();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    Ok((url, text))
}

async fn rsa_key(client: &reqwest::Client, page: &str) -> Result<String, String> {
    if let Some(k) = crate::util::extract_rsa_key(page) {
        if crate::crypto::parse_pub_key(&k).is_some() {
            return Ok(k);
        }
    }
    let Some(src) = script_src(page) else {
        return Ok(crate::crypto::DEFAULT_PUB_B64.into());
    };
    let Ok(url) = password_target(&src) else {
        return Ok(crate::crypto::DEFAULT_PUB_B64.into());
    };
    let (_, js) = get(client, &url).await?;
    if let Some(k) = crate::util::extract_rsa_key(&js) {
        if crate::crypto::parse_pub_key(&k).is_some() {
            return Ok(k);
        }
    }
    Ok(crate::crypto::DEFAULT_PUB_B64.into())
}

fn script_src(html: &str) -> Option<String> {
    let re = regex::Regex::new(r#"(?i)src\s*=\s*["']([^"']*login_neu\.js[^"']*)["']"#).ok()?;
    re.captures(html).map(|c| c[1].to_string())
}

fn form_action(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    let sel = Selector::parse("form#loginForm, form").ok()?;
    doc.select(&sel).next().and_then(|el| el.value().attr("action").map(|s| s.to_string())).filter(|s| !s.is_empty())
}



fn query_param(url: &str, key: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    u.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.into_owned()).filter(|s| !s.is_empty())
}

fn html_title(html: &str) -> String {
    let doc = Html::parse_document(html);
    let Ok(sel) = Selector::parse("title") else {
        return String::new();
    };
    doc.select(&sel)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .unwrap_or_default()
}

fn page_needs_sms(html: &str) -> bool {
    html.contains("当前设备需进行身份验证") || html.contains("second_auth_form")
}

fn page_needs_captcha(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    lower.contains("name=\"captcha") || lower.contains("id=\"captcha") || lower.contains("captcharesponse")
}

pub(crate) fn unwrap_json(text: &str) -> Result<serde_json::Value, String> {
    let start = text.find('{').ok_or("网关没有返回结果")?;
    let end = text.rfind('}').ok_or("网关没有返回结果")?;
    if end < start {
        return Err("网关没有返回结果".into());
    }
    serde_json::from_str(&text[start..=end]).map_err(|_| "网关返回看不懂".into())
}

pub(crate) fn already_online(v: &serde_json::Value) -> bool {
    if v.get("error").and_then(|e| e.as_str()) == Some("not_online_error") {
        return false;
    }
    v.get("user_name").and_then(|u| u.as_str()).is_some_and(|s| !s.is_empty())
        || v.get("error").and_then(|e| e.as_str()) == Some("ok")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonp_and_online_flag() {
        let v = unwrap_json("nidong({\"error\":\"not_online_error\"});").unwrap();
        assert!(!already_online(&v));
        let v = unwrap_json("cb({\"user_name\":\"20210000\",\"error\":\"ok\"})").unwrap();
        assert!(already_online(&v));
    }

    #[test]
    fn ticket_query() {
        assert_eq!(
            query_param("http://ipgw.neu.edu.cn/srun_portal_sso?ac_id=1&ticket=ST-1", "ticket").as_deref(),
            Some("ST-1")
        );
    }

    #[test]
    fn password_stays_on_school_https() {
        assert!(password_target("/tpass/login").unwrap().starts_with("https://pass.neu.edu.cn/"));
        assert!(password_target("login").unwrap().starts_with("https://pass.neu.edu.cn/tpass/login"));
        assert!(password_target("https://pass.neu.edu.cn/tpass/login").is_ok());
        assert!(password_target("https://pass.neu.edu.cn:443/tpass/login").is_ok());
        assert!(password_target("http://pass.neu.edu.cn/tpass/login").is_err());
        assert!(password_target("https://pass.neu.edu.cn.evil.example/tpass/login").is_err());
        assert!(password_target("https://evil.example/tpass/login").is_err());
        assert!(password_target("https://user:secret@pass.neu.edu.cn/tpass/login").is_err());
        assert!(password_target("//evil.example/tpass/login").is_err());
        assert!(password_target("https://pass.neu.edu.cn:8443/tpass/login").is_err());
    }

    #[test]
    fn cert_failure_is_not_a_generic_outage() {
        assert_eq!(transport_err("invalid peer certificate: UnknownIssuer"), "这不是校园网");
        assert_eq!(transport_err("CertNotValidForName"), "这不是校园网");
        assert!(transport_err("timed out").starts_with("网络不通"));
    }

    #[test]
    fn ticket_only_from_school_host() {
        assert_eq!(
            ticket_from_school("http://ipgw.neu.edu.cn/srun_portal_sso?ticket=ST-9").as_deref(),
            Some("ST-9")
        );
        assert!(ticket_from_school("https://evil.example/?ticket=ST-9").is_none());
    }
}
