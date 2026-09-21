import { useState, type ReactNode } from "react";
import type { Agenda } from "./types";

function pad(n: number) {
  return String(n).padStart(2, "0");
}

type Parts = { y: string; m: string; d: string; hh: string; mm: string };

function splitStamp(s: string): Parts {
  const t = (s || "").replace(" ", "T");
  const p = t.slice(0, 10).split("-");
  return {
    y: p[0] || "",
    m: p[1] ? String(Number(p[1]) || p[1]) : "",
    d: p[2] ? String(Number(p[2]) || p[2]) : "",
    hh: t.slice(11, 13) ? String(Number(t.slice(11, 13))) : "",
    mm: t.slice(14, 16) ? String(Number(t.slice(14, 16))) : "",
  };
}

function joinStamp(p: Parts) {
  const y = Number(p.y);
  const m = Number(p.m);
  const d = Number(p.d);
  const hh = Number(p.hh);
  const mm = Number(p.mm);
  if (
    p.y.length !== 4 ||
    !y ||
    p.m.trim() === "" ||
    p.d.trim() === "" ||
    p.hh.trim() === "" ||
    p.mm.trim() === "" ||
    !Number.isFinite(m) ||
    m < 1 ||
    m > 12 ||
    !Number.isFinite(d) ||
    d < 1 ||
    d > 31 ||
    !Number.isFinite(hh) ||
    hh < 0 ||
    hh > 23 ||
    !Number.isFinite(mm) ||
    mm < 0 ||
    mm > 59
  ) {
    return "";
  }
  return `${p.y}-${pad(m)}-${pad(d)}T${pad(hh)}:${pad(mm)}`;
}

function nextHalfHour(now: Date) {
  const d = new Date(now);
  d.setSeconds(0, 0);
  const add = d.getMinutes() < 30 ? 30 - d.getMinutes() : 60 - d.getMinutes();
  d.setMinutes(d.getMinutes() + add);
  return d;
}

