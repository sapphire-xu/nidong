import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Button, Tab, TabList } from "@fluentui/react-components";
import { api } from "../../api";
import { fmtMoney, isJunkBill, pad, ymd } from "../../format";
import type { DaySpend, Snapshot } from "../../types";

function daysInMonth(y: number, m: number) {
  return new Date(y, m, 0).getDate();
}

const MIN_LOAD_MS = 1500;

function WaveBars(props: { count: number }) {
  const cols = useRef<(HTMLDivElement | null)[]>([]);
  useLayoutEffect(() => {
    const n = Math.max(props.count, 1);
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const paint = (phase: number) => {
      const denom = Math.max(n - 1, 1);
      for (let i = 0; i < n; i++) {
        const el = cols.current[i];
        if (!el) continue;
        const x = (i / denom) * Math.PI * 2;
        const wave = Math.sin(x - phase);
        const along = (wave + 1) / 2;
        el.style.height = `${(22 + along * 96).toFixed(1)}px`;
        el.style.background = `color-mix(in srgb, var(--colorBrandBackground) ${(38 + along * 62).toFixed(1)}%, transparent)`;
      }
    };
    if (reduce) {
      paint(0.8);
      return;
    }
    const t0 = performance.now();
    let raf = 0;
    const loop = (t: number) => {
      paint(((t - t0) / 1000) * 2.4);
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [props.count]);
  return (
    <>
      {Array.from({ length: props.count }, (_, i) => (
        <div key={i} className="col wave-col" aria-hidden="true">
          <div className="bar wave" ref={(el) => { cols.current[i] = el; }} />
        </div>
      ))}
    </>
  );
}

function LoadingCaption() {
  const [step, setStep] = useState(0);
  useEffect(() => {
    const id = window.setInterval(() => setStep((n) => (n + 1) % 4), 500);
    return () => window.clearInterval(id);
  }, []);
  return (
    <div className="bill-empty">
      加载中<span className="load-dots">{".".repeat(step)}</span>
    </div>
  );
}

export function Bills(props: {
  snap: Snapshot | null;
  now: Date;
  tab: "card" | "net";
  onTab: (tab: "card" | "net") => void;
  onSnap: (s: Snapshot) => void;
  onRequestLogin: () => void;
}) {
  const { snap, now, tab } = props;
  const [ym, setYm] = useState(() => {
    if (snap?.cardMonth) {
      const [y, m] = snap.cardMonth.split("-").map(Number);
      if (y && m) return { y, m };
    }
    return { y: now.getFullYear(), m: now.getMonth() + 1 };
  });
  const [selected, setSelected] = useState(ymd(now));
  const [hover, setHover] = useState<string | null>(null);
  const [err, setErr] = useState("");
  const [hold, setHold] = useState<{ want: string; until: number } | null>(null);
  const holdTimer = useRef(0);
  const pane = useRef({ tab, motion: "" as "" | "to-net" | "to-card" });
  if (pane.current.tab !== tab) {
    pane.current = { tab, motion: tab === "net" ? "to-net" : "to-card" };
  }

  useEffect(() => () => window.clearTimeout(holdTimer.current), []);

  useEffect(() => {
    const open = tab === "card";
    api.setBillsOpen(open).catch(() => undefined);
    return () => {
      if (open) api.setBillsOpen(false).catch(() => undefined);
    };
  }, [tab]);

  const want = `${ym.y}-${pad(ym.m)}`;
  const ready = snap?.cardMonth === want && snap.cardSpend != null;

  useEffect(() => {
    if (tab !== "card" || !snap?.loggedIn || !snap) return;
    if (snap.cardMonth === want && snap.cardSpend != null) return;
    let cancel = false;
    const until = performance.now() + MIN_LOAD_MS;
    setHold({ want, until });
    window.clearTimeout(holdTimer.current);
    holdTimer.current = window.setTimeout(() => {
      setHold((current) => (current && current.until === until ? null : current));
    }, MIN_LOAD_MS);
    setErr("");
    api.loadMonth(ym.y, ym.m).then((s) => {
      if (cancel) return;
      props.onSnap(s);
      const days = s.cardSpend || [];
      const t = ymd(now);
      setSelected(days.some((d) => d.date === t) ? t : days[days.length - 1]?.date || t);
    }).catch((e) => {
      if (!cancel) setErr(String(e));
    });
    return () => {
      cancel = true;
    };
  }, [tab, ym.y, ym.m, want, snap?.cardMonth, snap?.loggedIn]);

  const spend = ready ? snap?.cardSpend || [] : [];
  const maxSpend = Math.max(...spend.map((d) => d.spend), 1);
  const monthSpend = spend.reduce((a, d) => a + d.spend, 0);
  const todaySpend = spend.find((d) => d.date === ymd(now))?.spend || 0;
  const selectedDay: DaySpend | undefined = spend.find((d) => d.date === selected);
  const isCurrentMonth = ym.y === now.getFullYear() && ym.m === now.getMonth() + 1;
  const today = ymd(now);
  const holding = !err && hold != null && hold.want === want && performance.now() < hold.until;
  const showLoading = !err && (holding || !ready);
  const revealed = ready && !showLoading;
  const third = isCurrentMonth ? todaySpend : monthSpend / Math.max(spend.filter((d) => d.date <= today).length, 1);

  function shiftMonth(delta: number) {
    let y = ym.y;
    let m = ym.m + delta;
    if (m < 1) { m = 12; y--; }
    if (m > 12) { m = 1; y++; }
    if (y > now.getFullYear() || (y === now.getFullYear() && m > now.getMonth() + 1)) return;
    setErr("");
    setHover(null);
    setYm({ y, m });
  }

  if (!snap?.loggedIn) {
    return (
      <div className="stack">
        <div className="row-between">
          <h1 className="page-title">账单</h1>
        </div>
        <div className="empty">校园卡和网费需要登录。</div>
        <Button appearance="primary" onClick={props.onRequestLogin}>去登录</Button>
      </div>
    );
  }

  return (
    <div className="stack">
      <div className="row-between">
        <h1 className="page-title">账单</h1>
        <TabList className="bill-tabs" selectedValue={tab} onTabSelect={(_, data) => props.onTab(data.value as "card" | "net")}>
          <Tab value="card">校园卡</Tab>
          <Tab value="net">网费</Tab>
        </TabList>
      </div>

      <div className="bill-switch">
      <div key={tab} className={`bill-pane${pane.current.motion ? ` ${pane.current.motion}` : ""}`}>
      {tab === "net" ? <NetDoc /> : (
        <>
          <div className="row-between">
            <div className="row-between">
              <Button appearance="subtle" onClick={() => shiftMonth(-1)}>上个月</Button>
              <strong>{isCurrentMonth ? `${ym.m}月` : `${ym.y}年${ym.m}月`}</strong>
              <Button appearance="subtle" disabled={isCurrentMonth} onClick={() => shiftMonth(1)}>下个月</Button>
            </div>
          </div>
          <div className="bill-metrics">
            <div><div className="k">余额</div><div className="v num">{snap?.card ? fmtMoney(snap.card.balance) + " 元" : "—"}</div></div>
            <div><div className="k">{isCurrentMonth ? "本月" : `${ym.m}月`}</div><div className="v num">{revealed ? `${fmtMoney(monthSpend)} 元` : "-"}</div></div>
            <div>
              <div className="k">{isCurrentMonth ? "今日" : "日均"}</div>
              <div className="v num">{revealed ? `${fmtMoney(third)} 元` : "-"}</div>
            </div>
          </div>
          <div className="chart-block" aria-busy={showLoading || undefined}>
            {!showLoading && hover && (() => {
              const d = spend.find((x) => x.date === hover);
              if (!d) return null;
              const i = spend.findIndex((x) => x.date === hover);
              const future = d.date > today;
              const text = future ? "还没到" : d.spend ? `¥${fmtMoney(d.spend)} · ${d.count} 笔` : "没刷卡";
              const r = (i + 0.5) / Math.max(spend.length, 1);
              const h = d.spend ? Math.max(4, Math.round((d.spend / maxSpend) * 120)) : 0;
              const edge = r < 0.12 ? "start" : r > 0.88 ? "end" : "middle";
              return (
                <div
                  className="tip"
                  style={{
                    ...(edge === "start"
                      ? { left: 0, transform: "none" }
                      : edge === "end"
                        ? { right: 0, left: "auto", transform: "none" }
                        : { left: `${r * 100}%` }),
                    bottom: `${28 + h}px`,
                  }}
                >
                  {d.date.slice(5)} · {text}
                </div>
              );
            })()}
            <div className="chart">
              {showLoading ? <WaveBars count={daysInMonth(ym.y, ym.m)} /> : spend.map((d) => {
                const h = d.spend ? Math.max(4, Math.round((d.spend / maxSpend) * 120)) : 0;
                const future = d.date > today;
                const on = d.date === today || d.date === selected;
                return (
                  <button
                    key={d.date}
                    type="button"
                    className="col"
                    onMouseEnter={() => setHover(d.date)}
                    onMouseLeave={() => setHover(null)}
                    onClick={() => setSelected(d.date)}
                  >
                    {d.spend && !future ? <div className={`bar ${on ? "on" : ""}`} style={{ height: h }} /> : null}
                  </button>
                );
              })}
            </div>
            <div className="xaxis"><span>1日</span><span>15日</span><span>{showLoading ? `${daysInMonth(ym.y, ym.m)}日` : spend.length ? `${spend.length}日` : ""}</span></div>
          </div>
          {showLoading ? (
            <div className="bills"><LoadingCaption /></div>
          ) : selectedDay ? (
            <>
              <div className="day-title">{selectedDay.date} · {selectedDay.count} 笔 · {fmtMoney(selectedDay.spend)} 元</div>
              <div className="bills">
                {selectedDay.date > today ? (
                  <div className="bill-empty">还没到这一天</div>
                ) : selectedDay.bills.filter((b) => !isJunkBill(b)).length === 0 ? (
                  <div className="bill-empty">这天没刷卡</div>
                ) : selectedDay.bills.filter((b) => !isJunkBill(b)).map((b, i) => (
                  <div className="bill" key={i}>
                    <span className="t num">{b.time.slice(11, 16)}</span>
                    <span className="s" title={[b.subject, b.station].filter(Boolean).join(" · ")}>
                      {b.subject}{b.station ? ` · ${b.station}` : ""}
                    </span>
                    <span className={`a num ${b.amount > 0 ? "pos" : ""}`}>{b.amount > 0 ? "+" : ""}{fmtMoney(b.amount)}</span>
                  </div>
                ))}
              </div>
            </>
          ) : null}
          <div className="foot">{snap?.billsTruncated ? "本月可能不完整 · " : ""}只计消费，充值不进柱高{err ? ` · ${err}` : ""}</div>
        </>
      )}
      </div>
      </div>
    </div>
  );
}

function NetDoc() {
  return (
    <article className="net-doc">
      <p>东大校园网按「这个月用了多少校外 IPv4 下载流量」阶梯收费。</p>
      <h3>什么要钱，什么不要钱</h3>
      <ul>
        <li><b>要钱：</b>用校园网的 IPv4 访问校外网站时，<b>下载</b>的那部分流量。</li>
        <li><b>不要钱：</b>校内网站（教务、邮箱、门户等）、IPv6、你上传发出去的流量。</li>
        <li><b>夜里不要钱：</b>每天 <b>0:00–6:00</b> 访问外网也不计费，适合下大文件。</li>
      </ul>
      <h3>怎么算这个月要交多少</h3>
      <p>按自然月累计上面那种「要钱的流量」，对号入座。前面几档用满了，再按后面的档算。</p>
      <table>
        <thead>
          <tr><th>本月已用</th><th>这个月网费</th></tr>
        </thead>
        <tbody>
          <tr><td>0–10 GB（含）</td><td>免费</td></tr>
          <tr><td>10–25 GB（含）</td><td>超出 10 GB 的部分，1 元 / GB</td></tr>
          <tr><td>25–40 GB（含）</td><td>一口价 15 元</td></tr>
          <tr><td>40–45 GB（含）</td><td>15 元 + 超出 40 GB 的部分 1 元 / GB</td></tr>
          <tr><td>45–80 GB（含）</td><td>一口价 20 元</td></tr>
          <tr><td>80 GB 以上</td><td>20 元 + 超出 80 GB 的部分 1 元 / GB</td></tr>
        </tbody>
      </table>
      <h3>举几个例子</h3>
      <ul>
        <li>用了 8 GB → <b>0 元</b></li>
        <li>用了 18 GB → 超出免费的 8 GB × 1 元 → <b>8 元</b></li>
        <li>用了 30 GB → 落在第三档 → <b>15 元</b></li>
        <li>用了 42 GB → 15 元再加 2 GB → <b>17 元</b></li>
        <li>用了 50 GB → 第五档一口价 → <b>20 元</b></li>
        <li>用了 90 GB → 20 元再加 10 GB → <b>30 元</b></li>
      </ul>
      <h3>和看板上数字的关系</h3>
      <ul>
        <li>大数字是账户里<b>预存的余额</b>，流量费从这里扣。</li>
        <li>「已用」是门户统计的<b>本月流量合计</b>。夜间免费段会不会从合计里剔除，以学校网关为准。</li>
        <li>一个账号最多 <b>5 台</b>设备同时在线（电脑、手机都算）。</li>
      </ul>
      <p className="src">沈阳本校学生政策。秦皇岛分校不一样。收费依据：《东北大学会议纪要》（纪要〔2021〕13 号）。</p>
    </article>
  );
}
