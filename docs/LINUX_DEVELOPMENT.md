# DRPA Next Linux 开发与移植交接

本文面向接手 Linux 桌面端的开发者，记录当前代码已经具备的跨平台基础、仍属于 Windows 的实现、可直接复现的 Linux 开发流程，以及把 Linux 从“可编译”推进到“可离线发布”所需的验收清单。

## 1. 当前结论

DRPA Next `1.0.0` 已正式提供 Linux x86_64 runtime-complete AppImage、现代发行版 deb 与 UOS Desktop 20 专用 deb，与 Windows Setup 共用 `desktop-v1.0.0` Release。Ubuntu 22.04 workflow 负责构建 sealed runtime、三种格式打包、解包验证、断网 bootstrap、deb 安装卸载和 X11 启动后上传；UOS 包还必须在 Debian 10/glibc 2.28 容器中通过完整安装与运行门禁。Ubuntu 24.04、真实 UOS 20、Wayland 和人工 GUI 验收仍是持续回归项，不能因容器测试通过而删除这些门禁。

| 能力 | Linux 当前状态 | 证据或入口 |
| --- | --- | --- |
| React/Vite 浏览器开发 | 可用 | `npm run dev` 使用 `mockGateway` |
| Rust core | CI 覆盖 | `drpa-protocol`、`drpa-package`、`drpa-host` 在 Ubuntu 测试 |
| Tauri Host 编译 | CI 覆盖 | Ubuntu 安装 WebKitGTK 后执行 `cargo check -p drpa-desktop` |
| Tauri 桌面联调 | 可在本机进行 | `npm run tauri:dev`，需要图形会话和 WebKitGTK |
| Linux 数据目录与 `xdg-open` | 已有实现 | `app_local_data_dir()/workspace`、`open_directory_in_file_explorer` |
| Linux x86_64 runtime 规格 | 已声明 | `offline/runtime-spec.json` 的 `linux-x86_64` |
| Linux sealed runtime 构建器 | 已有通路 | `tools/offline/build_runtime_bundle.py` 支持 `linux-x86_64` |
| Linux sealed runtime CI | 已接入 | `.github/workflows/linux-desktop.yml` 的 Ubuntu 22.04 原生构建与 air-gap smoke |
| AppImage 最终布局 | 已实现并发布，跨发行版回归持续进行 | `tauri.linux.conf.json` 把 runtime 放入只读 resource，Host 使用 `resource_dir` 定位 |
| deb 最终布局 | 已实现并发布 | AppImage AppDir→`/opt/drpa-next` 私有桌面运行时、`dpkg-deb` manifest、无系统 WebKitGTK 安装/启动/卸载检查 |
| UOS 20 deb | 已接入发布门禁 | `/opt/drpa-next-uos20` 私有 glibc/C++ 层、ELF 固定解释器/RPATH、Debian 10/glibc 2.28 运行验证 |
| Linux 文件级热更新 | 未实现 | 当前命令、协议与独立 Worker 只接受 `windows-x86_64` |
| Linux GUI、浏览器、Jupyter 端到端 | CI 已覆盖首层 | AppImage 解包 bootstrap、Chrome/Jupyter smoke 与 Xvfb 启动；Wayland/人工验收待完成 |
| Linux 任务取消 | 已实现 | Python worker/Studio Kernel 独立 process group，`SIGTERM` 后超时 `SIGKILL` |

因此，Linux 后续工作重点已从“接通代码”转为“维持发行质量”：每次发布都必须取得专用 workflow 绿灯，并继续在 Ubuntu 22.04/24.04、X11/Wayland 和真实断网机器上扩展验收。不要用一次 `cargo check` 或单独生成 AppImage 替代完整发布门禁。

## 2. 目标基线

第一阶段只处理 **Linux x86_64 + glibc**：

