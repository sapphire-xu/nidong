import {
  Button,
  Checkbox,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Switch,
} from "@fluentui/react-components";
import {
  ArrowUpload20Regular,
  Delete20Regular,
  FolderOpen20Regular,
  PlugConnected20Regular,
} from "@fluentui/react-icons";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import { api, type PluginHttpRequest, type PluginInfo } from "../../api";
import type { Agenda } from "../../types";

function cleanErr(e: unknown) {
  return String(e).replace(/^Error:\s*/, "");
}

async function dispatchPluginCall(pluginId: string, action: string, payload: Record<string, unknown>) {
  switch (action) {
    case "setBadge":
      return api.pluginSetBadge(pluginId, (payload.badge as string | null) ?? null);
    case "storage.get":
      return api.pluginStorageGet(pluginId, String(payload.key || ""));
    case "storage.set":
      return api.pluginStorageSet(pluginId, String(payload.key || ""), payload.value ?? null);
    case "agenda.list":
      return api.pluginAgendaList(pluginId);
    case "agenda.upsert":
      return api.pluginAgendaUpsert(pluginId, payload.item as Agenda);
    case "agenda.delete":
      return api.pluginAgendaDelete(pluginId, String(payload.id || ""));
    case "island.notify":
      return api.pluginIslandNotify(
        pluginId,
        payload.kind === "remind" ? "remind" : "hint",
        String(payload.text || ""),
        payload.start ? String(payload.start) : undefined
      );
    case "http.fetch":
      return api.pluginHttpFetch(pluginId, payload.req as PluginHttpRequest);
    case "auth.openWindow":
      return api.pluginOpenAuthWindow(
        pluginId,
        String(payload.url || "https://www.icourse163.org/member/login.htm"),
        payload.username ? String(payload.username) : undefined,
        payload.password ? String(payload.password) : undefined
      );
    case "auth.getCookies":
      return api.pluginGetCookies(pluginId);
    case "auth.setCookies":
      return api.pluginSetCookies(pluginId, payload.cookies ?? null);
    case "mcp.listTools":
      return api.pluginMcpListTools(pluginId);
    case "mcp.call":
      return api.pluginMcpCall(pluginId, String(payload.tool || ""), payload.arguments ?? {});
    case "log.append":
      return api.pluginAppendLog(pluginId, String(payload.line || ""));
    case "log.open":
      return api.pluginOpenLog(pluginId);
    case "openExternal":
      return api.pluginOpenExternal(pluginId, String(payload.url || ""));
    default:
      throw new Error(`未知插件宿主接口：${action}`);
  }
}

/**
 * 全局插件通信桥与后台常驻容器（挂载在 DeskApp 顶层）。
 * 负责处理所有插件 iframe 的 postMessage 调用，以及为开启了 background 的插件维持后台巡检。
 */
