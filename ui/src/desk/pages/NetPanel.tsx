import { useEffect, useState } from "react";
import { Button, Dialog, DialogActions, DialogBody, DialogContent, DialogSurface, DialogTitle } from "@fluentui/react-components";
import { api, type MeterDay, type MeterStatus, type MeterTotals } from "../../api";
import { formatBytes } from "../../format";

function formatRate(n: number) {
  if (n >= 1024 * 1024) return (n / 1024 / 1024).toFixed(1) + " MB/s";
  if (n >= 1024) return Math.round(n / 1024) + " KB/s";
  return Math.round(n) + " B/s";
}

function cell(totals: MeterTotals | null, pick: (t: MeterTotals) => number) {
  if (!totals) return "—";
  return formatBytes(pick(totals));
}

function MeterCols() {
  return (
    <colgroup>
      <col className="net-col-head" />
      <col className="net-col-eq" />
      <col className="net-col-eq" />
      <col className="net-col-eq" />
      <col className="net-col-eq" />
      <col className="net-col-eq" />
    </colgroup>
  );
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const el = document.createElement("textarea");
    el.value = text;
    el.setAttribute("readonly", "");
    el.style.cssText = "position:fixed;left:-9999px;top:0;opacity:0;user-select:text";
    document.body.appendChild(el);
    el.focus();
    el.select();
    el.setSelectionRange(0, text.length);
    const ok = document.execCommand("copy");
    document.body.removeChild(el);
    return ok;
  }
}

function AddrRow(props: { label: string; value: string; empty?: string }) {
  const [copied, setCopied] = useState(false);
  const text = props.value.trim();
  const empty = props.empty ?? "没有读到";
  async function copy() {
    if (!text) return;
    if (await copyText(text)) {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    }
  }
  return (
    <div className="net-addr">
      <span className="k">{props.label}</span>
      <code title={text || undefined}>{text || empty}</code>
      <Button size="small" appearance="secondary" disabled={!text} onClick={() => { void copy(); }}>
        {copied ? "已复制" : "复制"}
      </Button>
    </div>
  );
}

