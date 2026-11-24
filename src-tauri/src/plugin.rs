//! 泥洞插件宿主（WebView 插件 + MCP Stdio 插件兼容层）。
//!
//! 插件统一安装在 `%APPDATA%\edu.neu.nidong\plugins\<plugin-id>\`，
//! 与泥洞本体仓库完全隔离，不会进入本体的 Git 仓库。

use crate::error::Result;
use crate::models::AgendaItem;
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

fn default_runtime() -> String {
    "webview".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// `webview` | `mcp` | `hybrid`
    #[serde(default = "default_runtime")]
    pub runtime: String,
    /// 相对插件根目录的 HTML 入口，例如 `ui/index.html`
    #[serde(default)]
    pub entry: Option<String>,
    /// 是否在后台常驻运行（随泥洞脉冲定时巡检）
    #[serde(default)]
    pub background: bool,
    /// 权限列表，如 `agenda:write`、`island:notify`、`http:*.icourse163.org`、`mcp:stdio`
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub mcp: Option<McpConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub manifest: PluginManifest,
    pub enabled: bool,
    pub has_ui: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_badge: Option<String>,
}

/// 单文件打包格式（`.ndplugin`），便于跨平台零依赖分发。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginBundle {
    pub manifest: PluginManifest,
    #[serde(default)]
    pub files: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RegistryState {
    #[serde(default)]
    disabled: Vec<String>,
}

pub struct PluginHub {
    data_dir: PathBuf,
    badges: Mutex<HashMap<String, String>>,
}

impl PluginHub {
    pub fn open(data_dir: PathBuf) -> Self {
        let plugins_dir = data_dir.join("plugins");
        let _ = fs::create_dir_all(&plugins_dir);
        Self {
            data_dir,
            badges: Mutex::new(HashMap::new()),
        }
    }

    pub fn plugins_dir(&self) -> PathBuf {
        self.data_dir.join("plugins")
    }
}

/// 后台定时脉冲：每 60 秒向工作区发送 `plugin-tick`，唤醒后台常驻插件巡检。
pub fn spawn_ticker(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let now = crate::clock::now().to_rfc3339();
            let _ = app.emit("plugin-tick", now);
        }
    });
}

pub fn validate_plugin_id(id: &str) -> Result<()> {
    let trimmed = id.trim();
    if trimmed.is_empty() || trimmed.len() > 80 {
        return Err("插件 ID 长度需在 1–80 字符之间".into());
    }
    if trimmed.starts_with('.') || trimmed.contains("..") {
        return Err("插件 ID 包含非法路径字符".into());
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err("插件 ID 只能包含字母、数字、点、横线和下划线".into());
    }
    Ok(())
}

pub fn agenda_prefix(plugin_id: &str) -> String {
    format!("plg:{plugin_id}:")
}

pub fn make_plugin_agenda_id(plugin_id: &str, raw_id: &str) -> String {
    let prefix = agenda_prefix(plugin_id);
    let clean = raw_id.trim();
    if clean.starts_with(&prefix) {
        clean.to_string()
    } else if clean.is_empty() {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        format!("{prefix}{ts:x}")
    } else {
        format!("{prefix}{clean}")
    }
}

pub fn has_permission(manifest: &PluginManifest, required: &str) -> bool {
    manifest
        .permissions
        .iter()
        .any(|p| p.trim().eq_ignore_ascii_case(required))
}

