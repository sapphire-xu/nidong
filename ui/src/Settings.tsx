import { useEffect, useRef, useState, type ReactNode } from "react";
import type { AppSettings, SemesterTable } from "./types";

type Page = "hub" | "appearance" | "dorm" | "schedule" | "account" | "panel" | "remind";

const PAGES: { id: Page; title: string; desc: string; icon: ReactNode }[] = [
  {
    id: "appearance",
    title: "外观",
    desc: "浅色、深色，或跟随系统",
    icon: (
      <svg viewBox="0 0 16 16" aria-hidden="true">
        <path fill="currentColor" d="M8 1.5A6.5 6.5 0 1 0 8 14.5 6.5 6.5 0 0 0 8 1.5zm0 1v11A5.5 5.5 0 0 0 8 2.5z" />
      </svg>
    ),
  },
  {
    id: "dorm",
    title: "宿舍与余额",
    desc: "电费房间码、低余额阈值",
    icon: (
      <svg viewBox="0 0 16 16" aria-hidden="true">
        <path fill="currentColor" d="M2.5 13.5V7.2L8 3.1l5.5 4.1v6.3H9.5v-3.2h-3v3.2H2.5z" />
      </svg>
    ),
  },
  {
    id: "schedule",
    title: "课表",
    desc: "第一周周一、重新同步",
    icon: (
      <svg viewBox="0 0 16 16" aria-hidden="true">
        <path fill="currentColor" d="M4 2.5h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-9a2 2 0 0 1 2-2zm0 3.5v7.5h8V6H4zm1.5-3h1v2h-1v-2zm4 0h1v2h-1v-2z" />
      </svg>
    ),
  },
  {
    id: "account",
    title: "启动与登录",
    desc: "记住密码、自动登录、开机自启",
    icon: (
      <svg viewBox="0 0 16 16" aria-hidden="true">
        <path fill="currentColor" d="M8 2a3 3 0 1 1 0 6 3 3 0 0 1 0-6zm0 7c2.8 0 5.5 1.4 5.5 3.2V14h-11v-1.8C2.5 10.4 5.2 9 8 9z" />
      </svg>
    ),
  },
  {
    id: "panel",
    title: "面板行为",
    desc: "自动隐藏、快捷键、悬停展开",
    icon: (
      <svg viewBox="0 0 16 16" aria-hidden="true">
        <path fill="currentColor" d="M2 3.5h12v2.2H2V3.5zm0 3.6h12v5.4H2V7.1z" />
      </svg>
    ),
  },
  {
    id: "remind",
    title: "上课提醒",
    desc: "上课前弹出置顶提醒",
    icon: (
      <svg viewBox="0 0 16 16" aria-hidden="true">
        <path fill="currentColor" d="M8 1.8a5 5 0 0 1 5 5v2.4l1.2 1.8H1.8L3 9.2V6.8a5 5 0 0 1 5-5zm-1.6 11h3.2a1.6 1.6 0 0 1-3.2 0z" />
      </svg>
    ),
  },
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
      const back = parseYmd(valueRef.current);
      setDraft(back);
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

  return (
    <div className="ymd">
      <input
        type="text"
        inputMode="numeric"
        maxLength={4}
        aria-label="年"
        placeholder="年"
        value={draft.y}
        onChange={(e) => setDraft({ ...draft, y: e.target.value.replace(/\D/g, "").slice(0, 4) })}
      />
      <span>年</span>
      <input
        type="text"
        inputMode="numeric"
        maxLength={2}
        aria-label="月"
        placeholder="月"
        value={draft.m}
        onChange={(e) => setDraft({ ...draft, m: e.target.value.replace(/\D/g, "").slice(0, 2) })}
      />
      <span>月</span>
      <input
        type="text"
        inputMode="numeric"
        maxLength={2}
        aria-label="日"
        placeholder="日"
        value={draft.d}
        onChange={(e) => setDraft({ ...draft, d: e.target.value.replace(/\D/g, "").slice(0, 2) })}
      />
      <span>日</span>
    </div>
  );
}

