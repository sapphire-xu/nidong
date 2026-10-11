import { Button, Checkbox, Dialog, DialogActions, DialogBody, DialogContent, DialogSurface, DialogTitle, Field, Input } from "@fluentui/react-components";
import { QuestionCircle20Regular } from "@fluentui/react-icons";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import { api, type ShareFile, type ShareOffer } from "../../api";
import type { AppSettings } from "../../types";

const MAX_FILES = 100;

function message(error: unknown) {
  return String(error).replace(/^Error: /, "");
}

function sizeText(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

function mergeByName(prev: ShareFile[], incoming: ShareFile[]) {
  const next = [...prev];
  for (const file of incoming) {
    const index = next.findIndex((item) => item.name === file.name);
    if (index >= 0) {
      const kept = next[index].id;
      next[index] = kept ? { ...file, id: kept } : file;
    } else {
      next.push(file);
    }
  }
  return next;
}

function without(list: ShareFile[], file: ShareFile) {
  if (file.id) return list.filter((item) => item.id !== file.id);
  return list.filter((item) => item.path !== file.path);
}

export function SharePanel(props: { settings: AppSettings; onSettings: (patch: Partial<AppSettings>) => void }) {
  const { settings, onSettings } = props;
  const [items, setItems] = useState<ShareFile[]>([]);
  const [offer, setOffer] = useState<ShareOffer | null>(null);
  const [note, setNote] = useState("");
  const [copied, setCopied] = useState("");
  const [busy, setBusy] = useState(false);
  const [over, setOver] = useState(false);
  const [help, setHelp] = useState(false);
  const [skip, setSkip] = useState(false);
  const [warn, setWarn] = useState(false);
  const sharing = offer !== null;
  const sharingRef = useRef(false);
  const itemsRef = useRef(items);
  const warnRef = useRef(false);
  const startLock = useRef(false);
  const addRef = useRef<(paths: string[]) => void>(() => {});
  const gate = useRef(Promise.resolve());

  function commitItems(next: ShareFile[]) {
    itemsRef.current = next;
    setItems(next);
  }

  function runExclusive(job: () => Promise<void>) {
    const next = gate.current.then(job, job);
    gate.current = next.then(() => {}, () => {});
    return next;
  }

  async function add(paths: string[]) {
    if (paths.length === 0) return;
    await runExclusive(async () => {
      try {
        const out = await api.shareDescribe(paths);
        if (out.files.length === 0) {
          setNote(out.note || "");
          return;
        }
        let merged = mergeByName(itemsRef.current, out.files);
        let hint = out.note || "";
        if (merged.length > MAX_FILES) {
          merged = merged.slice(0, MAX_FILES);
          hint = "一次最多分享 100 个文件";
        }
        if (sharingRef.current) {
          const next = await api.shareRevise(merged.map((file) => file.path));
          sharingRef.current = true;
          setOffer(next);
          commitItems(next.files);
        } else {
          commitItems(merged);
        }
        setNote(hint);
      } catch (error) {
        setNote(message(error));
        if (!sharingRef.current) return;
        try {
          const current = await api.shareOffer();
          if (!current) return;
          sharingRef.current = true;
          setOffer(current);
          commitItems(current.files);
        } catch {
          // 名单以服务端为准。拉不回来就留着上面那句错误。
        }
      }
    });
  }

  addRef.current = (paths) => {
    void add(paths);
  };

  function take(kind: string, paths: string[]) {
    if (kind === "enter" || kind === "over") {
      setOver(true);
      return;
    }
    if (kind === "leave") {
      setOver(false);
      return;
    }
    setOver(false);
    addRef.current(paths);
  }

  function openHelp() {
    setSkip(settings.shareHintDismissed === true);
    setHelp(true);
  }

  function closeHelp() {
    setHelp(false);
    if (skip !== (settings.shareHintDismissed === true)) {
      onSettings({ shareHintDismissed: skip });
    }
  }

  function dismissWarn() {
    warnRef.current = false;
    setWarn(false);
  }

  useEffect(() => {
    // 开发模式会把 effect 跑两遍。这里是同步打开，不能用 ref 把第二次吃掉。
    if (settings.shareHintDismissed === true) return;
    setSkip(false);
    setHelp(true);
  }, []);

  useEffect(() => {
    let dead = false;
    void api.shareOffer().then((current) => {
      if (dead || !current) return;
      sharingRef.current = true;
      setOffer(current);
      commitItems(current.files);
    });
    return () => {
      dead = true;
    };
  }, []);

  useEffect(() => {
    if (!sharing) return;
    const timer = window.setInterval(() => {
      void api.shareOffer().then((current) => {
        if (!sharingRef.current) return;
        if (!current) {
          sharingRef.current = false;
          setOffer(null);
          return;
        }
        setOffer((prev) => (prev ? { ...current, files: prev.files } : prev));
      });
    }, 2000);
    return () => window.clearInterval(timer);
  }, [sharing]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let dead = false;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "drop") take(payload.type, payload.paths);
        else if (payload.type === "enter") take(payload.type, payload.paths);
        else take(payload.type, []);
      })
      .then((stop) => {
        if (dead) stop();
        else unlisten = stop;
      })
      .catch(() => {});
    return () => {
      dead = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let dead = false;
    void listen<{ kind: string; paths: string[] }>("desk-file-drop", (event) => {
      take(event.payload.kind, event.payload.paths || []);
    }).then((stop) => {
      if (dead) stop();
      else unlisten = stop;
    });
    return () => {
      dead = true;
      unlisten?.();
    };
  }, []);

  async function browse() {
    const picked = await open({ multiple: true, directory: false, title: "选择要分享的文件" });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    await add(paths);
  }

  async function remove(file: ShareFile) {
    await runExclusive(async () => {
      const next = without(itemsRef.current, file);
      if (!sharingRef.current) {
        commitItems(next);
        return;
      }
      try {
        const revised = await api.shareRevise(next.map((item) => item.path));
        sharingRef.current = true;
        setOffer(revised);
        commitItems(revised.files);
      } catch (error) {
        setNote(message(error));
        try {
          const current = await api.shareOffer();
          if (!current) return;
          sharingRef.current = true;
          setOffer(current);
          commitItems(current.files);
        } catch {
          // 同上，留着错误说明。
        }
      }
    });
  }

  async function begin(paths: string[]) {
    setBusy(true);
    setCopied("");
    setNote("");
    try {
      const next = await api.shareStart(paths);
      sharingRef.current = true;
      setOffer(next);
      commitItems(next.files);
    } catch (error) {
      setNote(message(error));
    } finally {
      setBusy(false);
    }
  }

  async function start() {
    if (busy || startLock.current || warnRef.current || itemsRef.current.length === 0) return;
    startLock.current = true;
    try {
      await runExclusive(async () => {
        const paths = itemsRef.current.map((file) => file.path);
        if (paths.length === 0) return;
        setCopied("");
        setNote("");
        let onCampus = false;
        try {
          onCampus = await api.shareOnCampus();
        } catch (error) {
          setNote(message(error));
          return;
        }
        if (!onCampus) {
          warnRef.current = true;
          setHelp(false);
          setWarn(true);
          return;
        }
        await begin(paths);
      });
    } finally {
      startLock.current = false;
    }
  }

  async function continueShare() {
    dismissWarn();
    if (startLock.current) return;
    startLock.current = true;
    try {
      await runExclusive(async () => {
        const paths = itemsRef.current.map((file) => file.path);
        if (paths.length === 0) return;
        await begin(paths);
      });
    } finally {
      startLock.current = false;
    }
  }

  async function stop() {
    await runExclusive(async () => {
      setBusy(true);
      try {
        await api.shareStop();
        sharingRef.current = false;
        setOffer(null);
        setCopied("");
      } catch (error) {
        setNote(message(error));
      } finally {
        setBusy(false);
      }
    });
  }

  async function copy(text: string, done: string) {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(done);
    } catch {
      setCopied("复制失败，请手动选择文字");
    }
  }

  function helpButton() {
    return (
      <Button appearance="subtle" icon={<QuestionCircle20Regular />} aria-label="说明" onClick={openHelp} />
    );
  }

  return (
    <>
      {sharing && offer ? (
        <>
          <Field label="网址">
            <div className="ssid-row">
              <Input readOnly value={offer.url} spellCheck={false} />
              <Button appearance="secondary" onClick={() => { void copy(offer.url, "网址已复制"); }}>复制</Button>
            </div>
          </Field>
          <Field label="口令" hint="对方打开网址后输入。分开告诉对方。只转发网址时，没有口令下不了文件。">
            <div className="ssid-row">
              <Input className="share-code" readOnly value={offer.code} spellCheck={false} />
              <Button appearance="secondary" onClick={() => { void copy(offer.code, "口令已复制"); }}>复制</Button>
            </div>
          </Field>
          {copied ? <p className="hint">{copied}</p> : null}
        </>
      ) : null}
      <div className={over ? "share-drop over" : "share-drop"}>{over ? "松开即可添加" : "拖入文件上传"}</div>
      {items.length > 0 ? (
        <ul className="share-files">
          {items.map((file) => (
            <li key={file.id ? `id-${file.id}` : file.path}>
              <span className="name">{file.name}</span>
              <span className="size">{sizeText(file.bytes)}</span>
              <Button appearance="subtle" onClick={() => { void remove(file); }}>删除</Button>
            </li>
          ))}
        </ul>
      ) : null}
      {sharing && offer ? (
        <p className="share-live">
          {offer.unlocks > 0
            ? `口令已通过 ${offer.unlocks} 次，正在下载 ${offer.activeDownloads} 路。`
            : "还没有人输对口令。"}
          {offer.lastPeer ? ` 最近打开：${offer.lastPeer}` : ""}
        </p>
      ) : null}
      <div className="share-actions">
        <Button appearance="secondary" onClick={() => { void browse(); }}>添加文件</Button>
        {sharing ? (
          <Button appearance="secondary" disabled={busy} onClick={() => { void stop(); }}>停止分享</Button>
        ) : (
          <Button appearance="primary" disabled={busy || items.length === 0} onClick={() => { void start(); }}>
            {busy ? "正在开始" : "开始分享"}
          </Button>
        )}
        {helpButton()}
      </div>
      {note ? <p className="hint warn">{note}</p> : null}
      <Dialog open={warn} onOpenChange={(_, data) => { if (!data.open) dismissWarn(); }}>
        <DialogSurface className="safety-dialog">
          <DialogBody>
            <DialogTitle>不在校园网</DialogTitle>
            <DialogContent className="safety-copy">
              <p>当前不在校园网，该操作不会节省校园流量，并且仅在同局域网内可能可以使用，这取决于当前网络是否能互访。</p>
              <p>除非你明确知道你在干什么，否则不建议开启分享服务。</p>
            </DialogContent>
            <DialogActions fluid>
              <div className="share-warn-actions">
                <Button appearance="secondary" onClick={() => { void continueShare(); }}>仍然继续</Button>
                <Button appearance="primary" onClick={dismissWarn}>取消开启</Button>
              </div>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
      <Dialog open={help} onOpenChange={(_, data) => { if (!data.open) closeHelp(); }}>
        <DialogSurface className="safety-dialog">
          <DialogBody>
            <DialogTitle>免流文件互传</DialogTitle>
            <DialogContent className="safety-copy">
              <p><strong>基于校园网的文件传输，此过程不消耗流量。</strong></p>
              <p>发送和接收都要先连上校园网并登录。支持一百人以内同时下载。</p>
              <p>接收者无需安装泥洞。电脑和手机的浏览器都可以打开网址，输入口令后下载。</p>
              <p>关掉窗口也会继续分享，退出泥洞/停止分享才会停。</p>
              <p>分享开始后也可以增删文件。拖入同名文件，以新的那份为准。</p>
              <p>对方打不开页面时，请检查防火墙中设置允许泥洞访问公用网络。</p>
            </DialogContent>
            <DialogActions fluid>
              <div className="share-help-actions">
                <Checkbox label="不再提示" checked={skip} onChange={(_, data) => setSkip(data.checked === true)} />
                <Button appearance="primary" onClick={closeHelp}>好</Button>
              </div>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
    </>
  );
}