pub fn has_http_permission(manifest: &PluginManifest, target_url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(target_url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host_lower = host.to_ascii_lowercase();
    for perm in &manifest.permissions {
        let p = perm.trim().to_ascii_lowercase();
        let Some(pat) = p.strip_prefix("http:") else {
            continue;
        };
        let pat = pat.trim();
        if pat == "*" {
            return true;
        }
        if let Some(suffix) = pat.strip_prefix("*.") {
            if host_lower == suffix || host_lower.ends_with(&format!(".{suffix}")) {
                return true;
            }
        } else if host_lower == pat {
            return true;
        }
    }
    false
}

fn load_registry(plugins_dir: &Path) -> RegistryState {
    fs::read(plugins_dir.join("registry.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_registry(plugins_dir: &Path, reg: &RegistryState) -> Result<()> {
    let _ = fs::create_dir_all(plugins_dir);
    fs::write(
        plugins_dir.join("registry.json"),
        serde_json::to_vec_pretty(reg).unwrap_or_default(),
    )?;
    Ok(())
}

pub fn read_manifest(plugin_dir: &Path) -> Result<PluginManifest> {
    let raw = fs::read(plugin_dir.join("plugin.json"))
        .map_err(|_| "未找到 plugin.json 清单文件")?;
    let manifest: PluginManifest =
        serde_json::from_slice(&raw).map_err(|e| format!("plugin.json 格式错误：{e}"))?;
    validate_plugin_id(&manifest.id)?;
    if manifest.name.trim().is_empty() {
        return Err("插件名称不能为空".into());
    }
    Ok(manifest)
}

pub fn list_plugins_in(
    plugins_dir: &Path,
    badges: &HashMap<String, String>,
) -> Vec<PluginInfo> {
    let reg = load_registry(plugins_dir);
    let mut list = Vec::new();
    let Ok(entries) = fs::read_dir(plugins_dir) else {
        return list;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if dir_name.starts_with('.') {
            continue;
        }
        let Ok(manifest) = read_manifest(&path) else {
            continue;
        };
        let enabled = !reg.disabled.iter().any(|d| d == &manifest.id);
        let has_ui = manifest
            .entry
            .as_ref()
            .is_some_and(|rel| !rel.trim().is_empty() && path.join(rel).is_file());
        let status_badge = badges.get(&manifest.id).cloned();
        list.push(PluginInfo {
            manifest,
            enabled,
            has_ui,
            status_badge,
        });
    }
    list.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name).then(a.manifest.id.cmp(&b.manifest.id)));
    list
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == ".git" || name_str == "node_modules" || name_str == "target" {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(&name);
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn sanitize_rel_path(rel: &str) -> Result<PathBuf> {
    let normalized = rel.trim().replace('\\', "/");
    if normalized.is_empty() || normalized.starts_with('/') || normalized.contains("..") {
        return Err(format!("非法插件文件路径：{rel}").into());
    }
    let mut out = PathBuf::new();
    for seg in normalized.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return Err(format!("非法插件文件路径：{rel}").into());
        }
        out.push(seg);
    }
    Ok(out)
}

pub fn install_from_bundle(plugins_dir: &Path, bundle: PluginBundle) -> Result<PluginManifest> {
    validate_plugin_id(&bundle.manifest.id)?;
    if bundle.manifest.name.trim().is_empty() {
        return Err("插件名称不能为空".into());
    }
    let target_dir = plugins_dir.join(&bundle.manifest.id);
    let _ = fs::create_dir_all(&target_dir);
    fs::write(
        target_dir.join("plugin.json"),
        serde_json::to_vec_pretty(&bundle.manifest).unwrap_or_default(),
    )?;
    for (rel, content) in &bundle.files {
        let rel_path = sanitize_rel_path(rel)?;
        let full = target_dir.join(rel_path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(full, content.as_bytes())?;
    }
    Ok(bundle.manifest)
}

pub fn install_from_dir(plugins_dir: &Path, src_dir: &Path) -> Result<PluginManifest> {
    let manifest_root = if src_dir.join("plugin.json").is_file() {
        src_dir.to_path_buf()
    } else {
        // 如果压缩包解压后套了一层文件夹，向下找一层
        let mut found = None;
        if let Ok(entries) = fs::read_dir(src_dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() && p.join("plugin.json").is_file() {
                    found = Some(p);
                    break;
                }
            }
        }
        found.ok_or("未在目录中找到 plugin.json")?
    };

    let manifest = read_manifest(&manifest_root)?;
    let target_dir = plugins_dir.join(&manifest.id);
    if target_dir.exists() && target_dir != manifest_root {
        // 保留已有的 storage.json 和 cookies.json
        let saved_storage = fs::read(target_dir.join("storage.json")).ok();
        let saved_cookies = fs::read(target_dir.join("cookies.json")).ok();
        let _ = fs::remove_dir_all(&target_dir);
        copy_dir_recursive(&manifest_root, &target_dir)?;
        if let Some(st) = saved_storage {
            let _ = fs::write(target_dir.join("storage.json"), st);
        }
        if let Some(ck) = saved_cookies {
            let _ = fs::write(target_dir.join("cookies.json"), ck);
        }
    } else if target_dir != manifest_root {
        copy_dir_recursive(&manifest_root, &target_dir)?;
    }
    Ok(manifest)
}

pub fn install_plugin_from_path(plugins_dir: &Path, source: &Path) -> Result<PluginManifest> {
    let _ = fs::create_dir_all(plugins_dir);
    if !source.exists() {
        return Err("所选插件路径不存在".into());
    }
    if source.is_dir() {
        return install_from_dir(plugins_dir, source);
    }
    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    // 1. 支持 .ndplugin 或 .json 单文件插件包
    if ext == "ndplugin" || ext == "json" {
        let bytes = fs::read(source)?;
        if let Ok(bundle) = serde_json::from_slice::<PluginBundle>(&bytes) {
            return install_from_bundle(plugins_dir, bundle);
        }
        if source
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case("plugin.json"))
        {
            if let Some(parent) = source.parent() {
                return install_from_dir(plugins_dir, parent);
            }
        }
        return Err("无法解析该插件清单或 .ndplugin 包".into());
    }

    // 2. 支持 .zip 压缩包（使用系统内置 tar 或 PowerShell 解压到临时目录）
    if ext == "zip" {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let tmp_dir = plugins_dir.join(format!(".tmp_extract_{ts}"));
        fs::create_dir_all(&tmp_dir)?;
        let tar_status = Command::new("tar")
            .arg("-xf")
            .arg(source)
            .arg("-C")
            .arg(&tmp_dir)
            .status();
        let extracted = match tar_status {
            Ok(st) if st.success() => true,
            _ => {
                let ps = Command::new("powershell")
                    .args(["-NoProfile", "-Command"])
                    .arg(format!(
                        "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                        source.display(),
                        tmp_dir.display()
                    ))
                    .status();
                matches!(ps, Ok(st) if st.success())
            }
        };
        if !extracted {
            let _ = fs::remove_dir_all(&tmp_dir);
            return Err("解压 .zip 插件包失败".into());
        }
        let res = install_from_dir(plugins_dir, &tmp_dir);
        let _ = fs::remove_dir_all(&tmp_dir);
        return res;
    }

    Err("只支持安装 .zip、.ndplugin 插件包或包含 plugin.json 的文件夹".into())
}

const BRIDGE_SCRIPT_TEMPLATE: &str = r#"
<style id="nidong-plugin-theme">
  :root, html[data-theme="light"] {
    --bg-island: rgba(244, 247, 251, 0.88);
    --bg-raised: #ffffff;
    --bg-page: #fafafa;
    --ink: #1a2433;
    --ink-dim: #5c6b80;
    --neu-blue: #0b3a82;
    --neu-blue-deep: #07285c;
    --warn: #c45a12;
    --danger: #b42318;
    --ok: #1f7a4d;
    --agenda: #147a6e;
    --agenda-bg: #d5f4ec;
    --agenda-ink: #0b3a82;
    --stroke: rgba(10, 30, 60, 0.10);
    color-scheme: light;
  }
  html[data-theme="dark"] {
    --bg-island: rgba(18, 22, 28, 0.78);
    --bg-raised: #1c222c;
    --bg-page: #141820;
    --ink: #e8eef6;
    --ink-dim: #93a1b5;
    --neu-blue: #6fa2ff;
    --neu-blue-deep: #3d6fd4;
    --warn: #f0a35a;
    --danger: #ff7a72;
    --ok: #5bd49a;
    --agenda: #8fd4c6;
    --agenda-bg: #8fd4c6;
    --agenda-ink: #06231f;
    --stroke: rgba(255, 255, 255, 0.10);
    color-scheme: dark;
  }
  body {
    font-family: "Segoe UI Variable", "Segoe UI", "Microsoft YaHei UI", "PingFang SC", sans-serif;
    color: var(--ink);
    background: transparent;
    margin: 0;
  }
