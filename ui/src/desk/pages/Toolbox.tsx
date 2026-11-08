import { Button, Dialog, DialogActions, DialogBody, DialogContent, DialogSurface, DialogTitle, Dropdown, Field, Input, Option, Switch } from "@fluentui/react-components";
import { ArrowDownload20Regular, Share20Regular, SpeakerMute20Regular, Wifi120Regular } from "@fluentui/react-icons";
import { useEffect, useRef, useState } from "react";
import { api } from "../../api";
import { clampIdleMinutes } from "../../prefs";
import type { AppSettings } from "../../types";
import { Threshold } from "../../Threshold";
import { FetchPanel } from "./Fetch";
import { SharePanel } from "./Share";

const MODES: { value: NonNullable<AppSettings["campusWifiMode"]>; text: string }[] = [
  { value: "off", text: "关闭" },
  { value: "offline", text: "检测到未联网时自动连接校园网并登录" },
  { value: "boot", text: "仅在开机时尝试连接一次校园网并登录" },
  { value: "hotspot", text: "仅当从指定网络断开连接时自动连接校园网并登录" },
  { value: "manual", text: "仅在手动连接至校园网时自动登录" },
];

const CAMPUS_NETS = ["NEU", "NEU-2.4G", "NEU-Mobile"];

function sameNet(a: string, b: string) {
  const left = a.trim();
  const right = b.trim();
  return left.length > 0 && left.localeCompare(right, undefined, { sensitivity: "accent" }) === 0;
}

function isCampusNet(name: string, configured: string) {
  return sameNet(name, configured) || CAMPUS_NETS.some((n) => sameNet(name, n));
}

