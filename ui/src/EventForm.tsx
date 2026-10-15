import { useState } from "react";
import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Dropdown,
  Field,
  Input,
  Option,
  Switch,
  Textarea,
} from "@fluentui/react-components";
import { repeatSpansDays } from "./agenda";
import { REMIND_CODES } from "./classify";
import type { Agenda, RepetFreq, RepetRule } from "./types";

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

const WEEKDAYS = [
  { value: "Mo", label: "周一" },
  { value: "Tu", label: "周二" },
  { value: "We", label: "周三" },
  { value: "Th", label: "周四" },
  { value: "Fr", label: "周五" },
  { value: "Sa", label: "周六" },
  { value: "Su", label: "周日" },
];

const FREQS: { value: RepetFreq; label: string }[] = [
  { value: "DAILY", label: "每天" },
  { value: "WEEKLY", label: "按星期" },
];

function joinDate(p: Parts) {
  const y = Number(p.y);
  const m = Number(p.m);
  const d = Number(p.d);
  if (p.y.length !== 4 || !y || p.m.trim() === "" || p.d.trim() === "" || m < 1 || m > 12 || d < 1 || d > 31) return "";
  return `${p.y}-${pad(m)}-${pad(d)}`;
}

function weekdayCode(start: Parts): string {
  const stamp = joinStamp({ ...start, hh: start.hh.trim() === "" ? "0" : start.hh, mm: start.mm.trim() === "" ? "0" : start.mm });
  if (!stamp) return "Mo";
  const d = new Date(`${stamp}:00`);
  if (Number.isNaN(d.getTime())) return "Mo";
  return ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"][d.getDay()];
}