</style>
<script id="nidong-plugin-bridge">
(function() {
  const PLUGIN_ID = __PLUGIN_ID_JSON__;
  let seq = 0;
  const pending = new Map();
  const tickListeners = [];
  const themeListeners = [];
  const authListeners = [];

  window.addEventListener("message", function(ev) {
    const msg = ev.data;
    if (!msg || typeof msg !== "object") return;
    if (msg.__nidongPluginReply && msg.pluginId === PLUGIN_ID) {
      const waiter = pending.get(msg.reqId);
      if (waiter) {
        pending.delete(msg.reqId);
        if (msg.ok) waiter.resolve(msg.data);
        else waiter.reject(new Error(msg.error || "插件调用失败"));
      }
    } else if (msg.__nidongPluginEvent) {
      if (msg.type === "tick") {
        tickListeners.forEach(function(fn) { try { fn(msg.payload); } catch (_) {} });
      } else if (msg.type === "theme") {
        document.documentElement.setAttribute("data-theme", msg.payload || "light");
        themeListeners.forEach(function(fn) { try { fn(msg.payload); } catch (_) {} });
      } else if (msg.type === "auth" && (!msg.pluginId || msg.pluginId === PLUGIN_ID)) {
        authListeners.forEach(function(fn) { try { fn(msg.payload); } catch (_) {} });
      }
    }
  });

  function callHost(action, payload) {
    return new Promise(function(resolve, reject) {
      const reqId = ++seq;
      pending.set(reqId, { resolve: resolve, reject: reject });
      window.parent.postMessage({
        __nidongPlugin: true,
        pluginId: PLUGIN_ID,
        reqId: reqId,
        action: action,
        payload: payload || {}
      }, "*");
    });
  }

  window.nidong = {
    pluginId: PLUGIN_ID,
    setBadge: function(badge) {
      return callHost("setBadge", { badge: badge || null });
    },
    storage: {
      get: function(key) { return callHost("storage.get", { key: key }); },
      set: function(key, value) { return callHost("storage.set", { key: key, value: value }); }
    },
    agenda: {
      list: function() { return callHost("agenda.list", {}); },
      upsert: function(item) { return callHost("agenda.upsert", { item: item }); },
      delete: function(id) { return callHost("agenda.delete", { id: id }); }
    },
    island: {
      hint: function(text) { return callHost("island.notify", { kind: "hint", text: text }); },
      remind: function(opts) {
        const name = typeof opts === "string" ? opts : (opts && opts.name) || "";
        const start = (opts && opts.start) || "";
        return callHost("island.notify", { kind: "remind", text: name, start: start });
      }
    },
    http: {
      fetch: function(req) { return callHost("http.fetch", { req: req }); }
    },
    auth: {
      openWindow: function(opts) { return callHost("auth.openWindow", opts || {}); },
      getCookies: function() { return callHost("auth.getCookies", {}); },
      setCookies: function(cookies) { return callHost("auth.setCookies", { cookies: cookies }); }
    },
    mcp: {
      listTools: function() { return callHost("mcp.listTools", {}); },
      call: function(tool, args) { return callHost("mcp.call", { tool: tool, arguments: args || {} }); }
    },
    log: {
      append: function(line) { return callHost("log.append", { line: String(line || "") }); },
      open: function() { return callHost("log.open", {}); }
    },
    openExternal: function(url) {
      return callHost("openExternal", { url: String(url || "") });
    },
    onTick: function(fn) { if (typeof fn === "function") tickListeners.push(fn); },
    onTheme: function(fn) { if (typeof fn === "function") themeListeners.push(fn); },
    onAuth: function(fn) { if (typeof fn === "function") authListeners.push(fn); }
  };
})();
</script>
"#;

pub fn build_plugin_srcdoc(plugins_dir: &Path, plugin_id: &str, theme: &str) -> Result<String> {
    validate_plugin_id(plugin_id)?;
    let plugin_dir = plugins_dir.join(plugin_id);
    let manifest = read_manifest(&plugin_dir)?;
    let entry_rel = manifest
        .entry
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or("该插件没有前端页面入口")?;
    let safe_rel = sanitize_rel_path(entry_rel)?;
    let entry_path = plugin_dir.join(safe_rel);
    let raw_html = fs::read_to_string(&entry_path)
        .map_err(|_| format!("读取插件入口文件失败：{entry_rel}"))?;

    let id_json = serde_json::to_string(plugin_id).unwrap_or_else(|_| "\"\"".into());
    let injected = BRIDGE_SCRIPT_TEMPLATE.replace("__PLUGIN_ID_JSON__", &id_json);
    let safe_theme = if theme == "dark" { "dark" } else { "light" };

    let mut html = raw_html;
    if html.contains("<html") {
        html = html.replacen("<html", &format!("<html data-theme=\"{safe_theme}\""), 1);
    } else {
        html = format!("<html data-theme=\"{safe_theme}\">{html}</html>");
    }
    if html.contains("</head>") {
        html = html.replacen("</head>", &format!("{injected}</head>"), 1);
    } else {
        html = format!("{injected}{html}");
    }
    Ok(html)
}

pub fn storage_get_in(plugins_dir: &Path, plugin_id: &str, key: &str) -> Result<Value> {
    validate_plugin_id(plugin_id)?;
    let p = plugins_dir.join(plugin_id).join("storage.json");
    let map: HashMap<String, Value> = fs::read(&p)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    Ok(map.get(key).cloned().unwrap_or(Value::Null))
}

pub fn storage_set_in(
    plugins_dir: &Path,
    plugin_id: &str,
    key: &str,
    value: Value,
) -> Result<()> {
    validate_plugin_id(plugin_id)?;
    let dir = plugins_dir.join(plugin_id);
    if !dir.is_dir() {
        return Err("插件未安装".into());
    }
    let p = dir.join("storage.json");
    let mut map: HashMap<String, Value> = fs::read(&p)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    if value.is_null() {
        map.remove(key);
    } else {
        map.insert(key.to_string(), value);
    }
    fs::write(&p, serde_json::to_vec_pretty(&map).unwrap_or_default())?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginHttpRequest {
    pub url: String,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginHttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
    #[serde(default)]
    pub cookies: HashMap<String, String>,
}

pub fn cookies_get_in(plugins_dir: &Path, plugin_id: &str) -> Result<HashMap<String, String>> {
    validate_plugin_id(plugin_id)?;
    let p = plugins_dir.join(plugin_id).join("cookies.json");
    let map: HashMap<String, String> = fs::read(&p)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    Ok(map)
}

pub fn cookies_set_in(
    plugins_dir: &Path,
    plugin_id: &str,
    cookies: &HashMap<String, String>,
) -> Result<()> {
    validate_plugin_id(plugin_id)?;
    let dir = plugins_dir.join(plugin_id);
    if !dir.is_dir() {
        return Err("插件未安装".into());
    }
    let p = dir.join("cookies.json");
    fs::write(&p, serde_json::to_vec_pretty(cookies).unwrap_or_default())?;
    Ok(())
}

pub fn parse_cookie_pairs(raw: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for seg in raw.split(';') {
        let part = seg.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((k, v)) = part.split_once('=') {
            let key = k.trim();
            let key_lower = key.to_ascii_lowercase();
            if key.is_empty()
                || matches!(
                    key_lower.as_str(),
                    "path" | "domain" | "expires" | "max-age" | "samesite" | "secure" | "httponly"
                )
            {
                continue;
            }
            out.insert(key.to_string(), v.trim().to_string());
        }
    }
    out
}

fn parse_set_cookie_kv(header_val: &str) -> Option<(String, String)> {
    let first = header_val.split(';').next()?.trim();
    let (k, v) = first.split_once('=')?;
    let k = k.trim();
    if k.is_empty() {
        return None;
    }
    Some((k.to_string(), v.trim().to_string()))
}

fn require_enabled_manifest(plugins_dir: &Path, plugin_id: &str) -> Result<PluginManifest> {
    validate_plugin_id(plugin_id)?;
    let reg = load_registry(plugins_dir);
    if reg.disabled.iter().any(|d| d == plugin_id) {
        return Err("该插件已停用".into());
    }
    read_manifest(&plugins_dir.join(plugin_id))
}

/// 标准 MCP (Model Context Protocol) JSON-RPC 2.0 over Stdio 客户端调用。
fn run_mcp_stdio(mcp: &McpConfig, method: &str, params: Value) -> Result<Value> {
    let mut cmd = Command::new(&mcp.command);
    cmd.args(&mcp.args)
        .envs(&mcp.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 MCP 进程失败 ({}): {e}", mcp.command))?;

    let mut stdin = child.stdin.take().ok_or("无法打开 MCP stdin")?;
    let stdout = child.stdout.take().ok_or("无法打开 MCP stdout")?;
    let mut reader = BufReader::new(stdout);

    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "nidong", "version": env!("CARGO_PKG_VERSION") }
        }
    });
    writeln!(stdin, "{init_req}")?;
    stdin.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;

    let initialized_notif = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    writeln!(stdin, "{initialized_notif}")?;

    let call_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": method,
        "params": params
    });
    writeln!(stdin, "{call_req}")?;
    stdin.flush()?;

    let mut result_val = None;
    for _ in 0..32 {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        if v.get("id").and_then(|i| i.as_i64()) == Some(2) {
            if let Some(err) = v.get("error") {
                let _ = child.kill();
                return Err(format!("MCP 返回错误：{err}").into());
            }
            result_val = v.get("result").cloned();
            break;
        }
    }
    let _ = child.kill();
    result_val.ok_or_else(|| "MCP 进程未返回有效响应".into())
}

