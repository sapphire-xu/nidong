# 泥洞 · 给后续 AI / 实现者

读这份再改代码。学校接口细节只在 [开发文档.md](开发文档.md)；颜色、尺寸、文案气质只在 [视觉设计.md](视觉设计.md)。不要把那两份再抄一遍到这里。

和代码冲突时，以代码和本文为准。下面两份不要当现行规格：

- [界面改造计划.md](界面改造计划.md)：2026-09-26 的双窗口改造已经做完
- [prototype/](prototype/index.html)：旧的「岛向下展开」，只留着对照

## 搜代码之前先看这张表

这些说法还留在旧段落里。照着改会把已经做对的行为改回去。

| 过时说法 | 现行 |
|---|---|
| 岛悬停展开、`set_expanded`、`island-expand` | 岛永远 40px。点岛打开 `desk` |
| `open_desk` 里用 `set_focus` | `hotkey::focus_without_menu`。`set_focus` 失败会注入 Alt，连点把消息循环卡死，退出码 `0xcfffffff` |
| 双击岛或双击 Logo 复位到屏幕中央 | 复位只在托盘。连点只把窗口提到前面 |
| 左侧五项导航 | 七项：主页、日程、账单、工具箱、任意门、设置、关于 |
| 岛上有齿轮 | 设置在窗口左侧。表单在 `ui/src/Settings.tsx` |
| 课表不参与定时刷新、不每天打教务 | 每天自动打一次教务，见「数据流」 |
| 0:00–6:00 不查电费 | 那是校园网免费时段（账单页文案）。电费是 23:00–6:00 仍查，失败留缓存 |
| 默认房间码 `3510409` | 那是 2026-09-08 的实测例子。`AppSettings.room_no` 默认空字符串 |
| 按 `prototype/index.html` 换皮 | 窗口是 Fluent。原型不要当视觉源 |

## 这是什么

东北大学私人桌面看板。贴顶的一条 40px 岛负责下一节课、倒计时和余额警告。点岛打开带系统标题栏的 `desk` 窗口：主页、日程、账单、工具箱、任意门、设置、关于。未登录时窗口里先是登录。

产品名是「泥洞」（东大戏称），开源仓库 `nidong`（https://github.com/sapphire-xu/nidong）。包 id：`edu.neu.nidong`。不是学校官方应用。不适用于秦皇岛分校。

只跑在用户自己的电脑上。密码进 OS 凭据管理，cookie / 课表 / 账单 / 日程缓存在 `%APPDATA%\edu.neu.nidong`。不要把账号送到任何自建服务器。没要求发布就不要改版本号（当前 2.1.0）。

## 技术栈（当前）

- 壳：Tauri 2 + Rust
- UI：React 18 + TypeScript，入口 `ui/src/`，Vite 开发服端口 1420
- 抓数：全部在 Rust `reqwest`，UI 只 `invoke` / 听事件
- 启动：仓库根目录 `启动.bat` 或 `npm start`（即 `tauri dev`）
- 系统浏览器：`tauri-plugin-opener` 2.5。能力 `opener:default` 在 `src-tauri/capabilities/default.json`

## 仓库实际结构

