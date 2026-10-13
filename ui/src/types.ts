export type Bill = {
  time: string;
  subject: string;
  amount: number;
  fee?: number;
  balanceAfter: number;
  station?: string;
};

export type DaySpend = {
  date: string;
  spend: number;
  recharge: number;
  count: number;
  bills: Bill[];
};

export type Snapshot = {
  fetchedAt: string;
  network: string;
  loggedIn: boolish;
  username?: string;
  card?: { balance: number; subsidy: number; status: string };
  cardBills?: Bill[];
  cardSpend?: DaySpend[];
  cardMonth?: string;
  billsTruncated?: boolean;
  net?: {
    balance: number;
    monthBytes: number;
    monthSeconds: number;
    monthTimes: number;
    /** 门户 JSON 是否真正给出了 sum_bytes。缺省当 true，避免旧缓存被当成失败。 */
    flowFromPortal?: boolean;
  };
  elec?: {
    unbound: boolean;
    remainKwh?: number;
    roomNo?: string;
    roomLabel?: string;
    lastReadAt?: string;
    /** 23:00–06:00 没连上电费系统，度数是上次成功的读数。 */
    stale?: boolean;
  };
  coursesToday?: Course[];
  semester?: SemesterTable;
  teachingWeek?: number;
  offline?: boolean;
  agenda?: Agenda[];
  errors: Partial<Record<"card" | "net" | "elec" | "course" | "auth", string>>;
};

export type Agenda = {
  id: string;
  name: string;
  note?: string;
  /** 本地 `YYYY-MM-DDTHH:MM` */
  start: string;
  end: string;
  remind: boolean;
  remindMinutes?: number;
  location?: string;
  /** 1、2、15、3、4、5、6、7。 */
  remindCode?: string;
  /** 代码 7：前一天的 HH:MM。 */
  setTime?: string;
  /** 重复只存这一条。 */
  isRepet?: boolean;
  repetRule?: RepetRule;
  /** 仅展示：这一次发生的起止。不写入 agenda.json。 */
  occStart?: string;
  occEnd?: string;
};

export type RepetFreq = "DAILY" | "WEEKLY";

export type RepetRule = {
  freq: RepetFreq;
  byday?: string[];
  endmode: "DATE" | "NUMBER";
  /** `DATE` 时的 `YYYY-MM-DD`。 */
  time?: string;
  num?: number;
};

export type Course = {
  name: string;
  place: string;
  teacher?: string;
  start: string;
  end: string;
  weekday: number;
  weeks?: number[];
  sections?: [number, number];
  campus?: "nanhu" | "hunnan" | string;
};

export type TermWeek = {
  n: number;
  start: string;
  end: string;
};

export type SemesterTable = {
  semesterId: string;
  label: string;
  week1Monday: string;
  fetchedAt: string;
  courses: Course[];
  termWeeks?: TermWeek[];
};

type boolish = boolean;

export type AppSettings = {
  roomNo: string;
  cardWarn: number;
  netWarn: number;
  elecWarn: number;
  theme: string;
  autostart: boolean;
  rememberPassword?: boolean;
  autoLogin?: boolean;
  lastUsername?: string;
  islandX?: number | null;
  islandY?: number | null;
  week1Monday?: string;
  autoHide?: boolean;
  autoHideHotkey?: boolean;
  autoHideHover?: boolean;
  revealHotkey?: string;
  classRemind?: boolean;
  classRemindMinutes?: number;
  remindSound?: boolean;
  /** 0–100。旧配置没有此字段时按 100。 */
  remindVolume?: number;
  campusWifiMode?: "off" | "offline" | "boot" | "hotspot" | "manual";
  campusWifiSsid?: string;
  campusHotspotSsid?: string;
  skipLogin?: boolean;
  shareHintDismissed?: boolean;
};

export type SavedCreds = {
  username: string;
  rememberPassword: boolean;
  autoLogin: boolean;
};

export type LoginOutcome =
  | { status: "ok" }
  | { status: "need_sms"; phoneMask: string; captchaPng: string };