function DebouncedNumber(props: {
  value: number;
  fallback: number;
  min?: number;
  max?: number;
  integer?: boolean;
  onCommit: (n: number) => void;
}) {
  const { value, fallback, min, max, integer, onCommit } = props;
  const [text, setText] = useState(String(value));
  const textRef = useRef(text);
  const valueRef = useRef(value);
  const onCommitRef = useRef(onCommit);
  textRef.current = text;
  valueRef.current = value;
  onCommitRef.current = onCommit;
  useEffect(() => {
    setText(String(value));
  }, [value]);

  function parse(raw: string): number | null {
    const n = integer ? Math.floor(Number(raw)) : Number(raw);
    if (!Number.isFinite(n)) return null;
    if (raw.trim() === "") return null;
    if (min != null && n < min) return null;
    if (max != null && n > max) return null;
    return n;
  }

  function apply(raw: string) {
    const n = parse(raw);
    if (n == null) {
      setText(String(fallback));
      if (valueRef.current !== fallback) onCommitRef.current(fallback);
      return;
    }
    setText(String(n));
    if (n !== valueRef.current) onCommitRef.current(n);
  }

  useEffect(() => {
    if (text === String(value)) return;
    const t = window.setTimeout(() => apply(text), 3000);
    return () => window.clearTimeout(t);
  }, [text, value]);

  useEffect(() => () => {
    const raw = textRef.current;
    if (raw === String(valueRef.current)) return;
    apply(raw);
  }, []);

  return (
    <input
      type="number"
      min={min}
      max={max}
      step={integer ? 1 : undefined}
      value={text}
      onChange={(e) => setText(e.target.value)}
    />
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
    <button
      type="button"
      className={`hotkey-btn ${capturing ? "capturing" : ""}`}
      onClick={() => setCapturing((v) => !v)}
    >
      {capturing ? "按下组合键…" : (value || "Ctrl+Q").replace(/\+/g, " + ")}
    </button>
  );
}