```
nidong/
  ui/index.html      岛
  ui/desk.html       工作区窗口
  ui/src/
    App.tsx          岛：拖动、自动隐藏、上课提醒。点一下调用 open_desk
    dayModel.ts      今日/明日列表、岛上收起文案
    desk/            窗口：Fluent 导航和各页
      pages/Settings.tsx   设置页外壳，内容转给 SettingsPanel
      pages/Doors.tsx      任意门
      pages/Home.tsx       主页
      pages/Toolbox.tsx    工具箱。校园网自动登录在这一页
      updateCheck.ts      向 Gitee 检查新版本，失败只返回 failed
    Settings.tsx     设置表单 SettingsPanel：六类；退出登录在底部
    EventForm.tsx    添加/编辑日程，Fluent Dialog，日期是年/月/日三格
    week.ts          时钟周视图的纯布局，不读窗口
    course.ts        教学周、今日课、岛上文案（纯前端推导）
    agenda.ts        日程按日筛选、与课表混排
    api.ts           invoke 封装
    types.ts         与 Rust camelCase JSON 对齐
    styles.css       岛的外观。窗口不走这份
  src-tauri/src/
    lib.rs           启动、托盘、命令、定时刷新、open_desk、open_door
    campus.rs        校园网五种模式的监视循环，提示发 `campus-hint`
    wlan.rs          Windows wlanapi，按配置名连接
    ipgw.rs          网关登录。单独客户端，不打开浏览器
    client.rs        CAS / 卡 / 网 / 电 / 账单
    course.rs        教务 JSON → SemesterTable，以及该不该打教务
    agenda.rs        本机日程 agenda.json
    clock.rs         可覆盖的“现在”（dev-now.txt / NIDONG_NOW）
    hotkey.rs        全局和弦快捷键、悬停热区、focus_without_menu
    models.rs        Snapshot / AppSettings / Course / AgendaItem
    crypto.rs        一号通 RSA
    periods.rs       南湖/浑南作息
    util.rs / webvpn.rs
  docs/              开发文档、视觉设计、本文、prototype
  scripts/           set-now.bat / clear-now.bat 测时钟；release.bat 打带签名的安装包
  updates/latest.example.json   更新清单示例。不要改名成 latest.json 拿去当线上文件
```

旧开发文档里的 `auth/cas.rs`、`fetch/card.rs` 分包还没拆，抓数都在 `client.rs`。

## 窗口（改交互先看这个）

两个 WebView，不共享 React 状态。数据仍只走现有 Rust 命令和事件，两个页面各自 `bootstrap`。

| 窗口 | 标签 | 几何 |
|---|---|---|
| 岛 | `main` | 420×40，无边框、透明、置顶、不进任务栏、`focus: false` |
| 工作区 | `desk` | 系统标题栏，不透明，进任务栏，可缩放，不置顶。初始 1100×720，最小 880×560，第一次打开时在岛所在屏幕的工作区居中。云母试过：透明窗口把标题栏按钮弄没了，已退回 Fluent 不透明底 |

岛不展开。没有钉住，没有 `set_expanded`，没有 `island-expand`。点岛（指针位移小于 6px）调用 `open_desk`：显示、取消最小化、把窗口带到前面。窗口已经可见时再点，同样只是把它提到前面，不关闭。拖动仍只移动岛。

连点曾经把岛跳回中央并卡死（未响应，退出码 `0xcfffffff`）。现行约束：

- 岛上没有双击复位。复位只留托盘 `reset_island`
- `open_desk_on_ui` 用 `desk_open_lock.try_lock`。`SetForegroundWindow` 会泵消息；阻塞等同一把锁会死锁。`WouldBlock` 直接返回
- 前置用 `focus_without_menu`（`SetWindowPos(HWND_TOP)` + `SetForegroundWindow`），不要 `set_focus`
- 岛的 `WM_NCHITTEST` 返回 `HTCLIENT`，避免系统把拖动或最小化按钮做成模态循环
- 自己调用 `set_position` 时用 `move_echo` 吞掉紧接着的那次 `Moved`。只在位移超过 2 逻辑像素时才纠正，否则 DPI 抖动会来回 `set_position`
- 不要用 WebView 的 `startDragging`
- `DESK_PLACED`：每个进程只把 `desk` 居中一次，之后记住用户拖过的位置

关掉 `desk` 时拦截关闭，改成隐藏。进程和岛继续跑。下次 `open_desk` 把同一个窗口叫回来，页面停在离开时的位置。

| 动作 | 结果 |
|---|---|
| 悬停在可见的岛上 | 岛保持 40px，不打开窗口 |
| 自动隐藏后悬停约 1 秒 | 只把岛叫出来。热区是岛的真实外接矩形，每边加 24 物理像素 |
| 快捷键（默认 Ctrl+Q） | 只把岛叫出来。松开不收起、不打开窗口。3 秒自动隐藏照旧 |
| 托盘「打开窗口」 | 与点岛相同 |
| 再次启动已在运行的实例 | 叫出岛，并打开或前置 `desk` |
| 托盘「复位岛」 | 岛贴顶居中，高度保持 40。不关 `desk` |
| Esc | 岛上没有面板。窗口里只关 Fluent 对话框 |
| 上课提醒 | 岛上 40px 置顶约 10 秒。点这下岛会打开窗口 |
| 独占全屏 | 只藏岛。`desk` 保持原样 |

