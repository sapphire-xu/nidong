# 泥洞 · 给后续 AI / 实现者

读这份再改代码。学校接口细节只在 [开发文档.md](开发文档.md)；颜色、尺寸、文案气质只在 [视觉设计.md](视觉设计.md)。不要把那两份再抄一遍到这里。

## 这是什么

东北大学私人桌面看板。无边框透明窗贴在屏幕上沿，平时一条 40px 高的岛，悬停或点一下向下展开。看校园卡、网费、电费、今日课表和本机日程；点校园卡看月花费柱和流水；点「日程」看周历（含课表与日程）。

产品名是「泥洞」（东大戏称），开源仓库 `nidong`（https://github.com/sapphire-xu/nidong）。包 id：`edu.neu.nidong`。不是学校官方应用。

只跑在用户自己的电脑上。密码进 OS 凭据管理，cookie / 课表 / 账单 / 日程缓存在 `%APPDATA%\edu.neu.nidong`。不要把账号送到任何自建服务器。

## 技术栈（当前）

- 壳：Tauri 2 + Rust
- UI：React 18 + TypeScript，入口 `ui/src/`，Vite 开发服端口 1420
- 抓数：全部在 Rust `reqwest`，UI 只 `invoke` / 听事件
- 启动：仓库根目录 `启动.bat` 或 `npm start`（即 `tauri dev`）

## 仓库实际结构

```
nidong/
  ui/src/
    App.tsx          岛的状态机：展开/收起/隐藏/提醒/钉住/叠层
    Settings.tsx     设置：先分类页再进小类；退出登录在分类页
    EventForm.tsx    添加/编辑日程叠层
    course.ts        教学周、今日课、收起态文案（纯前端推导）
    agenda.ts        日程按日筛选、与课表混排
    api.ts           invoke 封装 + 改窗口高度
    types.ts         与 Rust camelCase JSON 对齐
    styles.css       视觉 token 与组件
  src-tauri/src/
    lib.rs           启动、托盘、命令、定时刷新入口
    client.rs        CAS / 卡 / 网 / 电 / 账单
    course.rs        教务 JSON → SemesterTable
    agenda.rs        本机日程 agenda.json
    clock.rs         可覆盖的“现在”（dev-now.txt / NIDONG_NOW）
    hotkey.rs        全局和弦快捷键 + 悬停热区（Windows 钩子）
    models.rs        Snapshot / AppSettings / Course / AgendaItem
    crypto.rs        一号通 RSA
    ocr.rs           图形码本机识别（接口已留，模型未嵌）
    periods.rs       南湖/浑南作息
    util.rs / webvpn.rs
  docs/              开发文档、视觉设计、本文、prototype
  scripts/           set-now.bat / clear-now.bat 测时钟
```

旧开发文档里的 `auth/cas.rs`、`fetch/card.rs` 分包还没拆，抓数都在 `client.rs`。

## 窗口状态（改交互先看这个）

一个 WebView，不要再开系统模态窗。登录、短信、账单、网费说明、关于、日程（原学期周历）、添加日程、设置都是岛内 `view` 叠层。

| 状态 | 窗口 | 谁说了算 |
|---|---|---|
| 展开 | 宽 420，高随面板 ≤720 | `expanded`；钉住或鼠标在岛上 |
| 折叠 | 高 40，贴顶时上沿直角 | 离开 2s 且未钉住；Esc 随时收起（不改钉住） |
| 隐藏 | `window.hide()` | 自动隐藏开着，折叠后再过 3s |
| 上课提醒 | 强制折叠 40px 置顶 10s | 到点；不受隐藏影响 |

单实例：同一时间只跑一个泥洞。已经在跑时再点安装目录 / 开机自启 / 快捷方式，不新开窗口，而是把已有的岛喊出来并展开钉住（和托盘「展开面板」一样）。

钉住规则（当前实现）：

- 点击折叠岛、托盘「展开面板」：展开并钉住
- 登录 / 短信 / 账单 / 网费说明 / 关于 / 学期（日程）/ 添加日程：打开时钉住，关掉后恢复进入前的钉住
- **设置不默认钉住**。鼠标离开仍会收起；要盯着改就自己点钉
- 快捷键：默认 **Ctrl+Q**；两键在约 400ms 内同时按下才触发；**不改变钉住**。松开后若未钉住、鼠标也不在岛上，则收起
- 悬停展开（自动隐藏后）：展开但不钉住
- Esc：有叠层先关叠层；否则收起，**不改变钉住**

