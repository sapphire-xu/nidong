import { useEffect, useState } from "react";
import { Button } from "@fluentui/react-components";
import { getVersion } from "@tauri-apps/api/app";
import { api } from "../../api";

export function About(props: {
  updateVersion: string | null;
  updateBusy: boolean;
  updateFailed: boolean;
  checkState: "idle" | "checking" | "current" | "failed";
  onCheck: () => void;
  onInstall: () => void;
}) {
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
  const status = props.updateVersion
    ? `有新版本 ${props.updateVersion}`
    : props.checkState === "checking"
      ? "正在检查…"
      : props.checkState === "current"
        ? "已是最新版本"
        : props.checkState === "failed"
          ? "暂时检查不到更新"
          : "打开窗口时会悄悄检查一次";
  const updateButton = props.updateVersion ? (
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
      <dl className="about-meta">
        <dt>作者</dt>
        <dd>sapphire_fic</dd>
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
