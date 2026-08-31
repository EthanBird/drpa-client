# DRPA Next Windows 模块化离线版

Windows x64 Setup 现在只包含 DRPA Core、稳定 Launcher 和纯 Rust/egui 组件安装向导。桌面端、Python/RPAZ、Chromium、Fixed Version WebView2 与 JCode 分别发布为独立 `.drpac`，用户只下载实际需要的能力，基础安装不再携带约 800 MiB 的离线依赖。

## 最小内核与桌面界面

`drpa.exe` 是不依赖 WebView2、Chrome、Node.js 或系统 Python 的原生管理内核。即使机器没有 WebView2，也可以用它定位安装目录、管理隔离工作区、安装组件和运行 RPAZ。

```powershell
drpa status
drpa doctor
drpa component list
drpa workspace list
drpa rpaz list
drpa rpaz run <package-id> --profile <profile-id> --params '@parameters.json'
```

`DRPA Next.exe` 是稳定 Launcher。真正的 Tauri 图形客户端安装在 `components/org.drpa.desktop-ui/<version>/`；Launcher 按活动版本启动它。尚未安装桌面 UI 时，Launcher 会自动打开 `DRPA Component Installer.exe`，因此没有 WebView2 的全新机器仍有可操作的原生界面。

## 安装与升级

1. 下载轻量 `drpa-next-<version>-windows-x86_64-setup.exe`，并按需下载一个或多个 `.drpac`；文件可以放在 Setup 同目录，也可以放在下载目录任意位置。
2. 退出正在运行的 DRPA。
3. 首次安装选择非系统盘目录；升级选择原安装目录。
4. Setup 完成后打开原生组件向导。向导会扫描 Setup 所在目录、安装目录、下载目录和桌面，也支持拖放或通过内置 egui 文件浏览器选择任意 `.drpac`。
5. 选择所需组件并安装；向导会显示 Core、系统 WebView2、系统 Chromium 兼容浏览器、现有组件与包校验状态，逐包安装完成后可直接启动桌面工作台。检测到系统 Evergreen WebView2 或 Google Chrome/Chromium/Edge/Brave 时，相应的可选包默认不勾选，仍可手动选择固定版本组件。

组件页同时列出所有活动组件。使用“修复”可以从同版本 `.drpac` 执行事务重装；没有对应包时会先做完整性校验。使用“卸载”会在二次确认后移除该组件的全部版本，但不会删除 `data/` 下的工作区、项目或用户数据。桌面组件正在运行时，应先退出 DRPA Next 再修复或卸载。

安装器不申请管理员权限，也不写注册表。安装位置由安装目录的 `.drpa-install.json` 与用户目录的 `installations-v1.json` 定位；系统 WebView2 状态通过微软官方 Loader 查询。轻量 Setup 升级只协调 Core 文件，不会擅自删除现有组件；组件升级、移除、激活与历史清理由向导或 `drpa component` 完成。

```text
<install>/
├── DRPA Next.exe
├── drpa.exe
├── DRPA Component Installer.exe
├── core-files.json
├── .drpa-install.json
├── components/<component>/<version>/ 已展开的版本化组件
├── state/active-components.json      当前激活版本
└── data/                             包、项目、环境、历史、产物和配置
```

## 组件管理与回滚

```powershell
# 从单独下载的离线包安装浏览器
drpa component install D:\Downloads\org.drpa.browser.chromium.drpac

# 校验清单中的长度和 SHA-256
drpa component verify org.drpa.browser.chromium

# 移除浏览器的全部版本
drpa component remove org.drpa.browser.chromium --purge

# 回滚到仍保留的版本
drpa component activate org.drpa.python-runtime 2.1.0

# 每个活动组件只保留活动版本和一个回滚点
drpa component gc --all --keep 2
```

`.drpac` 是 ZIP 容器，根目录包含 `component.json`，负载在 `payload/`。安装器拒绝绝对路径、父目录穿越、符号链接源、未声明文件、重复文件以及长度或 SHA-256 不一致的内容；组件先写入暂存目录，验证通过后再切换活动状态。

桌面启动时 WebView2 的选择顺序为活动固定版组件、系统 Evergreen Runtime；两者都没有时自动回到 egui 组件向导，不会进入白屏或无响应的 Tauri 启动。浏览器自动化的选择顺序为显式 `DRPA_BROWSER_PATH`、活动 Chromium 组件、其他浏览器能力组件、系统 Chrome/Chromium/Edge/Brave。最终路径统一传给 DrissionPage、Agent Browser Host、JCode/MCP 与 RPAZ。

## 本地构建模块化 Setup

先准备包含 `DRPA Next.exe`、`runtime/`、`webview2/` 和可选 `jcode/` 的完整 stage，再运行：

```powershell
tools\windows\build_modular_setup.ps1 `
  -Stage D:\build\DRPA-Next-stage `
  -Version 3.0.0 `
  -Output D:\build\drpa-next-3.0.0-windows-x86_64-setup.exe `
  -ComponentsOutput D:\build\components
```

脚本会先构建最新前端与 `drpa-desktop.exe`，并用仓库内的 Python adapter 重建 wheel、`wheelhouse-lock.json`、`manifest.json` 与 `SHA256SUMS`；随后编译 `drpa.exe`、稳定 Launcher 和 egui 向导，把桌面 UI、Python、Chromium、WebView2、JCode 写入独立发布目录，最后仅用三个 Core 可执行文件生成轻量、不写注册表的 Setup。`components/catalog.json` 同时记录每个发布包的大小和 SHA-256。

桌面 Release 必须启用 Tauri `custom-protocol`，否则 WebView 会错误访问开发地址 `http://localhost:1420`。模块化构建脚本会强制启用该特性，源码也会拒绝缺少该特性的 Release 编译。Python adapter 使用当前构建环境中的 setuptools/wheel 并带 `--no-index` 构建，不访问 PyPI。

若省略 `-ComponentsOutput`，组件默认输出到 Setup 同目录的 `components/`。若 NSIS 没安装在默认路径，可额外传入 `-Makensis D:\tools\nsis\makensis.exe`。输入 stage 保留完整运行时布局，便于重复构建和重新拆包。

只修改了 Core、Launcher 或 egui 向导时，可追加 `-CoreOnly` 复用已生成的独立组件目录，跳过桌面构建、运行时重封和大包压缩。

## 数据、卸载与迁移

工作区数据仍在 `<install>/data/`，Core 或组件更新不会触碰该目录。卸载器会注销用户级安装定位信息，并删除核心文件、已安装组件和旧版依赖目录，但保留 `data/`，避免误删项目、会话、知识库、RPAZ 与运行产物。

迁移时可整体复制安装目录；若只迁移数据，复制 `data/` 后运行 `drpa doctor`，确认 Python 生成环境与新路径一致。