- 构建基线：Ubuntu 22.04；它能提供 WebKitGTK 4.1，也能降低 AppImage 的 glibc 最低版本。
- 验证系统：现代包至少覆盖 Ubuntu 22.04 和 Ubuntu 24.04；UOS 包的自动最低基线为 Debian 10/glibc 2.28，并继续在真实 UOS Desktop 20 Professional（eagle）机器人工回归。
- 发行格式：AppImage 适合 glibc 2.35+ 免安装分发；现代 deb 适合 Ubuntu 22.04+；文件名带 `uos20` 的 deb 面向 UOS 20/glibc 2.28。三者共用只读 runtime 与 XDG 数据合同。
- Python：CPython `3.11.9`。
- Node.js：`24`，以 root `package.json` 和 CI 为准。
- Rust：stable，最低 Rust 版本以 workspace `Cargo.toml` 为准。
- uv：`0.11.28`，以 `offline/runtime-spec.json` 为准。
- UI WebView：源码编译使用系统 WebKitGTK 开发包；正式 AppImage/deb 携带私有 WebKitGTK 4.1 闭包，不携带 Windows WebView2。
- 自动化浏览器：平台 sealed runtime 中的 Chrome for Testing，不依赖用户系统 Chrome。

Tauri 官方 Linux 前置要求与 AppImage 兼容性说明：

- <https://v2.tauri.app/start/prerequisites/>
- <https://v2.tauri.app/distribute/appimage/>

## 3. Ubuntu / Debian 开发环境

### 3.1 系统依赖

Ubuntu 22.04 或 Debian 12 可使用：

```bash
sudo apt update
sudo apt install -y \
  libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  patchelf \
  xdg-utils
```

CI 当前使用的最小编译依赖见 `.github/workflows/ci.yml`。上面的列表按 Tauri 2 官方开发前置要求补齐了本地运行和打包常用组件。

原生打包与 CI GUI smoke 还会使用以下工具/运行库：

```bash
sudo apt install -y \
  dbus-x11 \
  libfuse2 \
  libgbm1 \
  libnss3 \
  xauth \
  xvfb
```

依赖边界：WebKitGTK/GTK 开发包、编译器和 `patchelf` 只属于源码构建环境。AppImage 封装桌面运行库；现代 deb `1.0.0-2` 直接复用同一 AppDir，把 WebKitGTK、JavaScriptCoreGTK、GTK、GStreamer、NSS、Soup 和 helper process 放入 `/opt/drpa-next`，`Depends` 不得再出现 `libwebkit2gtk-4.1-0` 等 WebKit/GTK 桌面包。现代包的 glibc 2.35+、libgcc、libstdc++ 与图形用户态由系统提供；UOS deb 则私有携带 ABI 运行库、NSS、GBM/libdrm，以及固定版本的 GLVND + Mesa EGL + swrast/llvmpipe 软件渲染闭包，不加载目标机的厂商 DRI。FUSE 不可用时可用 AppImage 的 extract-and-run 模式。Chrome for Testing、Python、uv 和 Python wheels 不来自系统 apt。

随后安装 Node.js 24、Rust stable 与 Python 3.11.9。建议用版本管理器安装，不要修改仓库中的版本约束来迁就本机旧工具。

```bash
node --version
npm --version
rustc --version
cargo --version
python3.11 --version
```

### 3.2 拉取与安装前端依赖

```bash
git clone https://github.com/EthanBird/drpa-client.git
cd drpa-client
git switch codex/drpa-next-platform
npm ci
```

旧 `src/drpa_client/`、root `pyproject.toml` 和 `scripts/run-drpa.sh` 属于 PySide6 兼容参考。Linux 新功能进入 `apps/desktop/`、`crates/` 和 `runtime/python/`，不要从旧启动脚本开始移植。

## 4. 三种开发模式

### 4.1 只开发 React UI

```bash
npm run dev
```

访问 Vite 输出的本地地址。此模式使用 `apps/desktop/src/infra/mockGateway.ts`，适合页面、布局和组件测试，不覆盖文件系统、进程、sealed runtime 或 Tauri command。

### 4.2 运行真实 Linux Tauri Host

先准备仓库根目录的开发 Python 环境。为了与发行运行时一致，建议安装精确依赖集合：

```bash
python3.11 -m venv .venv
./.venv/bin/python -m pip install --upgrade pip
./.venv/bin/python -m pip install -r offline/requirements/runtime.txt
./.venv/bin/python -m pip install -e './runtime/python[test]'
```

设置显式开发路径，避免把测试数据写入真实用户目录：

