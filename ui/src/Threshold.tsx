import { Field, Input, Slider } from "@fluentui/react-components";
import { useEffect, useState } from "react";

export function Threshold({
  label,
  value,
  min = 0,
  max,
  onCommit,
}: {
  label: string;
  value: number;
  min?: number;
  max: number;
  onCommit: (n: number) => void;
}) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);

  function commit(raw: string) {
    const n = Math.min(max, Math.max(min, Math.round(Number(raw))));
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
          <Slider min={min} max={max} step={1} value={Math.min(value, max)} onChange={(_, data) => {
            setDraft(String(data.value));
            onCommit(data.value);
          }} />
          <Input
            className="threshold-input"
            type="number"
            min={min}
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
