# DRPA Core 与组件架构

DRPA Core 将产品分成四层：

1. `drpa.exe`：无 GUI 的稳定控制面，负责安装定位、组件、工作区与 RPAZ 生命周期。
2. 原生组件向导：`DRPA Component Installer.exe` 使用 Rust/egui，不依赖 WebView2，负责环境检测、发现或选择 `.drpac`、显示进度并调用组件事务。
3. 能力组件：Python、Chromium、WebView2、JCode 等版本化目录，通过 `provides` 和 `entrypoints` 暴露能力。
4. 稳定 Launcher 与客户端：快捷方式始终指向安装根目录的 `DRPA Next.exe`；有桌面组件时启动当前活动版本，没有时回退到组件向导。Tauri 客户端本身是 `org.drpa.desktop-ui` 组件。

## 文件系统定位

安装目录包含 `.drpa-install.json`。用户级定位索引位于：

- Windows：`%LOCALAPPDATA%\DRPA\installations-v1.json`
- Linux：`$XDG_STATE_HOME/DRPA/installations-v1.json`，未设置时使用 `~/.local/state/DRPA/installations-v1.json`

测试和便携隔离可设置 `DRPA_LOCATOR_HOME`。运行时可通过 `DRPA_INSTALL_ROOT`、`DRPA_DATA_ROOT`、`DRPA_RUNTIME_ROOT` 覆盖自动定位。

## 更新事务

组件安装顺序为：验证清单和平台、解压并逐文件计算 SHA-256、写入版本暂存目录、替换同版本目录、原子切换 `active-components.json`、保留或清理回滚版本。状态写入使用临时文件和备份，提交失败会恢复旧状态。

轻量 Setup 只协调 Core 文件，利用 `core-files.json` 删除旧版本不再需要的核心文件；独立组件不会被 Setup 更新误删。组件向导调用相同的安装事务，CLI 的 `component reconcile/remove/gc` 继续负责显式删除和回滚历史管理。

## Core 与 Browser Host

`drpa serve --stdio` 提供版本化 JSON Lines 协议，覆盖状态、组件、工作区和 RPAZ 生命周期；协议说明见 [`CORE_PROTOCOL.md`](CORE_PROTOCOL.md)。

Agent 浏览器采用会话级持久化 MCP Host：同一 Agent 会话复用一个 Python 进程、DrissionPage 页面对象、Chrome 调试端口和用户目录。`browser_open → browser_snapshot → browser_click/type/select` 的元素引用不会因为每个工具重新启动 Python 而丢失；取消、超时、运行时切换或协议断线时 Host 会被销毁，下一次浏览器调用自动重建。

浏览器能力由 Core 统一解析，并把最终可执行文件通过 `DRPA_BROWSER_PATH` 传给 DrissionPage、Agent Browser Host、JCode/MCP 与 RPAZ。解析优先级为：显式 `DRPA_BROWSER_PATH`、活动 Chromium 组件、其他提供 `browser.chromium` 的组件、系统 Google Chrome/Chromium/Microsoft Edge/Brave。组件向导检测到系统兼容浏览器时，Chromium 包默认不勾选；用户仍可主动安装组件以获得固定、可复现的浏览器版本。

Windows 桌面入口优先使用活动 Fixed Version WebView2 组件；未安装该组件时，通过 WebView2 官方 Loader 使用系统 Evergreen Runtime。两者都不存在时，稳定 Launcher 和桌面入口不会继续启动 Tauri，而是打开不依赖 WebView2 的 egui 组件向导并提示安装固定版组件。系统 Evergreen 可用时，WebView2 包默认不勾选。

## Python Runtime Profile 与热切换

提供 `runtime.python` 的多个组件可以同时处于已安装状态，例如 `org.drpa.python-runtime`（Python 3.11 Full）和 `org.drpa.python-runtime.py314-minimal`（Python 3.14 Minimal）。工作区选择保存在 `system/runtime-profile.json`；切换该指针后，只有新启动的 Agent、RPAZ、流程代码节点、插件服务和 Studio Kernel 使用新 Profile。

物化环境不再共享 `data/runtime-environment`，而是按“组件 id + 组件版本 + runtime manifest”摘要写到 `data/runtime-environments/<digest>/environment`。冻结型 Profile 直接执行组件内解释器，不创建 venv。Python 3.14 Minimal 使用官方 Windows embeddable distribution，仅携带标准库、DRPA Runner 和轻量 Studio Kernel；Full Profile 继续提供 Jupyter、文档处理、DrissionPage 与完整 RPA 依赖。

每次任务或长驻插件启动时都会创建进程租约。切换 Profile 不会终止旧任务；旧组件版本在最后一个租约释放前不能被同版本覆盖、卸载或垃圾回收。进程异常退出后，Core 会根据租约 PID 清理陈旧记录。当前环境也可通过 `DRPA_RUNTIME_PROFILE` 临时覆盖工作区选择，用于诊断和自动化测试。