未登录时 `desk` 只显示登录，短信验证是登录的下一步，不进导航。离线且上次没退出：直接进主页，信息条写「离线状态」。在线但会话失效：进登录。

`save_settings` 成功后向两个窗口发 `settings-updated`。日程的增删和手动刷新会发 `snapshot-updated`，岛上的提醒才看得到新日程。

自动隐藏：设置 → 面板行为。字段 `autoHideHover` 还在，文案是「悬停唤出」。上课提醒弹出时会把岛喊回来，10 秒后若原先是隐藏的，再藏回去。

独占全屏（D3D Exclusive）：普通置顶窗无法盖住。检测到 `QUNS_RUNNING_D3D_FULL_SCREEN` 时把岛藏起，退出全屏再显示。无边框窗口化游戏可以被置顶岛盖住。不要做注入/作弊覆盖。

位置：只拖岛，不设。默认水平居中、**y = 0 贴屏幕顶**（不要再留 8px）。左右不能拖出当前屏。托盘「复位岛」清坐标并贴顶。`y ≤ 8` 会当成 0。

## 数据流

1. UI `bootstrap` / `refresh` → Rust 组 `Snapshot`（camelCase JSON）
2. `lib.rs` 的 setup 里有 20 分钟循环，调用 `refresh_inner(retry_courses: false)`，只发 `snapshot-updated`。登录后的 `spawn_refresh`、`resume_session`、主页刷新和托盘「刷新」传 `true`
3. `refresh_gate`（`AsyncMutex`）把刷新串成一次一次，避免重叠打学校
4. `refresh_inner` 是否打教务由 `course::should_fetch_courses` 决定：
   - 没有 `semester` 缓存，或 `semester_cache_stale`（有第一周周一则今天超过该日 140 天，否则超过 `fetched_at` 120 天）→ 每次刷新都试，包括 20 分钟那轮
   - `fetched_local_date(fetched_at)` 等于今天 → `refresh_inner` 不打。这个日期只取 RFC3339 里的年月日数字，不换时区
   - 其余：自动任务若 `course_attempt_day` 已是今天就不再试；`retry_courses = true` 还可以再试。尝试日记在「有缓存、学期没过期、今天还没成功」时写入
   - 课表超时 90 秒，和卡/网/电在同一次 `tokio::join` 里
5. 设置「重新同步」、主页和日程页的「重试」走命令 `sync_courses`。在线就打教务，不受上面的每日闸限制；离线直接返回「当前离线，连上网络后再同步」
6. 今日课、教学周、收起态「还有多久」都从 `semester` + `clock::now()` 推
7. 课表真源是 `semester.json`

`clock::now()` 在有 `dev-now.txt` 或环境变量 `NIDONG_NOW` 时用文件时间（冻结），否则用系统时间。测上课提醒：开提醒，用 `scripts/set-now.bat` 拨到「开课时间 − N 分钟」。10 秒收回走墙钟。测完 `clear-now.bat`。设置里也有「预览动效」。

## 主页课程和明日

逻辑在 `ui/src/dayModel.ts`，画在 `desk/pages/Home.tsx`。

一行是三列：`7.5rem`（时间段和状态）、`minmax(0, 1fr)`（课名）、`auto`（教师和地点，右对齐，最宽 16rem）。最小窗口 880px，不要改回左右分栏，也不要把课名和地点都堆在左侧。

