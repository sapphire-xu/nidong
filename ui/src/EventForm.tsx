import { useState } from "react";
import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Field,
  Input,
  SpinButton,
  Switch,
  Textarea,
} from "@fluentui/react-components";
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

function DateTimeRow(props: { label: string; value: Parts; onChange: (v: Parts) => void }) {
  const p = props.value;
  function set(patch: Partial<Parts>) {
    props.onChange({ ...p, ...patch });
  }
  function cell(part: keyof Parts, label: string, max: number, wide = false) {
    return (
      <Input
        className={wide ? "y" : undefined}
        aria-label={`${props.label}${label}`}
        placeholder={label}
        inputMode="numeric"
        maxLength={max}
        value={p[part]}
        onChange={(_, data) => set({ [part]: data.value.replace(/\D/g, "").slice(0, max) })}
      />
    );
  }
  return (
    <Field label={props.label}>
      <div className="dt-row">
        {cell("y", "年", 4, true)}
        <span>年</span>
        {cell("m", "月", 2)}
        <span>月</span>
        {cell("d", "日", 2)}
        <span>日</span>
        {cell("hh", "时", 2)}
        <span>:</span>
        {cell("mm", "分", 2)}
      </div>
    </Field>
  );
}

export function EventForm(props: {
  open: boolean;
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
  const [remindMinutes, setRemindMinutes] = useState(init.remindMinutes ?? 15);
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
    if (remind && (!Number.isFinite(remindMinutes) || remindMinutes < 1 || remindMinutes > 180)) {
      setLocalErr("提醒时间须在 1–180 分钟");
      return;
    }
    setLocalErr("");
    props.onSave({
      id: init.id,
      name: n,
      note: note.trim() || undefined,
      start: startStamp,
      end: endStamp,
      remind,
      remindMinutes: remind ? remindMinutes : undefined,
    });
  }

  return (
    <Dialog open={props.open} onOpenChange={(_, data) => { if (!data.open) props.onClose(); }}>
      <DialogSurface>
        <DialogBody>
          <DialogTitle>{editing ? "编辑日程" : "添加日程"}</DialogTitle>
          <DialogContent className="stack">
            <Field label="名称">
              <Input maxLength={40} placeholder="例如 班会、实验" value={name} onChange={(_, d) => setName(d.value)} />
            </Field>
            <DateTimeRow label="开始" value={start} onChange={setStart} />
            <DateTimeRow label="结束" value={end} onChange={setEnd} />
            <Switch label="提醒" checked={remind} onChange={(_, d) => setRemind(d.checked)} />
            {remind ? (
              <Field label="提前多少分钟">
                <SpinButton
                  min={1}
                  max={180}
                  step={1}
                  value={remindMinutes}
                  onChange={(_, data) => {
                    const n = data.value ?? Math.floor(Number(data.displayValue));
                    if (Number.isFinite(n)) setRemindMinutes(n);
                  }}
                />
              </Field>
            ) : null}
            {remind ? <p className="hint">到点弹出岛，约 10 秒。须为 1–180 的整数。</p> : null}
            <Field label="备注">
              <Textarea rows={3} maxLength={200} placeholder="选填" value={note} onChange={(_, d) => setNote(d.value)} />
            </Field>
            {localErr || props.err ? <div className="hint warn">{localErr || props.err}</div> : null}
          </DialogContent>
          <DialogActions>
            {editing && props.onDelete ? (
              <Button appearance="secondary" disabled={props.busy} onClick={() => props.onDelete!(init.id)}>删除</Button>
            ) : null}
            <Button appearance="secondary" onClick={props.onClose}>取消</Button>
            <Button appearance="primary" disabled={props.busy} onClick={save}>{props.busy ? "保存中…" : "保存"}</Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