function DateTimeRow(props: { label: string; value: Parts; onChange: (v: Parts) => void; dateOnly?: boolean }) {
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
        {props.dateOnly ? null : (
          <>
            {cell("hh", "时", 2)}
            <span>:</span>
            {cell("mm", "分", 2)}
          </>
        )}
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
    remindCode: "15",
  };
  const [name, setName] = useState(init.name);
  const [location, setLocation] = useState(init.location || "");
  const [note, setNote] = useState(init.note || "");
  const [start, setStart] = useState(() => splitStamp(init.start));
  const [end, setEnd] = useState(() => splitStamp(init.end));
  const [remind, setRemind] = useState(!!init.remind);
  const [remindCode, setRemindCode] = useState(init.remindCode || "15");
  const [setHour, setSetHour] = useState(() => (init.setTime || "18:00").slice(0, 2).replace(/^0/, "") || "18");
  const [setMinute, setSetMinute] = useState(() => (init.setTime || "18:00").slice(3, 5) || "00");
  const [repet, setRepet] = useState(!!init.isRepet);
  const [freq, setFreq] = useState<RepetFreq>(init.repetRule?.freq === "WEEKLY" ? "WEEKLY" : "DAILY");
  const [byday, setByday] = useState<string[]>(init.repetRule?.byday || []);
  const [endmode, setEndmode] = useState<"DATE" | "NUMBER">(init.isRepet && init.repetRule?.endmode === "DATE" ? "DATE" : "NUMBER");
  const [endOn, setEndOn] = useState(() => splitStamp(init.isRepet && init.repetRule?.time ? `${init.repetRule.time}T00:00` : ""));
  const [repetNum, setRepetNum] = useState(String(init.isRepet ? init.repetRule?.num || 10 : 10));
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
    if (repet && repeatSpansDays(startStamp, endStamp)) {
      setLocalErr("跨天日程不能重复");
      return;
    }
    let rule: RepetRule | undefined;
    if (repet) {
      const num = Math.floor(Number(repetNum));
      rule = { freq, endmode, byday: [] };
      if (freq === "WEEKLY") {
        const days = WEEKDAYS.map((d) => d.value).filter((d) => byday.includes(d));
        if (!days.length) {
          setLocalErr("请选择重复的星期");
          return;
        }
        rule.byday = days;
      }
      if (endmode === "DATE") {
        const day = joinDate(endOn);
        if (!day) {
          setLocalErr("请设置重复何时结束");
          return;
        }
        if (day < startStamp.slice(0, 10)) {
          setLocalErr("结束日期不能早于开始日期");
          return;
        }
        rule.time = day;
        rule.num = 0;
      } else if (!Number.isFinite(num) || num < 1 || num > 999) {
        setLocalErr("重复次数须在 1–999");
        return;
      } else {
        rule.num = num;
      }
    }
    const code = remind ? remindCode : undefined;
    let setTime: string | undefined;
    if (remind && code === "7") {
      const hh = Number(setHour);
      const mm = Number(setMinute);
      if (!Number.isFinite(hh) || hh < 0 || hh > 23 || setHour.trim() === "" || !Number.isFinite(mm) || mm < 0 || mm > 59 || setMinute.trim() === "") {
        setLocalErr("请填写前一天的提醒时间");
        return;
      }
      setTime = `${pad(hh)}:${pad(mm)}`;
    }
    setLocalErr("");
    props.onSave({
      id: init.id,
      name: n,
      note: note.trim() || undefined,
      location: location.trim() || undefined,
      start: startStamp,
      end: endStamp,
      remind,
      remindCode: remind ? code : undefined,
      setTime: remind ? setTime : undefined,
      isRepet: repet || undefined,
      repetRule: rule,
    });
  }

  function pickFreq(next: RepetFreq) {
    setFreq(next);
    if (next === "WEEKLY" && byday.length === 0) setByday([weekdayCode(start)]);
  }

  const startDay = joinDate(start);
  const endDay = joinDate(end);
  const crossRepeat = repet && repeatSpansDays(startDay ? `${startDay}T00:00` : "", endDay ? `${endDay}T00:00` : "");

  return (
    <Dialog open={props.open} onOpenChange={(_, data) => { if (!data.open) props.onClose(); }}>
      <DialogSurface>
        <DialogBody>
          <DialogTitle>{editing ? "编辑日程" : "添加日程"}</DialogTitle>
          <DialogContent className="stack event-form">
            <Field label="名称">
              <Input maxLength={40} placeholder="例如 班会、实验" value={name} onChange={(_, d) => setName(d.value)} />
            </Field>
            <Field label="地点">
              <Input maxLength={80} placeholder="选填" value={location} onChange={(_, d) => setLocation(d.value)} />
            </Field>
            <DateTimeRow label="开始" value={start} onChange={setStart} />
            <DateTimeRow label="结束" value={end} onChange={setEnd} />
            <Switch
              label="重复"
              checked={repet}
              onChange={(_, d) => {
                setRepet(d.checked);
                if (d.checked) pickFreq(freq);
              }}
            />
            {crossRepeat ? <p className="hint warn">跨天日程不能重复</p> : null}
            {repet ? (
              <>
                <Field label="重复频率">
                  <Dropdown
                    value={FREQS.find((f) => f.value === freq)?.label || "每天"}
                    selectedOptions={[freq]}
                    onOptionSelect={(_, data) => {
                      const next = FREQS.find((f) => f.value === data.optionValue)?.value;
                      if (next) pickFreq(next);
                    }}
                  >
                    {FREQS.map((f) => (
                      <Option key={f.value} value={f.value} text={f.label}>{f.label}</Option>
                    ))}
                  </Dropdown>
                </Field>
                {freq === "WEEKLY" ? (
                  <div className="chip-row" role="group" aria-label="星期">
                    {WEEKDAYS.map((d) => {
                      const on = byday.includes(d.value);
                      return (
                        <button
                          key={d.value}
                          type="button"
                          aria-pressed={on}
                          onClick={() => setByday(on ? byday.filter((x) => x !== d.value) : [...byday, d.value])}
                        >
                          {d.label}
                        </button>
                      );
                    })}
                  </div>
                ) : null}
                <Field label="结束重复">
                  <Dropdown
                    value={endmode === "DATE" ? "指定结束日期" : "指定重复次数"}
                    selectedOptions={[endmode]}
                    onOptionSelect={(_, data) => {
                      if (data.optionValue === "DATE") {
                        setEndmode("DATE");
                        setEndOn((prev) => (prev.y ? prev : { ...start }));
                      } else if (data.optionValue === "NUMBER") {
                        setEndmode("NUMBER");
                      }
                    }}
                  >
                    <Option value="DATE" text="指定结束日期">指定结束日期</Option>
                    <Option value="NUMBER" text="指定重复次数">指定重复次数</Option>
                  </Dropdown>
                </Field>
                {endmode === "DATE" ? (
                  <DateTimeRow label="结束日期" value={endOn} onChange={setEndOn} dateOnly />
                ) : (
                  <Field label="重复次数">
                    <Input
                      inputMode="numeric"
                      maxLength={3}
                      value={repetNum}
                      onChange={(_, d) => setRepetNum(d.value.replace(/\D/g, "").slice(0, 3))}
                    />
                  </Field>
                )}
              </>
            ) : null}
            <Switch label="提醒" checked={remind} onChange={(_, d) => setRemind(d.checked)} />
            {remind ? (
              <Field label="何时提醒">
                <Dropdown
                  value={REMIND_CODES.find((r) => r.value === remindCode)?.label || "事项开始前15分钟"}
                  selectedOptions={[remindCode]}
                  onOptionSelect={(_, data) => {
                    if (data.optionValue) setRemindCode(data.optionValue);
                  }}
                >
                  {REMIND_CODES.map((r) => (
                    <Option key={r.value} value={r.value} text={r.label}>{r.label}</Option>
                  ))}
                </Dropdown>
              </Field>
            ) : null}
            {remind && remindCode === "7" ? (
              <Field label="前一天的时刻">
                <div className="dt-row">
                  <Input
                    aria-label="提醒时"
                    placeholder="时"
                    inputMode="numeric"
                    maxLength={2}
                    value={setHour}
                    onChange={(_, d) => setSetHour(d.value.replace(/\D/g, "").slice(0, 2))}
                  />
                  <span>:</span>
                  <Input
                    aria-label="提醒分"
                    placeholder="分"
                    inputMode="numeric"
                    maxLength={2}
                    value={setMinute}
                    onChange={(_, d) => setSetMinute(d.value.replace(/\D/g, "").slice(0, 2))}
                  />
                </div>
              </Field>
            ) : null}
            {remind ? <p className="hint">到点弹出岛，约 10 秒。</p> : null}
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
