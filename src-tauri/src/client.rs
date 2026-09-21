use crate::course::{
    build_semester, catalog_has_courses, enrich_from_catalog, looks_like_login_page,
    merge_weekly_stamped, parse_arranged_list, parse_calendars, parse_courses,
    parse_scheduled_campus, parse_term_weeks, stamp_courses_week, week_param_filters,
};
use crate::crypto::{self, DEFAULT_PUB_B64};
use crate::error::{AppError, Result};
use crate::models::*;
use crate::ocr;
use crate::util::{
    attr_value, days_in_month, decode_bytes, extract_rsa_key, hidden_fields, input_id_value,
    on_campus_net, parse_money, phone_mask, yyyy_mm, UA,
};
use crate::webvpn::{encrypt_url_to_webvpn, is_intranet_url, WEBVPN_CAS_SERVICE};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{Datelike, Local, NaiveDate};
use cookie_store::CookieStore;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, REFERER, USER_AGENT};
use reqwest::redirect::Policy;
use reqwest_cookie_store::CookieStoreMutex;
use scraper::{Html, Selector};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SERVICE_PORTAL: &str = "https://portal.neu.edu.cn/tp_up/";
const SERVICE_CARD: &str = "http://ecard.neu.edu.cn/selflogin/login.aspx";
const SERVICE_PAY: &str = "https://pay.neu.edu.cn/drCasLogin";
const SERVICE_JWXT: &str = "https://jwxt.neu.edu.cn/jwapp/sys/homeapp/index.do";
const JWXT_API: &str = "https://jwxt.neu.edu.cn/jwapp/sys/homeapp";
const JWXT_REFERER: &str =
    "https://jwxt.neu.edu.cn/jwapp/sys/homeapp/home/index.html?av=&contextPath=/jwapp";
const LOGIN: &str = "https://pass.neu.edu.cn/tpass/login";

fn build_http(
    headers: HeaderMap,
    jar: Arc<CookieStoreMutex>,
    redirect: Policy,
    proxy: Option<&str>,
) -> Result<reqwest::Client> {
    let mut b = reqwest::Client::builder()
        .default_headers(headers)
        .cookie_provider(jar)
        .redirect(redirect)
        .timeout(std::time::Duration::from_secs(20))
        .connect_timeout(std::time::Duration::from_secs(8))
        .tcp_nodelay(true)
        .http1_only();
    b = if let Some(p) = proxy {
        b.proxy(reqwest::Proxy::all(p)?)
    } else {
        b.no_proxy()
    };
    Ok(b.build()?)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Valid,
    NeedSms,
    Expired,
}

pub struct NeuClient {
    http: reqwest::Client,
    http_stop: reqwest::Client,
    http_via: Option<reqwest::Client>,
    webvpn_on: std::sync::Mutex<bool>,
    jar: Arc<CookieStoreMutex>,
    sms_form: std::sync::Mutex<HashMap<String, String>>,
    last_sms: std::sync::Mutex<Option<std::time::Instant>>,
    last_graphic: std::sync::Mutex<Option<String>>,
    sms_pending: std::sync::Mutex<bool>,
    cas_lock: tokio::sync::Mutex<()>,
    pub data_dir: PathBuf,
    username: std::sync::Mutex<Option<String>>,
}