fn sync_snapshot_agenda(app: &AppHandle, state: &AppState, items: Vec<AgendaItem>) {
    let snap = {
        let mut s = state.snapshot.lock().unwrap();
        s.agenda = items;
        s.clone()
    };
    let _ = app.emit("snapshot-updated", &snap);
}

// ==================== Tauri Commands ====================

#[tauri::command]
pub fn plugin_list(hub: State<'_, PluginHub>) -> Result<Vec<PluginInfo>> {
    let badges = hub.badges.lock().unwrap().clone();
    Ok(list_plugins_in(&hub.plugins_dir(), &badges))
}

#[tauri::command]
pub fn plugin_install(app: AppHandle, hub: State<'_, PluginHub>, path: String) -> Result<PluginInfo> {
    let manifest = install_plugin_from_path(&hub.plugins_dir(), Path::new(path.trim()))?;
    let badges = hub.badges.lock().unwrap().clone();
    let all = list_plugins_in(&hub.plugins_dir(), &badges);
    let _ = app.emit("plugins-updated", &all);
    all.into_iter()
        .find(|p| p.manifest.id == manifest.id)
        .ok_or_else(|| "安装完成但未读取到插件".into())
}

#[tauri::command]
pub fn plugin_toggle(
    app: AppHandle,
    hub: State<'_, PluginHub>,
    plugin_id: String,
    enabled: bool,
) -> Result<Vec<PluginInfo>> {
    validate_plugin_id(&plugin_id)?;
    let plugins_dir = hub.plugins_dir();
    let mut reg = load_registry(&plugins_dir);
    reg.disabled.retain(|id| id != &plugin_id);
    if !enabled {
        reg.disabled.push(plugin_id.clone());
        hub.badges.lock().unwrap().remove(&plugin_id);
    }
    save_registry(&plugins_dir, &reg)?;
    let badges = hub.badges.lock().unwrap().clone();
    let all = list_plugins_in(&plugins_dir, &badges);
    let _ = app.emit("plugins-updated", &all);
    Ok(all)
}

#[tauri::command]
pub fn plugin_uninstall(
    app: AppHandle,
    hub: State<'_, PluginHub>,
    state: State<'_, AppState>,
    plugin_id: String,
    remove_agenda: bool,
) -> Result<Vec<PluginInfo>> {
    validate_plugin_id(&plugin_id)?;
    let plugins_dir = hub.plugins_dir();
    let target = plugins_dir.join(&plugin_id);
    if target.is_dir() {
        fs::remove_dir_all(&target)?;
    }
    let mut reg = load_registry(&plugins_dir);
    reg.disabled.retain(|id| id != &plugin_id);
    let _ = save_registry(&plugins_dir, &reg);
    hub.badges.lock().unwrap().remove(&plugin_id);

    if remove_agenda {
        let prefix = agenda_prefix(&plugin_id);
        let mut items = crate::agenda::load(&state.data_dir);
        let before = items.len();
        items.retain(|x| !x.id.starts_with(&prefix));
        if items.len() != before {
            crate::agenda::save(&state.data_dir, &items)?;
            sync_snapshot_agenda(&app, &state, items);
        }
    }

    let badges = hub.badges.lock().unwrap().clone();
    let all = list_plugins_in(&plugins_dir, &badges);
    let _ = app.emit("plugins-updated", &all);
    Ok(all)
}

#[tauri::command]
pub fn plugin_load_html(
    hub: State<'_, PluginHub>,
    plugin_id: String,
    theme: String,
) -> Result<String> {
    build_plugin_srcdoc(&hub.plugins_dir(), &plugin_id, &theme)
}

#[tauri::command]
pub fn plugin_set_badge(
    app: AppHandle,
    hub: State<'_, PluginHub>,
    plugin_id: String,
    badge: Option<String>,
) -> Result<()> {
    validate_plugin_id(&plugin_id)?;
    {
        let mut map = hub.badges.lock().unwrap();
        match badge.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
            Some(b) => {
                map.insert(plugin_id, b);
            }
            None => {
                map.remove(&plugin_id);
            }
        }
    }
    let badges = hub.badges.lock().unwrap().clone();
    let all = list_plugins_in(&hub.plugins_dir(), &badges);
    let _ = app.emit("plugins-updated", &all);
    Ok(())
}

#[tauri::command]
pub fn plugin_storage_get(
    hub: State<'_, PluginHub>,
    plugin_id: String,
    key: String,
) -> Result<Value> {
    let _ = require_enabled_manifest(&hub.plugins_dir(), &plugin_id)?;
    storage_get_in(&hub.plugins_dir(), &plugin_id, &key)
}

#[tauri::command]
pub fn plugin_storage_set(
    hub: State<'_, PluginHub>,
    plugin_id: String,
    key: String,
    value: Value,
) -> Result<()> {
    let _ = require_enabled_manifest(&hub.plugins_dir(), &plugin_id)?;
    storage_set_in(&hub.plugins_dir(), &plugin_id, &key, value)
}

