import { Button, Card, MessageBar, MessageBarBody } from "@fluentui/react-components";
import { Add20Regular, ArrowSync20Regular, Checkmark20Regular } from "@fluentui/react-icons";
import type { Agenda, AppSettings, Course, DaySpend, Snapshot } from "../../types";
import { deriveDay } from "../../dayModel";
import { fmtElec, fmtMoney, formatBytes, placeLabel, ymd } from "../../format";
import { WEEKDAY_SHORT } from "../../course";
import type { MixRow } from "../../agenda";

function courseMeta(c: Course) {
  return [c.teacher, placeLabel(c.place)].filter((s) => s && s.trim()).join(" · ");
}

function MixBody({ row }: { row: MixRow }) {
  const event = row.type === "event";
  return (
    <>
      <div className="when">
        <span className="num">{row.end ? `${row.start}–${row.end}` : row.start}</span>
        {row.label ? <span className="when-label">{row.label}</span> : null}
      </div>
      <div className="cname">
        <span className="cname-main">
          {event ? <i className="kind-dot" aria-hidden="true" /> : null}
          {row.name}
        </span>
      </div>
      <div className="place">{row.course ? courseMeta(row.course) : row.meta || (event ? "日程" : "")}</div>
    </>
  );
}

function DayLists(props: {
  day: ReturnType<typeof deriveDay>;
  onOpenEvent: (item: Agenda | null) => void;
  courses?: boolean;
}) {
  const { day } = props;
  if (props.courses === false) {
    const seen = new Set<string>();
    const events = [...day.todayMix, ...day.tomorrowMix].filter((r) => {
      if (r.type !== "event" || seen.has(r.key)) return false;
      seen.add(r.key);
      return true;
    });
    if (!events.length) return <div className="empty">课程需要登录。日程可以在这里添加。</div>;
    return (
      <div className="mix">
        {events.map((r) => (
          <MixRowView key={r.key} row={r} onEvent={() => r.event && props.onOpenEvent(r.event)} />
        ))}
      </div>
    );
  }
  const revealTomorrow = day.todayDone || day.todayCourses.length === 0;
  const showTomorrow = revealTomorrow && day.tomorrowMix.length > 0;
  return (
    <>
      {day.todayDone ? (
        <>
          <div className="done-note">今日课程已上完</div>
          {day.remainingTodayEvents.length ? (
            <div className="mix">
              {day.remainingTodayEvents.map((r) => (
                <MixRowView key={r.key} row={r} onEvent={() => r.event && props.onOpenEvent(r.event)} />
              ))}
            </div>
          ) : null}
        </>
      ) : day.todayCourses.length === 0 ? (
        <>
          <div className="done-note">今天没有课</div>
          {day.todayMix.length ? (
            <div className="mix">
              {day.todayMix.map((r) => (
                <MixRowView key={r.key} row={r} onEvent={() => r.event && props.onOpenEvent(r.event)} />
              ))}
            </div>
          ) : null}
        </>
      ) : (
        <div className="mix">
          {day.todayMix.map((r) => (
            <MixRowView key={r.key} row={r} onEvent={() => r.event && props.onOpenEvent(r.event)} />
          ))}
        </div>
      )}
      {showTomorrow ? (
        <>
          <strong>明日</strong>
          <div className="mix">
            {day.tomorrowMix.map((r) => (
              <MixRowView
                key={`tm-${r.key}`}
                row={{ ...r, kind: "later", label: r.type === "event" ? "日程" : "明天" }}
                onEvent={() => r.event && props.onOpenEvent(r.event)}
              />
            ))}
          </div>
        </>
      ) : null}
    </>
  );
}

function MixRowView({ row, onEvent }: { row: MixRow; onEvent?: () => void }) {
  if (row.type === "event") {
    return (
      <button type="button" className={`mix-row ${row.kind}`} onClick={onEvent}>
        <MixBody row={row} />
      </button>
    );
  }
  return (
    <div className={`mix-row ${row.kind}`}>
      <MixBody row={row} />
    </div>
  );
}

