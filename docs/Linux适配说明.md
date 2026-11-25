# 泥洞 · Linux 适配与构建说明（Phase 1–2）

本文说明泥洞在 Linux 桌面环境（重点面向 **Ubuntu 24.04 / 26.04 GNOME**，同时兼容 X11 桌面）的功能范围、系统依赖、构建打包方式与使用说明。

---

## 1. 环境要求与重要提醒

- **支持发行版**：Ubuntu 22.04 / 24.04 / 26.04 LTS、Debian 12+、Fedora 39+、Arch Linux 等现代 64 位 Linux（需提供 `webkit2gtk-4.1` 与 `libsoup-3.0`）。
- **不支持 Ubuntu 16.04 / 18.04 / 20.04**：Tauri 2 强制依赖 `libwebkit2gtk-4.1` 和较新 glibc，旧版系统（如部分高校实验平台的 Ubuntu 16.04 容器）无法编译或运行。
- **在 Windows 上自行编译 Linux 包**：可使用 WSL2 安装 Ubuntu 24.04 / 26.04（`wsl --install -d Ubuntu-24.04`）后在 WSL 内运行 `scripts/build-linux.sh`，产出的 `.deb` / `.AppImage` 可直接发给 Ubuntu 26.04 机器使用。

---

## 2. Phase 1–2 支持的功能与边界

| 模块 | Linux 支持状态 | 说明 |
|---|---|---|
| 一号通登录、WebVPN、卡/网/电/账单/课表/日程 | 完整支持 | 密码存入 Secret Service（GNOME Keyring / KWallet），`cookies.json` 权限锁定为 `0600` |
| 任意门、离线缓存、跳过登录 | 完整支持 | 系统默认浏览器走 `xdg-open` |
| 免流文件分享 | 完整支持 | 原生拖放文件；如对方访问不了，请检查防火墙放行 TCP `38721` |
| 定时下载（断点续传、系统代理、下完关机） | 完整支持 | 默认目录读 XDG 下载目录；代理读 GNOME `gsettings` / KDE `kioslaverc` / 环境变量；关机走 `systemd-logind` D-Bus（无需 root 密码） |
| 校园网自动连接与防仿冒校验 | 完整支持 | 对接 NetworkManager（`nmcli`），不可用时回退 `iwgetid` / `iw` 只读查询 |
| 防社死 · 智能静音 | 完整支持 | 对接 PipeWire（`wpctl`）与 PulseAudio（`pactl`），不创建录音流（不会在 GNOME 顶栏亮录音红点） |
| 插件系统（WebView / `.zip` / `.ndplugin` / MCP Stdio） | 完整支持 | `.zip` 已改用纯 Rust 解压，不依赖外部 `tar`/`unzip`；自动补齐 GUI 进程常见的用户 `PATH` |
| 开机自启 | 完整支持 | 写入 `~/.config/autostart/edu.neu.nidong.desktop`（自动识别 AppImage 路径） |
| 系统托盘 | 已适配 | Linux AppIndicator 不支持左键直开和多行悬停提示，已在托盘菜单顶部嵌入实时卡/网/电余额只读项 |
| 顶部悬浮岛 | X11 完整 / Wayland 适配 | 见下节说明 |
| 本机校园网流量监控（`nidong-meter`） | 暂不支持（Phase 3） | 网费页优雅降级为展示门户流量与计费规则 |

---

## 3. Ubuntu 26.04 GNOME（Wayland）使用说明

1. **拖动悬浮岛**：Linux 下已切换为原生窗口拖动（`startDragging`），可拖动到屏幕任意位置。
2. **仅托盘模式与系统通知**：Wayland 协议限制普通应用自行贴顶置顶、监听全局鼠标悬停与全局键盘钩子。
   - 上课与日程提醒在 Wayland 下会同步发送 GNOME 系统通知；
   - 如果不希望悬浮岛停留在桌面，可在 **设置 → 面板行为** 关闭「**显示悬浮岛**」，进入仅托盘模式（提醒照常通过系统通知和铃声送达）。
3. **绑定系统快捷键（推荐）**：可在 GNOME「设置 → 键盘 → 查看及自定义快捷键 → 自定义快捷键」中新增：
   - 唤出悬浮岛：命令填 `nidong --reveal`
   - 打开/前置主窗口：命令填 `nidong --open`
4. **系统托盘**：Ubuntu Desktop 默认已启用 AppIndicator 扩展；如使用精简版 GNOME 桌面未显示托盘图标，请安装扩展 `AppIndicator and KStatusNotifierItem Support`（`gnome-shell-extension-appindicator`）。

---

## 4. 在 Ubuntu 24.04 / 26.04 上构建与安装

### 安装系统构建依赖
```bash
sudo apt update
sudo apt install -y \
  build-essential curl wget file patchelf pkg-config xdg-utils \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libssl-dev libdbus-1-dev \
  gstreamer1.0-plugins-base gstreamer1.0-plugins-good libnotify-bin
```

同时确保已安装 **Node.js 18+** 与 **Rust stable**（`rustup`）。

### 开发模式运行
```bash
npm install
npm start
```

### 一键打包 `.deb` 与 `.AppImage`
```bash
chmod +x scripts/build-linux.sh
./scripts/build-linux.sh
```

- 如只需打 `.deb`（推荐直接给 Ubuntu 26.04 安装）：
  ```bash
  NIDONG_LINUX_TARGETS=deb ./scripts/build-linux.sh
  ```
- 安装 `.deb`：
  ```bash
  sudo apt install ./src-tauri/target/release/bundle/deb/nidong_*.deb
  ```
- 运行 `.AppImage`（如环境缺少 FUSE2，可先 `sudo apt install libfuse2t64` 或使用 `--appimage-extract-and-run`）：
  ```bash
  chmod +x src-tauri/target/release/bundle/appimage/nidong_*.AppImage
  ./src-tauri/target/release/bundle/appimage/nidong_*.AppImage
  ```

> **提示**：如遇到部分 NVIDIA 显卡 + Wayland 下 WebKitGTK 白屏/闪烁，可尝试以 `WEBKIT_DISABLE_DMABUF_RENDERER=1 nidong` 启动。

---

## 5. GitHub Actions 双端自动构建（Windows + Linux）

仓库已内置 [.github/workflows/release.yml](../.github/workflows/release.yml)，无需自备 Linux 服务器即可同时构建 Windows（NSIS + `nidong-meter.exe`）与 Linux（`.deb` + `.AppImage`）：

- **手动触发**：在 GitHub 仓库的 **Actions → Build & Release (Windows + Linux) → Run workflow** 中可选择 `all` / `linux` / `windows`，以及是否创建 Draft Release。
- **标签自动发版**：推送与 `src-tauri/tauri.conf.json` 版本一致的标签（如 `git tag v3.0.2 && git push origin v3.0.2`），自动运行双端测试与打包，生成 `SHA256SUMS.txt` 并创建 **Draft Release**（核对后手动点击 Publish 即可对外发布）。
- **自动更新签名（可选）**：
  - 未配置 Secret 时，默认自动打出可直接安装的未签名安装包；
  - 如需同时产出支持应用内静默更新的 `.sig` 签名文件，在 GitHub 仓库 `Settings → Secrets and variables → Actions` 中新增 `TAURI_SIGNING_PRIVATE_KEY`（填入本机 `%USERPROFILE%\.tauri\nidong.key` 的文件内容，切勿提交到仓库），`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 留空或不配即可。
  - 国内自动更新清单仍由 Gitee `updates/latest.json` 承载，切勿将 90 MB+ 的 `.AppImage` 提交进 Gitee Git 仓库历史。