```bash
export DRPA_DATA_DIR="$PWD/.drpa-data"
export DRPA_RUNTIME_PYTHON="$PWD/.venv/bin/python"
export DRPA_RUNTIME_PYTHONPATH="$PWD/runtime/python/src"

# 运行浏览器 RPA 时设置；仅打开桌面 UI 或编辑文档时可以省略。
export DRPA_BROWSER_PATH="$(command -v google-chrome || command -v chromium || command -v chromium-browser)"

npm run tauri:dev
```

`DRPA_RUNTIME_PYTHON` 与 `DRPA_RUNTIME_PYTHONPATH` 必须成对使用。Debug Host 原本也会尝试仓库根目录 `.venv/bin/python`，显式设置更容易诊断路径问题。

### 4.3 使用 Linux sealed runtime 联调

已有 runtime 目录时，不再设置开发 Python，而是指定 runtime 根目录：

```bash
unset DRPA_RUNTIME_PYTHON DRPA_RUNTIME_PYTHONPATH DRPA_BROWSER_PATH
export DRPA_DATA_DIR="$PWD/.drpa-data-sealed"
export DRPA_RUNTIME_ROOT="$PWD/offline-build/runtime"
npm run tauri:dev
```

`DRPA_RUNTIME_ROOT` 必须直接指向包含 `manifest.json`、`bootstrap_runtime.py`、`python/`、`wheelhouse/` 和 `browser/` 的目录。Host 会验证 manifest 的平台值必须为 `linux-x86_64`，并在数据目录中创建可写的 `runtime-environment/environment`。

## 5. Linux 平台合同

### 5.1 数据目录

Linux 未设置 `DRPA_DATA_DIR` 时，Host 使用：

```text
app.path().app_local_data_dir()/workspace
```

实际根目录由 Tauri 按 XDG 规则解析。开发和自动化测试应总是设置 `DRPA_DATA_DIR`，这样清理测试数据不会触碰真实用户资料。

数据子目录与 Windows 保持同一领域结构：

```text
workspace/
├── packages/
├── projects/
├── build/
├── runs/
├── runtime-environment/
├── knowledge/
├── agent/
└── updates/
```

Linux 发布包必须保持用户数据与只读应用文件分离。卸载或替换应用时不得删除 XDG 数据目录。

### 5.2 WebView 与自动化浏览器

这两个浏览器角色不能混淆：

- Tauri UI 由系统 WebKitGTK 渲染。
- RPA 脚本通过 DrissionPage 使用 `DRPA_BROWSER_PATH` 指向的 Chrome for Testing。

Linux 包不应携带 WebView2；Chrome for Testing 仍属于 sealed runtime。开发机只有在运行浏览器脚本时才需要配置 Chrome/Chromium。

### 5.3 打开本地目录

Linux Host 使用 `xdg-open` 打开工作区、构建输出和任务产物。发行包应依赖或检测 `xdg-utils`。在无桌面会话、容器或纯 SSH 环境中，目录打开动作应返回清晰错误，不能影响任务本身的成功状态。

### 5.4 进程与取消

Linux Host 会在启动 Python worker 与 Studio Kernel 前调用 `CommandExt::process_group(0)`。任务运行期间 `RunProcessManager` 保存 `run_id → pgid`：取消时先把领域状态写为 `cancelled`，向整个进程组发送 `SIGTERM`，两秒后仍未退出则发送 `SIGKILL`。后台退出处理会检查 cancelled 状态，不再把它覆盖为 failed。`cargo test -p drpa-desktop` 包含真实 `sh + sleep` 进程组回归。

浏览器由 Python worker 派生并继承同一进程组，因此取消普通 RPAZ 会覆盖 Chrome 子进程。Agent 尚没有独立“取消当前 turn”协议，不属于这条任务取消链。

### 5.5 更新能力

当前 `apply_windows_update`、`drpa-updater`、更新清单 target 和重启确认协议均为 Windows 实现。Host 通过 `get_platform_capabilities` 返回 `supportsWindowsUpdates=false`，Linux 设置页不会渲染 Windows 更新入口。Linux 使用完整 AppImage 替换或通过包管理器安装新版 deb；不要复用 Windows `.drpa-update`。

## 6. 构建 Linux sealed runtime

构建器已经支持 `linux-x86_64`，但必须在原生 Linux x86_64、Python 3.11.9 上执行：