export function PluginBridge(props: {
  plugins: PluginInfo[];
  theme: "light" | "dark";
  activePluginId: string | null;
}) {
  const { plugins, theme, activePluginId } = props;
  const [bgHtmlMap, setBgHtmlMap] = useState<Record<string, string>>({});
  const themeRef = useRef(theme);
  themeRef.current = theme;

  useEffect(() => {
    const onMessage = async (ev: MessageEvent) => {
      const msg = ev.data;
      if (!msg || typeof msg !== "object" || !msg.__nidongPlugin) return;
      const pluginId = String(msg.pluginId || "");
      const reqId = msg.reqId;
      const action = String(msg.action || "");
      const payload = (msg.payload && typeof msg.payload === "object" ? msg.payload : {}) as Record<string, unknown>;
      const target = ev.source as WindowProxy | null;
      if (!pluginId || !target) return;
      try {
        const data = await dispatchPluginCall(pluginId, action, payload);
        target.postMessage({ __nidongPluginReply: true, pluginId, reqId, ok: true, data }, "*");
      } catch (e) {
        target.postMessage(
          { __nidongPluginReply: true, pluginId, reqId, ok: false, error: cleanErr(e) },
          "*"
        );
      }
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, []);

  // 将主题变更广播给所有挂载的插件 iframe
  useEffect(() => {
    const frames = document.querySelectorAll<HTMLIFrameElement>("iframe.plugin-frame, iframe.plugin-bg-frame");
    frames.forEach((f) => {
      f.contentWindow?.postMessage({ __nidongPluginEvent: true, type: "theme", payload: theme }, "*");
    });
  }, [theme]);

  // 将 Rust 的 60 秒 plugin-tick 脉冲及 plugin-auth-event 认证事件转发给所有插件 iframe
  useEffect(() => {
    let disposed = false;
    const unsubs: Array<() => void> = [];
    const register = (p: Promise<() => void>) => {
      p.then((fn) => {
        if (disposed) {
          fn();
        } else {
          unsubs.push(fn);
        }
      });
    };
    register(
      listen<string>("plugin-tick", (e) => {
        const frames = document.querySelectorAll<HTMLIFrameElement>("iframe.plugin-frame, iframe.plugin-bg-frame");
        frames.forEach((f) => {
          f.contentWindow?.postMessage({ __nidongPluginEvent: true, type: "tick", payload: e.payload }, "*");
        });
      })
    );
    register(
      listen<{ pluginId: string; status: string; cookies?: Record<string, string> }>("plugin-auth-event", (e) => {
        const frames = document.querySelectorAll<HTMLIFrameElement>("iframe.plugin-frame, iframe.plugin-bg-frame");
        frames.forEach((f) => {
          f.contentWindow?.postMessage(
            {
              __nidongPluginEvent: true,
              type: "auth",
              pluginId: e.payload?.pluginId,
              payload: e.payload,
            },
            "*"
          );
        });
      })
    );
    return () => {
      disposed = true;
      unsubs.forEach((fn) => fn());
    };
  }, []);

  // 为启用了 background 且当前未在前台打开的插件加载后台 HTML
  const bgPlugins = plugins.filter(
    (p) => p.enabled && p.hasUi && p.manifest.background && p.manifest.id !== activePluginId
  );
  const bgKey = bgPlugins.map((p) => `${p.manifest.id}@${p.manifest.version}`).join("|");

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const next: Record<string, string> = {};
      for (const p of bgPlugins) {
        try {
          next[p.manifest.id] = await api.pluginLoadHtml(p.manifest.id, themeRef.current);
        } catch {
          /* ignore load failures in background */
        }
      }
      if (!cancelled) setBgHtmlMap(next);
    })();
    return () => {
      cancelled = true;
    };
  }, [bgKey]);

  if (bgPlugins.length === 0) return null;

  return (
    <div style={{ display: "none" }} aria-hidden="true">
      {bgPlugins.map((p) => {
        const html = bgHtmlMap[p.manifest.id];
        if (!html) return null;
        return (
          <iframe
            key={p.manifest.id}
            className="plugin-bg-frame"
            title={`bg-${p.manifest.id}`}
            srcDoc={html}
            sandbox="allow-scripts"
          />
        );
      })}
    </div>
  );
}

/**
 * 工具箱内打开某个具体插件时的前台视图容器。
 */
export function PluginView(props: { pluginId: string; theme: "light" | "dark" }) {
  const { pluginId, theme } = props;
  const [html, setHtml] = useState<string>("");
  const [err, setErr] = useState<string>("");
  const initialTheme = useRef(theme);

  useEffect(() => {
    let cancelled = false;
    setErr("");
    api
      .pluginLoadHtml(pluginId, initialTheme.current)
      .then((doc) => {
        if (!cancelled) setHtml(doc);
      })
      .catch((e) => {
        if (!cancelled) setErr(cleanErr(e));
      });
    return () => {
      cancelled = true;
    };
  }, [pluginId]);

  if (err) {
    return <p className="hint warn">加载插件界面失败：{err}</p>;
  }
  if (!html) {
    return <p className="hint">正在加载插件…</p>;
  }
  return (
    <div className="plugin-frame-wrap">
      <iframe
        className="plugin-frame"
        title={pluginId}
        srcDoc={html}
        sandbox="allow-scripts allow-forms"
      />
    </div>
  );
}