function MeterTable(props: { today: MeterTotals | null; month: MeterTotals | null }) {
  const rows: { label: string; totals: MeterTotals | null }[] = [
    { label: "本日", totals: props.today },
    { label: "本月", totals: props.month },
  ];
  return (
    <table>
      <MeterCols />
      <thead>
        <tr>
          <th></th>
          <th>IPv4 下载（正常）</th>
          <th>IPv4 下载（半夜）</th>
          <th>IPv4 上传</th>
          <th>IPv6 下载</th>
          <th>IPv6 上传</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => (
          <tr key={row.label}>
            <td>{row.label}</td>
            <td className="num">{cell(row.totals, (t) => t.v4Day)}</td>
            <td className="num">{cell(row.totals, (t) => t.v4Night)}</td>
            <td className="num">{cell(row.totals, (t) => t.v4Up)}</td>
            <td className="num">{cell(row.totals, (t) => t.v6Down)}</td>
            <td className="num">{cell(row.totals, (t) => t.v6Up)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function NetPanel(props: { portalBytes?: number; portalKnown: boolean }) {
  const [status, setStatus] = useState<MeterStatus | null>(null);
  const [view, setView] = useState<"home" | "rules" | "log">("home");
  const [motion, setMotion] = useState("");
  const [busy, setBusy] = useState(false);
  const [ask, setAsk] = useState(false);
  const [askOff, setAskOff] = useState(false);
  const [err, setErr] = useState("");
  const [days, setDays] = useState<MeterDay[] | null>(null);
  const [outerV4, setOuterV4] = useState<string | null>(null);

  useEffect(() => {
    let stop = false;
    const pull = () => {
      api.meterStatus().then((s) => {
        if (stop) return;
        setStatus(s);
        if (!s.enabled) setView("home");
      }).catch(() => {});
    };
    pull();
    const id = window.setInterval(pull, 2000);
    return () => {
      stop = true;
      window.clearInterval(id);
    };
  }, []);

  useEffect(() => {
    if (view !== "log") return;
    let stop = false;
    api.meterLog().then((rows) => {
      if (!stop) setDays(rows);
    }).catch((e) => {
      if (!stop) setErr(String(e));
    });
    return () => {
      stop = true;
    };
  }, [view]);

  function openSub(next: "rules" | "log") {
    setMotion("net-sub");
    setView(next);
  }

  function back() {
    setMotion("net-hub");
    setView("home");
  }

  async function enable() {
    setBusy(true);
    setErr("");
    try {
      await api.meterEnable();
      setStatus(await api.meterStatus());
      setAsk(false);
    } catch (e) {
      setErr(String(e));
      setAsk(false);
    } finally {
      setBusy(false);
    }
  }

  async function disable() {
    setBusy(true);
    setErr("");
    try {
      await api.meterDisable();
      setStatus(await api.meterStatus());
      setAskOff(false);
      setView("home");
    } catch (e) {
      setErr(String(e));
      setAskOff(false);
    } finally {
      setBusy(false);
    }
  }

  const showLive = !!status && status.enabled && status.running && status.onCampus;

  useEffect(() => {
    if (!showLive) return;
    let stop = false;
    const pull = () => {
      api.publicIpv4().then((ip) => {
        if (!stop) setOuterV4(ip.trim());
      }).catch(() => {
        if (!stop) setOuterV4("");
      });
    };
    pull();
    const id = window.setInterval(pull, 30_000);
    return () => {
      stop = true;
      window.clearInterval(id);
    };
  }, [showLive]);

  if (!status) {
    return <article className="net-doc"><p className="hint">正在读取本机记录…</p></article>;
  }

  const enabled = status.enabled;

  return (
    <div className={motion}>
      {view === "rules" ? (
        <div className="net-doc">
          <div className="net-log-bar">
            <Button appearance="secondary" onClick={back}>返回</Button>
          </div>
          <NetDoc />
        </div>
      ) : view === "log" ? (
        <article className="net-doc">
          <div className="net-log-bar">
            <Button appearance="secondary" onClick={back}>返回</Button>
            <Button appearance="secondary" onClick={() => { void api.meterOpenLog().catch((e) => setErr(String(e))); }}>打开日志源文件</Button>
          </div>
          <h3>本机流量日志</h3>
          <p className="hint">只留下有流量的日子。没开机，或不在校园网，就不会有这一行。</p>
          {days == null ? <p className="hint">正在读取…</p> : days.length === 0 ? <p>还没有记下任何一天。</p> : (
            <div className="net-sheet net-log">
              <table>
                <MeterCols />
                <thead>
                  <tr>
                    <th>日期</th>
                    <th>IPv4 下载（正常）</th>
                    <th>IPv4 下载（半夜）</th>
                    <th>IPv4 上传</th>
                    <th>IPv6 下载</th>
                    <th>IPv6 上传</th>
                  </tr>
                </thead>
                <tbody>
                  {days.map((day) => (
                    <tr key={day.date}>
                      <td className="num">{day.date}</td>
                      <td className="num">{formatBytes(day.totals.v4Day)}</td>
                      <td className="num">{formatBytes(day.totals.v4Night)}</td>
                      <td className="num">{formatBytes(day.totals.v4Up)}</td>
                      <td className="num">{formatBytes(day.totals.v6Down)}</td>
                      <td className="num">{formatBytes(day.totals.v6Up)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          {err ? <p className="hint warn">{err}</p> : null}
        </article>
      ) : enabled ? (
        <article className="net-doc net-home">
          {showLive ? (
            <div className="net-live">
              <div className="net-live-head">
                <span className="net-dot ok" />
                <span>{status.night ? "正在记录 · 夜里免费时段" : "正在记录 · 计费时段"}</span>
              </div>
              <div className="net-addr-pair">
                <AddrRow label="校内 IPv4" value={status.ipv4} />
                <AddrRow
                  label="对外 IPv4"
                  value={outerV4 ?? ""}
                  empty={outerV4 == null ? "正在检查" : "没有读到"}
                />
              </div>
              <AddrRow label="IPv6" value={status.ipv6} />
              <div className="net-rates">
                <div className="net-rate">
                  <div className="k">校外 IPv4 下载{status.night ? <span className="net-tag free">免费</span> : <span className="net-tag bill">计费</span>}</div>
                  <div className="v num">{formatRate(status.v4DownBps)}</div>
                </div>
                <div className="net-rate">
                  <div className="k">校外 IPv6 下载</div>
                  <div className="v num">{formatRate(status.v6DownBps)}</div>
                </div>
              </div>
              {status.udpGap ? (
                <p className="hint warn">这一段事件丢了，UDP 没有写入。TCP 按连接计数补上。</p>
              ) : status.udpReady ? null : (
                <p className="hint">UDP 没有接上，隧道里的流量不会出现在上面的速度里。</p>
              )}
              <p className="net-portal">校园网记录的流量 {props.portalKnown && props.portalBytes != null ? formatBytes(props.portalBytes) : "—"}</p>
            </div>
          ) : (
            <div className="net-live muted">
              <div className="net-live-head">
                <span className="net-dot" />
                <span>{status.running ? "现在不在校园网，这一段不记。" : "后台这会儿没在记。开机后会自己起来。"}</span>
              </div>
            </div>
          )}
          <div className="net-sheet">
            <MeterTable today={status.today} month={status.month} />
          </div>
          <p className="src">上面的「校园网记录」是门户上的本月合计。表里是这台电脑自己记的。校内 IPv4 下载不进表。TCP 和 UDP 并进这五列；某一轮事件丢了，这一轮的 UDP 不写。</p>
          <hr className="net-split" />
          <div className="share-actions net-toolbar">
            <div className="net-toolbar-start">
              <Button appearance="secondary" onClick={() => openSub("rules")}>查看计费规则</Button>
              <Button appearance="secondary" onClick={() => openSub("log")}>打开日志</Button>
            </div>
            <Button className="net-btn-close" appearance="secondary" disabled={busy} onClick={() => setAskOff(true)}>关闭流量记录</Button>
          </div>
          {err ? <p className="hint warn">{err}</p> : null}
        </article>
      ) : (
        <article className="net-doc">
          <p>这台电脑可以记下自己经过校园网的流量，分成白天的校外 IPv4 下载、夜里的 IPv4 下载、IPv4 上传、IPv6 下载和 IPv6 上传。默认关着。门户上的「已用」仍是学校记的数。</p>
          <div className="net-actions">
            <Button appearance="primary" disabled={busy} onClick={() => { setErr(""); setAsk(true); }}>开启流量记录</Button>
          </div>
          {err || status.error ? <p className="hint warn">{err || status.error}</p> : null}
          <hr className="net-split" />
          <h3>网费计费规则</h3>
          <NetDoc />
        </article>
      )}

      <Dialog open={ask} onOpenChange={(_, data) => { if (!data.open && !busy) setAsk(false); }}>
        <DialogSurface className="safety-dialog">
          <DialogBody>
            <DialogTitle>开启流量记录</DialogTitle>
            <DialogContent className="safety-copy">
              <p>记下流量要管理员权限。确定后 Windows 会问一次是否允许。</p>
              <p>允许之后，开机自动在后台继续记，关掉这个窗口也不会停。不在校园网时不记。已经记下的日子留在这台电脑上。</p>
            </DialogContent>
            <DialogActions fluid>
              <div className="share-warn-actions">
                <Button appearance="secondary" disabled={busy} onClick={() => setAsk(false)}>取消</Button>
                <Button appearance="primary" disabled={busy} onClick={enable}>{busy ? "正在请求权限" : "开启"}</Button>
              </div>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>

      <Dialog open={askOff} onOpenChange={(_, data) => { if (!data.open && !busy) setAskOff(false); }}>
        <DialogSurface className="safety-dialog">
          <DialogBody>
            <DialogTitle>关闭流量记录</DialogTitle>
            <DialogContent className="safety-copy">
              <p>会停掉后台记录，并取消开机自启。已经记下的日子还留着。</p>
              <p>Windows 会再问一次管理员权限。</p>
            </DialogContent>
            <DialogActions fluid>
              <div className="share-warn-actions">
                <Button appearance="secondary" disabled={busy} onClick={() => setAskOff(false)}>取消</Button>
                <Button appearance="primary" disabled={busy} onClick={disable}>{busy ? "正在请求权限" : "关闭"}</Button>
              </div>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>
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