- 今天还有课：列今日课和今日日程
- 今日课全部下课：写「今日课程已上完」，下面留还没结束的今日日程，再列明日
- 今天一门课都没有、明天有课或日程：写「今天没有课」，今日日程仍留在今天，再列明日
- 岛上：正在上或下一节优先。今天没课且明天有课，中间是明天那门课的名字，辅文案「明天 HH:MM」。今天已经上完且明天有课，中间仍是「今天课上完了」，辅文案换成明天的开始时间

空学期文案仍是「教务还没有本学期课表。军训结束、选课落定后再刷新。」

## 任意门

页在 `ui/src/desk/pages/Doors.tsx`，命令 `open_door(url, policy)`。用系统浏览器打开，应用自己的 cookie 带不过去。校外的 WebVPN 链接仍要用户在浏览器里登录 webvpn。

校园网探测与抓数相同：`util::on_campus_net` 探 `ecard:80` 或 `jwxt:443`，大约 30 秒缓存。探测放在 `spawn_blocking` 里，避免堵住 UI 线程。

| policy | 行为 |
|---|---|
| `direct` | 原样打开 |
| `intranet` | 校园网用原 URL；校外用 `webvpn::encrypt_url_to_webvpn`。算法以 `webvpn.rs` 为准 |
| `campus-only` | 不在校园网就返回「校园网网关只能在校园网里打开」 |

学校站点：校园网网关 `https://ipgw.neu.edu.cn/`（仅校园网）、校园邮箱 `https://mails.neu.edu.cn/`（新生指南里的学生邮箱；教师邮箱是另一台 `mail.neu.edu.cn`）、教务 `https://jwxt.neu.edu.cn/jwapp/sys/homeapp/index.do`、知网 `https://www.cnki.net/`、一号通门户 `https://portal.neu.edu.cn/tp_up/`。

其他：neu小站 `https://www.xn--xhq44jb2fzpc.com/`（标签「非官方资源共享站」）、NeuMathe `https://www.neumathe.cn/`（标签「数学机考刷题」）、MoMofish `https://www.momofish.top/`（标签「思政刷题」）、本仓库 `https://github.com/sapphire-xu/nidong`。

学校 URL 只许来自开发文档里已实测的地址，或用户明确给过的这几条。不要再猜门户上的链接。

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

真源 `agenda.json`，不进 `snapshot.json`。字段：名称、开始/结束（本地 `YYYY-MM-DDTHH:MM`，必须有、结束必须晚于开始）、是否提醒、提前分钟、备注。主页「今日」旁的加号、日程页页眉的加号，都打开同一个对话框。点日程块或今日列表里的日程行进入编辑。日程页是教学周的时钟周视图，见 `week.ts`：07:30–22:30，课和日程同一条时间轴；相交的块按簇分列；跨天按天切开；完全落在轴外的列在当天列底的「轴外」。日程提醒不跟「上课提醒」开关，条目自己勾选。退出登录会清 `agenda.json`。

## 改功能时去哪

| 你想动 | 文件 |
|---|---|
| 岛的隐藏、提醒、点击打开窗口 | `ui/src/App.tsx`，必要时 `hotkey.rs` |
| 窗口导航和页面（含任意门） | `ui/src/desk/`。任意门列表在 `pages/Doors.tsx`，打开逻辑在 `lib.rs` 的 `open_door` |
| 设置项、分类、退出登录、课表同步中/完成 | `ui/src/Settings.tsx` 的 `SettingsPanel`。页壳是 `desk/pages/Settings.tsx`。`AppSettings` 在 `models.rs` 与 `types.ts` 一起加，新字段必须 `serde(default)` |
| 今日课、岛上文案、课上完或今天没课之后的明日 | `ui/src/course.ts`、`ui/src/dayModel.ts`（作息表与 Rust `periods.rs` 保持一致） |
| 主页课程行的三列 | `desk/pages/Home.tsx`、`desk/desk.css` 的 `.mix-row` |
| 时钟周视图的位置和分列 | `ui/src/week.ts`。`Schedule.tsx` 只负责画 |
| 自定义日程 | `ui/src/EventForm.tsx` + `agenda.ts` + `src-tauri/src/agenda.rs` |
| 登录、卡、网、电、账单 | `src-tauri/src/client.rs` |
| 网费计费说明 | `ui/src/desk/pages/Bills.tsx` 的网费一段。0:00–6:00 不计费是这段文案，和电费夜里缓存不是一回事 |
| 教务课表解析、每天打一次 | `src-tauri/src/course.rs`（规则见下） |
| 托盘、`open_desk`、刷新编排 | `src-tauri/src/lib.rs` |
| 岛的颜色和圆角 | `ui/src/styles.css` |
| 窗口的强调色和页面动效 | `ui/src/desk/theme.ts`、`ui/src/desk/desk.css` |
| 学校 URL / 字段 | 只许按开发文档里**已实测**的接口写，不要猜门户 HTML |