function stampOf(d: Date) {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function defaultStamp(now: Date) {
  const start = nextHalfHour(now);
  const end = new Date(start.getTime() + 60 * 60 * 1000);
  return { start: stampOf(start), end: stampOf(end) };
}

function DateTimeRow(props: {
  label: string;
  value: Parts;
  onChange: (v: Parts) => void;
}) {
  const p = props.value;
  function set(patch: Partial<Parts>) {
    props.onChange({ ...p, ...patch });
  }

  return (
    <>
      <label>{props.label}</label>
      <div className="dt-row">
        <input
          type="text"
          inputMode="numeric"
          maxLength={4}
          aria-label={`${props.label}年`}
          placeholder="年"
          value={p.y}
          onChange={(e) => set({ y: e.target.value.replace(/\D/g, "").slice(0, 4) })}
        />
        <span>年</span>
        <input
          type="text"
          inputMode="numeric"
          maxLength={2}
          aria-label={`${props.label}月`}
          placeholder="月"
          value={p.m}
          onChange={(e) => set({ m: e.target.value.replace(/\D/g, "").slice(0, 2) })}
        />
        <span>月</span>
        <input
          type="text"
          inputMode="numeric"
          maxLength={2}
          aria-label={`${props.label}日`}
          placeholder="日"
          value={p.d}
          onChange={(e) => set({ d: e.target.value.replace(/\D/g, "").slice(0, 2) })}
        />
        <span>日</span>
        <input
          className="dt-hm"
          type="text"
          inputMode="numeric"
          maxLength={2}
          aria-label={`${props.label}时`}
          placeholder="时"
          value={p.hh}
          onChange={(e) => set({ hh: e.target.value.replace(/\D/g, "").slice(0, 2) })}
        />
        <span>:</span>
        <input
          className="dt-hm"
          type="text"
          inputMode="numeric"
          maxLength={2}
          aria-label={`${props.label}分`}
          placeholder="分"
          value={p.mm}
          onChange={(e) => set({ mm: e.target.value.replace(/\D/g, "").slice(0, 2) })}
        />
      </div>
    </>
  );
}

export function EventForm(props: {
  pin: ReactNode;
  initial: Agenda | null;
  now: Date;
  busy?: boolean;
  err?: string;
  onClose: () => void;
  onSave: (item: Agenda) => void;
  onDelete?: (id: string) => void;
}) {
  const editing = !!props.initial?.id;
  const init = props.initial || {
    id: "",
    name: "",
    note: "",
    start: defaultStamp(props.now).start,
    end: defaultStamp(props.now).end,
    remind: false,
    remindMinutes: 15,
  };
  const [name, setName] = useState(init.name);
  const [note, setNote] = useState(init.note || "");
  const [start, setStart] = useState(() => splitStamp(init.start));
  const [end, setEnd] = useState(() => splitStamp(init.end));
  const [remind, setRemind] = useState(!!init.remind);
  const [remindText, setRemindText] = useState(String(init.remindMinutes ?? 15));
  const [localErr, setLocalErr] = useState("");

  function save() {
    const n = name.trim();
    if (!n) {
      setLocalErr("请填写日程名称");
      return;
    }
    const startStamp = joinStamp(start);
    if (!startStamp) {
      setLocalErr("请设置开始时间");
      return;
    }
    const endStamp = joinStamp(end);
    if (!endStamp) {
      setLocalErr("请设置结束时间");
      return;
    }
    if (endStamp <= startStamp) {
      setLocalErr("结束时间必须晚于开始时间");
      return;
    }
    let remindMinutes: number | undefined;
    if (remind) {
      const m = Math.floor(Number(remindText));
      if (!Number.isFinite(m) || m < 1 || m > 180) {
        setLocalErr("提醒时间须在 1–180 分钟");
        return;
      }
      remindMinutes = m;
    }
    setLocalErr("");
    props.onSave({
      id: init.id,
      name: n,
      note: note.trim() || undefined,
      start: startStamp,
      end: endStamp,
      remind,
      remindMinutes,
    });
  }

  return (
    <div className="panel event-form">
      <div className="bill-head drag">
        <button className="back" type="button" onClick={props.onClose}>
          ‹ {editing ? "编辑日程" : "添加日程"}
        </button>
        <div className="week" />
        {props.pin}
      </div>
      <label>名称</label>
      <input
        type="text"
        maxLength={40}
        placeholder="例如 班会、实验"
        value={name}
        onChange={(e) => setName(e.target.value)}
      />
      <DateTimeRow label="开始" value={start} onChange={setStart} />
      <DateTimeRow label="结束" value={end} onChange={setEnd} />
      <div className="toggle" onClick={() => setRemind((v) => !v)}>
        <span>提醒</span>
        <div className={`sw ${remind ? "on" : ""}`}><i /></div>
      </div>
      {remind ? (
        <>
          <label>提前多少分钟</label>
          <input
            type="text"
            inputMode="numeric"
            value={remindText}
            onChange={(e) => setRemindText(e.target.value.replace(/\D/g, "").slice(0, 3))}
          />
          <p className="hint">到点弹出折叠岛，约 10 秒。保存时检查须为 1–180 的整数。</p>
        </>
      ) : null}
      <label>备注</label>
      <textarea
        rows={3}
        maxLength={200}
        placeholder="选填"
        value={note}
        onChange={(e) => setNote(e.target.value)}
      />
      <button className="primary" type="button" disabled={props.busy} onClick={save}>
        {props.busy ? "保存中…" : "保存"}
      </button>
      {editing && props.onDelete ? (
        <button className="ghost danger" type="button" disabled={props.busy} onClick={() => props.onDelete!(init.id)}>
          删除
        </button>
      ) : null}
      {localErr || props.err ? <div className="err-inline">{localErr || props.err}</div> : null}
    </div>
  );
}