/**
 * 工具箱 › 插件管理 面板：支持导入 .zip / .ndplugin / 文件夹，启停插件、查看权限与测试 MCP、卸载插件。
 */
export function PluginManagerPanel(props: {
  plugins: PluginInfo[];
  onOpenPlugin: (pluginId: string) => void;
}) {
  const { plugins, onOpenPlugin } = props;
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState("");
  const [err, setErr] = useState("");
  const [removing, setRemoving] = useState<PluginInfo | null>(null);
  const [removeAgenda, setRemoveAgenda] = useState(true);
  const [mcpOutput, setMcpOutput] = useState<{ name: string; text: string } | null>(null);

  async function pickPackageFile() {
    if (busy) return;
    setErr("");
    setNote("");
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "泥洞插件包", extensions: ["zip", "ndplugin", "json"] }],
      });
      if (!selected || typeof selected !== "string") return;
      setBusy(true);
      const info = await api.pluginInstall(selected);
      setNote(`已安装插件「${info.manifest.name}」v${info.manifest.version}`);
    } catch (e) {
      setErr(cleanErr(e));
    } finally {
      setBusy(false);
    }
  }

  async function pickPluginFolder() {
    if (busy) return;
    setErr("");
    setNote("");
    try {
      const selected = await open({
        multiple: false,
        directory: true,
      });
      if (!selected || typeof selected !== "string") return;
      setBusy(true);
      const info = await api.pluginInstall(selected);
      setNote(`已从文件夹安装插件「${info.manifest.name}」v${info.manifest.version}`);
    } catch (e) {
      setErr(cleanErr(e));
    } finally {
      setBusy(false);
    }
  }

  async function togglePlugin(id: string, enabled: boolean) {
    setErr("");
    try {
      await api.pluginToggle(id, enabled);
    } catch (e) {
      setErr(cleanErr(e));
    }
  }

  async function confirmUninstall() {
    if (!removing || busy) return;
    setBusy(true);
    setErr("");
    try {
      await api.pluginUninstall(removing.manifest.id, removeAgenda);
      setNote(`已卸载插件「${removing.manifest.name}」`);
      setRemoving(null);
    } catch (e) {
      setErr(cleanErr(e));
    } finally {
      setBusy(false);
    }
  }

  async function inspectMcp(p: PluginInfo) {
    if (busy) return;
    setBusy(true);
    setErr("");
    try {
      const res = await api.pluginMcpListTools(p.manifest.id);
      setMcpOutput({
        name: p.manifest.name,
        text: JSON.stringify(res, null, 2),
      });
    } catch (e) {
      setErr(`MCP 连接失败：${cleanErr(e)}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="tool-panel">
      <div className="row-between" style={{ flexWrap: "wrap" }}>
        <p className="hint">
          插件独立保存在本机数据目录，不与泥洞源码同仓库。支持 WebView 插件与标准 MCP Stdio 插件。
        </p>
        <div className="plugin-actions">
          <Button
            appearance="primary"
            icon={<ArrowUpload20Regular />}
            disabled={busy}
            onClick={() => {
              void pickPackageFile();
            }}
          >
            安装插件包 (.zip / .ndplugin)
          </Button>
          <Button
            appearance="secondary"
            icon={<FolderOpen20Regular />}
            disabled={busy}
            onClick={() => {
              void pickPluginFolder();
            }}
          >
            从文件夹安装
          </Button>
        </div>
      </div>

      {note ? <p className="hint" style={{ color: "var(--ok)" }}>{note}</p> : null}
      {err ? <p className="hint warn">{err}</p> : null}

      {plugins.length === 0 ? (
        <div className="share-drop">
          <p style={{ margin: "0 0 6px", fontWeight: 600 }}>暂未安装任何插件</p>
          <p className="hint">点击上方「安装插件包」或「从文件夹安装」导入第三方插件（如 MOOC 监视器、MCP 插件）。</p>
        </div>
      ) : (
        <div className="plugin-list">
          {plugins.map((p) => {
            const m = p.manifest;
            return (
              <div key={m.id} className={`plugin-card${p.enabled ? " on" : ""}`}>
                <div className="plugin-card-head">
                  <div className="plugin-card-title-row">
                    <span className="plugin-icon" aria-hidden="true">
                      <PlugConnected20Regular />
                    </span>
                    <span className="plugin-name">{m.name}</span>
                    <span className="plugin-ver">v{m.version}</span>
                    <span className="plugin-tag">
                      {m.runtime === "mcp" ? "MCP" : m.mcp ? "WebView + MCP" : "WebView"}
                    </span>
                    {m.background ? <span className="plugin-tag">后台常驻</span> : null}
                    {p.statusBadge ? <span className="plugin-badge-pill">{p.statusBadge}</span> : null}
                  </div>
                  <div className="plugin-card-controls">
                    {p.enabled && p.hasUi ? (
                      <Button appearance="secondary" size="small" onClick={() => onOpenPlugin(m.id)}>
                        打开面板
                      </Button>
                    ) : null}
                    {p.enabled && m.mcp ? (
                      <Button
                        appearance="secondary"
                        size="small"
                        disabled={busy}
                        onClick={() => {
                          void inspectMcp(p);
                        }}
                      >
                        测试 MCP
                      </Button>
                    ) : null}
                    <Switch
                      checked={p.enabled}
                      onChange={(_, d) => {
                        void togglePlugin(m.id, d.checked);
                      }}
                    />
                    <Button
                      appearance="subtle"
                      size="small"
                      icon={<Delete20Regular />}
                      aria-label={`卸载 ${m.name}`}
                      onClick={() => {
                        setRemoveAgenda(true);
                        setRemoving(p);
                      }}
                    />
                  </div>
                </div>
                {m.description ? <p className="hint">{m.description}</p> : null}
                <div className="plugin-meta">
                  <span>ID：{m.id}</span>
                  {m.author ? <span>作者：{m.author}</span> : null}
                  {m.permissions && m.permissions.length > 0 ? (
                    <span>权限：{m.permissions.join("、")}</span>
                  ) : null}
                </div>
              </div>
            );
          })}
        </div>
      )}

      <Dialog
        open={!!removing}
        onOpenChange={(_, data) => {
          if (!data.open) setRemoving(null);
        }}
      >
        <DialogSurface className="safety-dialog">
          <DialogBody>
            <DialogTitle>卸载插件「{removing?.manifest.name}」？</DialogTitle>
            <DialogContent className="safety-copy">
              <p>卸载后将从本机删除该插件文件及私有缓存。</p>
              <Checkbox
                label="同时清除该插件同步到日程表中的所有日程"
                checked={removeAgenda}
                onChange={(_, d) => setRemoveAgenda(!!d.checked)}
              />
            </DialogContent>
            <DialogActions>
              <Button appearance="secondary" onClick={() => setRemoving(null)}>
                取消
              </Button>
              <Button
                appearance="primary"
                disabled={busy}
                onClick={() => {
                  void confirmUninstall();
                }}
              >
                确认卸载
              </Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>

      <Dialog
        open={!!mcpOutput}
        onOpenChange={(_, data) => {
          if (!data.open) setMcpOutput(null);
        }}
      >
        <DialogSurface className="safety-dialog">
          <DialogBody>
            <DialogTitle>MCP 工具列表 · {mcpOutput?.name}</DialogTitle>
            <DialogContent className="safety-copy">
              <pre className="plugin-mcp-pre">{mcpOutput?.text}</pre>
            </DialogContent>
            <DialogActions>
              <Button appearance="primary" onClick={() => setMcpOutput(null)}>
                知道了
              </Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
    </div>
  );
}