export function SettingsPanel(props: {
  pin: ReactNode;
  settings: AppSettings;
  setSettings: (s: AppSettings) => void;
  semester?: SemesterTable;
  cacheLabel: string;
  onClose: () => void;
  onSync: () => void;
  onLogout: () => void;
  onPreviewRemind: () => void;
}) {
  const { pin, settings, setSettings, semester, cacheLabel, onClose, onSync, onLogout, onPreviewRemind } = props;
  const [page, setPage] = useState<Page>("hub");
  const title = page === "hub" ? "设置" : PAGES.find((p) => p.id === page)?.title || "设置";
  const back = () => {
    if (page === "hub") onClose();
    else setPage("hub");
  };
  const s = settings;
  const set = (patch: Partial<AppSettings>) => setSettings({ ...s, ...patch });

  return (
    <div className="panel settings-panel">
      <div className="bill-head drag">
        <button className="back" type="button" onClick={back}>‹ {title}</button>
        <div className="week" />
        {pin}
      </div>

      {page === "hub" && (
        <div className="set-hub">
          {PAGES.map((p) => (
            <button key={p.id} type="button" className="set-card" onClick={() => setPage(p.id)}>
              <span className="set-ico">{p.icon}</span>
              <span className="set-txt">
                <strong>{p.title}</strong>
                <small>{p.desc}</small>
              </span>
              <span className="set-chev">›</span>
            </button>
          ))}
          <button type="button" className="set-logout" onClick={onLogout}>
            退出登录
          </button>
        </div>
      )}

      {page === "appearance" && (
        <div className="set-body">
          <p className="set-lead">看板颜色。宿舍关灯用深色，教室投影用浅色。</p>
          <div className="theme-row">
            {(["system", "light", "dark"] as const).map((t) => (
              <button
                key={t}
                type="button"
                className={`chip ${s.theme === t ? "on" : ""}`}
                onClick={() => set({ theme: t })}
              >
                {t === "system" ? "跟随系统" : t === "light" ? "浅色" : "深色"}
              </button>
            ))}
          </div>
        </div>
      )}

      {page === "dorm" && (
        <div className="set-body">
          <h3>电费房间码</h3>
          <input
            type="text"
            autoComplete="off"
            spellCheck={false}
            value={s.roomNo}
            onChange={(e) => set({ roomNo: e.target.value })}
            placeholder="宿舍3位+寝室4位，可留空"
          />
          <p className="hint">改过房间码的话，返回看板会重新拉电费。</p>
          <h3>低余额阈值</h3>
          <label>卡低于（元）</label>
          <DebouncedNumber value={s.cardWarn} fallback={100} min={0} onCommit={(cardWarn) => set({ cardWarn })} />
          <label>网低于（元）</label>
          <DebouncedNumber value={s.netWarn} fallback={10} min={0} onCommit={(netWarn) => set({ netWarn })} />
          <label>电低于（度）</label>
          <DebouncedNumber value={s.elecWarn} fallback={20} min={0} onCommit={(elecWarn) => set({ elecWarn })} />
        </div>
      )}

      {page === "schedule" && (
        <div className="set-body">
          <h3>本学期第一周周一</h3>
          <YmdField value={s.week1Monday || ""} onChange={(week1Monday) => set({ week1Monday })} />
          <p className="hint">
            不填则用教务校历
            {semester?.week1Monday ? `（${semester.week1Monday.replace(/-/g, "·")}）` : ""}
            ，用来算「第几周」。
          </p>
          <div className="cache-line">
            <span>{cacheLabel ? `课表 · ${cacheLabel}已缓存` : "课表 · 尚未同步"}</span>
            <button className="link" type="button" onClick={onSync}>重新同步</button>
          </div>
        </div>
      )}

      {page === "account" && (
        <div className="set-body">
          <div
            className="toggle"
            onClick={() => {
              const next = !(s.rememberPassword !== false);
              set({ rememberPassword: next, autoLogin: next && !!s.autoLogin });
            }}
          >
            <span>记住密码</span>
            <div className={`sw ${s.rememberPassword !== false ? "on" : ""}`}><i /></div>
          </div>
          <div
            className={`toggle ${s.rememberPassword === false ? "off" : ""}`}
            onClick={() => {
              if (s.rememberPassword === false) return;
              set({ autoLogin: !s.autoLogin });
            }}
          >
            <span>启动时自动登录</span>
            <div className={`sw ${s.autoLogin !== false && s.rememberPassword !== false ? "on" : ""}`}><i /></div>
          </div>
          <div className="toggle" onClick={() => set({ autostart: !s.autostart })}>
            <span>开机自启</span>
            <div className={`sw ${s.autostart ? "on" : ""}`}><i /></div>
          </div>
          <p className="hint">密码只进这台电脑的系统凭据。关掉「记住密码」会从系统凭据里删掉已存的密码。退出登录不会清。</p>
        </div>
      )}

      {page === "panel" && (
        <div className="set-body">
          <p className="set-lead">折叠后可以再藏进屏幕上沿。上课提醒仍会弹出，不受隐藏影响。独占全屏游戏时会自动藏起；游戏开无边框窗口化，岛才能继续置顶。</p>
          <div className="toggle" onClick={() => set({ autoHide: !s.autoHide })}>
            <span>自动隐藏</span>
            <div className={`sw ${s.autoHide ? "on" : ""}`}><i /></div>
          </div>
          {s.autoHide ? (
            <>
              <p className="hint">变成折叠岛后再过 3 秒，向上收起。</p>
              <div className="toggle" onClick={() => set({ autoHideHotkey: !s.autoHideHotkey })}>
                <span>快捷键唤出</span>
                <div className={`sw ${s.autoHideHotkey ? "on" : ""}`}><i /></div>
              </div>
              {s.autoHideHotkey ? (
                <>
                  <div className="hotkey-row">
                    <span>组合键</span>
                    <HotkeyField
                      value={s.revealHotkey || "Ctrl+Q"}
                      onChange={(revealHotkey) => set({ revealHotkey })}
                    />
                  </div>
                  <p className="hint">两键要在约 0.4 秒内一起按下。不改变钉住；松开后若鼠标不在岛上会收起。</p>
                </>
              ) : null}
              <div className="toggle" onClick={() => set({ autoHideHover: !s.autoHideHover })}>
                <span>悬停展开</span>
                <div className={`sw ${s.autoHideHover ? "on" : ""}`}><i /></div>
              </div>
              {s.autoHideHover ? (
                <p className="hint">鼠标移到原来岛的位置附近，停超过 1 秒就会展开。</p>
              ) : (
                <p className="hint">关掉后，用快捷键或托盘「展开面板」唤出。</p>
              )}
            </>
          ) : (
            <p className="hint">打开后，折叠岛会在无人使用时收进屏幕边缘。</p>
          )}
        </div>
      )}

      {page === "remind" && (
        <div className="set-body">
          <p className="set-lead">到点弹出折叠大小的提醒：加粗课名、底栏倒计时，置顶 10 秒后收回。</p>
          <div className="toggle" onClick={() => set({ classRemind: !s.classRemind })}>
            <span>上课提醒</span>
            <div className={`sw ${s.classRemind ? "on" : ""}`}><i /></div>
          </div>
          {s.classRemind ? (
            <>
              <label>上课前多少分钟提醒</label>
              <DebouncedNumber
                value={s.classRemindMinutes ?? 15}
                fallback={15}
                min={1}
                max={180}
                integer
                onCommit={(classRemindMinutes) => set({ classRemindMinutes })}
              />
              <p className="hint">只填整数。默认 15 分钟。</p>
              <button type="button" className="ghost" onClick={onPreviewRemind}>
                预览动效
              </button>
            </>
          ) : null}
        </div>
      )}
    </div>
  );
}