impl NeuClient {
    pub fn new(data_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&data_dir)?;
        let cookie_path = data_dir.join("cookies.json");
        let store = if cookie_path.exists() {
            let f = fs::File::open(&cookie_path)?;
            cookie_store::serde::json::load(std::io::BufReader::new(f)).unwrap_or_else(|_| CookieStore::default())
        } else {
            CookieStore::default()
        };
        let jar = Arc::new(CookieStoreMutex::new(store));
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(UA));
        headers.insert(
            reqwest::header::ACCEPT,
            HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
        );
        headers.insert(
            reqwest::header::ACCEPT_LANGUAGE,
            HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8"),
        );
        let proxy = crate::util::detect_proxy();
        let http = build_http(headers.clone(), jar.clone(), Policy::limited(12), None)?;
        let http_stop = build_http(headers.clone(), jar.clone(), Policy::none(), None)?;
        let http_via = proxy.as_deref().and_then(|p| {
            build_http(headers.clone(), jar.clone(), Policy::limited(12), Some(p)).ok()
        });
        Ok(Self {
            http,
            http_stop,
            http_via,
            webvpn_on: std::sync::Mutex::new(false),
            jar,
            sms_form: std::sync::Mutex::new(HashMap::new()),
            last_sms: std::sync::Mutex::new(None),
            last_graphic: std::sync::Mutex::new(None),
            sms_pending: std::sync::Mutex::new(false),
            cas_lock: tokio::sync::Mutex::new(()),
            data_dir,
            username: std::sync::Mutex::new(None),
        })
    }

    pub fn set_username(&self, username: impl Into<String>) {
        if let Ok(mut g) = self.username.lock() {
            *g = Some(username.into());
        }
    }

    pub fn username(&self) -> Option<String> {
        self.username.lock().ok().and_then(|g| g.clone())
    }

    fn set_sms_pending(&self, v: bool) {
        if let Ok(mut g) = self.sms_pending.lock() {
            *g = v;
        }
    }

    fn sms_pending(&self) -> bool {
        self.sms_pending.lock().map(|g| *g).unwrap_or(false)
    }

    pub fn clear_session(&self) -> Result<()> {
        {
            let mut locked = self.jar.lock().map_err(|e| e.to_string())?;
            *locked = CookieStore::default();
        }
        if let Ok(mut g) = self.sms_form.lock() {
            g.clear();
        }
        if let Ok(mut g) = self.last_sms.lock() {
            *g = None;
        }
        if let Ok(mut g) = self.last_graphic.lock() {
            *g = None;
        }
        self.set_sms_pending(false);
        if let Ok(mut g) = self.webvpn_on.lock() {
            *g = false;
        }
        let _ = fs::remove_file(self.data_dir.join("cookies.json"));
        Ok(())
    }

    fn webvpn_on(&self) -> bool {
        self.webvpn_on.lock().map(|g| *g).unwrap_or(false)
    }

    fn maybe_wrap(&self, url: &str) -> String {
        if self.webvpn_on() && is_intranet_url(url) {
            encrypt_url_to_webvpn(url)
        } else {
            url.to_string()
        }
    }

    fn has_webvpn_ticket(&self) -> bool {
        let Ok(locked) = self.jar.lock() else {
            return false;
        };
        let hit = locked.iter_unexpired().any(|c| {
            let n = c.name().to_ascii_lowercase();
            n.contains("wengine") || n.contains("vpn_ticket") || n.contains("webvpn")
        });
        hit
    }

    async fn ensure_webvpn(&self) -> Result<()> {
        if self.webvpn_on() && self.has_webvpn_ticket() {
            return Ok(());
        }
        // 必须用公网一号通换票，不能先打开 webvpn 首页：
        // 首页会跳到「套在网关里的 tpass」，CASTGC 发不到 pass.neu.edu.cn。
        let mut last = String::from("校外需经 WebVPN 访问一卡通，网关未登录成功");
        for attempt in 0..2u8 {
            self.debug_auth("webvpn", &format!("cas_login try={}", attempt + 1));
            match self.cas_service(WEBVPN_CAS_SERVICE).await {
                Ok((landed, html)) => {
                    self.debug_auth("webvpn_land", &landed);
                    let ok = self.has_webvpn_ticket()
                        || (landed.contains("webvpn.neu.edu.cn")
                            && !landed.contains("tpass/login")
                            && !Self::session_lost_html(&html));
                    if ok {
                        if let Ok(mut g) = self.webvpn_on.lock() {
                            *g = true;
                        }
                        let _ = self.persist_cookies();
                        self.debug_auth("webvpn", "on");
                        return Ok(());
                    }
                }
                Err(e) => last = e.to_string(),
            }
            if attempt == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            }
        }
        Err(last.into())
    }

    /// 一轮刷新开始时先判定校内/校外。校外必须先种上 WebVPN，再并行抓卡/课。
    pub async fn prepare_access(&self) -> bool {
        let campus = crate::util::probe_campus_net();
        if campus {
            if let Ok(mut g) = self.webvpn_on.lock() {
                *g = false;
            }
            self.debug_auth("net", "campus");
            return true;
        }
        self.debug_auth("net", "offcampus");
        if let Err(e) = self.ensure_webvpn().await {
            self.debug_auth("webvpn_fail", &e.to_string());
        }
        false
    }

    /// Drop CASTGC so the next GET shows the password form. Keep other cookies
    /// (device / remember-me) so a trusted computer can skip SMS.
    pub fn drop_tgt(&self) {
        let Ok(mut locked) = self.jar.lock() else {
            return;
        };
        let keys: Vec<(String, String, String)> = locked
            .iter_unexpired()
            .filter(|c| {
                let n = c.name();
                n.eq_ignore_ascii_case("CASTGC") || n.eq_ignore_ascii_case("TGC")
            })
            .filter_map(|c| {
                Some((
                    c.domain()?.to_string(),
                    c.path().unwrap_or("/").to_string(),
                    c.name().to_string(),
                ))
            })
            .collect();
        for (d, p, n) in keys {
            let _ = locked.remove(&d, &p, &n);
        }
        drop(locked);
        self.set_sms_pending(false);
        if self.has_tgt() {
            let _ = self.clear_session();
        }
    }

    /// 「返回登录」：丢掉未完成的二次验证，下次走完整密码登录。
    pub fn abort_second_auth(&self) {
        if let Ok(mut g) = self.sms_form.lock() {
            g.clear();
        }
        if let Ok(mut g) = self.last_sms.lock() {
            *g = None;
        }
        if let Ok(mut g) = self.last_graphic.lock() {
            *g = None;
        }
        self.set_sms_pending(false);
        self.drop_tgt();
    }

    fn capture_sms_form(&self, html: &str) {
        let mut form = hidden_in_form(html, "#second_auth_form");
        form.retain(|k, _| keep_2fa_field(k));
        if form
            .get("execution")
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
        {
            if let Some(v) = prefer_second_execution(html) {
                form.insert("execution".into(), v.clone());
                form.entry("RelayState".into()).or_insert(v);
            }
        }
        form.insert("method".into(), "mobile".into());
        form.remove("scendAuthCode");
        form.remove("secondAuthCode");
        form.remove("authCode");
        form.remove("imgCode");
        form.remove("code");
        let exec = form.get("execution").cloned().unwrap_or_default();
        let keys: Vec<String> = form.keys().cloned().collect();
        self.debug_auth(
            "capture_sms_form",
            &format!(
                "execution={exec} keys={} sms_field={}",
                keys.join(","),
                detect_sms_field(html)
            ),
        );
        let _ = fs::write(self.data_dir.join("sms-page.html"), html.as_bytes());
        if let Ok(mut g) = self.sms_form.lock() {
            *g = form;
        }
    }

    fn debug_auth(&self, step: &str, detail: &str) {
        use std::io::Write;
        let line = format!("{} {step} {detail}\n", Local::now().to_rfc3339());
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.data_dir.join("auth-debug.log"))
        {
            let _ = f.write_all(line.as_bytes());
        }
    }

    pub fn persist_cookies(&self) -> Result<()> {
        let path = self.data_dir.join("cookies.json");
        let mut f = fs::File::create(path)?;
        let locked = self.jar.lock().map_err(|e| e.to_string())?;
        cookie_store::serde::json::save(&locked, &mut f)
            .map_err(|e| AppError::Msg(e.to_string()))?;
        Ok(())
    }

    pub fn has_tgt(&self) -> bool {
        let Ok(locked) = self.jar.lock() else {
            return false;
        };
        let hit = locked
            .iter_unexpired()
            .any(|c| c.name() == "CASTGC" || c.name() == "TGC");
        hit
    }

    /// CASTGC sitting on disk is not enough: school binds TGT to IP / device.
    /// Switching from campus Wi-Fi to another network often invalidates it.
    pub async fn session_state(&self) -> SessionState {
        let url = format!("{LOGIN}?service={}", urlencoding::encode(SERVICE_PORTAL));
        let send = self.http_stop.get(&url).send();
        let resp = match tokio::time::timeout(std::time::Duration::from_secs(8), send).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) | Err(_) => {
                return if self.sms_pending() {
                    SessionState::NeedSms
                } else if self.has_tgt() {
                    SessionState::Valid
                } else {
                    SessionState::Expired
                };
            }
        };
        let loc = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if loc.contains("ticket=") {
            self.set_sms_pending(false);
            return SessionState::Valid;
        }
        let ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let html = match resp.bytes().await {
            Ok(b) => decode_bytes(&b, ct.as_deref()),
            Err(_) => String::new(),
        };
        if html.contains("ticket=") {
            self.set_sms_pending(false);
            SessionState::Valid
        } else if Self::login_needs_sms(&html) || loc.contains("second") {
            self.set_sms_pending(true);
            SessionState::NeedSms
        } else {
            SessionState::Expired
        }
    }

    pub fn session_lost_html(html: &str) -> bool {
        html.contains("当前设备需进行身份验证")
            || html.contains("second_auth_form")
            || (html.contains("id=\"un\"")
                && html.contains("id=\"pd\"")
                && html.contains("index_login_btn"))
    }

    /// 深信服 / WAF / 一号通网关拦截。浏览器里会写成「访问被拒绝」。
    pub fn access_denied_html(html: &str) -> bool {
        let t = html;
        t.contains("访问被拒绝")
            || t.contains("Access Denied")
            || t.contains("403 Forbidden")
            || t.contains("请求被拒绝")
            || t.contains("您的访问被拦截")
            || t.contains("非法访问")
    }

    fn auth_denied_err() -> AppError {
        "一号通访问被拒绝。是这台电脑或当前网络被拦截，不是密码错。换校园网、关掉系统代理后再打开 pass.neu.edu.cn 试试。".into()
    }

    fn is_connect_err(e: &reqwest::Error) -> bool {
        e.is_timeout() || e.is_connect() || e.to_string().contains("timed out")
    }

    async fn send_get(&self, url: &str) -> Result<reqwest::Response> {
        let url = self.maybe_wrap(url);
        match self.http.get(&url).send().await {
            Ok(r) => Ok(r),
            Err(e) if Self::is_connect_err(&e) => {
                if let Some(via) = &self.http_via {
                    via.get(&url).send().await.map_err(Into::into)
                } else {
                    Err(e.into())
                }
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn get_text(&self, url: &str) -> Result<(String, Option<String>)> {
        let resp = self.send_get(url).await?;
        let ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let bytes = resp.bytes().await?;
        Ok((decode_bytes(&bytes, ct.as_deref()), ct))
    }

    async fn post_form(&self, url: &str, form: &HashMap<String, String>) -> Result<String> {
        let url = self.maybe_wrap(url);
        let resp = match self.http.post(&url).header(REFERER, &url).form(form).send().await {
            Ok(r) => r,
            Err(e) if Self::is_connect_err(&e) => {
                if let Some(via) = &self.http_via {
                    via.post(&url).header(REFERER, &url).form(form).send().await?
                } else {
                    return Err(e.into());
                }
            }
            Err(e) => return Err(e.into()),
        };
        let ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let bytes = resp.bytes().await?;
        Ok(decode_bytes(&bytes, ct.as_deref()))
    }

    async fn post_form_stop(
        &self,
        url: &str,
        form: &HashMap<String, String>,
    ) -> Result<(u16, Option<String>, String)> {
        let resp = self
            .http_stop
            .post(url)
            .header(REFERER, url)
            .form(form)
            .send()
            .await?;
        let status = resp.status().as_u16();
        let loc = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let bytes = resp.bytes().await?;
        Ok((status, loc, decode_bytes(&bytes, ct.as_deref())))
    }

    fn login_looks_ok(loc: Option<&str>, html: &str) -> bool {
        loc.map(|s| s.contains("ticket=")).unwrap_or(false) || html.contains("ticket=")
    }

    fn login_needs_sms(html: &str) -> bool {
        html.contains("当前设备需进行身份验证")
            || html.contains("second_auth_form")
            || html.contains("scendAuthCode")
            || html.contains("getScendAuthCode")
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<LoginOutcome> {
        self.login_ex(username, password, true).await
    }

    /// `force_credentials`: user typed 学号/密码. Drop TGT so CAS shows the form.
    /// Resume / auto-login keeps cookies so a trusted device can skip SMS.
    pub async fn login_ex(
        &self,
        username: &str,
        password: &str,
        force_credentials: bool,
    ) -> Result<LoginOutcome> {
        self.set_username(username);
        if !force_credentials {
            match self.session_state().await {
                SessionState::Valid => {
                    let _ = self.persist_cookies();
                    return Ok(LoginOutcome::Ok);
                }
                SessionState::NeedSms => {
                    let service = urlencoding::encode(SERVICE_CARD);
                    let login_url = format!("{LOGIN}?service={service}");
                    let page = self.peek_second_auth_page(&login_url).await?;
                    self.capture_sms_form(&page);
                    return self.need_sms(&page).await;
                }
                SessionState::Expired => {}
            }
        } else if !self.sms_pending()
            && self.has_tgt()
            && self.session_state().await == SessionState::Valid
        {
            let _ = self.persist_cookies();
            return Ok(LoginOutcome::Ok);
        }
        self.drop_tgt();
        let service = urlencoding::encode(SERVICE_CARD);
        let login_url = format!("{LOGIN}?service={service}");
        let (page, _) = self.get_text(&login_url).await?;
        if Self::access_denied_html(&page) {
            return Err(Self::auth_denied_err());
        }
        if Self::login_needs_sms(&page) {
            self.capture_sms_form(&page);
            return self.need_sms(&page).await;
        }
        let lt = attr_value(&page, "lt").ok_or("登录页没有 lt")?;
        let execution = attr_value(&page, "execution").unwrap_or_else(|| "e1s1".into());
        let mut key = DEFAULT_PUB_B64.to_string();
        if let Some(k) = extract_rsa_key(&page) {
            if crypto::parse_pub_key(&k).is_some() {
                key = k;
            }
        }
        let rsa = crypto::encrypt_rsa(username, password, &key)?;
        let mut form: HashMap<String, String> = hidden_fields(&page).into_iter().collect();
        form.insert("rsa".into(), rsa);
        form.insert("ul".into(), username.len().to_string());
        form.insert("pl".into(), password.len().to_string());
        form.insert("lt".into(), lt);
        form.insert("execution".into(), execution);
        form.insert("_eventId".into(), "submit".into());
        let (status, loc, html) = self.post_form_stop(&login_url, &form).await?;
        let _ = self.persist_cookies();
        if Self::access_denied_html(&html) {
            return Err(Self::auth_denied_err());
        }
        if Self::login_needs_sms(&html) {
            self.capture_sms_form(&html);
            return self.need_sms(&html).await;
        }
        if Self::login_looks_ok(loc.as_deref(), &html) {
            self.set_sms_pending(false);
            return Ok(LoginOutcome::Ok);
        }
        if self.has_tgt() && !Self::login_needs_sms(&html) {
            self.set_sms_pending(false);
            return Ok(LoginOutcome::Ok);
        }
        if (html.contains("密码") && html.contains("错误"))
            || html.contains("认证失败")
            || html.contains("账号不存在")
        {
            return Err("学号或密码不对".into());
        }
        Err(format!("登录未拿到会话（HTTP {status}）").into())
    }

    async fn need_sms(&self, html: &str) -> Result<LoginOutcome> {
        self.set_sms_pending(true);
        let mask = phone_mask(html);
        let (png, hint) = self.fetch_captcha().await?;
        Ok(LoginOutcome::NeedSms {
            phone_mask: mask,
            captcha_png: png,
            ocr_hint: hint,
        })
    }

    pub async fn fetch_captcha(&self) -> Result<(String, Option<String>)> {
        let ts = chrono::Local::now().timestamp_millis();
        let resp = self
            .http
            .get(format!("https://pass.neu.edu.cn/tpass/code?_={ts}"))
            .send()
            .await?;
        let bytes = resp.bytes().await?;
        let b64 = STANDARD.encode(&bytes);
        let hint = ocr::recognize(&bytes);
        Ok((b64, hint))
    }

    pub async fn send_sms(&self, captcha: &str) -> Result<()> {
        let captcha = captcha.trim();
        if captcha.is_empty() {
            return Err("请输入图形验证码".into());
        }
        if let Ok(g) = self.last_sms.lock() {
            if let Some(t) = *g {
                let elapsed = t.elapsed().as_secs();
                if elapsed < 60 {
                    return Err(format!("请 {} 秒后再获取短信码", 60 - elapsed).into());
                }
            }
        }
        let mut form: HashMap<String, String> = HashMap::new();
        form.insert("code".into(), captcha.to_string());
        form.insert("method".into(), "mobile".into());
        let resp = self
            .http
            .post("https://pass.neu.edu.cn/tpass/secondAuthCode")
            .header(REFERER, LOGIN)
            .header(reqwest::header::ACCEPT, "application/json, text/javascript, */*; q=0.01")
            .header("X-Requested-With", "XMLHttpRequest")
            .form(&form)
            .send()
            .await?;
        let ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let bytes = resp.bytes().await?;
        let body = decode_bytes(&bytes, ct.as_deref());
        parse_second_auth_send(&body, ct.as_deref())?;
        if let Ok(mut g) = self.last_sms.lock() {
            *g = Some(std::time::Instant::now());
        }
        if let Ok(mut g) = self.last_graphic.lock() {
            *g = Some(captcha.to_string());
        }
        Ok(())
    }

    pub async fn submit_sms(&self, sms: &str, _trust: bool) -> Result<LoginOutcome> {
        let sms = sms.trim();
        if sms.is_empty() {
            return Err("请输入短信码".into());
        }
        if !self.sms_pending() {
            match self.session_state().await {
                SessionState::Valid => return Ok(LoginOutcome::Ok),
                SessionState::NeedSms => self.set_sms_pending(true),
                SessionState::Expired => return Err("二次认证已过期，请重新登录".into()),
            }
        }
        let service = urlencoding::encode(SERVICE_CARD);
        let login_url = format!("{LOGIN}?service={service}");
        let mut form = self
            .sms_form
            .lock()
            .map_err(|e| e.to_string())?
            .clone();
        if !execution_is_second_step(form.get("execution").map(String::as_str).unwrap_or("")) {
            let html = self.peek_second_auth_page(&login_url).await?;
            if html.contains("ticket=") {
                self.set_sms_pending(false);
                let _ = self.persist_cookies();
                return Ok(LoginOutcome::Ok);
            }
            self.capture_sms_form(&html);
            form = self
                .sms_form
                .lock()
                .map_err(|e| e.to_string())?
                .clone();
        }
        form.retain(|k, _| keep_2fa_field(k));
        form.insert("_eventId".into(), "submit".into());
        form.insert("method".into(), "mobile".into());
        form.insert("scendAuthCode".into(), sms.to_string());
        if let Ok(g) = self.last_graphic.lock() {
            if let Some(graphic) = g.as_ref().filter(|s| !s.is_empty()) {
                form.insert("imgCode".into(), graphic.clone());
            }
        }
        form.remove("rememberMe");
        form.remove("code");
        let exec = form.get("execution").cloned().unwrap_or_default();
        if !execution_is_second_step(&exec) {
            return Err("二次验证已失效，请返回登录后重试".into());
        }
        let keys: Vec<String> = form.keys().cloned().collect();
        self.debug_auth(
            "submit_sms",
            &format!("sms_len={} execution={exec} keys={}", sms.len(), keys.join(",")),
        );
        let resp = self
            .http_stop
            .post(&login_url)
            .header(REFERER, &login_url)
            .header("Origin", "https://pass.neu.edu.cn")
            .form(&form)
            .send()
            .await?;
        let status = resp.status().as_u16();
        let loc = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let final_url = if loc.is_empty() {
            resp.url().to_string()
        } else {
            loc.clone()
        };
        let ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let html = decode_bytes(&resp.bytes().await?, ct.as_deref());
        self.persist_cookies()?;
        let still_sms = Self::login_needs_sms(&html);
        self.debug_auth(
            "submit_sms_result",
            &format!(
                "status={status} loc={final_url} still_sms={still_sms} tgt={} tip={}",
                self.has_tgt(),
                second_auth_error(&html)
            ),
        );
        if still_sms {
            let _ = fs::write(self.data_dir.join("sms-result.html"), html.as_bytes());
            self.capture_sms_form(&html);
            return Err(second_auth_error(&html).into());
        }
        if sms_submit_ok(&final_url, &html)
            || final_url.contains("ticket=")
            || final_url.contains("ecard.neu.edu.cn")
            || final_url.contains("portal.neu.edu.cn")
        {
            self.set_sms_pending(false);
            return Ok(LoginOutcome::Ok);
        }
        if (300..400).contains(&status) && !final_url.contains("tpass/login") {
            self.set_sms_pending(false);
            return Ok(LoginOutcome::Ok);
        }
        let _ = fs::write(self.data_dir.join("sms-result.html"), html.as_bytes());
        Err(second_auth_error(&html).into())
    }

    async fn peek_second_auth_page(&self, login_url: &str) -> Result<String> {
        let resp = self.http_stop.get(login_url).send().await?;
        let loc = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if loc.contains("ticket=") {
            return Ok(format!("ticket={loc}"));
        }
        let ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        Ok(decode_bytes(&resp.bytes().await?, ct.as_deref()))
    }

    #[allow(dead_code)]
    async fn try_device_sms(&self, sms: &str, trust: bool) -> Result<bool> {
        let Some(user) = self.username() else {
            return Ok(false);
        };
        let mut form: HashMap<String, String> = HashMap::new();
        form.insert("m".into(), "3".into());
        form.insert("u".into(), user);
        form.insert("c".into(), sms.to_string());
        form.insert("s".into(), if trust { "1".into() } else { "0".into() });
        let resp = self
            .http
            .post("https://pass.neu.edu.cn/tpass/device")
            .header(REFERER, LOGIN)
            .header("X-Requested-With", "XMLHttpRequest")
            .header(reqwest::header::ACCEPT, "application/json, text/javascript, */*; q=0.01")
            .form(&form)
            .send()
            .await?;
        let body = decode_bytes(&resp.bytes().await?, None);
        self.debug_auth("device_sms", &body.chars().take(200).collect::<String>());
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body.trim()) {
            let info = v
                .get("info")
                .or_else(|| v.get("status"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            return Ok(info == "ok" || info == "most");
        }
        Ok(false)
    }

    async fn cas_service(&self, service: &str) -> Result<(String, String)> {
        let _guard = self.cas_lock.lock().await;
        let url = format!("{LOGIN}?service={}", urlencoding::encode(service));
        let resp = self.http_stop.get(&url).send().await?;
        let loc = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if loc.contains("ticket=") {
            let fetch = if self.webvpn_on() && is_intranet_url(&loc) {
                encrypt_url_to_webvpn(&loc)
            } else {
                loc.clone()
            };
            self.debug_auth("cas_st", &format!("fetch={fetch}"));
            let (html, _) = self.get_text(&fetch).await?;
            if Self::access_denied_html(&html) {
                return Err(Self::auth_denied_err());
            }
            if Self::session_lost_html(&html) {
                return Err("会话已失效，请重新登录".into());
            }
            return Ok((loc, html));
        }
        let ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let final_url = resp.url().to_string();
        let html = decode_bytes(&resp.bytes().await?, ct.as_deref());
        if Self::access_denied_html(&html) {
            return Err(Self::auth_denied_err());
        }
        if Self::session_lost_html(&html) {
            return Err("会话已失效，请重新登录".into());
        }
        Ok((final_url, html))
    }

    pub async fn fetch_card(&self) -> Result<CardInfo> {
        let campus = on_campus_net();
        self.debug_auth(
            "card",
            &format!("campus={campus} vpn={}", self.webvpn_on()),
        );
        if campus {
            if let Ok(mut g) = self.webvpn_on.lock() {
                *g = false;
            }
            return self.fetch_card_once().await;
        }
        self.ensure_webvpn().await?;
        match self.fetch_card_once().await {
            Ok(c) => Ok(c),
            Err(e) => {
                self.debug_auth("card_retry", &e.to_string());
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                self.ensure_webvpn().await?;
                self.fetch_card_once().await
            }
        }
    }

    async fn fetch_card_once(&self) -> Result<CardInfo> {
        let (_loc, html) = self.cas_service(SERVICE_CARD).await?;
        if Self::session_lost_html(&html) {
            return Err("一卡通页未打开，校外请走 WebVPN".into());
        }
        if html.contains("username") && html.contains("auid") {
            let mut form: HashMap<String, String> = HashMap::new();
            if let Some(v) = attr_value(&html, "username") {
                form.insert("username".into(), v);
            }
            if let Some(v) = attr_value(&html, "timestamp") {
                form.insert("timestamp".into(), v);
            }
            if let Some(v) = attr_value(&html, "auid") {
                form.insert("auid".into(), v);
            }
            let _ = self
                .post_form("http://ecard.neu.edu.cn/selfsearch/SSOLogin.aspx", &form)
                .await;
        }
        let (page, _) = self
            .get_text("http://ecard.neu.edu.cn/selfsearch/User/baseinfo.aspx")
            .await?;
        if Self::session_lost_html(&page) {
            return Err("一卡通页未打开，校外请走 WebVPN".into());
        }
        let odd = input_id_value(&page, "ContentPlaceHolder1_txtOddFare")
            .or_else(|| attr_value(&page, "ctl00$ContentPlaceHolder1$txtOddFare"))
            .ok_or("页面上没有主钱包余额")?;
        let subsidy = input_id_value(&page, "ContentPlaceHolder1_txtSpecialOddFare")
            .or_else(|| input_id_value(&page, "ContentPlaceHolder1_txtSpecialOddfare"))
            .or_else(|| input_id_value(&page, "ContentPlaceHolder1_txtSubsidy"))
            .map(|s| parse_money(&s))
            .unwrap_or(0.0);
        let status = input_id_value(&page, "ContentPlaceHolder1_txtStatus")
            .unwrap_or_else(|| "正常".into());
        Ok(CardInfo {
            balance: parse_money(&odd),
            subsidy,
            status,
        })
    }

    pub async fn fetch_bills_month(&self, year: i32, month: u32) -> Result<(Vec<Bill>, bool)> {
        self.fetch_bills_month_ex(year, month, true).await
    }

    pub async fn fetch_bills_month_ex(
        &self,
        year: i32,
        month: u32,
        ensure_sso: bool,
    ) -> Result<(Vec<Bill>, bool)> {
        if ensure_sso {
            let _ = self.fetch_card().await;
        }
        let (page0, _) = self
            .get_text("http://ecard.neu.edu.cn/selfsearch/User/ConsumeInfo.aspx")
            .await?;
        let start = format!("{year:04}-{month:02}-01");
        let last = days_in_month(year, month);
        let today = crate::clock::today();
        let end_d = NaiveDate::from_ymd_opt(year, month, last).unwrap();
        let end = if NaiveDate::from_ymd_opt(year, month, 1).unwrap().with_day(1) == today.with_day(1)
            && today < end_d
        {
            today.format("%Y-%m-%d").to_string()
        } else {
            format!("{year:04}-{month:02}-{last:02}")
        };

        let mut hidden: HashMap<String, String> = hidden_fields(&page0).into_iter().collect();
        hidden.insert(
            "ctl00$ContentPlaceHolder1$rbtnType".into(),
            "0".into(),
        );
        hidden.insert("ctl00$ContentPlaceHolder1$txtStartDate".into(), start);
        hidden.insert("ctl00$ContentPlaceHolder1$txtEndDate".into(), end.clone());
        hidden.insert("ctl00$ContentPlaceHolder1$btnSearch".into(), "查  询".into());
        let mut html = self
            .post_form(
                "http://ecard.neu.edu.cn/selfsearch/User/ConsumeInfo.aspx",
                &hidden,
            )
            .await?;
        let mut bills = parse_bill_table(&html);
        let mut truncated = false;
        const MAX_BILL_PAGES: i32 = 10;
        for page in 2..=MAX_BILL_PAGES {
            if !html.contains("AspNetPager") || !has_next_page(&html, page) {
                break;
            }
            let mut h: HashMap<String, String> = hidden_fields(&html).into_iter().collect();
            h.insert(
                "__EVENTTARGET".into(),
                "ctl00$ContentPlaceHolder1$AspNetPager1".into(),
            );
            h.insert("__EVENTARGUMENT".into(), page.to_string());
            html = self
                .post_form(
                    "http://ecard.neu.edu.cn/selfsearch/User/ConsumeInfo.aspx",
                    &h,
                )
                .await?;
            let more = parse_bill_table(&html);
            if more.is_empty() {
                break;
            }
            bills.extend(more);
            if page == MAX_BILL_PAGES && has_next_page(&html, page + 1) {
                truncated = true;
            }
        }
        Ok((bills, truncated))
    }

    /// 门户 ST 之后要再打开 view 页，才会种上 getWlzzList 要的会话。
    async fn portal_wlzz(&self) -> Result<Option<serde_json::Value>> {
        // 门户种不上会话时，余额仍走缴费大厅，不要把整次网费打成「请重新登录」。
        if let Err(e) = self.cas_service(SERVICE_PORTAL).await {
            let es = e.to_string();
            if es.contains("访问被拒绝") {
                return Err(e);
            }
        }
        let _ = self
            .get_text("https://portal.neu.edu.cn/tp_up/view?m=up")
            .await;
        let resp = self
            .http
            .post("https://portal.neu.edu.cn/tp_up/up/subgroup/getWlzzList")
            .header(CONTENT_TYPE, "application/json;charset=UTF-8")
            .header(REFERER, "https://portal.neu.edu.cn/tp_up/view?m=up")
            .header("Origin", "https://portal.neu.edu.cn")
            .body("{}")
            .send()
            .await?;
        let ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let text = decode_bytes(&resp.bytes().await?, ct.as_deref());
        if Self::access_denied_html(&text) {
            return Err(Self::auth_denied_err());
        }
        // 门户页本身常写「统一身份认证」，不能当登录页。没 JSON 就当没种上会话。
        if Self::session_lost_html(&text) || text.contains("tpass/login") {
            return Ok(None);
        }
        Ok(serde_json::from_str(text.trim()).ok())
    }

    pub async fn fetch_net(&self, student_no: &str) -> Result<NetInfo> {
        let mut bytes: Option<u64> = None;
        let mut seconds: Option<u64> = None;
        let mut times: Option<u64> = None;
        let mut portal_bal: Option<f64> = None;
        let mut last_err: Option<String> = None;
        for attempt in 0..2u8 {
            match self.portal_wlzz().await {
                Ok(Some(v)) => {
                    portal_bal = json_find_f64(&v, &["user_balance", "userBalance", "balance"]);
                    bytes = json_find_u64(
                        &v,
                        &[
                            "sum_bytes",
                            "sumBytes",
                            "SUM_BYTES",
                            "used_bytes",
                            "usedBytes",
                            "usedFlow",
                            "used_flow",
                        ],
                    );
                    seconds = json_find_u64(&v, &["sum_seconds", "sumSeconds", "SUM_SECONDS"]);
                    times = json_find_u64(&v, &["sum_times", "sumTimes", "SUM_TIMES"]);
                    if bytes.is_some() {
                        break;
                    }
                    last_err = Some("门户网费没有流量字段".into());
                }
                Ok(None) => last_err = Some("门户网费还停在登录页".into()),
                Err(e) => {
                    last_err = Some(e.to_string());
                    if e.to_string().contains("访问被拒绝") {
                        break;
                    }
                }
            }
            if attempt == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
        if let Err(e) = self.cas_service(SERVICE_PAY).await {
            if e.to_string().contains("会话已失效") {
                return Err(e);
            }
        }
        let mut form: HashMap<String, String> = HashMap::new();
        form.insert("netNo".into(), student_no.to_string());
        form.insert("netType".into(), "1".into());
        form.insert("factorycode".into(), "N005".into());
        let mut pay_bal: Option<f64> = None;
        match self
            .http
            .post("https://pay.neu.edu.cn/queryNetAccBalance")
            .form(&form)
            .send()
            .await
        {
            Ok(resp) => {
                if let Ok(v) = resp.json::<serde_json::Value>().await {
                    pay_bal = v.get("netaccbal").and_then(|x| {
                        x.as_f64()
                            .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
                    });
                }
            }
            Err(e) => last_err = Some(e.to_string()),
        }
        let Some(balance) = pay_bal.or(portal_bal) else {
            return Err(last_err.unwrap_or_else(|| "网费查询失败".into()).into());
        };
        Ok(NetInfo {
            balance,
            month_bytes: bytes.unwrap_or(0),
            month_seconds: seconds.unwrap_or(0),
            month_times: times.unwrap_or(0),
            flow_from_portal: bytes.is_some(),
        })
    }

    pub async fn fetch_elec(&self, room: &str) -> Result<ElecInfo> {
        let room = room.trim().replace(['-', ' '], "");
        if room.is_empty() {
            return Ok(ElecInfo {
                unbound: true,
                remain_kwh: None,
                room_no: None,
                room_label: None,
                last_read_at: None,
            });
        }
        if let Err(e) = self.cas_service(SERVICE_PAY).await {
            if e.to_string().contains("会话已失效") {
                return Err(e);
            }
        }
        let mut check: HashMap<String, String> = HashMap::new();
        check.insert("id".into(), "414".into());
        let _ = self
            .http
            .post("https://pay.neu.edu.cn/checkProCouldPay")
            .form(&check)
            .send()
            .await;
        let mut form: HashMap<String, String> = HashMap::new();
        form.insert("elecroomno".into(), room.clone());
        form.insert("factorycode".into(), "E039".into());
        let resp = self
            .http
            .post("https://pay.neu.edu.cn/queryRommInfo")
            .form(&form)
            .send()
            .await?;
        let ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let text = decode_bytes(&resp.bytes().await?, ct.as_deref());
        let v: serde_json::Value = match serde_json::from_str(text.trim()) {
            Ok(v) => v,
            Err(_) => {
                if text.contains("维护") {
                    return Err("电费系统维护".into());
                }
                return Err("查电费失败".into());
            }
        };
        let code = v.get("returncode").and_then(|x| x.as_str()).unwrap_or("");
        if code != "SUCCESS" {
            let msg = v
                .get("returnmsg")
                .and_then(|x| x.as_str())
                .unwrap_or("查电费失败");
            return Err(msg.into());
        }
        // 维护页偶发仍标 SUCCESS 但没有余量字段；不要当成 0 度写回。
        let remain = json_elec_remain(&v).ok_or("电费余量暂时读不到")?;
        Ok(ElecInfo {
            unbound: false,
            remain_kwh: Some(remain),
            room_no: Some(room),
            room_label: v
                .get("elecroominfo")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            last_read_at: v
                .get("lastReadTime")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
        })
    }

    async fn jwxt_get(&self, path_and_query: &str) -> Result<serde_json::Value> {
        let url = self.maybe_wrap(&format!("{JWXT_API}{path_and_query}"));
        let referer = self.maybe_wrap(JWXT_REFERER);
        let resp = match self
            .http
            .get(&url)
            .header("Fetch-Api", "true")
            .header(REFERER, &referer)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) if Self::is_connect_err(&e) => {
                if let Some(via) = &self.http_via {
                    via.get(&url)
                        .header("Fetch-Api", "true")
                        .header(REFERER, &referer)
                        .send()
                        .await?
                } else {
                    return Err(e.into());
                }
            }
            Err(e) => return Err(e.into()),
        };
        let status = resp.status().as_u16();
        let ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let bytes = resp.bytes().await?;
        let text = decode_bytes(&bytes, ct.as_deref());
        if looks_like_login_page(&text) {
            return Err("校外连不上教务，连校园网后再试。".into());
        }
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) => Ok(v),
            Err(_) => Err(format!("教务返回不是课表 JSON（HTTP {status}）").into()),
        }
    }

    /// 整学期拉一次。调用方决定是否跳过（有缓存且学期没变则不要打）。
    pub async fn fetch_semester(&self) -> Result<SemesterTable> {
        if on_campus_net() {
            if let Ok(mut g) = self.webvpn_on.lock() {
                *g = false;
            }
            return self.fetch_semester_once().await;
        }
        self.ensure_webvpn().await?;
        self.fetch_semester_once().await
    }

    async fn fetch_semester_once(&self) -> Result<SemesterTable> {
        if let Err(e) = self.cas_service(SERVICE_JWXT).await {
            let es = e.to_string();
            if es.contains("会话已失效") && !self.webvpn_on() {
                return Err("校外连不上教务，连校园网后再试。".into());
            }
        }
        let today = crate::clock::today();
        let cal = match self
            .jwxt_get("/api/home/student/schoolCalendars.do")
            .await
        {
            Ok(v) => parse_calendars(&v, today),
            Err(e) => {
                // 校历失败仍可猜学期再拉格子课表
                let guess = crate::course::guess_term(today);
                if e.to_string().contains("校外连不上") {
                    return Err(e);
                }
                crate::course::CalendarHit {
                    label: crate::course::term_label(&guess, None),
                    week1_monday: crate::course::default_week1(&guess),
                    term_code: guess,
                }
            }
        };
        let term = cal.term_code.clone();
        let detail = match self
            .jwxt_get(&format!(
                "/api/home/student/getMyScheduleDetail.do?termCode={}",
                urlencoding::encode(&term)
            ))
            .await
        {
            Ok(v) => v,
            Err(_) => {
                self.jwxt_get(&format!(
                    "/api/home/student/getMyScheduleDetail.do?XNXQDM={}",
                    urlencoding::encode(&term)
                ))
                .await?
            }
        };
        let arranged = parse_arranged_list(&detail);
        let scheduled = match self
            .jwxt_get(&format!(
                "/api/home/student/getMyScheduledCampus.do?termCode={}",
                urlencoding::encode(&term)
            ))
            .await
        {
            Ok(v) => parse_scheduled_campus(&v),
            Err(_) => None,
        };
        let term_weeks = match self
            .jwxt_get(&format!(
                "/api/home/getTermWeeks.do?termCode={}",
                urlencoding::encode(&term)
            ))
            .await
        {
            Ok(v) => parse_term_weeks(&v),
            Err(_) => Vec::new(),
        };
        let mut courses = parse_courses(&arranged, scheduled);
        if let Some(weekly) = self
            .fetch_weekly_courses(&term, scheduled, &arranged, &term_weeks)
            .await
        {
            courses = weekly;
        } else {
            let mut catalog = None;
            for q in [
                format!(
                    "/api/home/student/courses.do?termCode={}",
                    urlencoding::encode(&term)
                ),
                format!(
                    "/api/home/student/courses.do?XNXQDM={}",
                    urlencoding::encode(&term)
                ),
            ] {
                if let Ok(v) = self.jwxt_get(&q).await {
                    if catalog_has_courses(&v) {
                        catalog = Some(v);
                        break;
                    }
                    if catalog.is_none() {
                        catalog = Some(v);
                    }
                }
            }
            if let Some(cat) = catalog {
                courses = enrich_from_catalog(courses, &cat);
            }
        }
        Ok(build_semester(
            cal,
            courses,
            Local::now().to_rfc3339(),
            term_weeks,
        ))
    }

    async fn fetch_week_arranged(&self, term: &str, week: u32) -> Option<Vec<serde_json::Value>> {
        let q = format!(
            "/api/home/student/getMyScheduleDetail.do?termCode={}&type=week&week={}",
            urlencoding::encode(term),
            week
        );
        let v = self.jwxt_get(&q).await.ok()?;
        Some(parse_arranged_list(&v))
    }

    /// 网页「周课表」会带 week 参数；服务端按教学周筛格子。同步时拉 1..N 周各一次，
    /// 用点名结果当 weeks[]，不再猜中文。日常刷新仍不打教务。
    async fn fetch_weekly_courses(
        &self,
        term: &str,
        scheduled: Option<crate::periods::Campus>,
        full: &[serde_json::Value],
        term_weeks: &[crate::models::TermWeek],
    ) -> Option<Vec<Course>> {
        if full.is_empty() {
            return None;
        }
        let probe = term_weeks
            .iter()
            .find(|w| {
                crate::clock::today()
                    .format("%Y-%m-%d")
                    .to_string()
                    .as_str()
                    >= w.start.get(..10).unwrap_or(&w.start)
                    && crate::clock::today()
                        .format("%Y-%m-%d")
                        .to_string()
                        .as_str()
                        <= w.end.get(..10).unwrap_or(&w.end)
            })
            .map(|w| w.n)
            .or_else(|| term_weeks.iter().map(|w| w.n).find(|n| *n >= 3))
            .unwrap_or(5);
        let weekly = self.fetch_week_arranged(term, probe).await?;
        if !week_param_filters(full, &weekly) {
            return None;
        }
        let mut serials: Vec<u32> = term_weeks
            .iter()
            .map(|w| w.n)
            .filter(|n| (1..=22).contains(n))
            .collect();
        if serials.is_empty() {
            serials = (1..=18).collect();
        }
        let mut parts: Vec<Vec<Course>> = Vec::new();
        for chunk in serials.chunks(3) {
            match chunk {
                [a, b, c] => {
                    let (ra, rb, rc) = tokio::join!(
                        self.fetch_week_arranged(term, *a),
                        self.fetch_week_arranged(term, *b),
                        self.fetch_week_arranged(term, *c),
                    );
                    for (w, hit) in [(*a, ra), (*b, rb), (*c, rc)] {
                        if let Some(items) = hit {
                            parts.push(stamp_courses_week(parse_courses(&items, scheduled), w));
                        }
                    }
                }
                [a, b] => {
                    let (ra, rb) = tokio::join!(
                        self.fetch_week_arranged(term, *a),
                        self.fetch_week_arranged(term, *b),
                    );
                    for (w, hit) in [(*a, ra), (*b, rb)] {
                        if let Some(items) = hit {
                            parts.push(stamp_courses_week(parse_courses(&items, scheduled), w));
                        }
                    }
                }
                [a] => {
                    if let Some(items) = self.fetch_week_arranged(term, *a).await {
                        parts.push(stamp_courses_week(parse_courses(&items, scheduled), *a));
                    }
                }
                _ => {}
            }
        }
        let merged = merge_weekly_stamped(parts);
        if merged.is_empty() {
            None
        } else {
            Some(merged)
        }
    }
}

fn keep_2fa_field(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "execution"
            | "relaystate"
            | "_eventid"
            | "method"
            | "scendauthcode"
            | "secondauthcode"
            | "imgcode"
    )
}

