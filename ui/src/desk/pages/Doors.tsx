import { useRef, useState, type ReactNode } from "react";
import {
  BookOpen20Regular,
  Building20Regular,
  Calculator20Regular,
  HatGraduation20Regular,
  Library20Regular,
  Mail20Regular,
  PeopleCommunity20Regular,
  Star20Regular,
  Wifi120Regular,
} from "@fluentui/react-icons";
import { api } from "../../api";

type Policy = "direct" | "intranet" | "campus-only";

type Door = {
  id: string;
  name: string;
  tag: string;
  url: string;
  policy: Policy;
  icon: ReactNode;
};

const SCHOOL: Door[] = [
  { id: "ipgw", name: "校园网网关", tag: "仅校园网", url: "https://ipgw.neu.edu.cn/", policy: "campus-only", icon: <Wifi120Regular /> },
  { id: "mail", name: "校园邮箱", tag: "随网络", url: "https://mails.neu.edu.cn/", policy: "intranet", icon: <Mail20Regular /> },
  { id: "jwxt", name: "教务系统", tag: "随网络", url: "https://jwxt.neu.edu.cn/jwapp/sys/homeapp/index.do", policy: "intranet", icon: <HatGraduation20Regular /> },
  { id: "cnki", name: "知网", tag: "校内渠道", url: "https://www.cnki.net/", policy: "intranet", icon: <Library20Regular /> },
  { id: "portal", name: "一号通门户", tag: "随网络", url: "https://portal.neu.edu.cn/tp_up/", policy: "intranet", icon: <Building20Regular /> },
];

const OTHER: Door[] = [
  { id: "xiaozhan", name: "neu小站", tag: "直接打开", url: "https://www.xn--xhq44jb2fzpc.com/", policy: "direct", icon: <PeopleCommunity20Regular /> },
  { id: "math", name: "数学机考刷题", tag: "直接打开", url: "https://www.neumathe.cn/", policy: "direct", icon: <Calculator20Regular /> },
  { id: "sizheng", name: "思政刷题", tag: "直接打开", url: "https://www.momofish.top/", policy: "direct", icon: <BookOpen20Regular /> },
  { id: "star", name: "为本项目点 star", tag: "GitHub", url: "https://github.com/sapphire-xu/nidong", policy: "direct", icon: <Star20Regular /> },
];

function DoorList(props: { items: Door[]; busy: string; onOpen: (door: Door) => void }) {
  return (
    <div className="door-list">
      {props.items.map((door) => {
        const opening = props.busy === door.id;
        return (
          <button
            key={door.id}
            type="button"
            className={opening ? "door-row opening" : "door-row"}
            aria-busy={opening}
            onClick={() => {
              if (props.busy) return;
              props.onOpen(door);
            }}
          >
            <span className="door-icon" aria-hidden="true">{door.icon}</span>
            <span className="door-name">{door.name}</span>
            <span className="door-tag">
              {opening ? <i className="status-spin" aria-hidden="true" /> : null}
              {opening ? "正在打开" : door.tag}
            </span>
          </button>
        );
      })}
    </div>
  );
}

const OPEN_HOLD_MS = 720;

function wait(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

export function Doors() {
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState("");
  const busyRef = useRef(false);

  async function open(door: Door) {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(door.id);
    setErr("");
    const started = performance.now();
    let failed = "";
    try {
      await api.openDoor(door.url, door.policy);
    } catch (e) {
      failed = String(e).replace(/^Error: /, "");
    } finally {
      const left = OPEN_HOLD_MS - (performance.now() - started);
      if (left > 0) await wait(left);
      if (failed) setErr(failed);
      busyRef.current = false;
      setBusy("");
    }
  }

  return (
    <div className="stack">
      <div>
        <h1 className="page-title">任意门</h1>
        <p className="hint">
          学校站点在校园网里直连。人在校外时，改走 webvpn.neu.edu.cn 再打开。网关只能在校园网里用。
        </p>
      </div>
      {err ? <p className="hint warn">{err}</p> : null}
      <div className="door-kicker">学校</div>
      <DoorList items={SCHOOL} busy={busy} onOpen={open} />
      <div className="door-kicker">其他</div>
      <DoorList items={OTHER} busy={busy} onOpen={open} />
    </div>
  );
}