自动隐藏：设置 → 面板行为。隐藏后用快捷键或托盘唤出；若开了悬停展开，Rust 按岛的屏幕坐标扫一块大约 420×80（再加边）的热区，停满 1s 展开。上课提醒弹出时会把岛喊回来，10s 后按原状态藏回去。

独占全屏（D3D Exclusive）：普通置顶窗无法盖住。检测到 `QUNS_RUNNING_D3D_FULL_SCREEN` 时把岛藏起，退出全屏再显示。无边框窗口化游戏可以被置顶岛盖住。不要做注入/作弊覆盖。

位置：只拖，不设。默认水平居中、**y = 0 贴屏幕顶**（不要再留 8px）。左右不能拖出当前屏。托盘「复位岛」清坐标并贴顶。`y ≤ 8` 会当成 0。

## 数据流

1. UI `bootstrap` / `refresh` → Rust 组 `Snapshot`（camelCase JSON）
2. 定时刷新：卡/网/电大约 20 分钟；**课表不参与定时刷新**。打开校园卡账单时暂停拉账单，避免图和选中日被冲掉
3. 今日课、教学周、收起态「还有多久」都从 `semester` + `clock::now()` 推，不要再打教务
4. 课表真源是 `semester.json`。点设置里「重新同步」才 `sync_courses`

`clock::now()` 在有 `dev-now.txt` 或环境变量 `NIDONG_NOW` 时用文件时间（冻结），否则用系统时间。测上课提醒：开提醒，用 `scripts/set-now.bat` 拨到「开课时间 − N 分钟」。10 秒收回走墙钟。测完 `clear-now.bat`。设置里也有「预览动效」。

## 离线

探测：`util::online()`（国内 DNS IP、`pass.neu.edu.cn`、校园网）。没网 ≠ 会话过期。

`session.open` 只在登录成功时写入，只在用户点「退出登录」时删掉。有这份文件就是「上次没退出」。旧版没有该文件时：snapshot 里有学号且还留着 `cookies.json`，启动时补写一份。

离线且 `session.open`：

- `logged_in = true`，`offline = true`，`errors.auth` 清空
- 直接显示 snapshot / semester / agenda 缓存
- **不要**提示重新登录，不要走 CAS
- UI 顶栏和底栏写「离线状态」

在线但 TGT 真过期：仍提示重新登录（现有逻辑）。刷新在没网时返回缓存，不要把卡/网数字打成失败。

## 日程

真源 `agenda.json`，不进 `snapshot.json`。字段：名称、开始/结束（本地 `YYYY-MM-DDTHH:MM`，必须有、结束必须晚于开始）、是否提醒、提前分钟、备注。主面板「今日」右侧加号添加；点一行日程编辑/删除。点「日程 ›」进原学期周历：课表格子不变，有日程的星期点赭金点，表下「本周日程」。今日课全部下课后，主面板不再列今日课程，改「今日课程已上完」+ 明日课。日程提醒不跟「上课提醒」开关，条目自己勾选。退出登录会清 `agenda.json`。

## 改功能时去哪

| 你想动 | 文件 |
|---|---|
| 展开/收起/隐藏/钉住/提醒时机 | `ui/src/App.tsx`，必要时 `hotkey.rs` |
| 设置项、分类、退出登录、课表同步中/完成 | `ui/src/Settings.tsx` + `AppSettings`（`models.rs` 与 `types.ts` 一起加，新字段必须 `serde(default)`） |
| 今日课、周历格子、收起文案、课上完后的明日 | `ui/src/course.ts`（作息表与 Rust `periods.rs` 保持一致） |
| 自定义日程 | `ui/src/EventForm.tsx` + `agenda.ts` + `src-tauri/src/agenda.rs` |
| 登录、卡、网、电、账单 | `src-tauri/src/client.rs` |
| 网费计费说明叠层 | `ui/src/App.tsx` 的 `view=net` |
| 教务课表解析 | `src-tauri/src/course.rs`（规则见下） |
| 托盘、命令、刷新编排 | `src-tauri/src/lib.rs` |
| 颜色/圆角/动效 | `ui/src/styles.css`，先对视觉设计 token |
| 学校 URL / 字段 | 只许按开发文档里**已实测**的接口写，不要猜门户 HTML |