fn execution_is_second_step(exec: &str) -> bool {
    exec.split('s')
        .nth(1)
        .and_then(|n| n.parse::<u32>().ok())
        .unwrap_or(0)
        >= 2
}

fn detect_sms_field(html: &str) -> String {
    let doc = Html::parse_document(html);
    if let Ok(sel) = Selector::parse("#scendAuthCode") {
        if let Some(el) = doc.select(&sel).next() {
            if let Some(n) = el.value().attr("name").filter(|s| !s.is_empty()) {
                return n.to_string();
            }
            return "scendAuthCode".into();
        }
    }
    if let Ok(sel) = Selector::parse("#second_auth_form input") {
        for el in doc.select(&sel) {
            let typ = el.value().attr("type").unwrap_or("text").to_ascii_lowercase();
            if typ == "hidden" || typ == "checkbox" || typ == "submit" || typ == "button" {
                continue;
            }
            if let Some(n) = el.value().attr("name").filter(|s| !s.is_empty()) {
                return n.to_string();
            }
        }
    }
    "scendAuthCode".into()
}

fn hidden_in_form(html: &str, form_sel: &str) -> HashMap<String, String> {
    let doc = Html::parse_document(html);
    let Ok(sel) = Selector::parse(&format!("{form_sel} input")) else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for el in doc.select(&sel) {
        let Some(name) = el.value().attr("name") else {
            continue;
        };
        if name.eq_ignore_ascii_case("scendAuthCode") {
            continue;
        }
        let value = el.value().attr("value").unwrap_or("").to_string();
        out.insert(name.to_string(), value);
    }
    out
}