```bash
./.venv/bin/python -m pip install 'uv==0.11.28'
./.venv/bin/python tools/offline/validate_requirements.py offline/requirements/runtime.txt
./.venv/bin/python tools/offline/build_runtime_bundle.py \
  --platform linux-x86_64 \
  --work-dir "$PWD/offline-build/linux-x86_64"
```

构建阶段需要网络，用于下载受控 Python、精确 wheels 和固定 Chrome for Testing。`offline/requirements/runtime.txt` 是跨平台精确版本集合，Windows-only 项使用 environment marker；Linux 构建必须在 CPython 3.11.9 x86_64 原生环境执行 `pip download --only-binary=:all:`，不能使用旧 `wheelhouse/linux-x86_64/` 兼容目录。构建器随后会在空 uv cache、禁止联网的条件下：

1. 创建全新环境；
2. 验证 adapter 与依赖；
3. 用 DrissionPage 启动内置 Chrome 并访问本地 HTML；
4. 启动真实 Jupyter Kernel 连续执行两个单元；
5. 生成 `wheelhouse-lock.json`（实际文件名、版本、大小、SHA-256）、`manifest.json`、文件库存、归档与归档校验文件。

Linux wheel 中需要重点关注的原生/ABI 包包括 `debugpy`、`lxml`、`numpy`、`pandas`、`psutil`、`pyzmq`、`rpds-py` 和 `tornado`；其余纯 Python wheel 同样进入锁和散列清单。最终布局验证器会拒绝 `win_amd64`、`macosx`、`musllinux` wheel 混入 glibc 包，并复核所有 wheel 的大小和散列。

输出目录位于：

```text
offline-build/linux-x86_64/out/
```

把归档解压后，将内部 `drpa-runtime-...-linux-x86_64/` 目录作为 `DRPA_RUNTIME_ROOT` 即可进行 Host 联调。也可运行：

```bash
python tools/linux/verify_runtime_layout.py --runtime-root /path/to/runtime
```

该检查覆盖平台、Python 版本一致性、安全路径、wheel 散列以及 Python/uv/Chrome 可执行位。

## 7. Tauri Linux 包

Tauri 使用 `apps/desktop/src-tauri/tauri.linux.conf.json` 生成 AppImage，并把构建阶段临时目录 `resources/linux/runtime/` 映射到 `$RESOURCES/runtime/`；该临时目录被 `.gitignore` 排除，禁止把几百 MB 二进制提交进 Git。deb 不再使用 Tauri 默认模板，因为该模板声明系统 WebKitGTK；`tools/linux/build_bundled_deb.py` 从验证后的 AppImage AppDir 生成 `/opt/drpa-next` 安装布局、`/usr/bin/drpa-next` 启动器、桌面文件和基础 `Depends`。

本地完整构建顺序：

```bash
runtime_stage="$(find offline-build/linux-x86_64 -maxdepth 1 -type d -name 'drpa-runtime-*-linux-x86_64' -print -quit)"
rm -rf apps/desktop/src-tauri/resources/linux/runtime
mkdir -p apps/desktop/src-tauri/resources/linux/runtime
cp -a "$runtime_stage/." apps/desktop/src-tauri/resources/linux/runtime/
python tools/linux/verify_runtime_layout.py --runtime-root apps/desktop/src-tauri/resources/linux/runtime
npm run tauri:build --workspace @drpa/desktop -- --bundles appimage
```

解包 AppImage 后可生成与正式流水线相同的 deb（Debian 修订号用于覆盖升级旧的 `1.0.0` 包）：

```bash
appimage="$(find target/release/bundle/appimage -maxdepth 1 -name '*.AppImage' -print -quit)"
extract_root="$(mktemp -d)"
chmod +x "$appimage"
(cd "$extract_root" && "$OLDPWD/$appimage" --appimage-extract >/dev/null)
python tools/linux/build_bundled_deb.py \
  --appdir "$extract_root/squashfs-root" \
  --package-version 1.0.0-2 \
  --output drpa-next-1.0.0-linux-x86_64.deb \
  --work-dir "$extract_root/deb-work"
```