export function Home(props: {
  snap: Snapshot | null;
  settings: AppSettings | null;
  now: Date;
  clockOverridden: boolean;
  refreshUi: "idle" | "spin" | "ok";
  err: string;
  homeSpend: DaySpend[];
  courseSync: "idle" | "spin" | "ok" | "fail";
  onRefresh: () => void;
  onOpenBills: (tab: "card" | "net") => void;
  onOpenSchedule: () => void;
  onOpenEvent: (item: Agenda | null) => void;
  onSyncCourses: () => void;
  onRequestLogin: () => void;
}) {
  const { snap, settings, now, clockOverridden, refreshUi, err, homeSpend, courseSync } = props;
  const day = deriveDay(snap, settings, now);
  const loggedIn = !!snap?.loggedIn;
  const offline = !!snap?.offline;
  const cardWarn = (snap?.card?.balance ?? 999) < (settings?.cardWarn ?? 20);
  const netWarn = (snap?.net?.balance ?? 999) < (settings?.netWarn ?? 5);
  const elecCached = !!snap?.elec && !snap.elec?.unbound && snap.elec?.remainKwh != null;
  const elecSyncFail = !!snap?.errors.elec;
  const elecHour = now.getHours();
  const elecQuiet = elecHour >= 23 || elecHour < 6;
  const elecShowCache = !!snap?.elec?.stale || (elecQuiet && elecSyncFail && elecCached);
  const elecWarn = elecCached && (snap?.elec?.remainKwh ?? 999) < (settings?.elecWarn ?? 10);
  const preview = homeSpend.length ? homeSpend : snap?.cardSpend || [];
  const previewMax = Math.max(...preview.map((d) => d.spend), 1);
  const today = ymd(now);

  return (
    <div className="stack">
      <div className="row-between">
        <h1 className="page-title">主页</h1>
        <div className="row-between">
          <span className="hint">
            {offline
              ? "离线状态"
              : loggedIn
                ? day.weekNow
                  ? `第 ${day.weekNow} 周 · 周${WEEKDAY_SHORT[day.weekdayNow - 1] || ""}`
                  : snap?.semester
                    ? `周${WEEKDAY_SHORT[day.weekdayNow - 1] || ""}`
                    : "已登录"
                : ""}
          </span>
          <Button
            appearance="subtle"
            className={refreshUi === "spin" ? "refresh-btn spinning" : refreshUi === "ok" ? "refresh-btn done" : "refresh-btn"}
            icon={refreshUi === "ok"
              ? <Checkmark20Regular className="refresh-icon refreshed" />
              : <ArrowSync20Regular className={refreshUi === "spin" ? "refresh-icon spinning" : "refresh-icon"} />}
            disabled={refreshUi === "spin"}
            onClick={props.onRefresh}
          >
            {refreshUi === "spin" ? "同步中" : refreshUi === "ok" ? "已同步" : "刷新"}
          </Button>
        </div>
      </div>

      {offline ? (
        <MessageBar intent="warning">
          <MessageBarBody>离线状态 · 显示上次缓存</MessageBarBody>
        </MessageBar>
      ) : null}

      <div className="cards">
        <Card className={!loggedIn ? undefined : cardWarn ? "warn-card" : undefined}>
          <button type="button" className="card-hit" onClick={() => (loggedIn ? props.onOpenBills("card") : props.onRequestLogin())}>
            <div className="card-k">校园卡</div>
            {loggedIn ? (
              <>
            <div className="card-v num">{snap?.card ? fmtMoney(snap.card.balance) : "—"}</div>
            <div className={`card-u ${cardWarn ? "warn" : ""}`}>元{snap?.errors.card ? " · 同步失败" : ""}</div>
            <div className="mini">
              {preview.filter((d) => d.date <= today).slice(-31).map((d) => {
                const h = d.spend ? Math.max(3, Math.round((d.spend / previewMax) * 16)) : 0;
                return <i key={d.date} className={d.date === today ? "today" : ""} style={{ height: h }} />;
              })}
            </div>
              </>
            ) : (
              <>
                <div className="card-v">登录后可用</div>
                <div className="card-u">校园卡</div>
              </>
            )}
          </button>
        </Card>
        <Card className={!loggedIn ? undefined : netWarn ? "warn-card" : undefined}>
          <button type="button" className="card-hit" onClick={() => (loggedIn ? props.onOpenBills("net") : props.onRequestLogin())}>
            <div className="card-k">网费</div>
            {loggedIn ? (
            <>
            <div className="card-v num">{snap?.net ? fmtMoney(snap.net.balance) : "—"}</div>
            <div className={`card-u ${netWarn ? "warn" : ""}`}>
              {snap?.net && snap.net.balance <= 0 ? "元 · 见底了" : "元"}
              {snap?.errors.net ? (/被拒绝|拦截/.test(snap.errors.net) ? " · 访问被拒" : " · 同步失败") : ""}
            </div>
            <div className="card-u">
              {snap?.net
                ? snap.net.flowFromPortal === false && !snap.net.monthBytes
                  ? "已用 —"
                  : `已用 ${formatBytes(snap.net.monthBytes)}`
                : ""}
            </div>
            </>
            ) : (
              <>
                <div className="card-v">登录后可用</div>
                <div className="card-u">网费</div>
              </>
            )}
          </button>
        </Card>
        <Card className={!loggedIn ? undefined : elecWarn ? "warn-card" : undefined}>
          <div className="card-hit">
            <div className="card-k">电费</div>
            {!loggedIn ? (
              <>
                <div className="card-v">登录后可用</div>
                <div className="card-u">电费</div>
              </>
            ) : elecCached ? (
              <>
                <div className="card-v num">{fmtElec(snap?.elec?.remainKwh || 0)}</div>
                <div className={`card-u ${elecWarn || (elecSyncFail && !elecShowCache) ? "warn" : ""}`}>
                  度{elecWarn ? " · 偏低" : ""}
                  {elecShowCache ? " · 缓存" : ""}
                  {!elecShowCache && elecSyncFail ? " · 同步失败" : ""}
                </div>
              </>
            ) : elecSyncFail ? (
              <>
                <div className="card-v">—</div>
                <div className="card-u warn">同步失败</div>
              </>
            ) : snap?.elec?.unbound || !snap?.elec ? (
              <>
                <div className="card-v">未绑定</div>
                <div className="card-u">去设置填房间码</div>
              </>
            ) : (
              <>
                <div className="card-v">—</div>
                <div className="card-u">度</div>
              </>
            )}
          </div>
        </Card>
      </div>

      <div className="row-between">
        <strong>今日</strong>
        <div>
          <Button appearance="subtle" icon={<Add20Regular />} onClick={() => props.onOpenEvent(null)}>添加日程</Button>
          <Button appearance="subtle" onClick={props.onOpenSchedule}>日程</Button>
        </div>
      </div>

      {!loggedIn ? (
        <DayLists day={day} courses={false} onOpenEvent={props.onOpenEvent} />
      ) : !snap?.semester && snap?.errors.course ? (
        <div>
          <p className="hint warn">
            {snap.network === "offcampus" || /校外|超时/.test(snap.errors.course)
              ? "校外连不上教务，连校园网后再试。"
              : "出了洞，教务不理人"}
          </p>
          <Button appearance="subtle" disabled={courseSync === "spin"} onClick={props.onSyncCourses}>重试</Button>
        </div>
      ) : courseSync === "spin" && !snap?.semester ? (
        <div className="empty">正在同步课表…</div>
      ) : !snap?.semester && day.todayMix.length === 0 ? (
        <div className="empty">教务还没有本学期课表。军训结束、选课落定后再刷新。</div>
      ) : snap?.semester && snap.semester.courses.length === 0 && day.todayMix.length === 0 && day.tomorrowCourses.length === 0 ? (
        <div className="empty">教务还没有本学期课表。军训结束、选课落定后再刷新。</div>
      ) : (
        <DayLists day={day} onOpenEvent={props.onOpenEvent} />
      )}

      <div className="foot">
        {offline ? "离线状态" : snap?.fetchedAt ? new Date(snap.fetchedAt).toLocaleTimeString() + " 同步" : "尚未同步"}
        {offline && snap?.fetchedAt ? ` · 上次 ${new Date(snap.fetchedAt).toLocaleTimeString()}` : ""}
        {clockOverridden ? " · 测试时钟" : ""}
        {!offline && snap?.network === "campus" ? " · 校园网直连" : !offline && snap?.network === "offcampus" ? " · 校外" : ""}
        {err ? ` · ${err}` : ""}
      </div>
    </div>
  );
}