Desktop“运行环境”页为当前 Profile 提供 Python 包清单、安装和卸载。用户安装的包不写入只读 `.drpac` 或摘要物化环境，而是进入 `data/runtime-package-overlays/<runtime-digest>/site-packages`；因此不同 Python 版本、Full/Minimal Profile 以及组件升级后的 ABI 不会互相污染。修改优先调用 Runtime 自带或其他已安装 Python 组件中的 `uv pip --target`，系统找不到 `uv` 时才降级到所选解释器的 `pip`；两者的下载缓存固定在 `data/cache/uv` 和 `data/cache/pip`，不继承用户全局缓存。Runtime 内置包只读，界面只允许卸载用户包层中的分发；变更后现有 Studio Kernel 会关闭并按需重建。Host 通过 `DRPA_PYTHON_PACKAGE_PATH` 把用户包层注入 Agent Python、Studio Kernel、流程代码节点和 RPAZ 进程，冻结型 embeddable Python 也不依赖 `PYTHONPATH`。

Windows 构建 Python 3.14 Minimal 组件：

```powershell
python tools/windows/build_python314_minimal.py `
  --output artifacts/runtime-components/org.drpa.python-runtime.py314-minimal.drpac `
  --component-version 3.0.0 `
  --cache artifacts/cache/python314
```

构建器只在构建期下载并校验固定 SHA-256 的 CPython 官方包；最终 `.drpac` 安装和切换完全离线。

## UOS 20 模块化发布

UOS 版本发布为一个轻量 Core DEB 和四个互相独立的 `linux-x86_64` 组件包：

- `drpa-next-core-*.deb`：安装 `drpa` 命令行、`drpa-next` 稳定入口、原生 egui 组件向导、桌面菜单和图标。
- `org.drpa.desktop-ui.drpac`：DRPA Next 图形工作台及经过 UOS 20 验证的私有 WebKitGTK/GTK/Mesa 运行环境。
- `org.drpa.python-runtime.drpac`：Python 3.11、Jupyter、文档处理和 RPAZ 运行环境，不再捆绑 Chromium。
- `org.drpa.browser.chromium.drpac`：可选隔离 Chromium；系统已有 Google Chrome/Chromium 时向导默认不勾选。
- `org.drpa.jcode.drpac`：Linux 原生 JCode，不包含或查找 `jcode.exe`。

UOS 20 的系统 glibc 为 2.28，而验证过的 WebKitGTK 私有运行时使用固定 ELF 解释器。为避免改写 WebKit 及其多进程 ELF，系统级组件统一安装到 `/opt/drpa-next-uos20/components/<id>/<version>`；安装或激活 Desktop 组件时，Core 以事务方式维护 `/opt/drpa-next-uos20/usr` 与 `/opt/drpa-next-uos20/uos-runtime` 两个兼容软链接，卸载时一并移除。普通用户在 GUI 向导安装、修复或卸载组件时，通过 PolicyKit 显示系统授权窗口；Core 和 Launcher 本身不以 root 运行。用户会话、工作区、RPAZ 和运行输出仍放在 `$XDG_DATA_HOME/drpa-next`，默认是 `~/.local/share/drpa-next`。

Python、Chromium 与 JCode 使用 UOS 20 自带的 `/lib64/ld-linux-x86-64.so.2`，并移除旧整包的绝对 RPATH。若上游离线 wheel 不兼容 glibc 2.28（例如仅提供 `manylinux_2_34`），应先在 UOS 20 构建同版本 wheel，再通过 `--python-wheel-overlay` 覆盖；覆盖文件及 SHA-256 会写入组件内的 `drpa-wheel-overlay.json`，最终安装仍完全离线。

桌面菜单始终执行 `/usr/bin/drpa-next`：活动 `org.drpa.desktop-ui` 存在时启动该版本的 `AppRun`；不存在时自动打开 `/usr/bin/drpa-component-installer`。命令行不依赖桌面组件，安装 Core 后可以立即使用 `drpa status`、`drpa doctor`、`drpa component ...`、`drpa workspace ...` 和 `drpa rpaz ...`。

在 UOS 20 构建机上运行：

```bash
cargo build --release -p drpa-cli -p drpa-launcher -p drpa-component-installer
python3 tools/linux/build_modular_uos.py \
  --workspace . \
  --core-bin-dir target/release \
  --source-deb /path/to/verified-monolithic-uos20.deb \
  --python-wheel-overlay /path/to/uos20-built-wheels \
  --output-dir artifacts/uos-modular \
  --work-dir target/uos-modular
```

构建脚本会保留 Desktop 的原始 ELF，给 Python/Chromium/JCode 适配 UOS 系统加载器，生成四个 `.drpac`、逐包调用 Core 校验，并输出包含来源、大小与 SHA-256 的总清单。旧全量 UOS DEB 仅作为已经验证的 WebKit/Python/Chromium/JCode 资产输入，不会作为新的安装包发布。

## 后续边界

下一阶段应把更多桌面命令逐步改为调用 Core 服务协议；`.drpac` 的离线可信发布还应增加发行公钥签名，SHA-256 目前负责完整性而非发布者身份认证。