加设置项：Rust `AppSettings` + Default + `#[serde(default)]` → TS `AppSettings` → `Settings.tsx` 某一类里。旧 `settings.json` 缺字段必须还能加载，否则会整份回落到 Default、用户设置被清空。

## 课表解析（改 `course.rs` / `client.rs` 先看）

东大 `wdkb`/`SKZC` 未开通（校园网 2026-09-20 打 `cxxszhxqkb.do` 得 `{"code":"404"}`）。网页周课表走的是：

`getMyScheduleDetail.do?termCode=&type=week&week=N`

服务端按教学周筛格子。一次同步里按周拉齐，用点名写 `weeks[]`。`week=` 若被忽略，回退中文：格子全文优先，单/双只修饰紧挨着的那段，没写教室的段不要继承 `placeName`。

今日/周历 = `weeks[]` ∩ 星期 ∩ 节次。官方周次用 `getTermWeeks`。细节见开发文档 §7.5.2。每天最多自动同步一次，见「数据流」。不要改回「只在登录和重新同步时拉」。

## 电费系统维护

每天 **23:00–6:00**（`elec_night`：`hour >= 23 || hour < 6`）电费系统经常连不上：仍然尝试查询，超时 5 秒（白天 12 秒）。失败且同一房间、`remain_kwh` 还在：`ElecInfo.stale = true`（`serde(default)`），`errors.elec` 清空，主页写「 · 缓存」。白天失败：`stale = false`，有缓存仍显示度数并标「同步失败」；没有缓存才只显示「同步失败」。不要把读不到的余量写成 0。房间码改了才丢掉旧房间缓存。

## 登录同步 vs 刷新

两套入口都进 `refresh_inner`，不要另写一套登录抓数。差别只在登录刚结束时 WebVPN 还没种上。校外必须先 `prepare_access()`（一号通换 WebVPN 票，失败再试一次），再并行拉卡/网/电/课；账单等卡的 SSO 回来再拉。不要把卡和 WebVPN 换票塞进同一个 22 秒超时里，否则登录后会误报「校外连不上」，手动刷新（票已经有了）却成功。

## 网费「已用 0B」

余额来自缴费大厅 `queryNetAccBalance`，流量来自门户 `getWlzzList` 的 `sum_bytes`。CAS 换票后要再 GET 门户 `view?m=up` 才种得上会话。门户种不上（含门户页写「统一身份认证」被误判成登录页）时，**不要**把整次网费打成「会话已失效，请重新登录」——卡/电/课还活着；余额仍走缴费大厅，流量 `flowFromPortal=false`。解析不到 `sum_bytes` 则刷新沿用上次流量；从未成功过则 UI 写「已用 —」。一号通 HTML 出现「访问被拒绝」要单独提示换网络，不要说成密码错。

## 不要做

