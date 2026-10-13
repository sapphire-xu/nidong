import { useEffect, useRef, useState } from "react";
import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Divider,
  Field,
  Input,
  Radio,
  RadioGroup,
  Slider,
  SpinButton,
  Switch,
} from "@fluentui/react-components";
import {
  Alert20Regular,
  CalendarLtr20Regular,
  Checkmark20Regular,
  Color20Regular,
  Home20Regular,
  Person20Regular,
  Window20Regular,
} from "@fluentui/react-icons";
import type { AppSettings, SemesterTable } from "./types";

type Page = "hub" | "appearance" | "dorm" | "schedule" | "account" | "panel" | "remind";

const PAGES: { id: Page; title: string; desc: string; icon: JSX.Element }[] = [
  { id: "appearance", title: "外观", desc: "浅色、深色，或跟随系统", icon: <Color20Regular /> },
  { id: "dorm", title: "宿舍与余额", desc: "电费房间码、低余额阈值", icon: <Home20Regular /> },
  { id: "schedule", title: "课表", desc: "第一周周一和课表", icon: <CalendarLtr20Regular /> },
  { id: "account", title: "启动与登录", desc: "记住密码、自动登录、开机自启", icon: <Person20Regular /> },
  { id: "panel", title: "面板行为", desc: "自动隐藏、快捷键、悬停唤出", icon: <Window20Regular /> },
  { id: "remind", title: "上课提醒", desc: "上课前弹出置顶提醒，可开铃声", icon: <Alert20Regular /> },
];

function pad(n: number) {
  return String(n).padStart(2, "0");
}

function parseYmd(value: string) {
  const p = (value || "").slice(0, 10).split("-");
  return { y: p[0] || "", m: p[1] || "", d: p[2] || "" };
}

function formatYmd(v: { y: string; m: string; d: string }) {
  if (!v.y && !v.m && !v.d) return "";
  const y = Number(v.y);
  const m = Number(v.m);
  const d = Number(v.d);
  if (v.y.length === 4 && y >= 1970 && m >= 1 && m <= 12 && d >= 1 && d <= 31) {
    return `${v.y}-${pad(m)}-${pad(d)}`;
  }
  return null;
}

function YmdField({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [draft, setDraft] = useState(() => parseYmd(value));
  const draftRef = useRef(draft);
  const valueRef = useRef(value);
  const onChangeRef = useRef(onChange);
  draftRef.current = draft;
  valueRef.current = value;
  onChangeRef.current = onChange;
  useEffect(() => {
    setDraft(parseYmd(value));
  }, [value]);

  function apply(next: { y: string; m: string; d: string }) {
    const out = formatYmd(next);
    if (out === null) {
      setDraft(parseYmd(valueRef.current));
      return;
    }
    setDraft(out ? parseYmd(out) : { y: "", m: "", d: "" });
    if (out !== valueRef.current) onChangeRef.current(out);
  }

  useEffect(() => {
    const saved = parseYmd(value);
    if (draft.y === saved.y && draft.m === saved.m && draft.d === saved.d) return;
    const t = window.setTimeout(() => apply(draft), 3000);
    return () => window.clearTimeout(t);
  }, [draft, value]);

  useEffect(() => () => {
    const cur = draftRef.current;
    const saved = parseYmd(valueRef.current);
    if (cur.y === saved.y && cur.m === saved.m && cur.d === saved.d) return;
    apply(cur);
  }, []);

  function cell(part: "y" | "m" | "d", label: string, max: number) {
    return (
      <Input
        aria-label={label}
        placeholder={label}
        inputMode="numeric"
        maxLength={max}
        value={draft[part]}
        onChange={(_, data) => setDraft({ ...draft, [part]: data.value.replace(/\D/g, "").slice(0, max) })}
      />
    );
  }

  return (
    <div className="ymd">
      {cell("y", "年", 4)}
      <span>年</span>
      {cell("m", "月", 2)}
      <span>月</span>
      {cell("d", "日", 2)}
      <span>日</span>
    </div>
  );
}

function HotkeyField({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [capturing, setCapturing] = useState(false);
  useEffect(() => {
    if (!capturing) return;
    const held = new Set<string>();
    const label = (e: KeyboardEvent) => {
      if (e.key === "Control") return "Ctrl";
      if (e.key === "Alt") return "Alt";
      if (e.key === "Shift") return "Shift";
      if (e.key === "Meta") return "Win";
      if (e.key === "Tab") return "Tab";
      if (e.key === " ") return "Space";
      if (e.key === "Escape") return "Esc";
      if (e.key.length === 1) return e.key.toUpperCase();
      if (/^F\d{1,2}$/i.test(e.key)) return e.key.toUpperCase();
      return e.key;
    };
    const down = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape" && held.size === 0) {
        setCapturing(false);
        return;
      }
      held.add(label(e));
      const parts = ["Ctrl", "Alt", "Shift", "Win", "Tab"].filter((k) => held.has(k));
      for (const k of held) {
        if (!parts.includes(k)) parts.push(k);
      }
      const main = parts.filter((k) => !["Ctrl", "Alt", "Shift", "Win", "Tab"].includes(k));
      if (parts.length >= 2 && main.length >= 1) {
        onChange(parts.join("+"));
        setCapturing(false);
      }
    };
    const up = (e: KeyboardEvent) => {
      held.delete(label(e));
    };
    window.addEventListener("keydown", down, true);
    window.addEventListener("keyup", up, true);
    return () => {
      window.removeEventListener("keydown", down, true);
      window.removeEventListener("keyup", up, true);
    };
  }, [capturing, onChange]);

  return (
    <Button appearance={capturing ? "primary" : "secondary"} onClick={() => setCapturing((v) => !v)}>
      {capturing ? "按下组合键…" : (value || "Ctrl+Q").replace(/\+/g, " + ")}
    </Button>
  );
}