Host 启动时把 `app.path().resource_dir()/runtime` 放在运行时候选列表中，优先级低于显式 `DRPA_RUNTIME_ROOT`、高于应用旁外置 runtime。AppImage 内 runtime 保持只读；`bootstrap_runtime.py` 在 XDG workspace 的 `runtime-environment/environment` 创建可写 venv。

完整 AppImage 可直接联调：

```bash
chmod +x path/to/DRPA-Next_1.0.0_amd64.AppImage
DRPA_DATA_DIR="$PWD/.drpa-appimage-data" \
  path/to/DRPA-Next_1.0.0_amd64.AppImage
```

CI 会用 `--appimage-extract` 找到 AppImage 的最终 `runtime/manifest.json`，对解包后的真实文件再次运行布局检查和离线 bootstrap，再用 `APPIMAGE_EXTRACT_AND_RUN=1 + Xvfb` 确认 GUI 不早退。对 deb，`tools/linux/verify_deb_bundle.py` 会检查 architecture/version/Depends、`/usr/bin/drpa-next`、`/opt/drpa-next` 中的 WebKitGTK/JavaScriptCoreGTK/GTK/GStreamer/helper process、只读 runtime 与 wheel 散列，并生成机器可读 manifest。Runner 随后卸载系统 `libwebkit2gtk-4.1-0`，确认 `apt` 安装 deb 不会将其拉回，再完成 Xvfb 启动和卸载，确认 XDG 用户数据不被删除。

deb 安装与卸载：

```bash
sudo apt install ./drpa-next-1.0.0-linux-x86_64.deb
sudo apt remove drpa-next
```

### 7.1 UOS Desktop 20 / glibc 2.28 专用包

本节给出 Linux 交接所需的架构摘要；完整的依赖分层、包内布局、ELF 修补原理、可复现命令、历史故障与实体机检查表见 [`UOS20_PACKAGING.md`](UOS20_PACKAGING.md)。修改 UOS 打包策略前必须同时阅读两处，并以构建器、验证器和成功 workflow 的实际行为为准。

Ubuntu 22.04 生成的普通 AppImage 和现代 deb 不能在 UOS 20 上直接运行。原因不只是 Debian `Depends`：桌面 Host 需要 `GLIBC_2.34`，随包 WebKitGTK/JavaScriptCoreGTK 需要 `GLIBC_2.35`、`GLIBCXX_3.4.30` 和比 GCC 8 系统库更高的 C++ ABI。禁止通过删除版本约束或伪造 control 文件宣称兼容。

`tools/linux/build_uos20_deb.py` 从同一个已验证 AppDir 生成独立包：

- 固定安装根为 `/opt/drpa-next-uos20`，Debian 包名仍为 `drpa-next`，可由包管理器正常升级或卸载；
- 从 Ubuntu 22.04 原生 runner 复制 glibc 2.35 动态加载器、libc/NSS、libstdc++ 和 libgcc，并递归解析全部 `DT_NEEDED`，把 Fribidi、X11/XCB、ALSA、字体等非驱动用户态库补入私有层；所有文件保存来源、散列与许可证；
- 使用 `patchelf` 给 AppDir 中每个 x86_64 动态 ELF 写入 `/opt/drpa-next-uos20/uos-runtime/ld-linux-x86-64.so.2`；
- 给每个动态 ELF 写入传递型 `DT_RPATH`，覆盖桌面 Host、WebKit 子进程、Python/uv、生成 venv、Python 原生扩展和 Chrome；
- 启动器只设置 GTK/AppDir 环境，不导出全局 `LD_LIBRARY_PATH`，避免 UOS 自带的 `xdg-open`、文件管理器或 shell 错误加载私有 libc；
- 启动器固定 `GTK_IM_MODULE=xim`，阻止私有 Ubuntu GTK 在输入框获得焦点时加载 UOS/Deepin 的系统 IBus/Fcitx GTK 模块；保留目标系统的 `XMODIFIERS`，中文输入仍经 XIM 服务进入应用；
- WebKitGTK、JavaScriptCoreGTK、GTK、GStreamer、NSS、Soup、Python/Jupyter、uv 和 Chrome 全部来自应用包，不安装系统 `libwebkit2gtk-4.1-0`；
- UOS 专用包不再加载目标机的 EGL/GL/厂商 DRI：GBM、通用 `libdrm.so.2`、GLVND、Mesa EGL/GL、swrast/kms_swrast、llvmpipe 及其 LLVM 闭包均私有携带；launcher 强制软件渲染，只有内核与 X11 server 来自系统。这是针对 Fantasy II-M 上 WebKitWebProcess 因 swrast 缺失和 `EGL_NOT_INITIALIZED` 退出的兼容策略。