- 不要把岛拉高变成窗口，也不要在两个系统窗口之间做形变
- 不要自绘标题栏。`desk` 用系统标题栏
- 不要在设置里放位置选项，也不要加岛的位置字段。校园网模式是 `AppSettings` 的 `campusWifiMode` / `campusWifiSsid` / `campusHotspotSsid`，空模式等于关闭
- 不要手选南湖/浑南；作息跟课的 `campus`
- 不要把课表塞进每 20 分钟的余额刷新。每天最多自动打一次；没缓存或学期过期时下次刷新继续试
- 不要用 `set_focus` 前置窗口，也不要在点击或双击里复位岛
- 不要把 `3510409` 写成默认房间码，也不要枚举全校房间
- 不要把密码/短信写进普通 json。关掉「记住密码」必须 `keyring_delete`，不要只改开关
- 不要让 Esc 关岛或关 `desk`。窗口里 Esc 只关 Fluent 对话框
- 不要在没网时把「会话已失效，请重新登录」盖在缓存上；没退出登录就继续显示缓存并标「离线状态」
- 不要用 `input type="date"`，也不要用 Fluent 月历，填日程日期和第一周周一（中文 Windows 的日期控件会显示成 yyyy/mm/日）；用年/月/日三格
- 不要在钩子回调里直接 `show` 窗口（`hotkey.rs` 已丢到主线程）
- 抓数失败时不要用 0 覆盖上次的真实流量/余额，除非接口明确返回 0
- 不要把 `SEGOEICONS.TTF` 或 WinUIonWeb 源码打进仓库。图标用 `@fluentui/react-icons` 的按名导入
- 不要按界面改造计划或 prototype 把导航改回五项、把电费夜里逻辑改回跳过查询
- 不要把 `%USERPROFILE%\.tauri\nidong.key` 写进仓库、Gitee 或 GitHub。公钥已经在 `tauri.conf.json`，私钥丢了之后发出去的安装包收不到后续更新
- 不要在检查更新失败时弹窗、改岛或挡住主页。正式包打开窗口时静默检查；只有关于页手动检查失败才写「暂时检查不到更新」
- 不要发现新版本就自动下载。下载只发生在用户点「更新并重启」之后

## 托盘

左键直接打开窗口，不弹出菜单。右键菜单：校园卡、网费、电费（只显示余额，点击不做任何事）→ 打开窗口 → 刷新 → 复位岛（贴顶居中，不关窗口）→ 退出。余额随 `snapshot-updated` 改字。

## 校园网自动登录

工具箱页，导航在账单和任意门之间。模式写在设置里：`off`、`offline`（未联网）、`boot`（本次启动只试一次）、`hotspot`（先看见指定网络，再离开，才连校园网）、`manual`（只在连上校园网的那一下登录，不主动切换）。校园网名称空着用 `NEU`，并且必须是系统里已经存过的无线配置。`hotspot` 的指定网络旁有「填入当前网络」，命令 `current_wifi` 读当前 SSID，没连上就在这一页写「现在没有连上无线」，不弹窗。证书说明不要写成长句。可点文字「这安全吗？」打开窗口内对话框（不是新的系统窗），说明下面的核对，并提醒同名网络要慎重、不要手输密码。连接失败仍然只上岛。南湖和浑南的无线名是同一套：`NEU`、`NEU-2.4G`、`NEU-Mobile`。秦皇岛分校是 `NEUQ`，不要写进这套判断。指定网络是其中任何一个，或和要连接的校园网同名，就清掉已看见的状态并保持空闲。人已经在这三个名字上时，只登录，不要再 `WlanConnect` 到另一个校园网。

`offline` 的「未联网」只测公共 DNS 的 TCP：`223.5.5.5:53` 和 `114.114.114.114:53`，超时 450ms，不要改成 `util::online()`。连续两次失败才动手。不在校园网覆盖里时会去连已保存的无线配置，最多 3 次，然后停，直到公共 DNS 重新通了才把计数清掉。不会在没信号的地方一直连。每次尝试岛上先是「正在连接校园网」，失败换成一句；同一句不重复发。三次之后不再出新的失败句。已经连着校园网时只登录，不再 `WlanConnect`。

网关登录只读「记住密码」放进系统凭据的一号通密码；关掉记住密码会删掉密码，这时仍可连接无线，岛上写「先记住密码」。连接和登录的短句发 `campus-hint`，岛中间换成这句并露出来，不弹窗，不打开网关页。失败也不要做图形码识别。