fn prefer_second_execution(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    let Ok(sel) = Selector::parse("#second_auth_form input[name=execution]") else {
        return None;
    };
    doc.select(&sel)
        .filter_map(|el| el.value().attr("value").map(|s| s.to_string()))
        .find(|s| execution_is_second_step(s))
}

fn sms_submit_ok(final_url: &str, html: &str) -> bool {
    if final_url.contains("ticket=") || html.contains("ticket=") {
        return true;
    }
    let on_cas_login = final_url.contains("tpass/login");
    !on_cas_login && !NeuClient::login_needs_sms(html)
}

fn is_instructional_text(t: &str) -> bool {
    let t = t.trim();
    t.is_empty()
        || t.contains("为保障")
        || t.contains("当前设备需进行身份验证")
        || t.contains("账号不存在")
        || t.contains("统一身份认证")
        || t.chars().count() > 40
}

fn looks_like_html(t: &str) -> bool {
    let s = t.trim_start().to_ascii_lowercase();
    s.starts_with("<!doctype")
        || s.starts_with("<html")
        || s.starts_with("<form")
        || s.contains("<html")
        || t.contains("当前设备需进行身份验证")
        || t.contains("second_auth_form")
}

fn second_auth_error(html: &str) -> String {
    for id in ["errormsghide", "errormsg", "tip"] {
        if let Some(t) = crate::util::input_id_value(html, id) {
            let t = t.trim();
            if !is_instructional_text(t) {
                return t.to_string();
            }
        }
    }
    if html.contains("验证码超时") || html.contains("已过期") {
        "短信码已过期，请重新获取".into()
    } else {
        "短信码不对".into()
    }
}

