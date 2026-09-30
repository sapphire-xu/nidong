import { Button } from "@fluentui/react-components";

export function About(props: {
  updateVersion: string | null;
  updateBusy: boolean;
  updateFailed: boolean;
  checkState: "idle" | "checking" | "current" | "failed";
  onCheck: () => void;
  onInstall: () => void;
}) {
  const status = props.updateVersion
    ? `有新版本 ${props.updateVersion}`
    : props.checkState === "checking"
      ? "正在检查…"
      : props.checkState === "current"
        ? "已是最新版本"
        : props.checkState === "failed"
          ? "暂时检查不到更新"
          : "打开窗口时会悄悄检查一次";
  return (
    <div className="stack">
      <h1 className="page-title">关于泥洞</h1>
      <div className="about-ver">v2.0.1</div>
      <p>非官方工具，数据只留在这台电脑上。</p>
      <p className="hint">作者 sapphire_fic</p>
      <p className="hint">界面控件来自 Fluent UI（MIT）。动效时长参考 WinUIonWeb，与微软无关。</p>
      <div className="row-between">
        <span className="hint">{status}</span>
        {props.updateVersion ? (
          <Button appearance="subtle" disabled={props.updateBusy} onClick={props.onInstall}>
            {props.updateBusy ? "正在下载…" : "更新并重启"}
          </Button>
        ) : (
          <Button appearance="subtle" disabled={props.checkState === "checking"} onClick={props.onCheck}>
            检查更新
          </Button>
        )}
      </div>
      {props.updateFailed ? <p className="hint warn">更新没有完成，稍后再试。</p> : null}
    </div>
  );
}