同名无线不算数。提交密码之前先访问 `https://pass.neu.edu.cn`，证书必须由系统信任，`danger_accept_invalid_certs` 保持关闭。密码只加密后 POST 到主机名恰好是 `pass.neu.edu.cn` 的 https（443，不能带用户名）。公钥只取这次页面，或程序里的学校公钥；脚本地址若不是这个主机，就用程序里的公钥，不要跟着别的网址走。证书无效、登录页落到别的主机、或表单要发到别的地址，返回「这不是校园网」，岛上写「这不是校园网，密码没提交」，断开当前无线，并暂停大约一分钟（40 个轮询）不再连接、不再登录。假热点若把流量原样转到学校服务器，密码会交到学校，假热点看不到明文。`http://ipgw.neu.edu.cn` 只用来看在线状态和交票据，票据只认 `pass.neu.edu.cn` 或 `ipgw.neu.edu.cn` 返回的，不要把密码写进这个 http 请求。

## 关于

关于页只有一句产品说明、作者 `sapphire_fic`、反馈邮箱 `xufeiyang@mails.neu.edu.cn`（`open_feedback` 打开 mailto），以及下方的版本和检查更新。不要把 Fluent / WinUIonWeb 那句写回关于页，它在 README。

## 验证

在 `src-tauri` 里 `cargo test --lib`。`client::tests::login_live` 会打学校网络，默认 ignore。前端 `npx tsc --noEmit`。这是桌面窗口，浏览器里打不开岛。

## 自动更新

插件是 `tauri-plugin-updater` 和 `tauri-plugin-process`。公钥、检查地址、Windows 安装方式 `passive` 写在 `tauri.conf.json` 的 `plugins.updater`。地址是 `https://gitee.com/sapphire_fic/nidong/raw/master/updates/latest.json`。Gitee 仓库是公开的，默认分支是 `master`。清单字段见 `updates/latest.example.json`：`version` 必须高于正在运行的版本，`platforms.windows-x86_64.signature` 是 `.sig` 全文，`url` 指向安装包本身。

能力在 `src-tauri/capabilities/default.json`：`updater:default`、`process:allow-restart`。插件在 `lib.rs` 里跟 opener 一样 `plugin(...)`。前端检查在 `ui/src/desk/updateCheck.ts`，状态和按钮在 `DeskApp.tsx`，关于页只显示。

正式包（`import.meta.env.PROD`）打开工作区时静默 `check`，超时 20 秒。`npm start` 不自动打 Gitee。关于页「检查更新」总会打。有新版本时，主内容上方一条：版本、`更新并重启`、`稍后`（这次打开先藏起来，关于页仍可安装）。点更新才 `downloadAndInstall`，然后 `relaunch`。Windows 上安装程序会先退出泥洞，剩下的是 NSIS 的小进度窗。检查走插件自带的 rustls，和抓数用的 native-tls 不是同一条。校园网拦 TLS 时检查失败，保持安静，不要为了这个去改 TLS。

GitHub 上的 v2.0.0 安装包没有这个插件，不能自己更新。第一份带更新的安装包版本要高于 2.0.0，用户手动装一次。

## 打包与开源

带签名的 Windows 安装包用仓库根目录的 `scripts\release.bat`。它要求私钥在 `%USERPROFILE%\.tauri\nidong.key`，口令为空，再跑 `npx tauri build`。`bundle.createUpdaterArtifacts` 已打开，没有这把私钥时 `tauri build` 会失败。`tauri dev` 不打包，不读私钥。NSIS 简体中文，产物 `src-tauri/target/release/bundle/nsis/泥洞_<version>_x64-setup.exe`，旁边的 `.sig` 全文贴进 Gitee 的 `updates/latest.json`。先上传安装包，再改清单。图标用 BMP-in-ICO（16/24/32/48/64/256），`installerIcon` 指向 `icons/icon.ico`。

开源仓库：https://github.com/sapphire-xu/nidong 。安装包可以放 GitHub Release，更新用的那份放 Gitee Release，清单放 Gitee 仓库的 `updates/latest.json`。不要把 `target/`、`tmp/`、cookie、snapshot、私钥推进 git。用户向 README 写功能和使用方法；实现细节仍以本文为准。
