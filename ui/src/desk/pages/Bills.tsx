import { useEffect, useState } from "react";
import { Button, Tab, TabList } from "@fluentui/react-components";
import { api } from "../../api";
import { fmtMoney, isJunkBill, pad, ymd } from "../../format";
import type { DaySpend, Snapshot } from "../../types";

export function Bills(props: {
  snap: Snapshot | null;
  now: Date;
  tab: "card" | "net";
  onTab: (tab: "card" | "net") => void;
  onSnap: (s: Snapshot) => void;
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

  useEffect(() => {
    const open = tab === "card";
    api.setBillsOpen(open).catch(() => undefined);
    return () => {
      if (open) api.setBillsOpen(false).catch(() => undefined);
    };
  }, [tab]);

  useEffect(() => {
    if (tab !== "card") return;
    const want = `${ym.y}-${pad(ym.m)}`;
    if (snap?.cardMonth === want && (snap.cardSpend?.length ?? 0) > 0) return;
    let cancel = false;
    api.loadMonth(ym.y, ym.m).then((s) => {
      if (!cancel) props.onSnap(s);
    }).catch(() => undefined);
    return () => {
      cancel = true;
    };
  }, [tab, ym.y, ym.m, snap?.cardMonth, snap?.cardSpend?.length]);

  const spend = snap?.cardSpend || [];
  const maxSpend = Math.max(...spend.map((d) => d.spend), 1);
  const monthSpend = spend.reduce((a, d) => a + d.spend, 0);
  const todaySpend = spend.find((d) => d.date === ymd(now))?.spend || 0;
  const selectedDay: DaySpend | undefined = spend.find((d) => d.date === selected);
  const isCurrentMonth = ym.y === now.getFullYear() && ym.m === now.getMonth() + 1;
  const today = ymd(now);

  async function shiftMonth(delta: number) {
    let y = ym.y;
    let m = ym.m + delta;
    if (m < 1) { m = 12; y--; }
    if (m > 12) { m = 1; y++; }
    if (y > now.getFullYear() || (y === now.getFullYear() && m > now.getMonth() + 1)) return;
    setYm({ y, m });
    try {
      const s = await api.loadMonth(y, m);
      props.onSnap(s);
      const days = s.cardSpend || [];
      const t = ymd(now);
      setSelected(days.find((d) => d.date === t)?.date || days[days.length - 1]?.date || t);
    } catch (e) {
      setErr(String(e));
    }
  }

  return (
    <div className="stack">
      <div className="row-between">
        <h1 className="page-title">账单</h1>
        <TabList selectedValue={tab} onTabSelect={(_, data) => props.onTab(data.value as "card" | "net")}>
          <Tab value="card">校园卡</Tab>
          <Tab value="net">网费</Tab>
        </TabList>
      </div>

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
            <div><div className="k">{isCurrentMonth ? "本月" : `${ym.m}月`}</div><div className="v num">{fmtMoney(monthSpend)} 元</div></div>
            <div>
              <div className="k">{isCurrentMonth ? "今日" : "日均"}</div>
              <div className="v num">
                {fmtMoney(isCurrentMonth ? todaySpend : monthSpend / Math.max(spend.filter((d) => d.date <= today).length, 1))} 元
              </div>
            </div>
          </div>
          <div className="chart-block">
            {hover && (() => {
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
              {spend.map((d) => {
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
            <div className="xaxis"><span>1日</span><span>15日</span><span>{spend.length ? `${spend.length}日` : ""}</span></div>
          </div>
          {selectedDay && (
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
          )}
          <div className="foot">{snap?.billsTruncated ? "本月可能不完整 · " : ""}只计消费，充值不进柱高{err ? ` · ${err}` : ""}</div>
        </>
      )}
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
