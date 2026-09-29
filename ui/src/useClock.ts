import { useEffect, useState } from "react";
import { api } from "./api";

export function useClock(fast: boolean) {
  const [now, setNow] = useState(() => new Date());
  const [overridden, setOverridden] = useState(false);
  useEffect(() => {
    let t = 0;
    let cancelled = false;
    const tick = async () => {
      try {
        const c = await api.clockNow();
        if (cancelled) return;
        const d = new Date(c.iso);
        if (!Number.isNaN(d.getTime())) setNow(d);
        setOverridden(!!c.overridden);
        t = window.setTimeout(tick, c.overridden || fast ? 1000 : 5000);
      } catch {
        if (cancelled) return;
        setNow(new Date());
        setOverridden(false);
        t = window.setTimeout(tick, fast ? 1000 : 5000);
      }
    };
    tick();
    return () => {
      cancelled = true;
      window.clearTimeout(t);
    };
  }, [fast]);
  return { now, overridden };
}