专用包只能在 glibc 2.35 的 Ubuntu 22.04 runner 组装，不能在开发者当前发行版随意生成：

```bash
python tools/linux/build_uos20_deb.py \
  --appdir "$extract_root/squashfs-root" \
  --package-version '1.0.0-2+uos20.3' \
  --output drpa-next-1.0.0-linux-x86_64-uos20.deb \
  --work-dir "$extract_root/uos20-deb-work"

python tools/linux/verify_uos20_deb.py \
  --deb drpa-next-1.0.0-linux-x86_64-uos20.deb \
  --expected-version '1.0.0-2+uos20.3' \
  --extract-root "$extract_root/uos20-verify" \
  --manifest-output drpa-next-1.0.0-linux-x86_64-uos20-deb-manifest.json
```

正式 workflow 随后使用 `tools/linux/uos20-smoke.Dockerfile` 在 Debian 10 的真实 glibc 2.28 用户态中安装包，确认系统没有 WebKitGTK 4.1，再执行：

1. 私有加载器解析桌面 Host，并确认 libc 与 libstdc++ 来自 `/opt/drpa-next-uos20/uos-runtime`；
2. 断网创建运行环境，导入 Jupyter、ZMQ、debugpy、lxml、NumPy、Pandas、psutil、rpds 和 tornado 等原生 wheel；
3. 用内置 Chrome 完成 headless 页面测试；
4. 在 Debian 10 与 Deepin 20.8 用户态用普通用户、Xvfb 和 X11 启动桌面 Host，等待 React 两帧绘制后的 Tauri IPC 就绪标记；
5. 用 `xdotool` 发送真实 X11 键盘和鼠标事件：打开命令面板、聚焦输入框、输入哨兵文本、关闭面板并再次点击侧栏；只有输入事件、输入后的点击、WebKit timer 和 Tauri IPC 均继续工作，才会生成输入交互就绪标记；
6. 确认 WebKitWebProcess 持续存在，日志没有 EGL/swrast 致命错误，并对实际截图执行颜色数和标准差门禁；有系统字体时额外执行文字区域连通组件门禁，`font_available=0` 的最小 Deepin 镜像则必须依靠真实输入/后续点击 marker 证明页面没有冻结；
7. 卸载 `drpa-next` 并确认 XDG 用户数据哨兵仍存在。截图、输入交互标记、进程树、窗口树与日志作为 Actions 诊断资产保存。

安装时必须选择文件名带 `uos20` 的资产：

```bash
sudo apt install ./drpa-next-1.0.0-linux-x86_64-uos20.deb
```

自动化容器能证明 glibc 2.28 用户态兼容，但共享 GitHub runner 的宿主内核不是 4.19。因此每次改变 Chrome、glibc 私有层、WebKitGTK 或图形依赖后，还要在用户给出的 UOS 20 / kernel 4.19 / GCC 8.3 物理机或虚拟机完成一次人工启动、RPAZ、Jupyter 和浏览器回归，并把结果记录到 Release 或 PR。

## 8. 本地验证命令

### 8.1 前端

```bash
npm ci
npm run typecheck
npm run test
npm run build
```

### 8.2 Rust

```bash
cargo fmt --all --check
cargo clippy -p drpa-protocol -p drpa-package -p drpa-host --all-targets -- -D warnings
cargo test -p drpa-protocol -p drpa-package -p drpa-host
cargo check -p drpa-desktop
cargo test -p drpa-desktop
```

### 8.3 Python 与离线政策

```bash
./.venv/bin/python -m pytest -q runtime/python/tests tools/windows/tests
./.venv/bin/python -m unittest discover -s tools/offline/tests -v
./.venv/bin/python -m unittest discover -s tools/linux/tests -v
./.venv/bin/python -m compileall -q tools/offline tools/linux offline/bootstrap runtime/python/src
./.venv/bin/python tools/offline/validate_requirements.py offline/requirements/runtime.txt
./.venv/bin/python tools/release/check_version_consistency.py --expected 1.0.0
```