加设置项：Rust `AppSettings` + Default + `#[serde(default)]` → TS `AppSettings` → `Settings.tsx` 某一类里。旧 `settings.json` 缺字段必须还能加载，否则会整份回落到 Default、用户设置被清空。

## 课表解析（改 `course.rs` / `client.rs` 先看）

东大 `wdkb`/`SKZC` 未开通（校园网 2026-09-20 打 `cxxszhxqkb.do` 得 `{"code":"404"}`）。网页周课表走的是：

`getMyScheduleDetail.do?termCode=&type=week&week=N`

服务端按教学周筛格子。同步时（仅此时）按周拉齐，用点名写 `weeks[]`；日常刷新不打教务。`week=` 若被忽略，回退中文：格子全文优先，单/双只修饰紧挨着的那段，没写教室的段不要继承 `placeName`。

今日/周历 = `weeks[]` ∩ 星期 ∩ 节次。官方周次用 `getTermWeeks`。细节见开发文档 §7.5.2。

## 电费系统维护

每天 **0:00–6:00** 视为电费系统维护：刷新不打电费接口，沿用缓存余量，**不要**写「缓存」或「同步失败」。其余时段失败：有缓存仍显示度数并标「同步失败」；没有缓存才只显示「同步失败」。不要把读不到的余量写成 0。房间码改了才丢掉旧房间缓存。

## 登录同步 vs 刷新

两套入口都进 `refresh_inner`，不要另写一套登录抓数。差别只在登录刚结束时 WebVPN 还没种上。校外必须先 `prepare_access()`（一号通换 WebVPN 票，失败再试一次），再并行拉卡/网/电/课；账单等卡的 SSO 回来再拉。不要把卡和 WebVPN 换票塞进同一个 22 秒超时里，否则登录后会误报「校外连不上」，手动刷新（票已经有了）却成功。

## 网费「已用 0B」

余额来自缴费大厅 `queryNetAccBalance`，流量来自门户 `getWlzzList` 的 `sum_bytes`。CAS 换票后要再 GET 门户 `view?m=up` 才种得上会话。门户种不上（含门户页写「统一身份认证」被误判成登录页）时，**不要**把整次网费打成「会话已失效，请重新登录」——卡/电/课还活着；余额仍走缴费大厅，流量 `flowFromPortal=false`。解析不到 `sum_bytes` 则刷新沿用上次流量；从未成功过则 UI 写「已用 —」。一号通 HTML 出现「访问被拒绝」要单独提示换网络，不要说成密码错。

## 不要做

- 不要为设置再开系统窗口
- 不要在设置里放位置选项
- 不要手选南湖/浑南；作息跟课的 `campus`
- 不要定时打教务
- 不要把密码/短信写进普通 json。关掉「记住密码」必须 `keyring_delete`，不要只改开关
- 不要让 Esc 改钉住；Esc 只关叠层或收起
- 不要在没网时把「会话已失效，请重新登录」盖在缓存上；没退出登录就继续显示缓存并标「离线状态」
- 不要用 `input type="date"` 填日程日期（和第一周周一同一原因）
- 不要在钩子回调里直接 `show` 窗口（`hotkey.rs` 已丢到主线程）
- 不要用 `input type="date"` 填第一周周一（中文 Windows 会显示成 yyyy/mm/日）；用年/月/日三格
- 抓数失败时不要用 0 覆盖上次的真实流量/余额，除非接口明确返回 0

## 托盘（右键通知区图标）

展开面板（钉住，无论是否隐藏）→ 刷新 → 复位岛（贴顶居中）→ 退出

## 打包与开源

Windows 安装包：`npx tauri build`，NSIS 简体中文，产物 `src-tauri/target/release/bundle/nsis/泥洞_<version>_x64-setup.exe`。图标用 BMP-in-ICO（16/24/32/48/64/256），`installerIcon` 指向 `icons/icon.ico`。

开源仓库：https://github.com/sapphire-xu/nidong 。安装包放 GitHub Release，不要把 `target/`、`tmp/`、cookie、snapshot 推进 git。用户向 README 写功能和使用方法；实现细节仍以本文为准。
