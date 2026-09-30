import { Button, Dialog, DialogActions, DialogBody, DialogContent, DialogSurface, DialogTitle, Dropdown, Field, Input, Option } from "@fluentui/react-components";
import { BookOpen20Regular, Clock20Regular, Library20Regular, Wifi120Regular } from "@fluentui/react-icons";
import { useState, type ReactNode } from "react";
import { api } from "../../api";
import type { AppSettings } from "../../types";

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

const LATER: { icon: ReactNode }[] = [
  { icon: <Clock20Regular /> },
  { icon: <BookOpen20Regular /> },
  { icon: <Library20Regular /> },
];

export function Toolbox(props: { settings: AppSettings; setSettings: (s: AppSettings) => void }) {
  const { settings, setSettings } = props;
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
        <h1 className="page-title">工具箱</h1>
      </div>
      <div className="tool-grid">
        <button type="button" className="tool-tile on" aria-pressed="true">
          <span className="tool-ico" aria-hidden="true"><Wifi120Regular /></span>
          <span className="tool-name">校园网自动登录</span>
        </button>
        {LATER.map((item, i) => (
          <button key={i} type="button" className="tool-tile is-wait" disabled>
            <span className="tool-ico" aria-hidden="true">{item.icon}</span>
            <span className="tool-name">敬请期待</span>
          </button>
        ))}
      </div>
      <div className="tool-panel">
        <Field label="校园网自动登录">
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
    </div>
  );
}