function Threshold({
  label,
  value,
  max,
  onCommit,
}: {
  label: string;
  value: number;
  max: number;
  onCommit: (n: number) => void;
}) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);

  function commit(raw: string) {
    const n = Math.min(max, Math.max(0, Math.round(Number(raw))));
    if (Number.isFinite(n)) {
      setDraft(String(n));
      onCommit(n);
    } else {
      setDraft(String(value));
    }
  }

  return (
    <div className="threshold">
      <Field label={`${label}：${value}`}>
        <div className="threshold-control">
          <Slider min={0} max={max} step={1} value={Math.min(value, max)} onChange={(_, data) => {
            setDraft(String(data.value));
            onCommit(data.value);
          }} />
          <Input
            className="threshold-input"
            type="number"
            min={0}
            max={max}
            step={1}
            value={draft}
            aria-label={label}
            onChange={(_, data) => setDraft(data.value)}
            onBlur={() => commit(draft)}
            onKeyDown={(e) => { if (e.key === "Enter") commit(draft); }}
          />
        </div>
      </Field>
    </div>
  );
}

export function SettingsPanel(props: {
  settings: AppSettings;
  setSettings: (s: AppSettings) => void;
  semester?: SemesterTable;
  cacheLabel: string;
  courseSync: "idle" | "spin" | "ok" | "fail";
  courseSyncErr?: string;
  offline?: boolean;
  onSync: () => void;
  onLogout: (keepAgenda: boolean) => void;
  onPreviewRemind: () => void;
  loggedIn: boolean;
  onRequestLogin: () => void;
}) {
  const { settings, setSettings, semester, cacheLabel, courseSync, courseSyncErr, offline, onSync, onLogout, onPreviewRemind, loggedIn, onRequestLogin } = props;
  const [page, setPage] = useState<Page>("hub");
  const [leaveOpen, setLeaveOpen] = useState(false);
  const [motion, setMotion] = useState<"idle" | "back">("idle");
  const openPage = (next: Page) => {
    setMotion(next === "hub" && page !== "hub" ? "back" : "idle");
    setPage(next);
  };
  const title = page === "hub" ? "设置" : PAGES.find((p) => p.id === page)?.title || "设置";
  const s = settings;
  const set = (patch: Partial<AppSettings>) => setSettings({ ...s, ...patch });

  return (
    <div className={`stack settings-panel ${page === "hub" ? "settings-hub-page" : "settings-sub-page"}`}>
      <div className="row-between">
        {page === "hub" ? (
          <h1 className="page-title">设置</h1>
        ) : (
          <div className="settings-breadcrumb">
            <button type="button" className="settings-breadcrumb-link" onClick={() => openPage("hub")}>设置</button>
            <span aria-hidden="true">›</span>
            <h1 className="page-title">{title}</h1>
          </div>
        )}
        {page !== "hub" ? (
          <span />
        ) : null}
      </div>

      {page === "hub" && (
        <div className={`set-list${motion === "back" ? " settings-hub-content" : ""}`}>
          {PAGES.map((p) => (
            <button key={p.id} type="button" className="set-item" onClick={() => openPage(p.id)}>
              <span className="set-icon" aria-hidden="true">{p.icon}</span>
              <strong>{p.title}</strong>
              <span className="chev" aria-hidden="true">›</span>
              <small>{p.desc}</small>
            </button>
          ))}
          <Divider />
          {loggedIn ? (
            <>
              <Button className="logout" appearance="secondary" onClick={() => setLeaveOpen(true)}>退出登录</Button>
              <Dialog open={leaveOpen} onOpenChange={(_, data) => { if (!data.open) setLeaveOpen(false); }}>
                <DialogSurface>
                  <DialogBody>
                    <DialogTitle>退出登录</DialogTitle>
                    <DialogContent>本机上的日程可以留下，也可以一起删掉。</DialogContent>
                    <DialogActions>
                      <Button appearance="secondary" onClick={() => setLeaveOpen(false)}>取消</Button>
                      <Button appearance="primary" onClick={() => { setLeaveOpen(false); onLogout(true); }}>保留日程</Button>
                      <Button appearance="secondary" onClick={() => { setLeaveOpen(false); onLogout(false); }}>删除日程</Button>
                    </DialogActions>
                  </DialogBody>
                </DialogSurface>
              </Dialog>
            </>
          ) : (
            <Button className="logout" appearance="secondary" onClick={onRequestLogin}>去登录</Button>
          )}
        </div>
      )}

      {page === "appearance" && (
        <div key={page} className="stack settings-sub-content">
          <p className="hint">看板颜色。宿舍关灯用深色，教室投影用浅色。</p>
          <RadioGroup value={s.theme === "light" || s.theme === "dark" ? s.theme : "system"} onChange={(_, d) => set({ theme: d.value })}>
            <Radio value="system" label="跟随系统" />
            <Radio value="light" label="浅色" />
            <Radio value="dark" label="深色" />
          </RadioGroup>
        </div>
      )}

      {page === "dorm" && (
        <div key={page} className="stack settings-sub-content">
          <Field label="电费房间码">
            <Input
              autoComplete="off"
              spellCheck={false}
              value={s.roomNo}
              placeholder="宿舍3位+寝室4位，可留空"
              onChange={(_, d) => set({ roomNo: d.value })}
            />
          </Field>
          <p className="hint">改过房间码的话，离开设置会重新拉电费。</p>
          <Threshold label="卡低于（元）" value={s.cardWarn} max={500} onCommit={(cardWarn) => set({ cardWarn })} />
          <Threshold label="网低于（元）" value={s.netWarn} max={200} onCommit={(netWarn) => set({ netWarn })} />
          <Threshold label="电低于（度）" value={s.elecWarn} max={300} onCommit={(elecWarn) => set({ elecWarn })} />
        </div>
      )}

      {page === "schedule" && (
        <div key={page} className="stack settings-sub-content">
          <Field label="本学期第一周周一">
            <YmdField value={s.week1Monday || ""} onChange={(week1Monday) => set({ week1Monday })} />
          </Field>
          <p className="hint">
            不填则用教务校历
            {semester?.week1Monday ? `（${semester.week1Monday.replace(/-/g, "·")}）` : ""}
            ，用来算「第几周」。
          </p>
          <div className={courseSync === "spin" ? "sync-row syncing" : "sync-row"}>
            <div className="row-between">
              <span>{cacheLabel ? `课表 · ${cacheLabel}已缓存` : "课表 · 尚未同步"}</span>
              {courseSync === "spin" ? (
                <span className="sync-status spin"><i className="status-spin" aria-hidden="true" />同步中</span>
              ) : courseSync === "ok" ? (
                <span className="sync-status ok"><Checkmark20Regular />已同步</span>
              ) : (
                <Button appearance="subtle" disabled={offline || !loggedIn} onClick={onSync}>
                  {!loggedIn ? "未登录" : offline ? "离线" : courseSync === "fail" ? "重试同步" : "重新同步"}
                </Button>
              )}
            </div>
          </div>
          {courseSync === "fail" && courseSyncErr ? <p className="hint warn">{courseSyncErr}</p> : null}
          {!loggedIn ? <p className="hint">登录后才能同步课表。</p> : null}
          {offline ? <p className="hint">离线时不能打教务，连上网络后再同步。</p> : null}
        </div>
      )}

      {page === "account" && (
        <div key={page} className="stack settings-sub-content">
          <Switch
            label="记住密码"
            checked={s.rememberPassword !== false}
            onChange={(_, d) => set({ rememberPassword: d.checked, autoLogin: d.checked && !!s.autoLogin })}
          />
          <Switch
            label="启动时自动登录"
            disabled={s.rememberPassword === false}
            checked={s.autoLogin !== false && s.rememberPassword !== false}
            onChange={(_, d) => set({ autoLogin: d.checked })}
          />
          <Switch label="开机自启" checked={!!s.autostart} onChange={(_, d) => set({ autostart: d.checked })} />
          <p className="hint">密码只进这台电脑的系统凭据。关掉「记住密码」会从系统凭据里删掉已存的密码。退出登录不会清。</p>
        </div>
      )}

      {page === "panel" && (
        <div key={page} className="stack settings-sub-content">
          <p className="hint">折叠后可以再藏进屏幕上沿。上课提醒仍会弹出，不受隐藏影响。独占全屏游戏时会自动藏起；游戏开无边框窗口化，岛才能继续置顶。</p>
          <Switch label="自动隐藏" checked={!!s.autoHide} onChange={(_, d) => set({ autoHide: d.checked })} />
          {s.autoHide ? (
            <>
              <p className="hint">岛露出来后再过 3 秒，向上收起。</p>
              <Switch label="快捷键唤出" checked={!!s.autoHideHotkey} onChange={(_, d) => set({ autoHideHotkey: d.checked })} />
              {s.autoHideHotkey ? (
                <>
                  <div className="row-between">
                    <span>组合键</span>
                    <HotkeyField value={s.revealHotkey || "Ctrl+Q"} onChange={(revealHotkey) => set({ revealHotkey })} />
                  </div>
                  <p className="hint">两键要在约 0.4 秒内一起按下。只把岛叫出来，松开不收起，也不打开窗口。</p>
                </>
              ) : null}
              <Switch label="悬停唤出" checked={!!s.autoHideHover} onChange={(_, d) => set({ autoHideHover: d.checked })} />
              {s.autoHideHover ? (
                <p className="hint">鼠标移到岛原来的位置附近，停超过 1 秒，只把岛叫出来，不打开窗口。</p>
              ) : (
                <p className="hint">关掉后，用快捷键把岛叫出来，或用托盘「打开窗口」。</p>
              )}
            </>
          ) : (
            <p className="hint">打开后，岛会在无人使用时收进屏幕上沿。</p>
          )}
        </div>
      )}

      {page === "remind" && (
        <div key={page} className="stack settings-sub-content">
          <p className="hint">到点在岛上弹出 40px 的提醒：加粗课名、倒计时，置顶约 10 秒后收回。点这下岛会打开窗口。</p>
          <Switch label="上课提醒" checked={!!s.classRemind} onChange={(_, d) => set({ classRemind: d.checked })} />
          {s.classRemind ? (
            <>
              <Field label="上课前多少分钟提醒">
                <SpinButton
                  min={1}
                  max={180}
                  step={1}
                  value={s.classRemindMinutes ?? 15}
                  onChange={(_, data) => {
                    const n = data.value ?? Math.floor(Number(data.displayValue));
                    if (Number.isFinite(n) && n >= 1 && n <= 180) set({ classRemindMinutes: n });
                  }}
                />
              </Field>
              <p className="hint">只填整数。默认 15 分钟。</p>
              <Button appearance="secondary" onClick={onPreviewRemind}>预览动效</Button>
            </>
          ) : null}
          <Switch label="提醒铃声" checked={!!s.remindSound} onChange={(_, d) => set({ remindSound: d.checked })} />
          <Field label={`音量：${s.remindVolume ?? 100}`}>
            <Slider
              min={0}
              max={100}
              step={1}
              value={s.remindVolume ?? 100}
              onChange={(_, data) => set({ remindVolume: data.value })}
            />
          </Field>
          <p className="hint">打开后，上课和日程提醒会响一声 Argon。音量只作用于这声提醒。默认关着。铃声开着时，预览动效也会按这个音量响。</p>
        </div>
      )}
    </div>
  );
}