export function Toolbox(props: { settings: AppSettings; setSettings: (s: AppSettings) => void; openShare?: number }) {
  const { settings, setSettings, openShare } = props;
  const [page, setPage] = useState<"hub" | "campus" | "share" | "mute" | "fetch">(openShare ? "share" : "hub");
  const [motion, setMotion] = useState<"idle" | "back">("idle");
  const sharePanelRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!openShare) return;
    setMotion("idle");
    setPage("share");
    const el = sharePanelRef.current;
    if (!el) return;
    el.classList.remove("settings-sub-content");
    void el.offsetWidth;
    el.classList.add("settings-sub-content");
  }, [openShare]);
  const openPage = (next: "hub" | "campus" | "share" | "mute" | "fetch") => {
    setMotion(next === "hub" && page !== "hub" ? "back" : "idle");
    setPage(next);
  };
  const mode = settings.campusWifiMode || "off";
  const current = MODES.find((m) => m.value === mode) || MODES[0];
  const set = (patch: Partial<AppSettings>) => setSettings({ ...settings, ...patch });
  const remembered = settings.rememberPassword !== false;
  const [filling, setFilling] = useState(false);
  const [fillNote, setFillNote] = useState("");
  const [safetyOpen, setSafetyOpen] = useState(false);
  const campusName = (settings.campusWifiSsid || "").trim() || "NEU";
  const hotspotName = (settings.campusHotspotSsid || "").trim();
  const hotspotIsCampus = isCampusNet(hotspotName, campusName);
  const idleMinutes = clampIdleMinutes(settings.smartMuteIdleMinutes);

  async function fillCurrent() {
    if (filling) return;
    setFilling(true);
    setFillNote("");
    try {
      const name = await api.currentWifi();
      set({ campusHotspotSsid: name });
    } catch (e) {
      const text = String(e).replace(/^Error: /, "");
      setFillNote(text || "现在没有连上无线");
    } finally {
      setFilling(false);
    }
  }

  return (
    <div className="stack">
      <div className="row-between">
        {page === "hub" ? (
          <h1 className="page-title">工具箱</h1>
        ) : (
          <div className="settings-breadcrumb">
            <button type="button" className="settings-breadcrumb-link" onClick={() => openPage("hub")}>工具箱</button>
            <span aria-hidden="true">›</span>
            <h1 className="page-title">{page === "campus" ? "校园网自动登录" : page === "share" ? "免流文件分享" : page === "mute" ? "防社死设置" : "定时下载"}</h1>
          </div>
        )}
      </div>
      {page === "hub" ? (
        <div className={`tool-grid${motion === "back" ? " settings-hub-content" : ""}`}>
          <button type="button" className="tool-tile on" onClick={() => openPage("campus")}>
            <span className="tool-ico" aria-hidden="true"><Wifi120Regular /></span>
            <span className="tool-name">校园网自动登录</span>
          </button>
          <button type="button" className="tool-tile on" onClick={() => openPage("share")}>
            <span className="tool-ico" aria-hidden="true"><Share20Regular /></span>
            <span className="tool-name">免流文件分享</span>
          </button>
          <button type="button" className="tool-tile on" onClick={() => openPage("mute")}>
            <span className="tool-ico" aria-hidden="true"><SpeakerMute20Regular /></span>
            <span className="tool-name">防社死设置</span>
          </button>
          <button type="button" className="tool-tile on" onClick={() => openPage("fetch")}>
            <span className="tool-ico" aria-hidden="true"><ArrowDownload20Regular /></span>
            <span className="tool-name">定时下载</span>
          </button>
        </div>
      ) : null}
      {page === "fetch" ? (
        <div className="settings-sub-content">
          <FetchPanel />
        </div>
      ) : null}
      {page === "share" ? (
        <div ref={sharePanelRef} className="tool-panel settings-sub-content">
          <SharePanel settings={settings} onSettings={set} />
        </div>
      ) : null}
      {page === "mute" ? (
        <div className="tool-panel settings-sub-content">
          <p className="hint">以下开关默认关闭，静音作用于系统默认输出设备。</p>
          <section className="mute-group" aria-labelledby="smart-mute-title">
            <div className="mute-head">
              <span className="mute-icon" aria-hidden="true"><SpeakerMute20Regular /></span>
              <div className="mute-copy">
                <h2 id="smart-mute-title" className="mute-title">智能静音</h2>
                <p className="hint">离开电脑、上课或日程中，自动静音。</p>
              </div>
            </div>
            <div className="mute-body">
              <div className="mute-item">
                <Switch
                  label="长时间无音频自动静音"
                  checked={!!settings.smartMuteIdle}
                  onChange={(_, data) => set({ smartMuteIdle: data.checked })}
                />
                {settings.smartMuteIdle ? (
                  <div className="mute-window">
                    <Threshold
                      label="无音频多久后静音（分钟）"
                      value={idleMinutes}
                      min={20}
                      max={200}
                      onCommit={(minutes) => set({ smartMuteIdleMinutes: clampIdleMinutes(minutes) })}
                    />
                    <p className="hint">可以调 20–200 分钟。手动取消静音后重新计时。</p>
                  </div>
                ) : null}
              </div>
              <div className="mute-item">
                <Switch
                  label="课程自动静音"
                  checked={!!settings.smartMuteClass}
                  onChange={(_, data) => set({ smartMuteClass: data.checked })}
                />
                <p className="hint">按本学期课表，在上课时间段内静音。</p>
              </div>
              <div className="mute-item">
                <Switch
                  label="日程自动静音"
                  checked={!!settings.smartMuteAgenda}
                  onChange={(_, data) => set({ smartMuteAgenda: data.checked })}
                />
                <p className="hint">在自定义日程的起止时间内静音。</p>
              </div>
            </div>
          </section>
          <p className="hint">手动取消静音后，本段课程、日程或这次无音频静音不再动手。</p>
        </div>
      ) : null}
      {page === "campus" ? (
      <>
      <div className="tool-panel settings-sub-content">
        <Field label="登录规则设置">
          <Dropdown
            className="tool-select"
            listbox={{ className: "tool-select-list" }}
            appearance="outline"
            value={current.text}
            selectedOptions={[current.value]}
            onOptionSelect={(_, data) => {
              const next = MODES.find((m) => m.value === data.optionValue);
              if (next) set({ campusWifiMode: next.value });
            }}
          >
            {MODES.map((m) => (
              <Option key={m.value} value={m.value} text={m.text}>
                {m.text}
              </Option>
            ))}
          </Dropdown>
        </Field>
        {mode !== "off" ? (
          <Field label="校园网名称" hint="和系统里保存的无线配置同名。空着就用 NEU。南湖、浑南还有 NEU-2.4G、NEU-Mobile。">
            <Input
              value={settings.campusWifiSsid || ""}
              placeholder="NEU"
              spellCheck={false}
              autoComplete="off"
              onChange={(_, d) => set({ campusWifiSsid: d.value })}
            />
          </Field>
        ) : null}
        {mode === "hotspot" ? (
          <div className="ssid-block">
            <Field label="指定网络" hint="只有这次运行里连上过它，再断开，才会去连校园网。">
              <div className="ssid-row">
                <Input
                  value={settings.campusHotspotSsid || ""}
                  placeholder="手机热点的名称"
                  spellCheck={false}
                  autoComplete="off"
                  onChange={(_, d) => {
                    setFillNote("");
                    set({ campusHotspotSsid: d.value });
                  }}
                />
                <Button appearance="secondary" disabled={filling} onClick={() => { void fillCurrent(); }}>
                  填入当前网络
                </Button>
              </div>
            </Field>
            {hotspotIsCampus ? <p className="hint warn">这是校园网的名字。从 NEU、NEU-2.4G、NEU-Mobile 断开时，不会再自动连上校园网。</p> : null}
            {fillNote ? <p className="hint warn">{fillNote}</p> : null}
          </div>
        ) : null}
        <p className={remembered ? "hint" : "hint warn"}>
          {remembered
            ? "登录网关要用这台电脑记住的一号通密码。关掉「记住密码」之后，仍可以连接校园网无线，但不能代为登录。"
            : "当前没有记住密码。可以连接校园网无线，但不能代为登录。"}
        </p>
        <button type="button" className="safety-ask" onClick={() => setSafetyOpen(true)}>
          这安全吗？
        </button>
      </div>
      <Dialog open={safetyOpen} onOpenChange={(_, data) => { if (!data.open) setSafetyOpen(false); }}>
        <DialogSurface className="safety-dialog">
          <DialogBody>
            <DialogTitle>这安全吗？</DialogTitle>
            <DialogContent className="safety-copy">
              <p>为了防止进入仿冒校园网，泥洞在提交密码之前先核对学校登录页。</p>
              <p>
                无线名是 NEU、NEU-2.4G 或 NEU-Mobile 时，仍会先打开 https://pass.neu.edu.cn，核对系统信任的证书。密码只加密后发给这个地址。证书对不上，或页面要把密码发往别处，泥洞就停下来。岛上写「这不是校园网，密码没提交」，并断开当前无线，大约一分钟内不再尝试。
              </p>
              <p>
                <span className="safety-warn">遇到可疑的校园网同名网络一定要慎重，谨防泄露密码。</span>
                不要在那种网络里手输一号通密码。别的软件不会替你核对。假网络若把流量原样转到学校，泥洞交给学校的密码对方看不到，没加密的上网内容仍可能被看到。
              </p>
            </DialogContent>
            <DialogActions>
              <Button appearance="primary" onClick={() => setSafetyOpen(false)}>知道了</Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
      </>
      ) : null}
    </div>
  );
}
