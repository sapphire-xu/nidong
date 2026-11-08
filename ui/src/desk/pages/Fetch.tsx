import { Button, Dialog, DialogActions, DialogBody, DialogContent, DialogSurface, DialogTitle, Field, Input, Switch } from "@fluentui/react-components";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import { api, type FetchFile, type FetchStatus } from "../../api";

function message(error: unknown) {
  return String(error).replace(/^Error: /, "");
}

function sizeText(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

function whenText(when: string) {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/.exec(when);
  if (!match) return when;
  return `${Number(match[2])}月${Number(match[3])}日 ${match[4]}:${match[5]}`;
}

function hhmm(when: string) {
  const match = /T(\d{2}:\d{2})/.exec(when);
  return match ? match[1] : "00:00";
}

function nextClockLabel(value: string, now = new Date()) {
  const match = /^(\d{1,2}):(\d{2})$/.exec(value.trim());
  if (!match) return "";
  const hour = Number(match[1]);
  const minute = Number(match[2]);
  if (hour > 23 || minute > 59) return "";
  const target = new Date(now);
  target.setHours(hour, minute, 0, 0);
  if (target.getTime() <= now.getTime()) target.setDate(target.getDate() + 1);
  const hh = String(hour).padStart(2, "0");
  const mm = String(minute).padStart(2, "0");
  return `${target.getMonth() + 1}月${target.getDate()}日 ${hh}:${mm}`;
}

function armedNote(when: string) {
  const text = whenText(when);
  const at = Date.parse(when);
  if (Number.isNaN(at) || at > Date.now()) return `等到 ${text} 开始。`;
  return "正在开始…";
}

function fileMeta(file: FetchFile) {
  if (file.phase === "pending") return "等待";
  if (file.phase === "failed") return file.error || "失败";
  if (file.phase === "done") return `完成 · ${sizeText(file.received)}`;
  if (file.total) return `${sizeText(file.received)} / ${sizeText(file.total)}`;
  return sizeText(file.received);
}

function proxyHint(status: FetchStatus | null, useProxy: boolean) {
  if (!useProxy) {
    return status?.proxy
      ? `这次直接连接，不走系统代理 ${status.proxy}。`
      : "这次直接连接，不走系统代理。";
  }
  if (status?.proxyKind === "http" && status.proxy) {
    return `走系统代理 ${status.proxy}。出校门的流量仍算校园网流量，白天一样计费。`;
  }
  if (status?.proxyKind === "socks") {
    return "系统代理是 SOCKS，泥洞不会用它。请改成 HTTP 代理，或开 TUN 后关掉这项。";
  }
  return "没有读到系统代理。开了 TUN 或 WARP 时，直接连接就会进隧道。";
}

export function FetchPanel() {
  const [status, setStatus] = useState<FetchStatus | null>(null);
  const [urls, setUrls] = useState("");
  const [time, setTime] = useState("00:00");
  const [dir, setDir] = useState("");
  const [shutdown, setShutdown] = useState(false);
  const [useProxy, setUseProxy] = useState(false);
  const [busy, setBusy] = useState(false);
  const [localError, setLocalError] = useState("");
  const [help, setHelp] = useState(false);
  const seeded = useRef(false);

  useEffect(() => {
    let stop = false;
    const pull = async () => {
      try {
        const next = await api.fetchStatus();
        if (stop) return;
        setStatus(next);
        if (!seeded.current) {
          seeded.current = true;
          setDir(next.dir || "");
          setShutdown(next.shutdown);
          setUseProxy(next.useProxy);
          if (next.links) setUrls(next.links);
        }
      } catch (error) {
        if (!stop) setLocalError(message(error));
      }
    };
    void pull();
    const timer = window.setInterval(() => { void pull(); }, 1000);
    return () => {
      stop = true;
      window.clearInterval(timer);
    };
  }, []);

  const phase = status?.phase || "idle";
  const frozen = phase === "armed" || phase === "running" || phase === "shutting-down" || phase === "missed" || phase === "interrupted";
  const shownUrls = frozen ? (status?.links || status?.files.map((file) => file.url).join("\n") || "") : urls;
  const shownTime = frozen && status?.when ? hhmm(status.when) : time;
  const shownDir = frozen ? (status?.dir || dir) : dir;
  const shownShutdown = frozen ? !!status?.shutdown : shutdown;
  const shownProxy = frozen ? (status ? status.useProxy : useProxy) : useProxy;
  const preview = nextClockLabel(shownTime || "00:00");
  const note = localError || (status?.note ? status.note : phase === "armed" && status?.when ? armedNote(status.when) : phase === "running" ? "正在下载。关掉窗口也会继续，退出泥洞才停。" : "");
  const warn = !!localError || phase === "missed" || phase === "interrupted" || note.includes("没有") || note.includes("没下完");

  async function run(task: () => Promise<FetchStatus>) {
    if (busy) return;
    setBusy(true);
    setLocalError("");
    try {
      setStatus(await task());
    } catch (error) {
      setLocalError(message(error));
    } finally {
      setBusy(false);
    }
  }

  async function pickDir() {
    const picked = await open({ directory: true, multiple: false, title: "保存到哪个文件夹" });
    if (typeof picked === "string" && picked) setDir(picked);
  }

  return (
    <div className="tool-panel">
      <Field label="下载链接" hint="一行一个，只支持 http 和 https。关掉窗口仍会下载，退出泥洞才停。">
        <textarea
          className="fetch-links"
          value={shownUrls}
          disabled={frozen}
          spellCheck={false}
          placeholder="https://"
          onChange={(event) => setUrls(event.target.value)}
        />
      </Field>
      <Field
        label="下载时间"
        hint={frozen && status?.when
          ? `预定 ${whenText(status.when)}。0:00–6:00 校外 IPv4 下载不计费。`
          : preview
            ? `将在 ${preview} 开始。0:00–6:00 校外 IPv4 下载不计费。`
            : "写成 00:00 这样。0:00–6:00 校外 IPv4 下载不计费。"}
      >
        <Input
          className="fetch-time"
          value={shownTime}
          disabled={frozen}
          spellCheck={false}
          placeholder="00:00"
          onChange={(_, data) => setTime(data.value)}
        />
      </Field>
      <Field label="保存到" hint="没下完的文件以 .partial 结尾，同一条链接下次会接着下。">
        <div className="ssid-row">
          <Input value={shownDir} readOnly disabled={frozen} spellCheck={false} />
          <Button appearance="secondary" disabled={frozen || busy} onClick={() => { void pickDir(); }}>
            选择文件夹
          </Button>
        </div>
      </Field>
      <div>
        <Switch
          label="下载完成后关机"
          checked={shownShutdown}
          disabled={frozen}
          onChange={(_, data) => setShutdown(data.checked)}
        />
        <p className="hint">全部下完才关机，大约一分钟。退出泥洞也不会取消，可以在这里取消。</p>
      </div>
      <div>
        <Switch
          label="使用系统代理"
          checked={shownProxy}
          disabled={frozen}
          onChange={(_, data) => setUseProxy(data.checked)}
        />
        <p className="hint">{proxyHint(status, shownProxy)}</p>
      </div>
      <p className="hint">
        夜间断电？想要Steam定时下载？点击{" "}
        <button type="button" className="safety-ask" onClick={() => setHelp(true)}>了解更多</button>
      </p>
      {status && status.files.length > 0 ? (
        <ul className="fetch-list">
          {status.files.map((file) => (
            <li key={file.url} className="fetch-row" title={file.url}>
              <span className="fetch-name">{file.name || file.url}</span>
              <span className={file.phase === "failed" ? "fetch-meta bad" : "fetch-meta"}>{fileMeta(file)}</span>
            </li>
          ))}
        </ul>
      ) : null}
      {note ? <p className={warn ? "hint warn" : "hint"}>{note}</p> : null}
      {(phase === "missed" || phase === "interrupted") ? (
        <p className="hint warn">现在继续会马上开始。若不在 0:00–6:00，校外 IPv4 下载会计费。</p>
      ) : null}
      <div className="fetch-actions">
        {phase === "shutting-down" ? (
          <Button appearance="secondary" disabled={busy} onClick={() => { void run(() => api.fetchCancel()); }}>取消关机</Button>
        ) : phase === "armed" || phase === "running" ? (
          <Button appearance="secondary" disabled={busy} onClick={() => { void run(() => api.fetchCancel()); }}>取消</Button>
        ) : phase === "missed" || phase === "interrupted" ? (
          <>
            <Button appearance="primary" disabled={busy} onClick={() => { void run(() => api.fetchResume()); }}>仍然继续</Button>
            <Button appearance="secondary" disabled={busy} onClick={() => { void run(() => api.fetchCancel()); }}>放弃</Button>
          </>
        ) : (
          <Button
            appearance="primary"
            disabled={busy || !shownUrls.trim() || !shownDir}
            onClick={() => { void run(() => api.fetchArm(shownUrls, shownTime || "00:00", shownDir, shownShutdown, shownProxy)); }}
          >
            到点下载
          </Button>
        )}
      </div>
      <p className="hint">请在下载时段保持电脑与泥洞开启</p>
      <Dialog open={help} onOpenChange={(_, data) => { if (!data.open) setHelp(false); }}>
        <DialogSurface className="safety-dialog">
          <DialogBody>
            <DialogTitle>了解更多</DialogTitle>
            <DialogContent className="safety-copy">
              <p>夜里如果因没电电脑关机，会停在已经落盘的进度。过了6点不会自动接着下。</p>
              <p>Steam的游戏下载不经过泥洞。若要实现Steam定时下载，在Steam的设置→下载里，可以把自动更新限制在0:00-6:00，并保持电脑和Steam开启。此方法也不能让Steam下载结束后关机。</p>
              <p>打开「使用系统代理」后，走Windows当前的代理，常见是127.0.0.1上的端口。开了TUN或WARP时，无论打开与否效果一样。如果你不了解这些，可以忽略。</p>
            </DialogContent>
            <DialogActions>
              <Button appearance="primary" onClick={() => setHelp(false)}>知道了</Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
    </div>
  );
}