`tools/windows/tests` 在 Linux CI 运行的是更新包的纯 Python 格式与策略测试，不代表 Windows Worker 已在 Linux 上变成可用能力。

## 9. 推荐实施顺序

### 里程碑 A：源码模式可用（代码已具备，需人工复核）

- 在 Ubuntu 22.04 图形会话启动 `npm run tauri:dev`。
- 工作区、知识文档、Studio 文件操作和 `xdg-open` 正常。
- 使用开发 Python 运行普通 RPAZ、Bing 示例和两个 Notebook 单元。
- 设置页隐藏或禁用 Windows-only 能力，并显示准确平台状态。

### 里程碑 B：sealed runtime 可复现（已进入专用 workflow）

- 在 Ubuntu 22.04 构建 `linux-x86_64` runtime 与 wheelhouse lock。
- 在原生 Ubuntu runner 完成 air-gap bootstrap、DrissionPage 和 Jupyter smoke。
- 发布 runtime artifact，但暂不发布桌面端。
- 修正所有写死“Windows 运行时”的诊断文案。

### 里程碑 C：最终包布局（AppImage 与 deb 已实现）

- AppImage 使用 Tauri resource path 定位 runtime；不要改回外置同目录猜测。
- deb 使用 AppImage 派生的 `/opt/drpa-next` 私有只读布局；卸载只删除包管理文件，不删除 XDG 用户数据。
- 最终包不依赖系统 Python、系统 Chrome、npm 或网络。
- 数据目录遵守 XDG，应用移动或升级不损坏用户数据。

### 里程碑 D：原生验收与发布回归（持续进行）

- 在 Ubuntu 22.04 构建，在 Ubuntu 22.04/24.04 干净虚拟机测试。
- 覆盖 X11 与 Wayland 至少各一次人工 GUI 验收。
- 验证中文、空格和非 ASCII 路径。
- 验证任务取消后 Python/Jupyter/Chrome 进程树全部退出。
- 验证断网首次初始化、Bing 本地流程、Notebook、知识库导入导出和 Agent 本地工具。
- Linux Release 使用独立平台资产名和平台清单，不复用 Windows `.drpa-update`。

## 10. Linux 发布与回归验收标准

Linux 发布流水线必须满足自动化条目；标注为人工覆盖的跨发行版与 Wayland 条目应在后续回归中持续补齐并记录：

1. 干净系统无需安装 WebKitGTK、Python、Node、Rust、Chrome 或新版图形用户态即可启动并运行 RPAZ；现代包要求系统 glibc 2.35+ 与 C/C++/图形运行库，UOS 包只要求系统 glibc 2.28、X11 server 与内核，并使用私有 glibc/C++/WebKitGTK/Mesa llvmpipe 层。
2. 首次运行断网可完成 runtime 初始化，后续运行也不触发 pip/uv 网络请求。
3. AppImage 或 deb 能稳定定位与自身匹配的 `linux-x86_64` runtime manifest。
4. UI 使用 WebKitGTK，自动化使用内置 Chrome，两者升级边界清晰。
5. 数据写入 XDG 目录；应用目录保持只读仍可正常运行。
6. Studio 源码、RPAZ 导出、Bing 示例、Jupyter 两单元和产物目录打开均通过。
7. 长任务日志实时输出，取消后完整进程树退出。
8. 现代包在 Ubuntu 22.04 与 24.04 通过安装、启动、移动、升级和卸载回归；UOS 包先通过 Debian 10 与 Deepin 20.8/glibc 2.28 的 React/IPC/截图自动门禁，再在 UOS 20/kernel 4.19/Fantasy II-M 机器完成发布后人工回归。
9. CI 保存 runtime 构建证明、最终包和机器可读安装库存。
10. README、Release notes 和应用设置页不再把未实现的 Windows 更新能力显示为 Linux 可用。

## 11. 常见问题

### 源码构建时报 `javascriptcoregtk` / `webkit2gtk` 找不到

确认安装的是 WebKitGTK **4.1** 开发包，并执行：

```bash
pkg-config --modversion webkit2gtk-4.1
cargo check -p drpa-desktop
```