#[tauri::command]
pub fn plugin_agenda_list(
    hub: State<'_, PluginHub>,
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<Vec<AgendaItem>> {
    let _ = require_enabled_manifest(&hub.plugins_dir(), &plugin_id)?;
    let prefix = agenda_prefix(&plugin_id);
    let items = crate::agenda::load(&state.data_dir)
        .into_iter()
        .filter(|x| x.id.starts_with(&prefix))
        .collect();
    Ok(items)
}

#[tauri::command]
pub fn plugin_agenda_upsert(
    app: AppHandle,
    hub: State<'_, PluginHub>,
    state: State<'_, AppState>,
    plugin_id: String,
    mut item: AgendaItem,
) -> Result<Vec<AgendaItem>> {
    let manifest = require_enabled_manifest(&hub.plugins_dir(), &plugin_id)?;
    if !has_permission(&manifest, "agenda:write") {
        return Err("该插件未声明 agenda:write 权限".into());
    }
    item.id = make_plugin_agenda_id(&plugin_id, &item.id);
    let all = crate::agenda::upsert(&state.data_dir, item)?;
    let prefix = agenda_prefix(&plugin_id);
    let mine: Vec<AgendaItem> = all.iter().filter(|x| x.id.starts_with(&prefix)).cloned().collect();
    sync_snapshot_agenda(&app, &state, all);
    Ok(mine)
}

#[tauri::command]
pub fn plugin_agenda_delete(
    app: AppHandle,
    hub: State<'_, PluginHub>,
    state: State<'_, AppState>,
    plugin_id: String,
    item_id: String,
) -> Result<Vec<AgendaItem>> {
    let manifest = require_enabled_manifest(&hub.plugins_dir(), &plugin_id)?;
    if !has_permission(&manifest, "agenda:write") {
        return Err("该插件未声明 agenda:write 权限".into());
    }
    let full_id = make_plugin_agenda_id(&plugin_id, &item_id);
    let all = crate::agenda::delete(&state.data_dir, &full_id)?;
    let prefix = agenda_prefix(&plugin_id);
    let mine: Vec<AgendaItem> = all.iter().filter(|x| x.id.starts_with(&prefix)).cloned().collect();
    sync_snapshot_agenda(&app, &state, all);
    Ok(mine)
}

#[tauri::command]
pub fn plugin_island_notify(
    app: AppHandle,
    hub: State<'_, PluginHub>,
    plugin_id: String,
    kind: String,
    text: String,
    start: Option<String>,
) -> Result<()> {
    let manifest = require_enabled_manifest(&hub.plugins_dir(), &plugin_id)?;
    if !has_permission(&manifest, "island:notify") {
        return Err("该插件未声明 island:notify 权限".into());
    }
    let msg = text.trim();
    if msg.is_empty() {
        return Ok(());
    }
    if kind == "remind" {
        let _ = app.emit(
            "plugin-remind",
            json!({
                "name": msg,
                "start": start.unwrap_or_default(),
                "kind": "event"
            }),
        );
    } else {
        let _ = app.emit("campus-hint", msg.to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn plugin_http_fetch(
    hub: State<'_, PluginHub>,
    plugin_id: String,
    req: PluginHttpRequest,
) -> Result<PluginHttpResponse> {
    let plugins_dir = hub.plugins_dir();
    let manifest = require_enabled_manifest(&plugins_dir, &plugin_id)?;
    if !has_http_permission(&manifest, &req.url) {
        return Err(format!("插件无权访问该地址：{}", req.url).into());
    }
    let mut jar = cookies_get_in(&plugins_dir, &plugin_id).unwrap_or_default();

    // 若 URL 是中国大学MOOC 的 .rpc 接口且尚未拼接 csrfKey，自动从插件 Cookie Jar 注入 NTESSTUDYSI
    let mut target_url = req.url.clone();
    if target_url.contains(".rpc") && !target_url.contains("csrfKey=") {
        if let Some(csrf) = jar.get("NTESSTUDYSI").filter(|s| !s.is_empty()) {
            let sep = if target_url.contains('?') { "&" } else { "?" };
            target_url = format!("{target_url}{sep}csrfKey={csrf}");
        }
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/128.0.0.0 Safari/537.36")
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())?;

    let method_str = req.method.as_deref().unwrap_or("GET").to_ascii_uppercase();
    let method = reqwest::Method::from_bytes(method_str.as_bytes())
        .map_err(|_| "HTTP 方法无效")?;

    let mut builder = client.request(method, &target_url);
    let mut has_cookie_header = false;
    let mut has_referer = false;
    let mut has_origin = false;
    for (k, v) in &req.headers {
        let lower = k.to_ascii_lowercase();
        if lower == "cookie" {
            has_cookie_header = true;
            for (ck, cv) in parse_cookie_pairs(v) {
                jar.insert(ck, cv);
            }
        } else if lower == "referer" {
            has_referer = true;
        } else if lower == "origin" {
            has_origin = true;
        }
        builder = builder.header(k, v);
    }

    if !has_cookie_header && !jar.is_empty() {
        let cookie_line = jar
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
        builder = builder.header("Cookie", cookie_line);
    }
    if !has_referer {
        if let Ok(u) = url::Url::parse(&target_url) {
            if let Some(host) = u.host_str() {
                builder = builder.header("Referer", format!("{}://{}/", u.scheme(), host));
            }
        }
    }
    if !has_origin {
        if let Ok(u) = url::Url::parse(&target_url) {
            if let Some(host) = u.host_str() {
                builder = builder.header("Origin", format!("{}://{}", u.scheme(), host));
            }
        }
    }

    if let Some(mut body) = req.body {
        if target_url.contains("/dwr/call/plaincall/") {
            if let Some(csrf) = jar.get("NTESSTUDYSI").filter(|s| !s.is_empty()) {
                body = body.replace("httpSessionId=__NTESSTUDYSI__", &format!("httpSessionId={csrf}"));
            }
        }
        builder = builder.body(body);
    }

    let resp = builder.send().await.map_err(|e| format!("网络请求失败：{e}"))?;
    let status = resp.status().as_u16();
    let mut headers = HashMap::new();
    let mut cookie_changed = false;
    for (k, v) in resp.headers() {
        if let Ok(val) = v.to_str() {
            if k.as_str().eq_ignore_ascii_case("set-cookie") {
                if let Some((ck, cv)) = parse_set_cookie_kv(val) {
                    if cv.is_empty() || cv == "\"\"" {
                        jar.remove(&ck);
                    } else {
                        jar.insert(ck, cv);
                    }
                    cookie_changed = true;
                }
            }
            headers.insert(k.to_string(), val.to_string());
        }
    }
    if cookie_changed {
        let _ = cookies_set_in(&plugins_dir, &plugin_id, &jar);
    }
    let body = resp.text().await.map_err(|e| e.to_string())?;
    Ok(PluginHttpResponse {
        status,
        headers,
        body,
        cookies: jar,
    })
}

#[tauri::command]
pub fn plugin_get_cookies(
    hub: State<'_, PluginHub>,
    plugin_id: String,
) -> Result<HashMap<String, String>> {
    let _ = require_enabled_manifest(&hub.plugins_dir(), &plugin_id)?;
    cookies_get_in(&hub.plugins_dir(), &plugin_id)
}

#[tauri::command]
pub fn plugin_set_cookies(
    hub: State<'_, PluginHub>,
    plugin_id: String,
    cookies: Value,
) -> Result<HashMap<String, String>> {
    let _ = require_enabled_manifest(&hub.plugins_dir(), &plugin_id)?;
    let mut jar = cookies_get_in(&hub.plugins_dir(), &plugin_id).unwrap_or_default();
    match cookies {
        Value::Null => {
            jar.clear();
        }
        Value::String(raw) => {
            if raw.trim().is_empty() {
                jar.clear();
            } else {
                for (k, v) in parse_cookie_pairs(&raw) {
                    jar.insert(k, v);
                }
            }
        }
        Value::Object(map) => {
            for (k, v) in map {
                if v.is_null() {
                    jar.remove(&k);
                } else if let Some(s) = v.as_str() {
                    if s.is_empty() {
                        jar.remove(&k);
                    } else {
                        jar.insert(k, s.to_string());
                    }
                }
            }
        }
        _ => {}
    }
    cookies_set_in(&hub.plugins_dir(), &plugin_id, &jar)?;
    Ok(jar)
}

fn build_urs_autofill_script(username: &str, password: &str) -> String {
    let u_json = serde_json::to_string(username).unwrap_or_else(|_| "\"\"".into());
    let p_json = serde_json::to_string(password).unwrap_or_else(|_| "\"\"".into());
    format!(
        r##"(function() {{
  const USER = {u_json};
  const PASS = {p_json};

  // 顶层 icourse163.org 页面：强制默认账号密码登录标签页、渲染加载提示条、并通过 document.title 回传登录完成信号
  if (window === window.top && location.hostname.indexOf("icourse163.org") !== -1) {{
    try {{
      localStorage.setItem("IS_LOGIN_BY_QR_CODE", "0");
      localStorage.setItem("LAST_LOGIN_TYPE", "0");
      if (location.pathname.indexOf("/member/login.htm") !== -1 && !sessionStorage.getItem("__nidong_auth_init__")) {{
        sessionStorage.setItem("__nidong_auth_init__", "1");
        document.cookie = "STUDY_INFO=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/; domain=.icourse163.org";
        document.cookie = "STUDY_INFO=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/; domain=www.icourse163.org";
        document.cookie = "STUDY_INFO=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/";
      }}
    }} catch (_) {{}}

    function mountBanner() {{
      if (!document.body || document.getElementById("__nidong_auth_bar__")) return;
      const bar = document.createElement("div");
      bar.id = "__nidong_auth_bar__";
      bar.style.cssText = "position:fixed;top:0;left:0;right:0;z-index:2147483647;background:#0f52ba;color:#fff;padding:8px 16px;font:13px/1.5 'Segoe UI','Microsoft YaHei',sans-serif;text-align:center;box-shadow:0 2px 8px rgba(0,0,0,0.18);";
      bar.textContent = USER
        ? "泥洞 MOOC 认证：正在加载登录组件并自动填入账号 " + USER + "，如出现易盾滑块请拖动完成登录…"
        : "泥洞 MOOC 认证：请在下方完成登录，登录成功后窗口将自动同步并关闭…";
      document.body.appendChild(bar);
    }}
    if (document.readyState === "loading") {{
      document.addEventListener("DOMContentLoaded", mountBanner);
    }} else {{
      mountBanner();
    }}

    let authReported = false;
    setInterval(function() {{
      mountBanner();
      if (authReported) return;
      const ck = document.cookie || "";
      const m = ck.match(/(?:^|;\s*)STUDY_INFO=([^;]+)/);
      if (m && m[1] && m[1].trim().length > 3) {{
        authReported = true;
        const bar = document.getElementById("__nidong_auth_bar__");
        if (bar) {{
          bar.style.background = "#107c41";
          bar.textContent = "登录成功！正在同步 MOOC 会话 Cookie 到泥洞插件…";
        }}
        document.title = "__NIDONG_AUTH_OK__:" + ck;
      }}
    }}, 350);
  }}

  if (!USER || !PASS) return;
  let filled = false;
  let clickedTab = false;
  let attempts = 0;

  function isVisible(el) {{
    if (!el) return false;
    const rect = el.getBoundingClientRect();
    return el.offsetParent !== null && rect.width > 12 && rect.height > 12;
  }}

  function setNativeVal(el, val) {{
    if (!el) return;
    el.focus();
    const proto = Object.getPrototypeOf(el);
    const desc = Object.getOwnPropertyDescriptor(proto, "value");
    if (desc && desc.set) desc.set.call(el, val);
    else el.value = val;
    el.dispatchEvent(new Event("input", {{ bubbles: true }}));
    el.dispatchEvent(new Event("change", {{ bubbles: true }}));
    el.dispatchEvent(new Event("keyup", {{ bubbles: true }}));
    el.dispatchEvent(new Event("blur", {{ bubbles: true }}));
  }}

  const timer = setInterval(function() {{
    if (filled || ++attempts > 75) {{
      clearInterval(timer);
      return;
    }}
    try {{
      // 查找真正可见的密码框（跳过网易 URS 埋入的 display:none;width:0;height:0 防自动填充假密码框）
      const pwdList = document.querySelectorAll('input[type="password"]');
      let pwdInput = null;
      for (let i = 0; i < pwdList.length; i++) {{
        if (isVisible(pwdList[i])) {{
          pwdInput = pwdList[i];
          break;
        }}
      }}

      if (!pwdInput) {{
        if (!clickedTab) {{
          const nodes = document.querySelectorAll("a, span, div, li");
          for (let i = 0; i < nodes.length; i++) {{
            const t = (nodes[i].textContent || "").trim();
            if ((t === "密码登录" || t === "使用密码登录") && isVisible(nodes[i])) {{
              nodes[i].click();
              clickedTab = true;
              break;
            }}
          }}
        }}
        return;
      }}

      const candidates = document.querySelectorAll(
        '#phoneipt, input[type="tel"], input[data-loginname="loginEmail"], input.j-nameforslide, input.dlemail, input[name="phone"], input[name="email"], input.j-inputtext, input[type="text"]'
      );
      let userInput = null;
      for (let i = 0; i < candidates.length; i++) {{
        const c = candidates[i];
        if (
          c !== pwdInput &&
          c.type !== "password" &&
          !c.classList.contains("u-pwdtext") &&
          c.name !== "checkcode" &&
          isVisible(c)
        ) {{
          userInput = c;
          break;
        }}
      }}
      if (!userInput) return;

      setNativeVal(userInput, USER);
      setNativeVal(pwdInput, PASS);

      // 勾选网易 URS 协议复选框（仅在未勾选时点击，避免触发 RegularJS 反向取消勾选）
      const unchkSpan = document.querySelector('.u-dl-agree:not(.u-dl-agree-select)');
      if (unchkSpan) {{
        const clauseInp = unchkSpan.querySelector('input.dl-clause-login') || unchkSpan;
        clauseInp.click();
      }}
      const mailChk = document.querySelector('input.j-mail-clause-checkbox');
      if (mailChk && !mailChk.checked) {{
        mailChk.click();
      }}

      filled = true;
      clearInterval(timer);

      // 延迟 350ms 自动点击一次登录按钮（若触发易盾滑块则由用户在窗口内完成）
      setTimeout(function() {{
        const btn = document.querySelector('#submitBtn, #dologin, a.u-loginbtn');
        if (btn && isVisible(btn)) btn.click();
      }}, 350);
    }} catch (_) {{}}
  }}, 350);
}})();"##
    )
}

#[tauri::command]
pub async fn plugin_open_auth_window(
    app: AppHandle,
    hub: State<'_, PluginHub>,
    plugin_id: String,
    url: String,
    username: Option<String>,
    password: Option<String>,
) -> Result<()> {
    let plugins_dir = hub.plugins_dir();
    let manifest = require_enabled_manifest(&plugins_dir, &plugin_id)?;
    if !has_http_permission(&manifest, &url) {
        return Err(format!("插件无权打开该认证地址：{url}").into());
    }
    let parsed_url = url::Url::parse(&url).map_err(|_| "认证 URL 格式无效")?;
    let origin_url = url::Url::parse(&format!(
        "{}://{}/",
        parsed_url.scheme(),
        parsed_url.host_str().unwrap_or("www.icourse163.org")
    ))
    .unwrap_or_else(|_| parsed_url.clone());

    for (lbl, existing) in app.webview_windows() {
        if lbl == "plugin-auth" || lbl.starts_with("plugin-auth-") {
            let _ = existing.close();
        }
    }

    let user_str = username.unwrap_or_default();
    let pass_str = password.unwrap_or_default();
    let script = build_urs_autofill_script(user_str.trim(), &pass_str);

    let captured = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let captured_for_title = captured.clone();
    let app_for_title = app.clone();
    let pid_for_title = plugin_id.clone();
    let dir_for_title = plugins_dir.clone();

    let auth_label = format!(
        "plugin-auth-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );

    let win = WebviewWindowBuilder::new(&app, auth_label, WebviewUrl::External(parsed_url))
        .title(format!("登录 {} - 泥洞插件认证", manifest.name))
        .inner_size(980.0, 680.0)
        .center()
        .visible(true)
        .focused(true)
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36")
        .initialization_script_for_all_frames(&script)
        .on_document_title_changed(move |w, title| {
            let Some(raw_ck) = title.strip_prefix("__NIDONG_AUTH_OK__:") else {
                return;
            };
            if captured_for_title.swap(true, std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            let mut map = HashMap::new();
            for part in raw_ck.split(';') {
                if let Some((k, v)) = part.split_once('=') {
                    let k = k.trim();
                    let v = v.trim();
                    if !k.is_empty() && !v.is_empty() {
                        map.insert(k.to_string(), v.to_string());
                    }
                }
            }
            let app_inner = app_for_title.clone();
            let pid_inner = pid_for_title.clone();
            let dir_inner = dir_for_title.clone();
            let origin_inner = origin_url.clone();
            std::thread::spawn(move || {
                // 启动一个唤醒辅助线程，40ms 后向主线程窗口投递一次标题更新消息，
                // 防止 webview2_com::wait_with_pump 在 GetCookies 回调后停留在 GetMessageA
                let w_kick = w.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(45));
                    let _ = w_kick.set_title("登录成功 - 泥洞插件认证");
                });
                if let Ok(ck_list) = w.cookies_for_url(origin_inner) {
                    for c in ck_list {
                        let k = c.name().trim().to_string();
                        let v = c.value().trim().to_string();
                        if !k.is_empty() && !v.is_empty() {
                            map.insert(k, v);
                        }
                    }
                }
                let _ = cookies_set_in(&dir_inner, &pid_inner, &map);
                let _ = app_inner.emit(
                    "plugin-auth-event",
                    json!({
                        "pluginId": pid_inner,
                        "status": "success",
                        "cookies": map
                    }),
                );
                let _ = w.close();
            });
        })
        .build()
        .map_err(|e| format!("创建认证窗口失败：{e}"))?;

    let app_for_close = app.clone();
    let pid_for_close = plugin_id.clone();
    let captured_for_close = captured.clone();
    win.on_window_event(move |ev| {
        if matches!(ev, tauri::WindowEvent::Destroyed)
            && !captured_for_close.load(std::sync::atomic::Ordering::SeqCst)
        {
            let _ = app_for_close.emit(
                "plugin-auth-event",
                json!({
                    "pluginId": pid_for_close,
                    "status": "closed"
                }),
            );
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn plugin_mcp_list_tools(
    hub: State<'_, PluginHub>,
    plugin_id: String,
) -> Result<Value> {
    let manifest = require_enabled_manifest(&hub.plugins_dir(), &plugin_id)?;
    let mcp = manifest.mcp.ok_or("该插件未配置 MCP 服务")?;
    tokio::task::spawn_blocking(move || run_mcp_stdio(&mcp, "tools/list", json!({})))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn plugin_mcp_call(
    hub: State<'_, PluginHub>,
    plugin_id: String,
    tool: String,
    arguments: Value,
) -> Result<Value> {
    let manifest = require_enabled_manifest(&hub.plugins_dir(), &plugin_id)?;
    let mcp = manifest.mcp.ok_or("该插件未配置 MCP 服务")?;
    tokio::task::spawn_blocking(move || {
        run_mcp_stdio(
            &mcp,
            "tools/call",
            json!({
                "name": tool,
                "arguments": arguments
            }),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

fn log_file_path(plugins_dir: &Path, plugin_id: &str) -> Result<PathBuf> {
    validate_plugin_id(plugin_id)?;
    let dir = plugins_dir.join(plugin_id);
    if !dir.is_dir() {
        return Err("插件未安装".into());
    }
    Ok(dir.join("plugin.log"))
}

pub fn append_log_in(plugins_dir: &Path, plugin_id: &str, line: &str) -> Result<PathBuf> {
    let path = log_file_path(plugins_dir, plugin_id)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let clean = line.trim_end_matches(&['\r', '\n'][..]);
    let mut existing = fs::read_to_string(&path).unwrap_or_default();
    if existing.len() > 1024 * 1024 {
        let keep_from = existing.len() - 512 * 1024;
        if let Some(nl) = existing[keep_from..].find('\n') {
            existing = existing[keep_from + nl + 1..].to_string();
        }
    }
    existing.push_str(clean);
    existing.push_str("\r\n");
    fs::write(&path, existing.as_bytes())?;
    Ok(path)
}

#[tauri::command]
pub fn plugin_append_log(
    hub: State<'_, PluginHub>,
    plugin_id: String,
    line: String,
) -> Result<()> {
    append_log_in(&hub.plugins_dir(), &plugin_id, &line)?;
    Ok(())
}

#[tauri::command]
pub fn plugin_open_log(
    app: AppHandle,
    hub: State<'_, PluginHub>,
    plugin_id: String,
) -> Result<()> {
    let path = log_file_path(&hub.plugins_dir(), &plugin_id)?;
    if !path.is_file() {
        let _ = append_log_in(
            &hub.plugins_dir(),
            &plugin_id,
            "=== 泥洞插件完整运行日志 ===",
        );
    }
    #[cfg(windows)]
    {
        let _ = &app;
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let path_w: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let rc = unsafe {
            ShellExecuteW(
                None,
                PCWSTR::null(),
                PCWSTR(path_w.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        if (rc.0 as isize) > 32 {
            Ok(())
        } else {
            Err("没有打开插件日志文件。".into())
        }
    }
    #[cfg(not(windows))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_path(path.to_string_lossy().to_string(), None::<&str>)
            .map_err(|e| e.to_string().into())
    }
}

#[tauri::command]
pub fn plugin_open_external(
    app: AppHandle,
    hub: State<'_, PluginHub>,
    plugin_id: String,
    url: String,
) -> Result<()> {
    let manifest = require_enabled_manifest(&hub.plugins_dir(), &plugin_id)?;
    let url = url.trim().to_string();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("外部链接格式无效".into());
    }
    if !has_http_permission(&manifest, &url) {
        return Err(format!("插件无权在浏览器打开该地址：{url}").into());
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> PluginManifest {
        PluginManifest {
            id: "edu.neu.plugin.mooc-monitor".into(),
            name: "MOOC 监视器".into(),
            version: "0.1.0".into(),
            author: "sapphire_fic".into(),
            description: "中国大学MOOC作业监视与互评插件".into(),
            runtime: "webview".into(),
            entry: Some("ui/index.html".into()),
            background: true,
            permissions: vec![
                "agenda:write".into(),
                "island:notify".into(),
                "http:*.icourse163.org".into(),
            ],
            mcp: None,
        }
    }

    #[test]
    fn validates_plugin_ids_and_rejects_traversal() {
        assert!(validate_plugin_id("edu.neu.plugin.mooc-monitor").is_ok());
        assert!(validate_plugin_id("mcp_server-1").is_ok());
        assert!(validate_plugin_id("").is_err());
        assert!(validate_plugin_id("../hack").is_err());
        assert!(validate_plugin_id("a/b").is_err());
    }

    #[test]
    fn matches_http_wildcard_permissions() {
        let m = sample_manifest();
        assert!(has_http_permission(
            &m,
            "https://www.icourse163.org/web/j/courseBean.getCourse.rpc"
        ));
        assert!(has_http_permission(&m, "https://icourse163.org/login"));
        assert!(!has_http_permission(&m, "https://evil-icourse163.org/login"));
        assert!(!has_http_permission(&m, "https://pass.neu.edu.cn/tpass/login"));
    }

    #[test]
    fn installs_bundle_and_builds_bridged_srcdoc() {
        let tmp = std::env::temp_dir().join(format!(
            "nidong_plugin_test_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut files = HashMap::new();
        files.insert(
            "ui/index.html".into(),
            "<!doctype html><html><head><title>MOOC</title></head><body><h1>MOOC</h1></body></html>"
                .into(),
        );
        let bundle = PluginBundle {
            manifest: sample_manifest(),
            files,
        };
        let installed = install_from_bundle(&tmp, bundle).unwrap();
        assert_eq!(installed.id, "edu.neu.plugin.mooc-monitor");

        let list = list_plugins_in(&tmp, &HashMap::new());
        assert_eq!(list.len(), 1);
        assert!(list[0].enabled);
        assert!(list[0].has_ui);

        let srcdoc = build_plugin_srcdoc(&tmp, "edu.neu.plugin.mooc-monitor", "dark").unwrap();
        assert!(srcdoc.contains("data-theme=\"dark\""));
        assert!(srcdoc.contains("window.nidong"));
        assert!(srcdoc.contains("edu.neu.plugin.mooc-monitor"));

        storage_set_in(&tmp, "edu.neu.plugin.mooc-monitor", "hwCount", json!(3)).unwrap();
        let val = storage_get_in(&tmp, "edu.neu.plugin.mooc-monitor", "hwCount").unwrap();
        assert_eq!(val, json!(3));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn prefixes_plugin_agenda_ids_idempotently() {
        let id1 = make_plugin_agenda_id("mooc", "hw_101");
        assert_eq!(id1, "plg:mooc:hw_101");
        let id2 = make_plugin_agenda_id("mooc", "plg:mooc:hw_101");
        assert_eq!(id2, "plg:mooc:hw_101");
    }

    #[test]
    fn tauri_conf_allows_plugin_srcdoc_inline_scripts() {
        let conf_str = include_str!("../tauri.conf.json");
        let conf: Value = serde_json::from_str(conf_str).expect("valid tauri.conf.json");
        let sec = &conf["app"]["security"];
        let csp = sec["csp"].as_str().unwrap_or("");
        assert!(csp.contains("script-src") && csp.contains("'unsafe-inline'"));
        assert!(csp.contains("style-src") && csp.contains("'unsafe-inline'"));
        let disabled = &sec["dangerousDisableAssetCspModification"];
        let ok = disabled.as_bool() == Some(true)
            || disabled
                .as_array()
                .map(|arr| {
                    let has_script = arr.iter().any(|v| v.as_str() == Some("script-src"));
                    let has_style = arr.iter().any(|v| v.as_str() == Some("style-src"));
                    has_script && has_style
                })
                .unwrap_or(false);
        assert!(
            ok,
            "tauri.conf.json must disable asset CSP hash injection for script-src/style-src so plugin srcDoc inline scripts work in release builds"
        );
    }
}