fn parse_second_auth_send(body: &str, content_type: Option<&str>) -> Result<()> {
    let t = body.trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
        if v.get("success").and_then(|x| x.as_bool()) == Some(true) {
            return Ok(());
        }
        let msg = v
            .get("message")
            .or_else(|| v.get("msg"))
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty() && !is_instructional_text(s))
            .unwrap_or("图形码不对");
        return Err(msg.into());
    }
    let ct = content_type.unwrap_or("").to_ascii_lowercase();
    if ct.contains("json") {
        return Err("图形码不对".into());
    }
    if looks_like_html(t) {
        return Err("发送失败，请返回登录后重试".into());
    }
    Err("图形码不对".into())
}

fn value_as_u64(x: &serde_json::Value) -> Option<u64> {
    x.as_u64()
        .or_else(|| x.as_i64().map(|n| n.max(0) as u64))
        .or_else(|| x.as_f64().map(|n| n.max(0.0) as u64))
        .or_else(|| {
            x.as_str().and_then(|s| {
                let t = s.trim().replace(',', "");
                t.parse::<u64>()
                    .ok()
                    .or_else(|| t.parse::<f64>().ok().map(|n| n.max(0.0) as u64))
            })
        })
}

fn json_elec_remain(v: &serde_json::Value) -> Option<f64> {
    let x = v.get("elecRemain")?;
    match x {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => {
            if s.chars().any(|c| c.is_ascii_digit()) {
                Some(parse_money(s))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn value_as_f64(x: &serde_json::Value) -> Option<f64> {
    x.as_f64()
        .or_else(|| x.as_i64().map(|n| n as f64))
        .or_else(|| x.as_u64().map(|n| n as f64))
        .or_else(|| x.as_str().and_then(|s| s.trim().parse().ok()))
}

fn json_find_u64(v: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    json_find(v, keys, value_as_u64)
}

fn json_find_f64(v: &serde_json::Value, keys: &[&str]) -> Option<f64> {
    json_find(v, keys, value_as_f64)
}

fn json_find<T>(v: &serde_json::Value, keys: &[&str], conv: fn(&serde_json::Value) -> Option<T>) -> Option<T> {
    match v {
        serde_json::Value::Object(map) => {
            for k in keys {
                if let Some(x) = map.get(*k) {
                    if let Some(n) = conv(x) {
                        return Some(n);
                    }
                    if let Some(s) = x.as_str() {
                        if let Ok(inner) = serde_json::from_str::<serde_json::Value>(s) {
                            if let Some(n) = json_find(&inner, keys, conv) {
                                return Some(n);
                            }
                        }
                    }
                }
            }
            for x in map.values() {
                if let Some(n) = json_find(x, keys, conv) {
                    return Some(n);
                }
            }
            None
        }
        serde_json::Value::Array(arr) => {
            for x in arr {
                if let Some(n) = json_find(x, keys, conv) {
                    return Some(n);
                }
            }
            None
        }
        _ => None,
    }
}

fn has_next_page(html: &str, page: i32) -> bool {
    html.contains(&format!("'{page}'")) || html.contains(&format!(">{page}<"))
}

fn parse_bill_table(html: &str) -> Vec<Bill> {
    let doc = Html::parse_document(html);
    let Ok(tr_sel) = Selector::parse("table tr") else {
        return vec![];
    };
    let Ok(td_sel) = Selector::parse("td") else {
        return vec![];
    };
    let re_dt = Regex::new(r"\d{4}[-/]\d{1,2}[-/]\d{1,2}").ok();
    let mut out = Vec::new();
    for tr in doc.select(&tr_sel) {
        let tds: Vec<String> = tr
            .select(&td_sel)
            .map(|td| td.text().collect::<String>().trim().to_string())
            .collect();
        if tds.len() < 5 {
            continue;
        }
        let time = tds[0].clone();
        if re_dt
            .as_ref()
            .map(|r| !r.is_match(&time))
            .unwrap_or(true)
        {
            continue;
        }
        let subject = tds[1].clone();
        if is_junk_bill_subject(&subject) {
            continue;
        }
        let mut amount = parse_money(&tds[2]);
        let station_peek = tds.get(7).cloned().or_else(|| tds.get(6).cloned());
        if amount.abs() < 0.005
            && station_peek
                .as_deref()
                .is_some_and(|s| s.contains("物联网采集"))
        {
            continue;
        }
        let is_recharge = subject.contains("充值")
            || subject.contains("转账")
            || subject.contains("补助");
        if amount > 0.0 && !is_recharge {
            amount = -amount;
        }
        if amount < 0.0 && is_recharge {
            amount = amount.abs();
        }
        let fee = if tds.len() > 3 {
            Some(parse_money(&tds[3]))
        } else {
            None
        };
        let balance_after = if tds.len() > 4 {
            parse_money(&tds[4])
        } else {
            0.0
        };
        let station = tds.get(7).cloned().or_else(|| tds.get(6).cloned());
        let iso = normalize_time(&time);
        out.push(Bill {
            time: iso,
            subject,
            amount,
            fee,
            balance_after,
            station,
        });
    }
    out
}

fn is_junk_bill_subject(subject: &str) -> bool {
    let s = subject.trim();
    if s.is_empty() {
        return false;
    }
    if s.contains("物联网采集") {
        return true;
    }
    Regex::new(r"^\d{4}\s*[-/年.]\s*\d{1,2}\s*[-/月.]\s*\d{1,2}")
        .ok()
        .map(|r| r.is_match(s))
        .unwrap_or(false)
}

fn pad2(s: &str) -> String {
    format!("{:02}", s.parse::<u32>().unwrap_or(0))
}

fn normalize_time(raw: &str) -> String {
    let t = raw
        .replace('/', "-")
        .replace('年', "-")
        .replace('月', "-")
        .replace('日', " ");
    let re = Regex::new(
        r"(\d{4})-(\d{1,2})-(\d{1,2})(?:[^\d]{0,4}(\d{1,2}):(\d{2})(?::(\d{2}))?)?",
    )
    .ok();
    if let Some(c) = re.as_ref().and_then(|r| r.captures(&t)) {
        let y = &c[1];
        let m = pad2(&c[2]);
        let d = pad2(&c[3]);
        let hh = c.get(4).map(|x| pad2(x.as_str())).unwrap_or_else(|| "00".into());
        let mm = c.get(5).map(|x| pad2(x.as_str())).unwrap_or_else(|| "00".into());
        let ss = c.get(6).map(|x| pad2(x.as_str())).unwrap_or_else(|| "00".into());
        return format!("{y}-{m}-{d}T{hh}:{mm}:{ss}");
    }
    t.chars().take(32).collect()
}

pub fn aggregate_month(year: i32, month: u32, bills: &[Bill]) -> Vec<DaySpend> {
    let last = days_in_month(year, month);
    let mut days: Vec<DaySpend> = (1..=last)
        .map(|d| DaySpend {
            date: format!("{year:04}-{month:02}-{d:02}"),
            spend: 0.0,
            recharge: 0.0,
            count: 0,
            bills: vec![],
        })
        .collect();
    for b in bills {
        let date = b.time.get(..10).unwrap_or("");
        if let Some(day) = days.iter_mut().find(|x| x.date == date) {
            if b.amount < 0.0 {
                day.spend += -b.amount;
                day.count += 1;
                day.bills.push(b.clone());
            } else if b.amount > 0.0 {
                day.recharge += b.amount;
                day.bills.push(b.clone());
            }
            // 钱包交易 0.00 且科目是时间串的行丢掉，不当一笔明细
        }
    }
    for d in &mut days {
        d.bills.sort_by(|a, b| b.time.cmp(&a.time));
    }
    days
}

pub fn save_month_cache(dir: &Path, year: i32, month: u32, spend: &[DaySpend]) -> Result<()> {
    let p = dir.join(format!("bills-{}.json", yyyy_mm(year, month)));
    fs::write(p, serde_json::to_vec_pretty(spend)?)?;
    Ok(())
}

pub fn load_month_cache(dir: &Path, year: i32, month: u32) -> Option<Vec<DaySpend>> {
    let p = dir.join(format!("bills-{}.json", yyyy_mm(year, month)));
    let raw = fs::read(p).ok()?;
    serde_json::from_slice(&raw).ok()
}

pub fn month_cache_fresh(dir: &Path, year: i32, month: u32, max_age_secs: u64) -> Option<Vec<DaySpend>> {
    let p = dir.join(format!("bills-{}.json", yyyy_mm(year, month)));
    let age = fs::metadata(&p)
        .ok()?
        .modified()
        .ok()?
        .elapsed()
        .ok()?
        .as_secs();
    if age > max_age_secs {
        return None;
    }
    load_month_cache(dir, year, month)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "hits pass.neu.edu.cn; run with --ignored"]
    async fn login_live() {
        let dir = std::env::temp_dir().join("nidong-login-test");
        let _ = std::fs::remove_dir_all(&dir);
        let c = NeuClient::new(dir).expect("client");
        let user = std::env::var("NIDONG_USER").expect("set NIDONG_USER");
        let pass = std::env::var("NIDONG_PASS").expect("set NIDONG_PASS");
        match c.login(&user, &pass).await {
            Ok(out) => println!("LOGIN_OK {out:?} tgt={}", c.has_tgt()),
            Err(e) => panic!("LOGIN_ERR {e}"),
        }
        match c.fetch_card().await {
            Ok(card) => println!("CARD_OK {card:?}"),
            Err(e) => println!("CARD_ERR {e}"),
        }
    }

    #[test]
    fn session_lost_html_does_not_flag_ecard_sso() {
        assert!(NeuClient::session_lost_html("当前设备需进行身份验证"));
        assert!(NeuClient::session_lost_html(
            r#"<input id="un"/><input id="pd"/><input id="index_login_btn"/>"#
        ));
        assert!(!NeuClient::session_lost_html("门户里有统一身份认证入口"));
        assert!(!NeuClient::session_lost_html(
            "<form action='http://ecard.neu.edu.cn/selfsearch/SSOLogin.aspx'><input name='username'/>"
        ));
    }

    #[test]
    fn access_denied_is_not_bad_password() {
        assert!(NeuClient::access_denied_html("访问被拒绝"));
        assert!(NeuClient::access_denied_html("Access Denied by WAF"));
        assert!(!NeuClient::access_denied_html("用户名或密码错误"));
    }

    #[test]
    fn password_page_is_not_sms_challenge() {
        let html = r#"<form id="loginForm"><input name="execution" value="e1s1"/><input id="lt" name="lt" value="LT-1"/><script id="mobile_template" type="text/html">手机登录</script></form>"#;
        assert!(
            !NeuClient::login_needs_sms(html),
            "password page templates must not look like 2FA"
        );
    }

    #[test]
    fn second_auth_page_is_sms_challenge() {
        let html = r#"<form id="second_auth_form"><input name="execution" value="e1s2"/><input id="scendAuthCode" name="scendAuthCode"/><p>当前设备需进行身份验证</p></form>"#;
        assert!(NeuClient::login_needs_sms(html));
        assert!(NeuClient::login_looks_ok(Some("https://ecard.neu.edu.cn/x?ticket=ST-1"), ""));
        assert!(!NeuClient::login_looks_ok(None, "second_auth_form"));
    }

    #[test]
    fn second_auth_error_reads_tip() {
        let html = r#"<span id="errormsghide">验证码超时</span>"#;
        assert!(second_auth_error(html).contains("超时") || second_auth_error(html).contains("过期"));
    }

    #[test]
    fn second_auth_error_ignores_page_heading() {
        let html = r#"<div id="tip">为保障您的账号安全，当前设备需进行身份验证</div>"#;
        assert_eq!(second_auth_error(html), "短信码不对");
    }

    #[test]
    fn second_auth_send_parses_school_json() {
        parse_second_auth_send(r#"{"success":true}"#, Some("application/json")).unwrap();
        let err = parse_second_auth_send(
            r#"{"success":false,"message":"验证码错误"}"#,
            Some("application/json"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("验证码错误"));
        let err = parse_second_auth_send(
            r#"{"success":false,"message":"请稍后再试"}"#,
            Some("application/json"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("请稍后再试"));
        let err = parse_second_auth_send(
            "<html><body>为保障您的账号安全，当前设备需进行身份验证 错误</body></html>",
            Some("text/html"),
        )
        .unwrap_err();
        assert!(
            !err.to_string().contains("图形码不对"),
            "{}",
            err
        );
    }

    #[test]
    fn sms_form_reads_execution_from_page() {
        let dir = std::env::temp_dir().join("nidong-sms-form-test");
        let _ = std::fs::create_dir_all(&dir);
        let c = NeuClient::new(dir).expect("client");
        c.capture_sms_form(
            r#"<form id="second_auth_form"><input type="hidden" name="execution" value="e2s2"/><input type="hidden" name="RelayState" value="e2s2"/><input type="hidden" name="_eventId" value="submit"/></form>"#,
        );
        let g = c.sms_form.lock().unwrap();
        assert_eq!(g.get("execution").map(String::as_str), Some("e2s2"));
        assert_eq!(g.get("RelayState").map(String::as_str), Some("e2s2"));
        assert_eq!(g.get("method").map(String::as_str), Some("mobile"));
    }

    #[test]
    fn keep_2fa_field_drops_password_rsa() {
        assert!(keep_2fa_field("execution"));
        assert!(keep_2fa_field("scendAuthCode"));
        assert!(keep_2fa_field("method"));
        assert!(keep_2fa_field("imgCode"));
        assert!(!keep_2fa_field("code"));
        assert!(!keep_2fa_field("rsa"));
        assert!(execution_is_second_step("e5s2"));
        assert!(execution_is_second_step("e1s2"));
        assert!(!execution_is_second_step("e5s1"));
        assert!(!keep_2fa_field("ul"));
        assert!(!keep_2fa_field("pl"));
        assert!(!keep_2fa_field("un"));
    }

    #[test]
    fn detect_sms_field_uses_name_attr() {
        let html = r#"<form id="second_auth_form"><input id="scendAuthCode" name="secondAuthCode" maxlength="6"/></form>"#;
        assert_eq!(detect_sms_field(html), "secondAuthCode");
    }

    #[test]
    fn sms_form_prefers_second_auth_execution() {
        let dir = std::env::temp_dir().join("nidong-sms-form-s2");
        let _ = std::fs::create_dir_all(&dir);
        let c = NeuClient::new(dir).expect("client");
        c.capture_sms_form(
            r#"<form id="loginForm"><input type="hidden" name="execution" value="e1s1"/></form>
               <form id="second_auth_form"><input type="hidden" name="execution" value="e1s2"/><input type="hidden" name="RelayState" value="e1s2"/><input id="scendAuthCode" name="scendAuthCode"/></form>"#,
        );
        let g = c.sms_form.lock().unwrap();
        assert_eq!(g.get("execution").map(String::as_str), Some("e1s2"));
        assert!(g.get("scendAuthCode").is_none());
    }

    #[test]
    fn junk_datetime_subject_is_skipped() {
        assert!(is_junk_bill_subject("2026/8/31 6:49:50"));
        assert!(is_junk_bill_subject("2026/8/3 16:49:50"));
        assert!(is_junk_bill_subject("2026-09-01 06:54:06"));
        assert!(is_junk_bill_subject("物联网采集工作站"));
        assert!(!is_junk_bill_subject("餐费支出"));
    }

    #[test]
    fn parse_bill_skips_zero_datetime_row() {
        let html = r#"<table>
          <tr><td>2026/8/3 16:49:50</td><td>2026/8/3 16:49:50</td><td>0.00</td><td>16.20</td><td>0.00</td><td></td><td></td><td>物联网采集工作站</td></tr>
          <tr><td>2026/8/31 6:49:50</td><td>餐费支出</td><td>16.20</td><td>0.00</td><td>240.47</td><td></td><td></td><td>浑南食堂一楼84#</td></tr>
        </table>"#;
        let bills = parse_bill_table(html);
        assert_eq!(bills.len(), 1);
        assert_eq!(bills[0].subject, "餐费支出");
        assert!((bills[0].amount + 16.2).abs() < 0.01);
        assert_eq!(bills[0].station.as_deref(), Some("浑南食堂一楼84#"));
    }

    #[test]
    fn sms_submit_ok_when_redirected_off_cas() {
        assert!(sms_submit_ok(
            "http://ecard.neu.edu.cn/selfsearch/SSOLogin.aspx",
            "<html>校园卡</html>"
        ));
        assert!(sms_submit_ok(
            "https://pass.neu.edu.cn/tpass/login?service=x&ticket=ST-1",
            ""
        ));
        assert!(!sms_submit_ok(
            "https://pass.neu.edu.cn/tpass/login?service=x",
            r#"<form id="second_auth_form"><input name="scendAuthCode"/><p>当前设备需进行身份验证</p></form>"#
        ));
    }

    #[test]
    fn normalize_ascii_datetime() {
        assert_eq!(
            normalize_time("2026-09-08 12:30:01"),
            "2026-09-08T12:30:01"
        );
        assert_eq!(normalize_time("2026/9/8"), "2026-09-08T00:00:00");
    }

    #[test]
    fn normalize_chinese_prefix_no_panic() {
        assert_eq!(
            normalize_time("消费时间 2026-09-08 12:00"),
            "2026-09-08T12:00:00"
        );
        let _ = normalize_time("消费时间地点金额余额操作");
        let _ = normalize_time("2026-09-8时间12:00");
    }

    #[test]
    fn nested_sum_bytes() {
        let v = serde_json::json!({
            "code": "0",
            "datas": { "user_balance": "0", "sum_bytes": "2038540630", "sum_seconds": "232251" }
        });
        assert_eq!(json_find_u64(&v, &["sum_bytes", "sumBytes"]), Some(2038540630));
        assert_eq!(json_find_f64(&v, &["user_balance"]).unwrap(), 0.0);
        let v2 = serde_json::json!([{ "sum_bytes": "1024" }]);
        assert_eq!(json_find_u64(&v2, &["sum_bytes"]), Some(1024));
        let miss = serde_json::json!({ "user_balance": "12.5" });
        assert_eq!(json_find_u64(&miss, &["sum_bytes"]), None);
    }
}