正式 deb 都不应要求该系统包。若 `apt` 仍提示安装 `libwebkit2gtk-4.1-0`，先用 `dpkg-deb -f <包> Version Depends` 检查：现代包版本为 `1.0.0-2`，UOS 包版本为 `1.0.0-2+uos20.3`；UOS 用户还必须确认文件名包含 `uos20`。出现 `1.0.0`、`uos20.1` 或 `uos20.2` 说明仍在使用已被 Release 覆盖的旧 deb；`uos20.1` 可能永久白屏，`uos20.2` 的 Deepin 验证截图没有任何文字。

### 点击输入框后页面冻结，但窗口仍能移动

这是私有 Ubuntu GTK 与目标系统输入法 GTK 模块混用时的高风险边界：输入框聚焦才会创建输入法上下文，因此首屏可以正常显示，而 WebView 的页面事件随后全部失去响应。UOS launcher 必须保留 `GTK_IM_MODULE=xim`，不得改回自动发现，也不得把 UOS 系统目录中的 IBus/Fcitx `.so` 复制进私有 GTK。排查时先确认：

```bash
grep -F 'GTK_IM_MODULE=xim' /usr/bin/drpa-next
pgrep -a -f 'drpa-desktop|WebKitNetworkProcess|WebKitWebProcess|WebKitGPUProcess'
```

发布门禁必须看到 `drpa-uos20-input-ready.json` 中 `nativeInputTyped`、`postInputClick` 和 `ipcRoundTrip` 都为 `true`；仅看到窗口或 NetworkProcess 存活不能证明输入路径可用。

### 运行环境页面提示找不到封装运行时

源码联调使用第 4.2 节的 `DRPA_RUNTIME_PYTHON`；sealed runtime 联调使用 `DRPA_RUNTIME_ROOT`。不要把 Windows runtime 复制到 Linux。

### DrissionPage 找不到浏览器

开发模式设置 `DRPA_BROWSER_PATH`。sealed 模式只信任 runtime manifest 的 `browserExecutable`，不要在 Host 中递归搜索浏览器。

### AppImage 能打开，但运行脚本失败

先检查是否同时提供了匹配的 runtime，并查看：

```bash
echo "$DRPA_RUNTIME_ROOT"
cat "$DRPA_RUNTIME_ROOT/manifest.json"
```

专用 workflow 生成的 AppImage 包含完整 runtime；自行运行普通 `tauri build` 而没有先 stage runtime 的结果不属于完整离线产品包。

### GUI 中启动的进程找不到 shell 里的命令

Linux GUI 应用不保证继承 `.bashrc`、`.profile` 等 shell 初始化文件中的 `PATH`。正式能力必须使用配置、manifest 或 Tauri resource path 的绝对路径，不能依赖交互 shell 环境。

## 12. 交接状态

- 主开发分支：`codex/drpa-next-platform`。
- 当前交接 PR：<https://github.com/EthanBird/drpa-client/pull/2>。
- Windows 稳定基线：`desktop-v1.0.0`，提交 `5e6c793`。
- Linux 第一优先级：保持 `Build and publish Linux x86_64 offline desktop` workflow 全绿，处理真实 Rust/Clippy/Tauri/AppImage、现代 deb 与 UOS deb 日志。
- Linux 第二优先级：在 UOS 20/kernel 4.19 真机完成专用 deb 回归，并在 Ubuntu 22.04/24.04 干净虚拟机分别完成 X11/Wayland、断网首次启动、RPAZ、Notebook、Agent 工具、取消进程树和中文路径人工验收。

开始编码前请先阅读：

1. [`DEVELOPMENT.md`](DEVELOPMENT.md)
2. [`architecture/DRPA_NEXT.md`](architecture/DRPA_NEXT.md)
3. [`../offline/README.md`](../offline/README.md)
4. [`JUPYTER_INTEGRATION.md`](JUPYTER_INTEGRATION.md)
5. [`PORTABLE_RELEASE.md`](PORTABLE_RELEASE.md)
6. [`UOS20_PACKAGING.md`](UOS20_PACKAGING.md)

平台适配应落在小而明确的 adapter 或配置层。不要复制一套 Linux 业务页面，也不要用 `cfg` 把领域规则分叉成两套。
