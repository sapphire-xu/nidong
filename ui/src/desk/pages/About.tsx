import { useEffect, useState } from "react";
import { Button } from "@fluentui/react-components";
import { getVersion } from "@tauri-apps/api/app";
import { api } from "../../api";

export function About(props: {
  updateVersion: string | null;
  updateBusy: boolean;
  updateFailed: boolean;
  checkState: "idle" | "checking" | "current" | "failed";
  autoUpdateSupported?: boolean;
  isLinux?: boolean;
  onCheck: () => void;
  onInstall: () => void;
}) {
  const autoUpdateSupported = props.autoUpdateSupported !== false;
  const [version, setVersion] = useState("");
  useEffect(() => {
    let alive = true;
    getVersion()
      .then((v) => {
        if (alive && v.trim()) setVersion(v.trim());
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);
  const status = !autoUpdateSupported
    ? "当前安装方式不支持应用内更新，请前往发布页下载新版本"
    : props.updateVersion
      ? `有新版本 ${props.updateVersion}`
      : props.checkState === "checking"
        ? "正在检查…"
        : props.checkState === "current"
          ? "已是最新版本"
          : props.checkState === "failed"
            ? "暂时检查不到更新"
            : "打开窗口时会悄悄检查一次";
  const updateButton = !autoUpdateSupported ? (
    <Button
      appearance="secondary"
      onClick={() => {
        void api.openDoor("https://github.com/sapphire-xu/nidong/releases", "direct").catch(() => undefined);
      }}
    >
      打开发布页
    </Button>
  ) : props.updateVersion ? (
    <Button appearance="secondary" disabled={props.updateBusy} onClick={props.onInstall}>
      {props.updateBusy ? "正在下载…" : "更新并重启"}
    </Button>
  ) : (
    <Button appearance="secondary" disabled={props.checkState === "checking"} onClick={props.onCheck}>
      检查更新
    </Button>
  );

  return (
    <div className="stack about-page">
      <h1 className="page-title">关于泥洞</h1>
      <p className="about-lead">非官方桌面看板工具，致力于集成多种校园生活功能，优化学生体验。</p>
      <p className="hint">
        提醒铃声 Argon 来自 Android 开源工程，版权归 The Android Open Source Project，依照 Apache-2.0 使用。全文在
        {props.isLinux ? "程序资源目录" : "安装目录"}
        的 Argon-NOTICE.txt。
      </p>
      <dl className="about-meta">
        <dt>作者</dt>
        <dd>sapphire_fic</dd>
        <dt>官网</dt>
        <dd>
          <button
            type="button"
            className="about-mail"
            onClick={() => { void api.openDoor("https://nidong.sapphirefic.tech/", "direct").catch(() => undefined); }}
          >
            https://nidong.sapphirefic.tech/
          </button>
        </dd>
        <dt>反馈</dt>
        <dd>
          <button type="button" className="about-mail" onClick={() => { void api.openFeedback().catch(() => undefined); }}>
            xufeiyang@mails.neu.edu.cn
          </button>
        </dd>
      </dl>
      <div className="about-update">
        <div className="about-update-copy">
          {version ? <div className="about-ver">v{version}</div> : null}
          <p className="hint">{status}</p>
          {props.updateFailed ? <p className="hint warn">更新没有完成，稍后再试。</p> : null}
        </div>
        {updateButton}
      </div>
    </div>
  );
}
